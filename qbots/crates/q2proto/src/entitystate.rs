//! `entity_state_t` — a single world entity (player/item/projectile), delta-decoded.
//!
//! Ports `CL_ParseEntityBits` + `CL_ParseDelta` from `client/cl_parse.c:86/140` (the
//! field→bit mapping is mirrored from `MSG_WriteDeltaEntity` too).

use crate::ops::{
    U_ANGLE1, U_ANGLE2, U_ANGLE3, U_EFFECTS16, U_EFFECTS8, U_EVENT, U_FRAME16, U_FRAME8, U_MODEL,
    U_MODEL2, U_MODEL3, U_MODEL4, U_MOREBITS1, U_MOREBITS2, U_MOREBITS3, U_NUMBER16, U_OLDORIGIN,
    U_ORIGIN1, U_ORIGIN2, U_ORIGIN3, U_RENDERFX16, U_RENDERFX8, U_SKIN16, U_SKIN8, U_SOLID,
    U_SOUND,
};
use crate::{DecodeError, Reader};

/// One entity snapshot. Field types match `entity_state_t` (`shared.h:1233`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntityState {
    pub number: i32,
    pub origin: [f32; 3],
    pub angles: [f32; 3],
    /// Previous-frame origin (for lerping); set to `from.origin` during delta.
    pub old_origin: [f32; 3],
    pub modelindex: i32,
    pub modelindex2: i32,
    pub modelindex3: i32,
    pub modelindex4: i32,
    pub frame: i32,
    pub skinnum: i32,
    pub effects: u32,
    pub renderfx: i32,
    pub solid: i32,
    pub sound: i32,
    pub event: i32,
}

impl EntityState {
    /// `CL_ParseEntityBits`: read the `U_*` bitmask (with the MOREBITS1/2/3 extension
    /// bytes) and the entity number (byte, or short if `U_NUMBER16`). Returns `(number, bits)`.
    pub fn parse_bits(r: &mut Reader) -> Result<(i32, u32), DecodeError> {
        let mut total = r.read_u8()? as u32;
        if total & U_MOREBITS1 != 0 {
            total |= (r.read_u8()? as u32) << 8;
        }
        if total & U_MOREBITS2 != 0 {
            total |= (r.read_u8()? as u32) << 16;
        }
        if total & U_MOREBITS3 != 0 {
            total |= (r.read_u8()? as u32) << 24;
        }
        let number = if total & U_NUMBER16 != 0 {
            r.read_i16()? as i32
        } else {
            r.read_u8()? as i32
        };
        Ok((number, total))
    }

