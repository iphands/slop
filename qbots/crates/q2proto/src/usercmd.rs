//! `usercmd_t` — the per-frame movement command a client sends (`clc_move`).
//!
//! Ports `usercmd_t` (`common/header/shared.h:676`) and its delta encode/decode
//! (`MSG_WriteDeltaUsercmd` at `movemsg.c:644`, `MSG_ReadDeltaUsercmd` at `:1181`).

use crate::crc::block_sequence_crc_byte;
use crate::error::DecodeError;
use crate::ops::{
    ClcOp, CM_ANGLE1, CM_ANGLE2, CM_ANGLE3, CM_BUTTONS, CM_FORWARD, CM_IMPULSE, CM_SIDE, CM_UP,
};
use crate::{Reader, Writer};

/// A single movement command. Fields match `usercmd_t` byte-for-byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usercmd {
    /// Duration this command covers.
    pub msec: u8,
    /// Button bitmask (BUTTON_ATTACK=1, BUTTON_USE=2, …).
    pub buttons: u8,
    /// View angles (pitch / yaw / roll) as signed shorts.
    pub angles: [i16; 3],
    /// Forward / side / up intended movement, signed.
    pub forwardmove: i16,
    pub sidemove: i16,
    pub upmove: i16,
    /// Weapon-select impulse, etc.
    pub impulse: u8,
    /// Light level the player is standing on.
    pub lightlevel: u8,
}

impl Usercmd {
    /// `MSG_WriteDeltaUsercmd(buf, from, self)`: write a bitmask of changed fields,
    /// then each changed field, then `msec` + `lightlevel` (always).
    pub fn write_delta(&self, w: &mut Writer, from: &Usercmd) {
        let mut bits = 0u8;
        if self.angles[0] != from.angles[0] {
            bits |= CM_ANGLE1;
        }
        if self.angles[1] != from.angles[1] {
            bits |= CM_ANGLE2;
        }
        if self.angles[2] != from.angles[2] {
            bits |= CM_ANGLE3;
        }
        if self.forwardmove != from.forwardmove {
            bits |= CM_FORWARD;
        }
        if self.sidemove != from.sidemove {
            bits |= CM_SIDE;
        }
        if self.upmove != from.upmove {
            bits |= CM_UP;
        }
        if self.buttons != from.buttons {
            bits |= CM_BUTTONS;
        }
        if self.impulse != from.impulse {
            bits |= CM_IMPULSE;
        }

        w.write_u8(bits);

        if bits & CM_ANGLE1 != 0 {
            w.write_i16(self.angles[0]);
        }
        if bits & CM_ANGLE2 != 0 {
            w.write_i16(self.angles[1]);
        }
        if bits & CM_ANGLE3 != 0 {
            w.write_i16(self.angles[2]);
        }
        if bits & CM_FORWARD != 0 {
            w.write_i16(self.forwardmove);
        }
        if bits & CM_SIDE != 0 {
            w.write_i16(self.sidemove);
        }
        if bits & CM_UP != 0 {
            w.write_i16(self.upmove);
        }
        if bits & CM_BUTTONS != 0 {
            w.write_u8(self.buttons);
        }
        if bits & CM_IMPULSE != 0 {
            w.write_u8(self.impulse);
        }

        w.write_u8(self.msec);
        w.write_u8(self.lightlevel);
    }

    /// `MSG_ReadDeltaUsercmd(msg, from)`: start from `from`, apply the flagged deltas,
    /// always read `msec` + `lightlevel`.
    pub fn read_delta(r: &mut Reader, from: &Usercmd) -> Result<Usercmd, DecodeError> {
        let mut m = *from;
        let bits = r.read_u8()?;

        if bits & CM_ANGLE1 != 0 {
            m.angles[0] = r.read_i16()?;
        }
        if bits & CM_ANGLE2 != 0 {
            m.angles[1] = r.read_i16()?;
        }
        if bits & CM_ANGLE3 != 0 {
            m.angles[2] = r.read_i16()?;
        }
        if bits & CM_FORWARD != 0 {
            m.forwardmove = r.read_i16()?;
        }
        if bits & CM_SIDE != 0 {
            m.sidemove = r.read_i16()?;
        }
        if bits & CM_UP != 0 {
            m.upmove = r.read_i16()?;
        }
        if bits & CM_BUTTONS != 0 {
            m.buttons = r.read_u8()?;
        }
        if bits & CM_IMPULSE != 0 {
            m.impulse = r.read_u8()?;
        }

        m.msec = r.read_u8()?;
        m.lightlevel = r.read_u8()?;
        Ok(m)
    }
}

