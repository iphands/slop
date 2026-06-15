//! OOB `status` query — the fleet verification lens (Plan 09).
//!
//! A Q2 server replies to the connectionless packet `\xff\xff\xff\xffstatus\n`
//! with `\xff\xff\xff\xffprint\n` + `SV_StatusString()` (`sv_main.c:92`):
//! the server infostring on one line (`\map\q2dm1\…\maxclients\16\…`), then one
//! line per connected client: `<frags> <ping> "<name>"`. We parse that into a
//! report so `qbots status` can confirm our bots are connected and fragging.

/// One connected client in a `status` reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerStatus {
    pub score: i32,
    pub ping: u32,
    pub name: String,
}

/// Parsed `status` reply: the interesting infostring fields + the client list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusReport {
    pub map: Option<String>,
    pub maxclients: Option<u32>,
    /// The infostring's `players` count, if the server reports one.
    pub infostring_player_count: Option<u32>,
    pub players: Vec<PlayerStatus>,
}

impl StatusReport {
    /// Number of clients in the player list (the authoritative count).
    pub fn player_count(&self) -> usize {
        self.players.len()
    }
}

/// Parse a raw OOB `status` reply packet. `None` if it isn't a valid OOB reply
/// (missing the 4-byte `\xff` prefix or the command/newline).
pub fn parse_status_response(packet: &[u8]) -> Option<StatusReport> {
    if packet.len() < 4 || !packet[..4].iter().all(|&b| b == 0xFF) {
        return None;
    }
    let rest = std::str::from_utf8(&packet[4..]).ok()?;
    // rest = "<cmd>\n<body>"; for status the cmd is "print".
    let body = rest.split_once('\n')?.1;
    Some(parse_status_body(body))
}

/// Parse the status body: an infostring line followed by player lines.
pub fn parse_status_body(body: &str) -> StatusReport {
    let mut lines = body.lines();
    let info = lines.next().unwrap_or("");
    StatusReport {
        map: infostring_value(info, "map").map(str::to_owned),
        maxclients: infostring_value(info, "maxclients").and_then(|v| v.parse().ok()),
        infostring_player_count: infostring_value(info, "players").and_then(|v| v.parse().ok()),
        players: lines.filter_map(parse_player_line).collect(),
    }
}

/// Parse one player line: `<score> <ping> "<name>"` (`sv_main.c:111`).
fn parse_player_line(line: &str) -> Option<PlayerStatus> {
    let line = line.trim();
    // splitn(3) keeps a name with internal spaces intact in the third field.
    let mut parts = line.splitn(3, ' ');
    let score: i32 = parts.next()?.parse().ok()?;
    let ping: u32 = parts.next()?.parse().ok()?;
    let mut name = parts.next()?.trim().to_string();
    // Names are quoted in the wire format; strip one surrounding pair.
    if name.len() >= 2 && name.starts_with('"') && name.ends_with('"') {
        name = name[1..name.len() - 1].to_string();
    }
    Some(PlayerStatus { score, ping, name })
}

/// Read one `\key\value\` pair from a Q2 infostring.
fn infostring_value<'a>(info: &'a str, key: &str) -> Option<&'a str> {
    let mut parts = info.split('\\').filter(|s| !s.is_empty());
    while let Some(k) = parts.next() {
        match parts.next() {
            Some(v) if k.eq_ignore_ascii_case(key) => return Some(v),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_infostring_and_players() {
        let body = "\\gamename\\baseq2\\map\\q2dm1\\maxclients\\16\\players\\3\\protocol\\34\n\
                    15 50 \"qb0\"\n\
                    8 65 \"qb1\"\n\
                    0 0 \"human\"\n";
        let r = parse_status_body(body);
        assert_eq!(r.map.as_deref(), Some("q2dm1"));
        assert_eq!(r.maxclients, Some(16));
        assert_eq!(r.infostring_player_count, Some(3));
        assert_eq!(r.player_count(), 3);
        assert_eq!(
            r.players[0],
            PlayerStatus {
                score: 15,
                ping: 50,
                name: "qb0".into()
            }
        );
        assert_eq!(r.players[1].score, 8);
        assert_eq!(r.players[2].name, "human");
    }

    #[test]
    fn parses_full_oob_packet() {
        let pkt = b"\xff\xff\xff\xffprint\n\\map\\q2dm1\\maxclients\\8\n5 40 \"qb0\"\n";
        let r = parse_status_response(pkt).unwrap();
        assert_eq!(r.map.as_deref(), Some("q2dm1"));
        assert_eq!(r.maxclients, Some(8));
        assert_eq!(r.player_count(), 1);
        assert_eq!(r.players[0].name, "qb0");
    }

    #[test]
    fn rejects_non_oob_or_malformed() {
        assert!(parse_status_response(b"hello").is_none());
        assert!(parse_status_response(b"\xff\xffstatus").is_none()); // only 2 prefix bytes
        assert!(parse_status_response(b"\xff\xff\xff\xff").is_none()); // no command/newline
    }

    #[test]
    fn name_with_spaces_preserved() {
        let body = "\\map\\q2dm1\n10 30 \"My Cool Bot\"\n";
        let r = parse_status_body(body);
        assert_eq!(r.players[0].name, "My Cool Bot");
    }

    #[test]
    fn blank_and_garbage_lines_skipped() {
        let body = "\\map\\q2dm1\n\nnot a player line\n3 20 \"qb0\"\n";
        let r = parse_status_body(body);
        assert_eq!(r.player_count(), 1, "only the valid player line counts");
    }
}
