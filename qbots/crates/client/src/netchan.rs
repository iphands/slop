//! Netchan — the reliable/unreliable datagram channel layered over UDP.
//!
//! Pure, transport-agnostic port of yquake2 `src/common/netchan.c`. The async UDP
//! transport lives in `conn.rs`; this module owns only sequence bookkeeping and packet
//! framing so it can be unit-tested without a socket.
//!
//! Wire format (`netchan.c:31-38`):
//! ```text
//! 31 bits  sequence                      (w1)
//!  1 bit   this packet carries reliable payload
//! 31 bits  acknowledge sequence          (w2)
//!  1 bit   acknowledge of even/odd reliable
//! 16 bits  qport                         (client→server only)
//! <reliable payload, if the bit is set>
//! <unreliable payload (the frame / clc_move)>
//! ```
//! The server never sends a qport, so on the client side we only read the two 32-bit
//! header words.

use bytes::Bytes;
use q2proto::Writer;

/// A client-side netchan channel. Ports `netchan_t` (`common/header/common.h:587`).
pub struct Netchan {
    pub qport: u16,

    incoming_sequence: u32,
    incoming_acknowledged: u32,
    incoming_reliable_acknowledged: u32,
    /// Even/odd bit for the reliable stream we've most recently seen from the server.
    incoming_reliable_sequence: u32,

    /// Starts at 1 (see `Netchan_Setup`).
    outgoing_sequence: u32,
    /// Even/odd bit for our outgoing reliable stream.
    reliable_sequence: u32,
    last_reliable_sequence: u32,

    /// Non-zero while a reliable message is in flight (un-acked).
    reliable_length: usize,
    reliable_buf: Vec<u8>,

    /// Accumulator for the *next* reliable message — callers write `clc_*` into this
    /// (`MSG_Write*(&netchan->message, …)`). Moved into `reliable_buf` on transmit.
    message: Writer,

    /// How many packets were dropped before the last accepted one.
    pub dropped: u32,
}

impl Netchan {
    /// `Netchan_Setup(NS_CLIENT, chan, adr, qport)`.
    pub fn new(qport: u16) -> Self {
        Self {
            qport,
            incoming_sequence: 0,
            incoming_acknowledged: 0,
            incoming_reliable_acknowledged: 0,
            incoming_reliable_sequence: 0,
            outgoing_sequence: 1,
            reliable_sequence: 0,
            last_reliable_sequence: 0,
            reliable_length: 0,
            reliable_buf: Vec::new(),
            message: Writer::new(),
            dropped: 0,
        }
    }

    /// Borrow the reliable-message accumulator so callers can queue `clc_*` commands.
    pub fn message_mut(&mut self) -> &mut Writer {
        &mut self.message
    }

    /// Whether an un-acked reliable message is still in flight. (`Netchan_CanReliable`.)
    pub fn can_reliable(&self) -> bool {
        self.reliable_length == 0
    }

    /// `Netchan_NeedReliable`: resend the last reliable if it was dropped, or send a
    /// freshly-queued one.
    fn need_reliable(&self) -> bool {
        if self.incoming_acknowledged > self.last_reliable_sequence
            && self.incoming_reliable_acknowledged != self.reliable_sequence
        {
            return true;
        }
        if self.reliable_length == 0 && !self.message.is_empty() {
            return true;
        }
        false
    }

    /// `Netchan_Transmit(chan, length, data)` for a client: frame `unreliable` (the
    /// per-frame payload, e.g. `clc_move`) with the netchan header + qport, prepending
    /// any pending reliable message, and return the full packet bytes.
    pub fn transmit(&mut self, unreliable: &[u8]) -> Bytes {
        let send_reliable = self.need_reliable();

        // Promote a freshly-accumulated reliable message into the in-flight buffer.
        // `freeze` consumes the Writer, so move it out with `mem::take` (which resets
        // the accumulator to a fresh, empty Writer).
        if self.reliable_length == 0 && !self.message.is_empty() {
            let pending = std::mem::take(&mut self.message).freeze();
            self.reliable_buf = pending.to_vec();
            self.reliable_length = self.reliable_buf.len();
            self.reliable_sequence ^= 1;
        }

        let w1 = (self.outgoing_sequence & !(1u32 << 31)) | (u32::from(send_reliable) << 31);
        let w2 = (self.incoming_sequence & !(1u32 << 31)) | (self.incoming_reliable_sequence << 31);
        self.outgoing_sequence = self.outgoing_sequence.wrapping_add(1);

        let mut w = Writer::new();
        w.write_i32(w1 as i32);
        w.write_i32(w2 as i32);
        w.write_i16(self.qport as i16); // client→server always carries the qport

        if send_reliable {
            w.write_bytes(&self.reliable_buf[..self.reliable_length]);
            self.last_reliable_sequence = self.outgoing_sequence;
        }

        // Unreliable payload — our frames are small vs MAX_MSGLEN, so always included.
        w.write_bytes(unreliable);
        w.freeze()
    }

