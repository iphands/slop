//! `usercmd_t` — the per-frame movement command a client sends (`clc_move`).
//!
//! Ports `usercmd_t` (`common/header/shared.h:676`) and its delta encode/decode
//! (`MSG_WriteDeltaUsercmd` at `movemsg.c:644`, `MSG_ReadDeltaUsercmd` at `:1181`).

use crate::error::DecodeError;
use crate::ops::{
    CM_ANGLE1, CM_ANGLE2, CM_ANGLE3, CM_BUTTONS, CM_FORWARD, CM_IMPULSE, CM_SIDE, CM_UP,
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
}