/// Build a `clc_move` payload, porting the body of `CL_SendMove` (`cl_input.c:786`):
/// `clc_move` opcode + a checksum byte + the serverframe ack + **three** delta usercmds
/// (nullcmd→a, a→b, b→c). `cmds` is `[oldest, mid, newest]`; `sequence` is the netchan
/// outgoing_sequence this packet will carry, which the server uses to recompute the
/// checksum — so it must equal the sequence `Netchan::transmit` writes to `w1`.
pub fn build_clc_move(serverframe: i32, cmds: [&Usercmd; 3], sequence: u32) -> Vec<u8> {
    let mut w = Writer::new();
    w.write_u8(ClcOp::Move.into());
    let checksum_index = w.len();
    w.write_u8(0); // checksum placeholder
    w.write_i32(serverframe);

    let nullcmd = Usercmd::default();
    cmds[0].write_delta(&mut w, &nullcmd);
    cmds[1].write_delta(&mut w, cmds[0]);
    cmds[2].write_delta(&mut w, cmds[1]);

    let bytes = w.freeze();
    let body = &bytes[checksum_index + 1..];
    let checksum = block_sequence_crc_byte(body, sequence);
    let mut out = bytes.to_vec();
    out[checksum_index] = checksum;
    out
}

/// Decode a `clc_move` message body, mirroring `SV_ReadClientMessage`'s `clc_move`
/// arm (`sv_user.c:683-702`): opcode, checksum byte, the serverframe ack, then three
/// delta-chained usercmds (`nullcmd -> oldest -> mid -> newest`). `sequence` is the
/// netchan sequence the packet carried — the server recomputes the checksum keyed on
/// `cl->netchan.incoming_sequence` (`sv_user.c:711-714`), so a
/// `build_clc_move(.., seq)` → `parse_clc_move(.., seq)` round-trip is the same check
/// a live server runs. `Err(ChecksumMismatch)` means our own packet would be silently
/// ignored by the server, which is exactly the silent failure this crate must not ship.
pub fn parse_clc_move(payload: &[u8], sequence: u32) -> Result<(i32, [Usercmd; 3]), DecodeError> {
    let mut r = Reader::new(payload);
    if ClcOp::from_u8(r.read_u8()?) != Some(ClcOp::Move) {
        return Err(DecodeError::Invalid("clc_move opcode"));
    }
    let checksum = r.read_u8()?;
    let serverframe = r.read_i32()?;
    let nullcmd = Usercmd::default();
    let oldest = Usercmd::read_delta(&mut r, &nullcmd)?;
    let mid = Usercmd::read_delta(&mut r, &oldest)?;
    let newest = Usercmd::read_delta(&mut r, &mid)?;
    // The server checksums exactly the bytes consumed through the third delta
    // (`readcount`, sv_user.c:712) — which is what we consumed here, not any trailing
    // bytes a real capture might carry.
    let consumed = payload.len() - r.remaining();
    if block_sequence_crc_byte(&payload[2..consumed], sequence) != checksum {
        return Err(DecodeError::ChecksumMismatch);
    }
    Ok((serverframe, [oldest, mid, newest]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(from: Usercmd, cmd: Usercmd) -> (Usercmd, usize) {
        let mut w = Writer::new();
        cmd.write_delta(&mut w, &from);
        let n = w.len();
        let bytes = w.freeze();
        let mut r = Reader::new(&bytes);
        let got = Usercmd::read_delta(&mut r, &from).unwrap();
        (got, n)
    }

    fn cmd(msec: u8, forwardmove: i16, yaw: i16, buttons: u8) -> Usercmd {
        Usercmd {
            msec,
            buttons,
            angles: [0, yaw, 0],
            forwardmove,
            ..Default::default()
        }
    }

    #[test]
    fn parse_clc_move_round_trips_a_distinct_triple() {
        // Three DIFFERENT cmds: the whole point of the wire format (sv_user.c:698-702
        // reads oldest/mid/newest and replays them on packet loss). A build→parse
        // round trip must return them unchanged, in order, checksum-valid.
        let cmds = [
            cmd(10, 100, 1000, 0),
            cmd(11, -200, 2000, 1),
            cmd(12, 0, -3000, 2),
        ];
        let seq = 7u32;
        let payload = build_clc_move(-1, [&cmds[0], &cmds[1], &cmds[2]], seq);
        let (sf, got) = parse_clc_move(&payload, seq).expect("our own packet must decode");
        assert_eq!(sf, -1, "serverframe ack passes through");
        assert_eq!(got, cmds, "triple must survive the delta chain in order");
    }

    #[test]
    fn parse_clc_move_detects_corruption_like_the_server() {
        let seq = 42u32;
        let cmds = [cmd(1, 0, 0, 0), cmd(2, 25, 0, 0), cmd(3, 50, 0, 0)];
        let mut payload = build_clc_move(111, [&cmds[0], &cmds[1], &cmds[2]], seq);
        // One bit inside a delta'd field changes the cmd AND breaks the checksum —
        // exactly what the server's check at sv_user.c:716 catches.
        let last = payload.len() - 1;
        payload[last] ^= 0x01;
        assert!(
            matches!(
                parse_clc_move(&payload, seq),
                Err(DecodeError::ChecksumMismatch)
            ),
            "a flipped bit must fail the same check the server runs"
        );
    }

    #[test]
    fn parse_clc_move_rejects_truncation_and_wrong_opcode() {
        assert!(matches!(parse_clc_move(&[], 1), Err(DecodeError::Eof)));
        assert!(matches!(
            parse_clc_move(&[2, 0, 0, 0, 0], 1),
            Err(DecodeError::Eof)
        ));
        // opcode 3 = clc_userinfo, not clc_move
        assert!(matches!(
            parse_clc_move(&[3, 0, 0, 0, 0, 0, 0], 1),
            Err(DecodeError::Invalid(_))
        ));
    }

    #[test]
    fn full_delta_round_trips() {
        let from = Usercmd::default();
        let cmd = Usercmd {
            msec: 16,
            buttons: 1,
            angles: [100, 200, -300],
            forwardmove: 400,
            sidemove: -50,
            upmove: 0,
            impulse: 7,
            lightlevel: 9,
        };
        let (got, n) = round_trip(from, cmd);
        assert_eq!(got, cmd);
        // 1 (bits) + 3*2 (angles) + 2*2 (fwd,side) + 1 (buttons) + 1 (impulse) + 1 (msec) + 1 (light)
        // upmove unchanged → not written. = 1+6+4+1+1+1+1 = 15
        assert_eq!(n, 15);
    }

    #[test]
    fn unchanged_cmd_is_3_bytes() {
        // from == cmd → bits byte + msec + lightlevel only.
        let cmd = Usercmd {
            msec: 16,
            lightlevel: 9,
            ..Default::default()
        };
        let (got, n) = round_trip(cmd, cmd);
        assert_eq!(got, cmd);
        assert_eq!(n, 3);
    }

    #[test]
    fn partial_delta_only_carries_changed_fields() {
        let from = Usercmd {
            angles: [1, 2, 3],
            msec: 10,
            ..Default::default()
        };
        // only angles[1] and msec/lightlevel differ
        let cmd = Usercmd {
            angles: [1, 99, 3],
            msec: 10,
            ..Default::default()
        };
        let (got, n) = round_trip(from, cmd);
        assert_eq!(got, cmd);
        // 1 (bits) + 2 (one angle) + 1 (msec) + 1 (light) = 5
        assert_eq!(n, 5);
    }

    #[test]
    fn truncated_read_is_err() {
        // bits claim an angle is present but there are no bytes for it
        let mut w = Writer::new();
        w.write_u8(CM_ANGLE1); // claims angle1 follows, but we write nothing else
        let bytes = w.freeze();
        let mut r = Reader::new(&bytes);
        assert_eq!(
            Usercmd::read_delta(&mut r, &Usercmd::default()).unwrap_err(),
            DecodeError::Eof
        );
    }

    #[test]
    fn clc_move_checksum_is_self_consistent() {
        let cmd = Usercmd {
            msec: 33,
            forwardmove: 400,
            ..Default::default()
        };
        let bytes = build_clc_move(100, [&cmd, &cmd, &cmd], 5);

        assert_eq!(bytes[0], ClcOp::Move as u8);
        // serverframe ack at bytes[2..6]
        assert_eq!(
            i32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]),
            100
        );
        // the stored checksum byte recomputes over the body with the same sequence
        let stored = bytes[1];
        let recomputed = block_sequence_crc_byte(&bytes[2..], 5);
        assert_eq!(stored, recomputed);
    }
}
