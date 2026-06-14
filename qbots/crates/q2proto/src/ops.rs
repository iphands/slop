//! Protocol opcodes and delta-compression bitmasks (protocol 34).
//!
//! Ported from yquake2 `src/common/header/common.h`:
//! - `PROTOCOL_VERSION` (line 185).
//! - `enum svc_ops_e` (line 199) / `enum clc_ops_e` (line 231) → [`SvcOp`] / [`ClcOp`].
//! - `CM_*` (line 265) — `usercmd_t` delta bits.
//! - `PS_*` (line 243) — `player_state_t` delta bits.
//! - `SND_*` (line 277) — `svc_sound` bits.
//! - `U_*`  (line 291) — `entity_state_t` delta bits.

/// Q2 network protocol version (`common.h:185`).
pub const PROTOCOL_VERSION: i32 = 34;

// =============================== server → client ===============================

/// Server→client message opcodes. Ports `enum svc_ops_e` (`common.h:199`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvcOp {
    Bad = 0,
    /// Known to the game DLL.
    Muzzleflash = 1,
    Muzzleflash2 = 2,
    TempEntity = 3,
    Layout = 4,
    Inventory = 5,
    /// Private to client/server.
    Nop = 6,
    Disconnect = 7,
    Reconnect = 8,
    Sound = 9,
    /// `[byte] id [string]`.
    Print = 10,
    /// `[string]` stuffed into the console buffer.
    Stufftext = 11,
    /// `[long] protocol ...`.
    Serverdata = 12,
    /// `[short] [string]`.
    Configstring = 13,
    Spawnbaseline = 14,
    /// `[string]` center of screen.
    Centerprint = 15,
    /// `[short] size [size bytes]`.
    Download = 16,
    /// variable.
    Playerinfo = 17,
    /// `[...]`.
    Packetentities = 18,
    Deltapacketentities = 19,
    Frame = 20,
}

impl SvcOp {
    /// Decode a raw opcode byte; `None` for values outside the known set.
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Bad,
            1 => Self::Muzzleflash,
            2 => Self::Muzzleflash2,
            3 => Self::TempEntity,
            4 => Self::Layout,
            5 => Self::Inventory,
            6 => Self::Nop,
            7 => Self::Disconnect,
            8 => Self::Reconnect,
            9 => Self::Sound,
            10 => Self::Print,
            11 => Self::Stufftext,
            12 => Self::Serverdata,
            13 => Self::Configstring,
            14 => Self::Spawnbaseline,
            15 => Self::Centerprint,
            16 => Self::Download,
            17 => Self::Playerinfo,
            18 => Self::Packetentities,
            19 => Self::Deltapacketentities,
            20 => Self::Frame,
            _ => return None,
        })
    }
}

impl From<SvcOp> for u8 {
    fn from(op: SvcOp) -> u8 {
        op as u8
    }
}

// =============================== client → server ===============================

/// Client→server message opcodes. Ports `enum clc_ops_e` (`common.h:231`).
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClcOp {
    Bad = 0,
    Nop = 1,
    /// `[usercmd_t]`.
    Move = 2,
    /// `[userinfo string]`.
    Userinfo = 3,
    /// `[string]`.
    Stringcmd = 4,
}

impl ClcOp {
    /// Decode a raw opcode byte; `None` for values outside the known set.
    pub fn from_u8(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Bad,
            1 => Self::Nop,
            2 => Self::Move,
            3 => Self::Userinfo,
            4 => Self::Stringcmd,
            _ => return None,
        })
    }
}

impl From<ClcOp> for u8 {
    fn from(op: ClcOp) -> u8 {
        op as u8
    }
}

// =============================== usercmd delta bits ============================
// `CM_*` — written as a single byte in `MSG_WriteDeltaUsercmd` (`movemsg.c:691`).

pub const CM_ANGLE1: u8 = 1 << 0;
pub const CM_ANGLE2: u8 = 1 << 1;
pub const CM_ANGLE3: u8 = 1 << 2;
pub const CM_FORWARD: u8 = 1 << 3;
pub const CM_SIDE: u8 = 1 << 4;
pub const CM_UP: u8 = 1 << 5;
pub const CM_BUTTONS: u8 = 1 << 6;
pub const CM_IMPULSE: u8 = 1 << 7;

// ============================== playerstate bits ===============================
// `PS_*` — `player_state_t` delta (`common.h:243`).

