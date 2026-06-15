//! Connection state machine + async driver.
//!
//! [`Conn`] is a synchronous FSM over the Q2 connect handshake — driven by injected
//! datagrams so the whole handshake can be unit-tested without a socket. [`run`] is the
//! thin tokio wrapper that wires a [`tokio::net::UdpSocket`] and a keep-alive timer to
//! the FSM (live-server verification is Plan 03 T8).
//!
//! Handshake (`cl_network.c`, `sv_conless.c`):
//! `getchallenge` → `challenge N p=34` → `connect <34> <qport> <N> "<userinfo>"` →
//! `client_connect` → (netchan up) reliable `clc_stringcmd "new"` → `svc_serverdata` →
//! reliable `clc_stringcmd "begin <servercount>"` → active.

use std::net::SocketAddr;
use std::time::Duration;

use bytes::Bytes;
use q2proto::{
    build_clc_move, is_oob, oob_payload, parse_frame, tokenize, write_oob, ClcOp, Frame, FrameRing,
    Reader, SvcOp, Usercmd, Writer, PROTOCOL_VERSION,
};
use tokio::net::UdpSocket;
use tokio::time;

use crate::parse::{parse_message, ConfigStrings, ServerData, SvcEvent};
use crate::{Netchan, Userinfo};

/// Connection lifecycle states (ports the `ca_*` enum, `client/header/client.h:194`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// Not talking to a server.
    Disconnected,
    /// Sending getchallenge/connect requests.
    Connecting,
    /// Netchan established, waiting for svc_serverdata.
    Connected,
    /// Spawned into the level.
    Active,
}

/// A synchronous connection FSM. Methods return an optional datagram to send back.
pub struct Conn {
    pub addr: SocketAddr,
    pub userinfo: Userinfo,
    pub qport: u16,
    pub state: ConnState,

    challenge: i32,
    netchan: Option<Netchan>,
    configstrings: ConfigStrings,
    pub serverdata: Option<ServerData>,
    ring: FrameRing,
    /// Most recently decoded server frame (our state + visible world).
    pub frame: Option<Frame>,
    begin_queued: bool,
}

impl Conn {
    /// A new connection that has not yet started the handshake.
    pub fn new(addr: SocketAddr, name: &str, qport: u16) -> Self {
        Self {
            addr,
            userinfo: Userinfo::new(name),
            qport,
            state: ConnState::Disconnected,
            challenge: 0,
            netchan: None,
            configstrings: ConfigStrings::default(),
            serverdata: None,
            ring: FrameRing::new(),
            frame: None,
            begin_queued: false,
        }
    }

    /// Begin the handshake: emit the `getchallenge` OOB packet and enter Connecting.
    pub fn start(&mut self) -> Option<Bytes> {
        self.state = ConnState::Connecting;
        Some(oob_line("getchallenge\n"))
    }

    /// Handle a received datagram (OOB or in-band). Returns any packet to send back.
    pub fn on_recv(&mut self, packet: &[u8]) -> Option<Bytes> {
        if is_oob(packet) {
            return self.on_oob(oob_payload(packet).unwrap_or(&[]));
        }
        // Peek at the reliable bit (bit 31 of w1 in LE = MSB of byte 3) before process
        // consumes the packet's sequence state. See netchan.rs wire-format comment.
        let has_reliable = packet.len() >= 4 && (packet[3] >> 7) != 0;
        // In-band netchan packet — let the channel validate + strip the header.
        let netchan = self.netchan.as_mut()?;
        let payload = netchan.process(packet)?;
        // netchan.process borrows &mut self.netchan; we must finish that borrow before
        // touching self again, so parse out of a local reader over the payload slice.
        // Reconnect is the only on_payload case that returns a packet (getchallenge OOB).
        let oob_reply = self.on_payload(payload);
        if oob_reply.is_some() {
            return oob_reply;
        }
        // Immediately ACK server reliables: YamagiQ2 drives configstrings via reliable
        // "cmd configstrings N K" stufftexts and the bot must reply quickly. Waiting for
        // the 100ms ticker risks the server retransmitting the same reliable, which
        // toggles incoming_reliable_sequence back — defeating the ACK. Standard Q2
        // clients send clc_move on every server frame, so we do the same: flush any
        // pending stringcmds (e.g. the next "configstrings N K" request or "begin N")
        // along with the ack.
        if has_reliable {
            let nc = self.netchan.as_mut()?;
            Some(nc.transmit(&[]))
        } else {
            None
        }
    }

