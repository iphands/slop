//! Parser for the connectionless (out-of-band) `status` reply.
//!
//! The wire format, straight from `SV_StatusString` (q2repro `src/server/main.c`):
//!
//! ```text
//! \cheats\0\dmflags\17424\fraglimit\0\mapname\q2dm7\maxclients\64\timelimit\10
//! 15 45 "PlayerOne"
//! -2 16 "Some Bot"
//! ```
//!
//! Line 1 is the serverinfo infostring; every following line is
//! `<frags> <ping> "<name>"`. Names may contain spaces, so the name is taken as
//! the quoted span, not as a whitespace-delimited token.
//!
//! What is deliberately *absent*: client numbers and addresses. `SV_StatusString`
//! emits only frags/ping/name, so kicking a player still needs the RCON `status`
//! table. See `status_cache` for how the two are merged.

use std::collections::BTreeMap;

use crate::status::StatusParseError;

/// A player as the OOB status reply describes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OobPlayer {
    pub frags: i32,
    pub ping: i32,
    pub name: String,
}

/// A parsed OOB `status` reply.
#[derive(Debug, Clone, Default)]
pub struct OobStatus {
    pub info: BTreeMap<String, String>,
    pub players: Vec<OobPlayer>,
}

impl OobStatus {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.info.get(key).map(String::as_str)
    }

    pub fn get_int(&self, key: &str) -> Option<i32> {
        self.get(key)?.trim().parse().ok()
    }
}

/// Parse an infostring: `\key\value\key\value`.
///
/// A trailing key with no value is dropped rather than stored as empty — a
/// truncated infostring must not look like a real `\timelimit\` of nothing.
fn parse_infostring(line: &str) -> BTreeMap<String, String> {
    let mut info = BTreeMap::new();
    // `split('\\')` on a leading-backslash string yields an empty first element.
    let mut parts = line.trim().split('\\').skip(1);
    while let (Some(key), Some(value)) = (parts.next(), parts.next()) {
        if !key.is_empty() {
            info.insert(key.to_string(), value.to_string());
        }
    }
    info
}

/// Parse one `<frags> <ping> "<name>"` player line.
fn parse_player_line(line: &str) -> Option<OobPlayer> {
    let open = line.find('"')?;
    let close = line.rfind('"')?;
    if close <= open {
        return None;
    }
    let name = line[open + 1..close].to_string();

    let mut nums = line[..open].split_whitespace();
    let frags = nums.next()?.parse().ok()?;
    let ping = nums.next()?.parse().ok()?;

    Some(OobPlayer { frags, ping, name })
}

/// Parse a full OOB `status` reply body (prefix and `print\n` header already stripped).
pub fn parse_oob_status(reply: &str) -> Result<OobStatus, StatusParseError> {
    let mut lines = reply.lines();

    let info_line = lines.next().ok_or(StatusParseError::InvalidFormat)?.trim();

    // A reply that doesn't lead with an infostring isn't an OOB status reply at
    // all — most likely the server is down and something else answered.
    if !info_line.starts_with('\\') {
        return Err(StatusParseError::InvalidFormat);
    }

    let info = parse_infostring(info_line);
    let players = lines
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter_map(parse_player_line)
        .collect();

    Ok(OobStatus { info, players })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured verbatim from noir.lan:27910.
    const REAL_REPLY: &str = "\\cheats\\0\\coop\\0\\deathmatch\\1\\dmflags\\17424\\fraglimit\\0\\gamedate\\Jul 11 2026\\gamename\\baseq2\\hostname\\HandsNet deathmatch\\mapname\\q2dm7\\maxclients\\64\\maxspectators\\4\\needpass\\0\\protocol\\34\\singleplayer\\0\\timelimit\\10\\version\\8.70 x86_64 Jul 11 2026 Linux
-9 16 \"xon_sg_trt_2\"
5 16 \"xon_sg_nob_1\"
0 16 \"xon_sg_nob_2\"";

    #[test]
    fn parses_a_real_reply() {
        let status = parse_oob_status(REAL_REPLY).unwrap();
        assert_eq!(status.get("mapname"), Some("q2dm7"));
        assert_eq!(status.get_int("timelimit"), Some(10));
        assert_eq!(status.get_int("fraglimit"), Some(0));
        assert_eq!(status.get_int("dmflags"), Some(17424));
        assert_eq!(status.get_int("maxclients"), Some(64));
        assert_eq!(status.get("hostname"), Some("HandsNet deathmatch"));
        assert_eq!(status.players.len(), 3);
    }

    #[test]
    fn parses_negative_frags() {
        let status = parse_oob_status(REAL_REPLY).unwrap();
        assert_eq!(
            status.players[0],
            OobPlayer {
                frags: -9,
                ping: 16,
                name: "xon_sg_trt_2".into()
            }
        );
    }

    #[test]
    fn parses_names_with_spaces() {
        let reply = "\\mapname\\q2dm1\n7 30 \"Player One Two\"";
        let status = parse_oob_status(reply).unwrap();
        assert_eq!(status.players[0].name, "Player One Two");
        assert_eq!(status.players[0].frags, 7);
    }

    #[test]
    fn parses_names_containing_quotes() {
        // The name is the span between the FIRST and LAST quote, so an embedded
        // quote stays part of the name instead of truncating it.
        let reply = "\\mapname\\q2dm1\n1 20 \"He said \"hi\"\"";
        let status = parse_oob_status(reply).unwrap();
        assert_eq!(status.players[0].name, "He said \"hi\"");
    }

    #[test]
    fn parses_empty_server() {
        let status = parse_oob_status("\\mapname\\q2dm1\\timelimit\\20").unwrap();
        assert_eq!(status.get("mapname"), Some("q2dm1"));
        assert!(status.players.is_empty());
    }

    #[test]
    fn missing_keys_are_none_not_zero() {
        let status = parse_oob_status("\\mapname\\q2dm1").unwrap();
        assert_eq!(status.get_int("timelimit"), None);
        assert_eq!(status.get_int("fraglimit"), None);
    }

    #[test]
    fn a_dangling_key_is_dropped_not_stored_empty() {
        let status = parse_oob_status("\\mapname\\q2dm1\\timelimit").unwrap();
        assert_eq!(status.get("timelimit"), None);
    }

    #[test]
    fn a_non_status_reply_is_an_error() {
        // e.g. an RCON error leaked into this path, or a garbage datagram.
        assert!(parse_oob_status("Bad rcon_password.").is_err());
        assert!(parse_oob_status("").is_err());
    }
}