pub const PS_M_TYPE: u16 = 1 << 0;
pub const PS_M_ORIGIN: u16 = 1 << 1;
pub const PS_M_VELOCITY: u16 = 1 << 2;
pub const PS_M_TIME: u16 = 1 << 3;
pub const PS_M_FLAGS: u16 = 1 << 4;
pub const PS_M_GRAVITY: u16 = 1 << 5;
pub const PS_M_DELTA_ANGLES: u16 = 1 << 6;
pub const PS_VIEWOFFSET: u16 = 1 << 7;
pub const PS_VIEWANGLES: u16 = 1 << 8;
pub const PS_KICKANGLES: u16 = 1 << 9;
pub const PS_BLEND: u16 = 1 << 10;
pub const PS_FOV: u16 = 1 << 11;
pub const PS_WEAPONINDEX: u16 = 1 << 12;
pub const PS_WEAPONFRAME: u16 = 1 << 13;
pub const PS_RDFLAGS: u16 = 1 << 14;

// =============================== sound bits ===================================
// `SND_*` — `svc_sound` (`common.h:277`).

pub const SND_VOLUME: u8 = 1 << 0;
pub const SND_ATTENUATION: u8 = 1 << 1;
pub const SND_POS: u8 = 1 << 2;
pub const SND_ENT: u8 = 1 << 3;
pub const SND_OFFSET: u8 = 1 << 4;

// ============================ entity-state bits ==============================
// `U_*` — `entity_state_t` delta (`common.h:291`). Note bit 13 is intentionally
// unused in the source.

pub const U_ORIGIN1: u32 = 1 << 0;
pub const U_ORIGIN2: u32 = 1 << 1;
pub const U_ANGLE2: u32 = 1 << 2;
pub const U_ANGLE3: u32 = 1 << 3;
pub const U_FRAME8: u32 = 1 << 4;
pub const U_EVENT: u32 = 1 << 5;
pub const U_REMOVE: u32 = 1 << 6;
pub const U_MOREBITS1: u32 = 1 << 7;
pub const U_NUMBER16: u32 = 1 << 8;
pub const U_ORIGIN3: u32 = 1 << 9;
pub const U_ANGLE1: u32 = 1 << 10;
pub const U_MODEL: u32 = 1 << 11;
pub const U_RENDERFX8: u32 = 1 << 12;
pub const U_EFFECTS8: u32 = 1 << 14;
pub const U_MOREBITS2: u32 = 1 << 15;
pub const U_SKIN8: u32 = 1 << 16;
pub const U_FRAME16: u32 = 1 << 17;
pub const U_RENDERFX16: u32 = 1 << 18;
pub const U_EFFECTS16: u32 = 1 << 19;
pub const U_MODEL2: u32 = 1 << 20;
pub const U_MODEL3: u32 = 1 << 21;
pub const U_MODEL4: u32 = 1 << 22;
pub const U_MOREBITS3: u32 = 1 << 23;
pub const U_OLDORIGIN: u32 = 1 << 24;
pub const U_SKIN16: u32 = 1 << 25;
pub const U_SOUND: u32 = 1 << 26;
pub const U_SOLID: u32 = 1 << 27;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svc_op_round_trip() {
        assert_eq!(SvcOp::from_u8(20), Some(SvcOp::Frame));
        assert_eq!(SvcOp::from_u8(0), Some(SvcOp::Bad));
        assert_eq!(SvcOp::from_u8(21), None);
        assert_eq!(u8::from(SvcOp::Frame), 20);
    }

    #[test]
    fn clc_op_round_trip() {
        assert_eq!(ClcOp::from_u8(2), Some(ClcOp::Move));
        assert_eq!(ClcOp::from_u8(4), Some(ClcOp::Stringcmd));
        assert_eq!(ClcOp::from_u8(5), None);
        assert_eq!(u8::from(ClcOp::Userinfo), 3);
    }

    #[test]
    fn flag_values_match_source() {
        assert_eq!(CM_IMPULSE, 0x80);
        assert_eq!(PS_RDFLAGS, 1 << 14);
        assert_eq!(U_SOLID, 1 << 27);
        assert_eq!(SND_ENT, 1 << 3);
    }

    #[test]
    fn protocol_version_is_34() {
        assert_eq!(PROTOCOL_VERSION, 34);
    }
}