    fn on_oob(&mut self, payload: &[u8]) -> Option<Bytes> {
        let line = std::str::from_utf8(payload).unwrap_or("");
        let argv = tokenize(line);
        match argv.first().map(String::as_str) {
            Some("challenge") => {
                // `challenge <N> p=34`
                if let Some(n) = argv.get(1).and_then(|s| s.parse::<i32>().ok()) {
                    self.challenge = n;
                    let line = format!(
                        "connect {} {} {} \"{}\"\n",
                        PROTOCOL_VERSION,
                        self.qport,
                        n,
                        self.userinfo.as_str()
                    );
                    return Some(oob_line(&line));
                }
            }
            Some("client_connect") if self.netchan.is_none() => {
                // Netchan up; queue the reliable `new`. (Dup client_connect is ignored.)
                let mut nc = Netchan::new(self.qport);
                nc.message_mut().write_u8(ClcOp::Stringcmd.into());
                nc.message_mut().write_string("new");
                self.netchan = Some(nc);
                self.state = ConnState::Connected;
            }
            Some("client_connect") => {}
            Some("print") => {
                // `print\n<message>` — informational; not fatal.
            }
            _ => {}
        }
        None
    }

    fn on_payload(&mut self, payload: &[u8]) -> Option<Bytes> {
        let mut r = Reader::new(payload);
        loop {
            match parse_message(&mut r) {
                Ok(SvcEvent::ServerData(sd)) => {
                    self.serverdata = Some(sd.clone());
                    // YamagiQ2 sends "cmd configstrings N 0" in this same reliable block.
                    // We respond via StuffText handling and only send "begin" on "precache".
                    self.state = ConnState::Active;
                }
                Ok(SvcEvent::ConfigString { index, value }) => {
                    self.configstrings.set(index, value);
                }
                Ok(SvcEvent::StuffText(s)) => {
                    if let Some(server_cmd) = s.strip_prefix("cmd ") {
                        // "cmd X" = forward X to the server as a reliable stringcmd.
                        // YamagiQ2 drives configstring pulling with "cmd configstrings N K".
                        if let Some(nc) = self.netchan.as_mut() {
                            nc.message_mut().write_u8(ClcOp::Stringcmd.into());
                            nc.message_mut().write_string(server_cmd);
                        }
                    } else if s.starts_with("precache") && !self.begin_queued {
                        // Server finished sending configstrings + baselines; spawn us.
                        if let Some(sd) = &self.serverdata {
                            let servercount = sd.servercount;
                            if let Some(nc) = self.netchan.as_mut() {
                                nc.message_mut().write_u8(ClcOp::Stringcmd.into());
                                nc.message_mut().write_string(&format!("begin {servercount}"));
                                self.begin_queued = true;
                            }
                        }
                    }
                    // Other stufftext ("kick", "cmd startdlights", etc.) is ignored.
                }
                Ok(SvcEvent::Print { .. }) | Ok(SvcEvent::Nop) => {}
                Ok(SvcEvent::Disconnect) => {
                    self.state = ConnState::Disconnected;
                    break;
                }
                Ok(SvcEvent::Reconnect) => {
                    // Restart the handshake from scratch.
                    self.netchan = None;
                    self.state = ConnState::Connecting;
                    return Some(oob_line("getchallenge\n"));
                }
                // svc_frame → decode the full snapshot (Plan 04); other un-handled ops
                // (spawnbaseline, sound, …) still stop the payload parse here.
                Ok(SvcEvent::Unhandled(op)) if SvcOp::from_u8(op) == Some(SvcOp::Frame) => {
                    match parse_frame(&mut r, &self.ring) {
                        Ok(frame) => {
                            self.ring.store(frame.clone());
                            self.frame = Some(frame);
                        }
                        Err(_) => break,
                    }
                }
                Ok(SvcEvent::Unhandled(_)) | Err(_) => break,
            }
        }
        None
    }

