//! `player_state_t` — our own player's state, delta-decoded.
//!
//! Ports `pmove_state_t` (`shared.h:657`) + `CL_ParsePlayerstate` (`cl_parse.c:547`).
//! Note `pmove.origin`/`velocity` are **raw shorts** (12.3 fixed-point) for bit-accurate
//! prediction — convert with `* 0.125` for world units.

use crate::ops::{
    PS_BLEND, PS_FOV, PS_KICKANGLES, PS_M_DELTA_ANGLES, PS_M_FLAGS, PS_M_GRAVITY, PS_M_ORIGIN,
    PS_M_TIME, PS_M_TYPE, PS_M_VELOCITY, PS_RDFLAGS, PS_VIEWANGLES, PS_VIEWOFFSET, PS_WEAPONFRAME,
    PS_WEAPONINDEX,
};
use crate::{DecodeError, Reader};

/// `MAX_STATS` (`shared.h:1149`).
pub const MAX_STATS: usize = 32;

/// `pmtype_t` (`shared.h:633`): `PM_NORMAL=0, PM_SPECTATOR, PM_DEAD, PM_GIB, PM_FREEZE`.
/// `PM_FREEZE` is what every client's `pm_type` becomes during intermission
/// (`game/player/client.c:2119`) — the scoreboard after fraglimit/timelimit.
pub const PM_FREEZE: u8 = 4;

/// `pmove_state_t` — the bit-accurate movement state (no floats; raw fixed-point shorts).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PmoveState {
    pub pm_type: u8,
    /// Raw 12.3 fixed-point; `origin_f32()` gives world units.
    pub origin: [i16; 3],
    pub velocity: [i16; 3],
    pub pm_flags: u8,
    /// Each unit = 8 ms.
    pub pm_time: u8,
    pub gravity: i16,
    pub delta_angles: [i16; 3],
}

impl PmoveState {
    /// World-space origin (raw shorts × 1/8).
    pub fn origin_f32(&self) -> [f32; 3] {
        scale3(self.origin)
    }

    /// World-space velocity (raw shorts × 1/8).
    pub fn velocity_f32(&self) -> [f32; 3] {
        scale3(self.velocity)
    }
}

fn scale3(v: [i16; 3]) -> [f32; 3] {
    [
        v[0] as f32 * 0.125,
        v[1] as f32 * 0.125,
        v[2] as f32 * 0.125,
    ]
}

/// `player_state_t` — view + pmove state for our client.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerState {
    pub pmove: PmoveState,
    pub viewangles: [f32; 3],
    pub viewoffset: [f32; 3],
    pub kick_angles: [f32; 3],
    pub gunangles: [f32; 3],
    pub gunoffset: [f32; 3],
    pub gunindex: i32,
    pub gunframe: i32,
    pub blend: [f32; 4],
    pub fov: f32,
    pub rdflags: i32,
    pub stats: [i16; MAX_STATS],
}

impl PlayerState {
    /// `CL_ParsePlayerstate(oldframe, newframe)`: delta from `from` (or zero), driven by
    /// the `PS_*` flag short, then the `stats` bitmask long.
    pub fn read_delta(
        r: &mut Reader,
        from: Option<&PlayerState>,
    ) -> Result<PlayerState, DecodeError> {
        let mut s = match from {
            Some(f) => f.clone(),
            None => PlayerState::default(),
        };

        let flags = r.read_i16()? as u16;

        if flags & PS_M_TYPE != 0 {
            s.pmove.pm_type = r.read_u8()?;
        }
        if flags & PS_M_ORIGIN != 0 {
            s.pmove.origin = [r.read_i16()?, r.read_i16()?, r.read_i16()?];
        }
        if flags & PS_M_VELOCITY != 0 {
            s.pmove.velocity = [r.read_i16()?, r.read_i16()?, r.read_i16()?];
        }
        if flags & PS_M_TIME != 0 {
            s.pmove.pm_time = r.read_u8()?;
        }
        if flags & PS_M_FLAGS != 0 {
            s.pmove.pm_flags = r.read_u8()?;
        }
        if flags & PS_M_GRAVITY != 0 {
            s.pmove.gravity = r.read_i16()?;
        }
        if flags & PS_M_DELTA_ANGLES != 0 {
            s.pmove.delta_angles = [r.read_i16()?, r.read_i16()?, r.read_i16()?];
        }

        if flags & PS_VIEWOFFSET != 0 {
            s.viewoffset = read_quarter(r)?;
        }
        if flags & PS_VIEWANGLES != 0 {
            s.viewangles = [r.read_angle16()?, r.read_angle16()?, r.read_angle16()?];
        }
        if flags & PS_KICKANGLES != 0 {
            s.kick_angles = read_quarter(r)?;
        }
        if flags & PS_WEAPONINDEX != 0 {
            s.gunindex = r.read_u8()? as i32;
        }
        if flags & PS_WEAPONFRAME != 0 {
            s.gunframe = r.read_u8()? as i32;
            s.gunoffset = read_quarter(r)?;
            s.gunangles = read_quarter(r)?;
        }
        if flags & PS_BLEND != 0 {
            s.blend = [
                r.read_u8()? as f32 / 255.0,
                r.read_u8()? as f32 / 255.0,
                r.read_u8()? as f32 / 255.0,
                r.read_u8()? as f32 / 255.0,
            ];
        }
        if flags & PS_FOV != 0 {
            s.fov = r.read_u8()? as f32;
        }
        if flags & PS_RDFLAGS != 0 {
            s.rdflags = r.read_u8()? as i32;
        }

        let statbits = r.read_i32()? as u32;
        for i in 0..MAX_STATS {
            if statbits & (1u32 << i) != 0 {
                s.stats[i] = r.read_i16()?;
            }
        }

        Ok(s)
    }
}

fn read_quarter(r: &mut Reader) -> Result<[f32; 3], DecodeError> {
    Ok([
        r.read_i8()? as f32 * 0.25,
        r.read_i8()? as f32 * 0.25,
        r.read_i8()? as f32 * 0.25,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Writer;

    #[test]
    fn delta_decodes_pmove_and_stats() {
        // Encode a playerstate with M_ORIGIN + M_GRAVITY + one stat bit set.
        let mut flags: u16 = 0;
        let mut body = Writer::new();
        flags |= PS_M_ORIGIN;
        body.write_i16(80); // origin x = 80 → 10.0 units
        body.write_i16(0);
        body.write_i16(0);
        flags |= PS_M_GRAVITY;
        body.write_i16(800);
        // stats bitmask + one stat
        body.write_i32(1); // bit 0
        body.write_i16(100); // stats[0] = 100

        let mut w = Writer::new();
        w.write_i16(flags as i16);
        w.write_bytes(body.as_bytes());
        let bytes = w.freeze();
        let mut r = Reader::new(&bytes);

        let s = PlayerState::read_delta(&mut r, None).unwrap();
        assert_eq!(s.pmove.origin, [80, 0, 0]);
        assert_eq!(s.pmove.origin_f32(), [10.0, 0.0, 0.0]);
        assert_eq!(s.pmove.gravity, 800);
        assert_eq!(s.stats[0], 100);
        // unchanged fields stay default
        assert_eq!(s.fov, 0.0);
    }
}
