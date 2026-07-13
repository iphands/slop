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

/// Max `svc_print` lines buffered between ticks (oldest dropped past this).
const PRINT_BUFFER_CAP: usize = 128;

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
    /// The server refused the connect handshake (e.g. `Server is full.`,
    /// `Bad challenge.`). Terminal — the reason is in [`Conn::reject_reason`].
    Rejected,
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
    /// Accumulated `svc_print` lines (obituaries, chat, MOTD) since the last
    /// [`Conn::drain_prints`]. Capped so a print burst can't grow unbounded.
    prints: Vec<String>,
    /// Server's rejection message when [`ConnState::Rejected`] (e.g. `Server is full.`).
    /// `None` until a pre-netchan OOB `print` classifies the handshake as refused.
    pub reject_reason: Option<String>,
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
            prints: Vec::new(),
            reject_reason: None,
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
                // The server rejects at `SVC_DirectConnect` — before `client_connect`,
                // i.e. before a netchan exists. So an OOB `print` received while we are
                // still `Connecting` is a handshake rejection (`Server is full.`,
                // `Bad challenge.`, `Connection refused.`, protocol/password), not chat.
                // Once Active, chat/MOTD arrives in-band as `svc_print` (see `on_payload`).
                if self.state == ConnState::Connecting {
                    let reason = line.strip_prefix("print").unwrap_or(line).trim();
                    self.reject_reason = Some(reason.to_string());
                    self.state = ConnState::Rejected;
                }
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
                                nc.message_mut()
                                    .write_string(&format!("begin {servercount}"));
                                self.begin_queued = true;
                            }
                        }
                    } else if s.starts_with("changing") {
                        // Map change, part 1 (`sv_init.c:614` broadcasts "changing\n"):
                        // the level is unloading. Mirror `CL_Changing_f`
                        // (`cl_network.c:436`): drop out of Active but KEEP the netchan —
                        // the server retains our client slot across the change.
                        self.reset_level_state();
                        self.state = ConnState::Connected;
                    } else if s.starts_with("reconnect") {
                        // Map change, part 2 (`sv_init.c:654` broadcasts "reconnect\n"):
                        // the new level is up. Mirror `CL_Reconnect_f` while connected
                        // (`cl_network.c:468`): request a fresh serverdata over the SAME
                        // netchan with a reliable "new" — no challenge/connect redo.
                        self.reset_level_state();
                        self.serverdata = None;
                        self.configstrings = ConfigStrings::default();
                        self.state = ConnState::Connected;
                        if let Some(nc) = self.netchan.as_mut() {
                            nc.message_mut().write_u8(ClcOp::Stringcmd.into());
                            nc.message_mut().write_string("new");
                        }
                    }
                    // Other stufftext ("kick", "cmd startdlights", etc.) is ignored.
                }
                Ok(SvcEvent::Print { text, .. }) => {
                    self.prints.push(text);
                    // Cap so a MOTD/chat burst between ticks can't grow this.
                    if self.prints.len() > PRINT_BUFFER_CAP {
                        let drop_n = self.prints.len() - PRINT_BUFFER_CAP;
                        self.prints.drain(0..drop_n);
                    }
                }
                Ok(SvcEvent::Nop) => {}
                Ok(SvcEvent::Disconnect) => {
                    self.state = ConnState::Disconnected;
                    break;
                }
                Ok(SvcEvent::Reconnect) => {
                    // Server-forced hard reconnect: restart the handshake from scratch.
                    // Clear all per-level state too — the next serverdata is a new level.
                    self.reset_level_state();
                    self.serverdata = None;
                    self.configstrings = ConfigStrings::default();
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

    /// Re-issue the handshake request while stuck in `Connecting`, else `None`.
    ///
    /// UDP guarantees nothing, and after a server-forced hard reconnect (rcon `map X`
    /// runs `SV_InitGame` → `SV_FinalMessage` with `svc_reconnect`, wiping every client
    /// slot) the server answers nothing while the new level loads — a single
    /// `getchallenge` sent into that window is simply swallowed. Real clients resend
    /// every ~3 s (`CL_CheckForResend`, `cl_main.c`); callers should pace this the same
    /// way. Restarting from `getchallenge` is always safe: `SV_GetChallenge` re-issues
    /// per-address and a duplicate `client_connect` is ignored by [`Conn::on_oob`].
    pub fn resend_connect(&self) -> Option<Bytes> {
        if self.state != ConnState::Connecting {
            return None;
        }
        Some(oob_line("getchallenge\n"))
    }

    /// Drop the per-level snapshot state (frame history + spawn latch) when the server
    /// changes levels. Stale [`FrameRing`] entries would poison delta decode against the
    /// new level's frames, and `begin_queued` must re-arm so the next `precache`
    /// stufftext queues a fresh `begin <servercount>`.
    fn reset_level_state(&mut self) {
        self.begin_queued = false;
        self.frame = None;
        self.ring = FrameRing::new();
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

    /// Drain `svc_print` lines accumulated since the last call (obituaries, chat,
    /// MOTD). The brain feeds these to the danger heatmap (Plan 08 T1). Returns
    /// the lines in arrival order; the buffer is cleared.
    pub fn drain_prints(&mut self) -> Vec<String> {
        std::mem::take(&mut self.prints)
    }

    /// Our player's world-space origin from the most recent frame, if any.
    pub fn self_origin(&self) -> Option<[f32; 3]> {
        self.frame
            .as_ref()
            .map(|f| f.playerstate.pmove.origin_f32())
    }

    /// Queue a reliable `clc_stringcmd "<text>"` to be sent to the server on the
    /// next transmit. This is how a client issues commands the game DLL consumes
    /// via `ClientCommand` — e.g. `use Rocket Launcher` to switch weapons, since
    /// Q2 ignores `usercmd.impulse`. No-op before the netchan is up.
    pub fn queue_stringcmd(&mut self, text: &str) {
        if let Some(nc) = self.netchan.as_mut() {
            nc.message_mut().write_u8(ClcOp::Stringcmd.into());
            nc.message_mut().write_string(text);
        }
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

    /// Build a disconnect packet to send to the server before teardown.
    /// Sends `clc_stringcmd "disconnect"` three times (per CL_Disconnect in yquake2).
    pub fn disconnect(&mut self) -> Option<Bytes> {
        let nc = self.netchan.as_mut()?;
        let payload = b"disconnect";
        Some(nc.transmit(payload))
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
    // Plan 57: ack the `clc_move` on frame arrival (mirrors the fleet loop in
    // `qbots/src/main.rs`), so the server measures our reply ~1 RTT after it sent the
    // frame instead of RTT + up-to-100 ms of free-running timer phase. The timer is
    // demoted to a keepalive that only fires when frames stall (no send in ~90 ms).
    const KEEPALIVE_GAP: Duration = Duration::from_millis(90);
    let mut last_send = time::Instant::now();
    loop {
        tokio::select! {
            res = sock.recv(&mut buf) => {
                let n = res?;
                let prev_sf = conn.frame.as_ref().map(|f| f.serverframe);
                if let Some(pkt) = conn.on_recv(&buf[..n]) {
                    let _ = sock.send(&pkt).await;
                }
                if conn.state() == ConnState::Disconnected {
                    break;
                }
                // A freshly-decoded frame → ack it immediately when Active.
                if conn.state() == ConnState::Active
                    && conn.frame.as_ref().map(|f| f.serverframe) != prev_sf
                {
                    if let Some(pkt) = conn.keepalive() {
                        let _ = sock.send(&pkt).await;
                        last_send = time::Instant::now();
                    }
                }
            }
            _ = ticker.tick() => {
                // Keepalive fallback: only send here if no frame-triggered send happened
                // in the last ~90 ms (frames stalled), so we don't double the send rate.
                let keepalive_due = conn.state() != ConnState::Active
                    || time::Instant::now().duration_since(last_send) >= KEEPALIVE_GAP;
                if keepalive_due {
                    if let Some(pkt) = conn.keepalive() {
                        let _ = sock.send(&pkt).await;
                        last_send = time::Instant::now();
                    }
                }
                // ~1s heartbeat: state + latest frame's serverframe, entity count, origin.
                ticks = ticks.wrapping_add(1);
                if ticks.is_multiple_of(10) {
                    match &conn.frame {
                        Some(f) => {
                            let o = f.playerstate.pmove.origin_f32();
                            tracing::debug!(
                                state = ?conn.state(),
                                frame = f.serverframe,
                                ents = f.entities.len(),
                                "origin=({:.1},{:.1},{:.1})",
                                o[0], o[1], o[2]
                            );
                        }
                        None => tracing::debug!(state = ?conn.state(), "(no frame yet)"),
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
    fn server_full_reject_is_classified() {
        let mut c = Conn::new(addr(), "qbots", 1234);
        c.start();
        assert_eq!(c.state(), ConnState::Connecting);
        // Server refuses the connect: OOB `print\nServer is full.\n` (pre-netchan).
        let out = c.on_recv(&server_oob("print\nServer is full.\n"));
        assert!(out.is_none(), "a reject emits no reply");
        assert_eq!(c.state(), ConnState::Rejected);
        assert_eq!(c.reject_reason.as_deref(), Some("Server is full."));
    }

    #[test]
    fn other_reject_reasons_are_captured() {
        for msg in [
            "Bad challenge.",
            "Connection refused.",
            "Server is protocol version 34.",
        ] {
            let mut c = Conn::new(addr(), "qbots", 7);
            c.start();
            c.on_recv(&server_oob(&format!("print\n{msg}\n")));
            assert_eq!(c.state(), ConnState::Rejected, "{msg} must reject");
            assert_eq!(c.reject_reason.as_deref(), Some(msg));
        }
    }

    #[test]
    fn inband_print_after_active_is_not_a_reject() {
        // An OOB print only rejects while Connecting. Once past the handshake, a stray
        // OOB print must not flip us to Rejected (defensive — real chat is in-band).
        let mut c = Conn::new(addr(), "qbots", 9);
        c.start();
        c.on_recv(&server_oob("challenge 999 p=34\n"));
        c.on_recv(&server_oob("client_connect\n"));
        assert_eq!(c.state(), ConnState::Connected);
        c.on_recv(&server_oob("print\nhello\n"));
        assert_eq!(
            c.state(),
            ConnState::Connected,
            "print past Connecting is ignored"
        );
        assert!(c.reject_reason.is_none());
    }

    /// A netchan payload of just `svc_stufftext "<text>"`.
    fn stufftext_payload(text: &str) -> Bytes {
        let mut w = Writer::new();
        w.write_u8(SvcOp::Stufftext.into());
        w.write_string(text);
        w.freeze()
    }

    /// Walk a fresh Conn through the handshake to Active with servercount 4242.
    fn active_conn() -> Conn {
        let mut c = Conn::new(addr(), "qbots", 1234);
        c.start();
        c.on_recv(&server_oob("challenge 999 p=34\n"));
        c.on_recv(&server_oob("client_connect\n"));
        let mut payload = serverdata_payload().to_vec();
        payload.extend_from_slice(&stufftext_payload("cmd configstrings 4242 0\n"));
        c.on_recv(&server_frame(1, 1, &payload));
        c.on_recv(&server_frame(2, 1, &stufftext_payload("precache 4242\n")));
        assert_eq!(c.state(), ConnState::Active);
        assert!(c.begin_queued);
        c
    }

    /// The Yamagi map-change flow (`sv_init.c:614/654`): stufftext "changing" drops us
    /// to Connected on the live netchan, stufftext "reconnect" queues a reliable "new",
    /// and the fresh serverdata + precache re-queue `begin <new servercount>` — which
    /// requires the `begin_queued` latch to have reset.
    #[test]
    fn map_change_stufftext_rehandshakes_on_live_netchan() {
        let mut c = active_conn();

        // Part 1: "changing" → not Active anymore, netchan kept, frame state dropped.
        c.on_recv(&server_frame(3, 1, &stufftext_payload("changing\n")));
        assert_eq!(c.state(), ConnState::Connected);
        assert!(c.netchan.is_some(), "netchan must survive the map change");
        assert!(c.frame.is_none());
        assert!(!c.begin_queued);

        // Part 2: "reconnect" → reliable "new" rides the next transmit.
        c.on_recv(&server_frame(4, 1, &stufftext_payload("reconnect\n")));
        assert_eq!(c.state(), ConnState::Connected);
        assert!(c.serverdata.is_none(), "old serverdata cleared");
        let out = c.keepalive().expect("transmit flushing the reliable new");
        // header(8) + qport(2) + Stringcmd(1) + "new\0"(4)
        assert!(out.len() >= 15, "reliable new must be in flight");

        // New level's serverdata (servercount 4343) + precache → begin re-queued.
        let mut w = Writer::new();
        w.write_u8(SvcOp::Serverdata.into());
        w.write_i32(34);
        w.write_i32(4343); // new servercount
        w.write_u8(0);
        w.write_string("baseq2");
        w.write_i16(0);
        w.write_string("q2dm2");
        let pkt = server_frame(5, 2, &w.freeze());
        c.on_recv(&pkt);
        assert_eq!(c.state(), ConnState::Active);
        assert_eq!(c.serverdata.as_ref().unwrap().servercount, 4343);
        assert!(!c.begin_queued, "begin waits for the new precache");
        c.on_recv(&server_frame(6, 2, &stufftext_payload("precache 4343\n")));
        assert!(c.begin_queued, "begin re-queued for the new level");
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