    /// Build a heartbeat frame. Once Active, send a real `clc_move` (walk forward) so
    /// the server moves us; before that, an empty transmit flushes the queued reliable
    /// `new`/`begin` and refreshes the server's last_received.
    pub fn keepalive(&mut self) -> Option<Bytes> {
        let payload: Vec<u8> = if self.state == ConnState::Active {
            let cmd = Usercmd {
                msec: 33,
                forwardmove: 400, // walk forward
                ..Default::default()
            };
            let serverframe = self.frame.as_ref().map(|f| f.serverframe).unwrap_or(-1);
            let seq = self.netchan.as_ref()?.outgoing_sequence();
            build_clc_move(serverframe, [&cmd, &cmd, &cmd], seq)
        } else {
            Vec::new()
        };
        let nc = self.netchan.as_mut()?;
        Some(nc.transmit(&payload))
    }

    /// Current state.
    pub fn state(&self) -> ConnState {
        self.state
    }

    /// Access the current configstrings table.
    pub fn configstrings(&self) -> &ConfigStrings {
        &self.configstrings
    }

    /// Our player's world-space origin from the most recent frame, if any.
    pub fn self_origin(&self) -> Option<[f32; 3]> {
        self.frame
            .as_ref()
            .map(|f| f.playerstate.pmove.origin_f32())
    }

    /// Build and transmit a move frame with the provided usercmd.
    /// Pre-active: flushes the reliable queue with an empty payload.
    /// Active: sends `clc_move` with the given command (sent 3× as Q2 expects).
    pub fn transmit_cmd(&mut self, cmd: &Usercmd) -> Option<Bytes> {
        let payload: Vec<u8> = if self.state == ConnState::Active {
            let serverframe = self.frame.as_ref().map(|f| f.serverframe).unwrap_or(-1);
            let seq = self.netchan.as_ref()?.outgoing_sequence();
            build_clc_move(serverframe, [cmd, cmd, cmd], seq)
        } else {
            Vec::new()
        };
        let nc = self.netchan.as_mut()?;
        Some(nc.transmit(&payload))
    }
}

/// Build a connectionless datagram carrying `line`.
fn oob_line(line: &str) -> Bytes {
    let mut w = Writer::new();
    write_oob(&mut w, line);
    w.freeze()
}