    /// `CL_ParseDelta(from, to, number, bits)`: start from `from` (lerping its origin
    /// into `old_origin`), set `number`, then overwrite the flagged fields.
    pub fn read_delta(
        r: &mut Reader,
        from: &EntityState,
        number: i32,
        bits: u32,
    ) -> Result<EntityState, DecodeError> {
        let mut to = from.clone();
        to.old_origin = from.origin;
        to.number = number;

        if bits & U_MODEL != 0 {
            to.modelindex = r.read_u8()? as i32;
        }
        if bits & U_MODEL2 != 0 {
            to.modelindex2 = r.read_u8()? as i32;
        }
        if bits & U_MODEL3 != 0 {
            to.modelindex3 = r.read_u8()? as i32;
        }
        if bits & U_MODEL4 != 0 {
            to.modelindex4 = r.read_u8()? as i32;
        }
        if bits & U_FRAME8 != 0 {
            to.frame = r.read_u8()? as i32;
        }
        if bits & U_FRAME16 != 0 {
            to.frame = r.read_i16()? as i32;
        }

        if bits & U_SKIN8 != 0 && bits & U_SKIN16 != 0 {
            to.skinnum = r.read_i32()?;
        } else if bits & U_SKIN8 != 0 {
            to.skinnum = r.read_u8()? as i32;
        } else if bits & U_SKIN16 != 0 {
            to.skinnum = r.read_i16()? as i32;
        }

        match bits & (U_EFFECTS8 | U_EFFECTS16) {
            v if v == (U_EFFECTS8 | U_EFFECTS16) => to.effects = r.read_i32()? as u32,
            v if v & U_EFFECTS8 != 0 => to.effects = r.read_u8()? as u32,
            v if v & U_EFFECTS16 != 0 => to.effects = r.read_i16()? as i32 as u32,
            _ => {}
        }

        match bits & (U_RENDERFX8 | U_RENDERFX16) {
            v if v == (U_RENDERFX8 | U_RENDERFX16) => to.renderfx = r.read_i32()?,
            v if v & U_RENDERFX8 != 0 => to.renderfx = r.read_u8()? as i32,
            v if v & U_RENDERFX16 != 0 => to.renderfx = r.read_i16()? as i32,
            _ => {}
        }

        if bits & U_ORIGIN1 != 0 {
            to.origin[0] = r.read_coord()?;
        }
        if bits & U_ORIGIN2 != 0 {
            to.origin[1] = r.read_coord()?;
        }
        if bits & U_ORIGIN3 != 0 {
            to.origin[2] = r.read_coord()?;
        }
        if bits & U_ANGLE1 != 0 {
            to.angles[0] = r.read_angle()?;
        }
        if bits & U_ANGLE2 != 0 {
            to.angles[1] = r.read_angle()?;
        }
        if bits & U_ANGLE3 != 0 {
            to.angles[2] = r.read_angle()?;
        }
        if bits & U_OLDORIGIN != 0 {
            to.old_origin = r.read_pos()?;
        }
        if bits & U_SOUND != 0 {
            to.sound = r.read_u8()? as i32;
        }

        // Events are single-frame: always set (0 when U_EVENT is absent).
        if bits & U_EVENT != 0 {
            to.event = r.read_u8()? as i32;
        } else {
            to.event = 0;
        }
        if bits & U_SOLID != 0 {
            to.solid = r.read_i16()? as i32;
        }
        Ok(to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Writer;

    /// Encode a minimal entity delta (low-7-bit flags only, no MOREBITS) for decode
    /// parity. Fields are written in the **same order** `read_delta` reads them.
    fn encode(to: &EntityState) -> Vec<u8> {
        let mut bits = 0u32;
        if to.frame != 0 {
            bits |= U_FRAME8;
        }
        if to.origin[0] != 0.0 {
            bits |= U_ORIGIN1;
        }
        if to.origin[1] != 0.0 {
            bits |= U_ORIGIN2;
        }
        if to.origin[2] != 0.0 {
            bits |= U_ORIGIN3;
        }
        if to.angles[1] != 0.0 {
            bits |= U_ANGLE2;
        }
        if to.event != 0 {
            bits |= U_EVENT;
        }
        assert!(bits < 0x80, "encode() only covers low-7-bit flags");

        let mut body = Writer::new();
        if bits & U_FRAME8 != 0 {
            body.write_u8(to.frame as u8);
        }
        if bits & U_ORIGIN1 != 0 {
            body.write_coord(to.origin[0]);
        }
        if bits & U_ORIGIN2 != 0 {
            body.write_coord(to.origin[1]);
        }
        if bits & U_ORIGIN3 != 0 {
            body.write_coord(to.origin[2]);
        }
        if bits & U_ANGLE2 != 0 {
            body.write_angle(to.angles[1]);
        }
        if bits & U_EVENT != 0 {
            body.write_u8(to.event as u8);
        }

        let mut out = Writer::new();
        out.write_u8(bits as u8);
        out.write_u8(to.number as u8);
        out.write_bytes(body.as_bytes());
        out.freeze().to_vec()
    }

    #[test]
    fn parse_bits_and_number() {
        let mut w = Writer::new();
        w.write_u8(U_ORIGIN1 as u8); // bits, no MOREBITS
        w.write_u8(7); // number (byte)
        let b = w.freeze();
        let mut r = Reader::new(&b);
        let (num, bits) = EntityState::parse_bits(&mut r).unwrap();
        assert_eq!(num, 7);
        assert_eq!(bits, U_ORIGIN1);
    }

    #[test]
    fn parse_bits_morebits_and_number16() {
        // U_MODEL (bit 11) + U_NUMBER16 (bit 8) need byte 2 (MOREBITS1) + a short number.
        let bits = U_MODEL | U_NUMBER16;
        let mut w = Writer::new();
        w.write_u8((bits & 0xff) as u8 | U_MOREBITS1 as u8); // byte 1 + MOREBITS1 flag
        w.write_u8((bits >> 8) as u8); // byte 2 (bits 8–15)
        w.write_i16(300); // 16-bit number
        let b = w.freeze();
        let mut r = Reader::new(&b);
        let (num, got) = EntityState::parse_bits(&mut r).unwrap();
        assert_eq!(num, 300);
        assert_eq!(got & bits, bits); // semantic bits present (MOREBITS1 also set in `got`)
    }

    #[test]
    fn delta_decodes_fields() {
        let from = EntityState::default();
        let to = EntityState {
            number: 5,
            origin: [10.0, -5.0, 0.0],
            angles: [0.0, 45.0, 0.0],
            frame: 3,
            ..Default::default()
        };
        let bytes = encode(&to);

        let mut r = Reader::new(&bytes);
        let (num, bits) = EntityState::parse_bits(&mut r).unwrap();
        let got = EntityState::read_delta(&mut r, &from, num, bits).unwrap();
        assert_eq!(got.number, 5);
        assert_eq!(got.origin, [10.0, -5.0, 0.0]);
        assert_eq!(got.angles[1], 45.0);
        assert_eq!(got.frame, 3);
        assert_eq!(got.event, 0); // force-cleared
        assert_eq!(got.old_origin, from.origin); // lerped
    }
}