    /// `Netchan_Process(chan, msg)` for a client: validate the header, update ack state,
    /// and return the payload (server-reliable bytes + frame) on success, or `None` if
    /// the packet is stale/duplicate/malformed.
    pub fn process<'a>(&mut self, msg: &'a [u8]) -> Option<&'a [u8]> {
        if msg.len() < 8 {
            return None;
        }
        let sequence = i32::from_le_bytes([msg[0], msg[1], msg[2], msg[3]]) as u32;
        let sequence_ack = i32::from_le_bytes([msg[4], msg[5], msg[6], msg[7]]) as u32;

        let reliable_message = sequence >> 31;
        let reliable_ack = sequence_ack >> 31;
        let sequence = sequence & !(1u32 << 31);
        let sequence_ack = sequence_ack & !(1u32 << 31);

        // Discard stale or duplicated packets.
        if sequence <= self.incoming_sequence {
            return None;
        }
        self.dropped = sequence.wrapping_sub(self.incoming_sequence.wrapping_add(1));

        // If the server acked our current reliable stream, it's been received.
        if reliable_ack == self.reliable_sequence {
            self.reliable_length = 0;
        }

        self.incoming_sequence = sequence;
        self.incoming_acknowledged = sequence_ack;
        self.incoming_reliable_acknowledged = reliable_ack;
        if reliable_message != 0 {
            self.incoming_reliable_sequence ^= 1;
        }

        Some(&msg[8..])
    }

    /// Sequence number of the next packet we'll send (debug / status).
    pub fn outgoing_sequence(&self) -> u32 {
        self.outgoing_sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use q2proto::ClcOp;

    fn header(packet: &[u8]) -> (u32, u32, u16) {
        // (w1 & 0x7fffffff, w2 & 0x7fffffff, qport)
        let w1 = i32::from_le_bytes([packet[0], packet[1], packet[2], packet[3]]) as u32;
        let w2 = i32::from_le_bytes([packet[4], packet[5], packet[6], packet[7]]) as u32;
        let qport = i16::from_le_bytes([packet[8], packet[9]]) as u16;
        (w1 & !(1u32 << 31), w2 & !(1u32 << 31), qport)
    }

    #[test]
    fn empty_transmit_header_and_qport() {
        let mut n = Netchan::new(0x1234);
        let pkt = n.transmit(&[]);
        // header(8) + qport(2), no payload
        assert_eq!(pkt.len(), 10);
        let (seq, ack, qp) = header(&pkt);
        assert_eq!(seq, 1); // outgoing_sequence starts at 1
        assert_eq!(ack, 0); // nothing received yet
        assert_eq!(qp, 0x1234);
        // sequence incremented after transmit
        assert_eq!(n.outgoing_sequence(), 2);
    }

    #[test]
    fn reliable_message_is_prepended() {
        let mut n = Netchan::new(7);
        // queue a clc_stringcmd "new" into the reliable accumulator
        n.message_mut().write_u8(ClcOp::Stringcmd.into());
        n.message_mut().write_string("new");

        let pkt = n.transmit(&[]); // empty unreliable
                                   // reliable bit must be set in w1
        let w1 = i32::from_le_bytes([pkt[0], pkt[1], pkt[2], pkt[3]]) as u32;
        assert_eq!(w1 >> 31, 1, "reliable bit set");
        // after qport(10 bytes) the reliable payload begins: clc_stringcmd(2) + "new\0"
        assert_eq!(pkt[10], u8::from(ClcOp::Stringcmd));
        assert_eq!(&pkt[11..15], b"new\0");
        // the channel now considers it in-flight
        assert!(!n.can_reliable());
    }

    #[test]
    fn process_returns_payload_and_advances_sequence() {
        let mut n = Netchan::new(1);
        // craft a server packet: seq=5, ack=1, no reliable bits, payload "PL"
        let mut w = Writer::new();
        w.write_i32(5); // w1: seq 5, reliable=0
        w.write_i32(1); // w2: ack 1, rack=0
        w.write_bytes(b"PL");
        let pkt = w.freeze();

        let payload = n.process(&pkt).expect("accepted");
        assert_eq!(payload, b"PL");
        assert_eq!(n.dropped, 4); // 5 - (0 + 1) = 4 dropped
    }

    #[test]
    fn process_rejects_stale_and_duplicate() {
        let mut n = Netchan::new(1);
        let mut w = Writer::new();
        w.write_i32(10);
        w.write_i32(0);
        let pkt = w.freeze();
        assert!(n.process(&pkt).is_some()); // seq 10 accepted

        // a duplicate (seq 10 again) → rejected
        assert!(n.process(&pkt).is_none());
        // an older one (seq 5) → rejected
        let mut w2 = Writer::new();
        w2.write_i32(5);
        w2.write_i32(0);
        assert!(n.process(&w2.freeze()).is_none());
    }

    #[test]
    fn reliable_ack_clears_in_flight() {
        let mut n = Netchan::new(1);
        n.message_mut().write_u8(ClcOp::Stringcmd.into());
        n.message_mut().write_string("new");
        let _ = n.transmit(&[]);
        assert!(!n.can_reliable(), "in flight");

        // After transmit, reliable_sequence flipped to 1. Craft a server packet whose
        // w2 reliable-ack bit equals reliable_sequence (1) → acks our reliable.
        let mut w = Writer::new();
        w.write_i32(20); // server seq
        w.write_i32(((1u32 << 31) | 1) as i32); // w2: rack=1 (matches), ack=1
        n.process(&w.freeze());
        assert!(n.can_reliable(), "acked");
    }

    #[test]
    fn reliable_then_unreliable_ordering() {
        // Frame a reliable "new" + an unreliable blob; the reliable payload is prepended.
        let mut n = Netchan::new(42);
        n.message_mut().write_u8(ClcOp::Stringcmd.into());
        n.message_mut().write_string("new");
        let blob = [0x55u8, 0x66, 0x77];
        let pkt = n.transmit(&blob);

        // After header(8) + qport(2): reliable payload (Stringcmd 1B + "new\0" 4B = 5),
        // then the unreliable blob.
        let after_hdr = &pkt[10..];
        assert_eq!(after_hdr[0], u8::from(ClcOp::Stringcmd));
        assert_eq!(&after_hdr[1..5], b"new\0");
        assert_eq!(&after_hdr[5..], &blob[..]);
    }
}