/// Connect to `addr`, run the handshake, and keep the connection alive until the server
/// disconnects or an error occurs. (Live verification is Plan 03 T8.)
pub async fn run(addr: SocketAddr, name: &str, qport: u16) -> std::io::Result<()> {
    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    sock.connect(addr).await?;
    let mut conn = Conn::new(addr, name, qport);

    if let Some(pkt) = conn.start() {
        sock.send(&pkt).await?;
    }

    let mut buf = vec![0u8; 4096];
    let mut ticker = time::interval(Duration::from_millis(100));
    let mut ticks = 0u32;
    loop {
        tokio::select! {
            res = sock.recv(&mut buf) => {
                let n = res?;
                if let Some(pkt) = conn.on_recv(&buf[..n]) {
                    let _ = sock.send(&pkt).await;
                }
                if conn.state() == ConnState::Disconnected {
                    break;
                }
            }
            _ = ticker.tick() => {
                if let Some(pkt) = conn.keepalive() {
                    let _ = sock.send(&pkt).await;
                }
                // ~1s heartbeat: state + latest frame's serverframe, entity count, origin.
                ticks = ticks.wrapping_add(1);
                if ticks.is_multiple_of(10) {
                    match &conn.frame {
                        Some(f) => {
                            let o = f.playerstate.pmove.origin_f32();
                            eprintln!(
                                "qbots: {:?} frame={} ents={} origin=({:.1},{:.1},{:.1})",
                                conn.state(),
                                f.serverframe,
                                f.entities.len(),
                                o[0],
                                o[1],
                                o[2]
                            );
                        }
                        None => eprintln!("qbots: {:?} (no frame yet)", conn.state()),
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use q2proto::SvcOp;

    fn addr() -> SocketAddr {
        "127.0.0.1:27910".parse().unwrap()
    }

    /// A server→client connectionless reply: 0xff×4 + `line`.
    fn server_oob(line: &str) -> Bytes {
        let mut w = Writer::new();
        write_oob(&mut w, line);
        w.freeze()
    }

    /// A server→client netchan packet: 8-byte header (seq, ack) + `payload`. No qport
    /// (servers don't send one).
    fn server_frame(sequence: u32, ack: u32, payload: &[u8]) -> Bytes {
        let mut w = Writer::new();
        w.write_i32(sequence as i32); // w1: seq, no reliable bit
        w.write_i32(ack as i32); // w2: ack, no reliable bit
        w.write_bytes(payload);
        w.freeze()
    }

    fn serverdata_payload() -> Bytes {
        let mut w = Writer::new();
        w.write_u8(SvcOp::Serverdata.into());
        w.write_i32(34); // protocol
        w.write_i32(4242); // servercount
        w.write_u8(0); // attractloop
        w.write_string("baseq2");
        w.write_i16(0); // playernum
        w.write_string("q2dm1");
        w.freeze()
    }

    #[test]
    fn handshake_walks_to_active() {
        let mut c = Conn::new(addr(), "qbots", 1234);

        // 1. start → getchallenge, state Connecting.
        let out = c.start().expect("emits getchallenge");
        assert!(is_oob(&out));
        assert_eq!(c.state(), ConnState::Connecting);

        // 2. server: challenge → we emit the connect OOB.
        let out = c.on_recv(&server_oob("challenge 999 p=34\n"));
        assert!(out.is_some(), "connect packet emitted");
        assert_eq!(c.state(), ConnState::Connecting);

        // 3. server: client_connect → netchan up, "new" queued, state Connected.
        let out = c.on_recv(&server_oob("client_connect\n"));
        assert!(out.is_none(), "new rides the next keepalive");
        assert_eq!(c.state(), ConnState::Connected);

        // 4. keepalive → a frame carrying the reliable "new".
        let frame = c.keepalive().expect("frame");
        // header(8) + qport(2) + reliable payload (Stringcmd 1B + "new\0" 4B)
        assert!(frame.len() >= 15);

        // 5. server sends svc_serverdata + stufftext "cmd configstrings 4242 0".
        // YamagiQ2 drives configstring pulling via stufftext; "begin" is queued only
        // when "precache" stufftext arrives (not immediately on serverdata).
        let mut sd_payload = serverdata_payload().to_vec();
        let mut w = q2proto::Writer::new();
        w.write_u8(SvcOp::Stufftext.into());
        w.write_string("cmd configstrings 4242 0\n");
        sd_payload.extend_from_slice(&w.freeze());
        let pkt = server_frame(1, 1, &sd_payload);
        c.on_recv(&pkt);
        assert_eq!(c.state(), ConnState::Active);
        let sd = c.serverdata.as_ref().unwrap();
        assert_eq!(sd.servercount, 4242);
        assert_eq!(sd.levelname, "q2dm1");
        // "begin" is NOT yet queued (waiting for "precache").
        assert!(!c.begin_queued, "begin must not be queued before precache");

        // 6. server later sends stufftext "precache 4242" → "begin 4242" is queued.
        let mut w2 = q2proto::Writer::new();
        w2.write_u8(SvcOp::Stufftext.into());
        w2.write_string("precache 4242\n");
        let pkt2 = server_frame(2, 1, &w2.freeze());
        c.on_recv(&pkt2);
        assert!(c.begin_queued, "begin queued after precache");

        // 7. next keepalive → a frame carrying the reliable "begin".
        let frame2 = c.keepalive().expect("frame");
        assert!(frame2.len() >= 10);
    }

    #[test]
    fn reconnect_restarts_handshake() {
        let mut c = Conn::new(addr(), "qbots", 1);
        c.start();
        c.on_recv(&server_oob("challenge 1 p=34\n"));
        c.on_recv(&server_oob("client_connect\n"));
        assert_eq!(c.state(), ConnState::Connected);

        // server forces a reconnect via an in-band svc_reconnect
        let mut p = Writer::new();
        p.write_u8(SvcOp::Reconnect.into());
        let pkt = server_frame(5, 1, &p.freeze());
        let out = c.on_recv(&pkt);
        assert_eq!(c.state(), ConnState::Connecting);
        // it emits a fresh getchallenge
        assert!(out.is_some() && is_oob(&out.unwrap()));
    }
}
