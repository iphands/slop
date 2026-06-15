//! Parsing server→client (`svc_*`) messages.
//!
//! Handles the connection-phase messages the handshake needs (`serverdata`,
//! `configstring`, `stufftext`, `print`, `disconnect`/`reconnect`/`nop`). Frame-level
//! ops (`playerinfo`, `packetentities`, `frame`) and `spawnbaseline` are returned as
//! [`SvcEvent::Unhandled`]; full decode lands in Plan 04 (it needs the entity_state
//! delta decoder). The caller stops at the first `Unhandled` — enough to reach the
//! "bot connected" milestone while staying alive via `clc_move`.

use q2proto::{DecodeError, Reader, SvcOp};

/// Total configstring slots (computed from the CS_* chain in `shared.h:1193-1210`):
/// `CS_GENERAL(1568) + MAX_GENERAL(512) = 2080`.
pub const MAX_CONFIGSTRINGS: usize = 2080;

/// `svc_serverdata` payload — parsed from `CL_ParseServerData` (`cl_parse.c:887`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerData {
    pub protocol: i32,
    pub servercount: i32,
    pub attractloop: bool,
    pub gamedir: String,
    /// Our client/entity number (`-1` means a cinematic, not a level).
    pub playernum: i16,
    pub levelname: String,
}

impl ServerData {
    /// Read a `svc_serverdata` body (the opcode has already been consumed).
    pub fn read(r: &mut Reader) -> Result<Self, DecodeError> {
        let protocol = r.read_i32()?;
        let servercount = r.read_i32()?;
        let attractloop = r.read_u8()? != 0;
        let gamedir = r.read_string()?;
        let playernum = r.read_i16()?;
        let levelname = r.read_string()?;
        Ok(Self {
            protocol,
            servercount,
            attractloop,
            gamedir,
            playernum,
            levelname,
        })
    }
}

/// The server's configstring table (models, sounds, statusbar, player skins, …).
#[derive(Debug, Clone, Default)]
pub struct ConfigStrings {
    slots: Vec<String>,
}

impl ConfigStrings {
    /// `svc_configstring` body: a short index + a NUL-terminated string.
    pub fn set_from_reader(&mut self, r: &mut Reader) -> Result<(usize, String), DecodeError> {
        let index = r.read_i16()?;
        if !(0..MAX_CONFIGSTRINGS as i16).contains(&index) {
            return Err(DecodeError::Invalid("configstring index"));
        }
        let value = r.read_string()?;
        let index = index as usize;
        if index >= self.slots.len() {
            self.slots.resize_with(index + 1, String::new);
        }
        self.slots[index] = value.clone();
        Ok((index, value))
    }

    /// Look up a configstring by index.
    pub fn get(&self, index: usize) -> Option<&str> {
        self.slots.get(index).map(String::as_str)
    }

    /// Store a configstring by index (out of range is dropped).
    pub fn set(&mut self, index: usize, value: impl Into<String>) {
        if index >= MAX_CONFIGSTRINGS {
            return;
        }
        if index >= self.slots.len() {
            self.slots.resize_with(index + 1, String::new);
        }
        self.slots[index] = value.into();
    }

    /// Iterate over all (index, value) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &str)> {
        self.slots.iter().enumerate().filter_map(|(i, s)| {
            if s.is_empty() {
                None
            } else {
                Some((i, s.as_str()))
            }
        })
    }
}

/// One parsed `svc_*` message.
#[derive(Debug, Clone)]
pub enum SvcEvent {
    ServerData(ServerData),
    ConfigString {
        index: usize,
        value: String,
    },
    StuffText(String),
    Print {
        level: u8,
        text: String,
    },
    Disconnect,
    Reconnect,
    Nop,
    /// An opcode we don't yet decode (frame/spawnbaseline/playerinfo/…) — stop parsing
    /// this payload here. Carries the raw opcode byte.
    Unhandled(u8),
}

/// Parse one `svc_*` message from the reader, advancing past it when fully handled.
pub fn parse_message(r: &mut Reader) -> Result<SvcEvent, DecodeError> {
    let raw = r.read_u8()?;
    let op = match SvcOp::from_u8(raw) {
        Some(op) => op,
        None => return Ok(SvcEvent::Unhandled(raw)),
    };
    Ok(match op {
        SvcOp::Nop => SvcEvent::Nop,
        SvcOp::Disconnect => SvcEvent::Disconnect,
        SvcOp::Reconnect => SvcEvent::Reconnect,
        SvcOp::Print => {
            let level = r.read_u8()?;
            let text = r.read_string()?;
            SvcEvent::Print { level, text }
        }
        SvcOp::Stufftext => {
            // The server's lines are `\n`-terminated; strip a trailing newline.
            let mut s = r.read_string()?;
            while s.ends_with('\n') {
                s.pop();
            }
            SvcEvent::StuffText(s)
        }
        SvcOp::Serverdata => SvcEvent::ServerData(ServerData::read(r)?),
        SvcOp::Configstring => {
            // index/value read together; the table isn't owned here, so return raw.
            // (We read it to advance the cursor; callers store via ConfigStrings.)
            let index = r.read_i16()?;
            let value = r.read_string()?;
            SvcEvent::ConfigString {
                index: index.max(0) as usize,
                value,
            }
        }
        // Everything else (sound, baseline, frame, …) is out of scope for the handshake.
        other => {
            // Rewind past the opcode byte so callers know we stopped right after it.
            let _ = other;
            SvcEvent::Unhandled(raw)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use q2proto::Writer;

    fn reader_of(bytes: &[u8]) -> Reader<'_> {
        Reader::new(bytes)
    }

    #[test]
    fn parses_serverdata() {
        let mut w = Writer::new();
        w.write_i32(34); // protocol
        w.write_i32(1234); // servercount
        w.write_u8(0); // attractloop
        w.write_string("baseq2");
        w.write_i16(0); // playernum
        w.write_string("q2dm1");
        let b = w.freeze();
        let mut r = reader_of(&b);
        let sd = ServerData::read(&mut r).unwrap();
        assert_eq!(sd.protocol, 34);
        assert_eq!(sd.servercount, 1234);
        assert!(!sd.attractloop);
        assert_eq!(sd.gamedir, "baseq2");
        assert_eq!(sd.playernum, 0);
        assert_eq!(sd.levelname, "q2dm1");
    }

    #[test]
    fn parses_configstring_and_stores() {
        // opcode + short index + string
        let mut w = Writer::new();
        w.write_u8(SvcOp::Configstring.into());
        w.write_i16(32); // CS_MODELS
        w.write_string("maps/q2dm1.bsp");
        let b = w.freeze();
        let mut r = reader_of(&b);
        let ev = parse_message(&mut r).unwrap();
        match ev {
            SvcEvent::ConfigString { index, value } => {
                assert_eq!(index, 32);
                assert_eq!(value, "maps/q2dm1.bsp");
            }
            other => panic!("unexpected {other:?}"),
        }

        // store into the table
        let mut cs = ConfigStrings::default();
        let mut r2 = reader_of(&b[1..]); // skip opcode
        let (idx, val) = cs.set_from_reader(&mut r2).unwrap();
        assert_eq!(idx, 32);
        assert_eq!(val, "maps/q2dm1.bsp");
        assert_eq!(cs.get(32), Some("maps/q2dm1.bsp"));
    }

    #[test]
    fn parses_stufftext_and_strips_newline() {
        let mut w = Writer::new();
        w.write_u8(SvcOp::Stufftext.into());
        w.write_string("precache\n");
        let b = w.freeze();
        let mut r = reader_of(&b);
        match parse_message(&mut r).unwrap() {
            SvcEvent::StuffText(s) => assert_eq!(s, "precache"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn unhandled_frame_stops_after_opcode() {
        // svc_frame is out of scope for the handshake → Unhandled, cursor right after op.
        let mut w = Writer::new();
        w.write_u8(SvcOp::Frame.into());
        w.write_i32(1); // serverframe (unread)
        let b = w.freeze();
        let mut r = reader_of(&b);
        match parse_message(&mut r).unwrap() {
            SvcEvent::Unhandled(op) => assert_eq!(op, SvcOp::Frame as u8),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(r.pos(), 1); // only the opcode consumed
    }
}
