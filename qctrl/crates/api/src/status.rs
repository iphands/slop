use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Error type for status parsing.
#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum StatusParseError {
    #[error("Invalid status output format")]
    InvalidFormat,
    #[error("Parse error: {0}")]
    ParseError(String),
}

/// Player information from server status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Player {
    pub client_num: i32,
    pub score: i32,
    pub address: String,
    pub name: String,
    pub ping: i32,
}

/// List of players.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[deprecated]
#[allow(dead_code)]
pub struct PlayerList {
    pub players: Vec<Player>,
}

/// Full status response including map, server settings, players, and the map clock.
#[derive(Debug, Clone, Serialize)]
pub struct StatusResponse {
    pub map: Option<String>,
    pub dmflags: Option<i32>,
    pub timelimit: Option<i32>,
    pub fraglimit: Option<i32>,
    pub maxclients: Option<i32>,
    pub players: Vec<Player>,
    /// How long the current map has been running — or an explicit "we don't know".
    /// See `crate::clock`.
    pub clock: crate::clock::MapClock,
    /// False when the status poller cannot reach the server.
    pub server_online: bool,
    /// Health of the qbots serverframe beacon link. See `crate::frames`.
    pub beacon: BeaconStatus,
}

/// Whether the qbots beacon socket is actually there and feeding us.
///
/// Deliberately separate from `MapClock`: the clock is about *time*, this is about the
/// *link*. And the two really are independent — the anchor stays valid (and keeps ticking
/// correctly) long after the socket drops, so a UI that inferred "beacon down" from a stale
/// clock would be wrong, and one that inferred "clock bad" from a dropped socket would be
/// wrong too.
#[derive(Debug, Clone, Serialize)]
pub struct BeaconStatus {
    /// Is a `frames.socket_path` configured at all? When false, nothing else here means
    /// anything — the feature does not exist in this deployment, and the UI should say
    /// nothing rather than cry "disconnected" about something nobody asked for.
    pub enabled: bool,
    /// Is the unix socket currently open?
    pub connected: bool,
    /// Bots feeding the beacon, as of the last line. A connected socket with zero bots means
    /// qbots is running but has no bots on the server.
    pub bots: u32,
    /// Age of the last line we accepted. `None` if we have never had one.
    pub last_frame_age_seconds: Option<u32>,
}

/// The map/cvar/player fields of a `status` reply, without the clock.
///
/// `parse_status_output` produces this. It is no longer the whole API response:
/// the OOB poll supplies the same fields more cheaply, and RCON `status` is now
/// read only for the columns OOB lacks (client numbers and addresses).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedStatus {
    pub map: Option<String>,
    pub dmflags: Option<i32>,
    pub timelimit: Option<i32>,
    pub fraglimit: Option<i32>,
    pub maxclients: Option<i32>,
    pub players: Vec<Player>,
}

/// Parse the RCON `status` command output.
pub fn parse_status_output(output: &str) -> Result<ParsedStatus, StatusParseError> {
    let mut map: Option<String> = None;
    let mut players = Vec::new();
    let lines: Vec<&str> = output.lines().collect();

    // First pass: look for map line
    for line in &lines {
        let line = line.trim();
        if line.starts_with("map") && line.contains(':') {
            // Extract map name after the colon
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() > 1 {
                let map_name = parts[1].trim();
                if !map_name.is_empty() {
                    map = Some(map_name.to_string());
                }
            }
        }
    }

    // Second pass: parse server settings (dmflags, timelimit, fraglimit, maxclients)
    let mut dmflags: Option<i32> = None;
    let mut timelimit: Option<i32> = None;
    let mut fraglimit: Option<i32> = None;
    let mut maxclients: Option<i32> = None;

    for line in &lines {
        let line = line.trim();
        // Look for serverinfo line with \key\value pairs (Quake 2 serverinfo format)
        // Handle both literal backslashes and escaped backslashes
        let search_line = line.replace("\\\\", "\\");

        if search_line.contains(r"\dmflags\") {
            if let Some(start) = search_line.find(r"\dmflags\") {
                let rest = &search_line[start + 9..];
                let value_end = rest.find('\\').unwrap_or(rest.len());
                if value_end > 0 {
                    if let Ok(val) = rest[..value_end].trim().parse() {
                        dmflags = Some(val);
                    }
                }
            }
        }
        if search_line.contains(r"\timelimit\") {
            if let Some(start) = search_line.find(r"\timelimit\") {
                let rest = &search_line[start + 11..];
                let value_end = rest.find('\\').unwrap_or(rest.len());
                if value_end > 0 {
                    if let Ok(val) = rest[..value_end].trim().parse() {
                        timelimit = Some(val);
                    }
                }
            }
        }
        if search_line.contains(r"\fraglimit\") {
            if let Some(start) = search_line.find(r"\fraglimit\") {
                let rest = &search_line[start + 11..];
                let value_end = rest.find('\\').unwrap_or(rest.len());
                if value_end > 0 {
                    if let Ok(val) = rest[..value_end].trim().parse() {
                        fraglimit = Some(val);
                    }
                }
            }
        }
        if search_line.contains(r"\maxclients\") {
            if let Some(start) = search_line.find(r"\maxclients\") {
                let rest = &search_line[start + 12..];
                let value_end = rest.find('\\').unwrap_or(rest.len());
                if value_end > 0 {
                    if let Ok(val) = rest[..value_end].trim().parse() {
                        maxclients = Some(val);
                    }
                }
            }
        }
    }

    // Third pass: parse players (existing logic)
    let mut in_players = false;
    let mut found_header = false;
    let mut saw_unparsed_line = false;
    for line in &lines {
        let line = line.trim();

        // Debug: log each line being processed
        tracing::debug!("Processing line: '{}'", line);

        // Find the header line and start parsing from the next line
        if line.starts_with("num") && line.contains("score") {
            tracing::debug!("Found player header line");
            in_players = true;
            found_header = true;
            continue;
        }

        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Skip separator lines (yquake2 format)
        if in_players && line.starts_with("---") {
            tracing::debug!("Skipping separator line");
            continue;
        }

        // Stop at footer
        if line.starts_with("-----") || line.starts_with("connection") {
            tracing::debug!("Reached footer, stopping player parsing");
            break;
        }

        if in_players {
            tracing::debug!("Attempting to parse player line: '{}'", line);
            if let Some(player) = parse_player_line(line) {
                tracing::debug!("Successfully parsed player: {}", player.name);
                players.push(player);
            } else {
                saw_unparsed_line = true;
                tracing::debug!(
                    "Failed to parse player line (likely not a player line): '{}'",
                    line
                );
            }
        }
    }

    // Only warn if there were actual data rows we couldn't parse — an empty
    // server prints the header + separator with no rows, which is normal.
    if found_header && players.is_empty() && saw_unparsed_line {
        tracing::warn!("Found player header but data rows failed to parse - check format!");
    }

    // Sort by score (descending)
    use std::cmp::Reverse;
    players.sort_by_key(|p| Reverse(p.score));

    tracing::debug!(
        "Parsed status: map={:?}, players count={}",
        map,
        players.len()
    );
    for player in &players {
        tracing::debug!(
            "  Player: {} (client_num={}, score={}, ping={})",
            player.name,
            player.client_num,
            player.score,
            player.ping
        );
    }

    Ok(ParsedStatus {
        map,
        dmflags,
        timelimit,
        fraglimit,
        maxclients,
        players,
    })
}

/// Parse a single player line from status output.
fn parse_player_line(line: &str) -> Option<Player> {
    // q2pro format: "num score ping name lastmsg address qport"
    // yquake2 format: "num score ping name lastmsg address qport" (with multi-word names)
    // yquake2 with brackets: " 0     0   11 RPI2                  9 [192.168.11.199]:4443842841"
    // Example: " 0    15    45  PlayerName     0  192.168.1.100:27  27"
    // Example: " 0    15    45 Player One    0  192.168.1.100:27     27"

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 6 {
        return None;
    }

    let client_num = i32::from_str(parts[0]).ok()?;
    let score = i32::from_str(parts[1]).ok()?;
    let ping = i32::from_str(parts[2]).ok()?;

    // Find the address by looking for a part containing "[" or ":" that looks like IP:port
    // Address can be: "192.168.1.100:27" or "[192.168.11.199]:4443842841"
    let mut address_idx = None;
    for (idx, part) in parts.iter().enumerate().skip(3) {
        // Look for IP address patterns: contains "[" or looks like IP:port
        if part.contains('[')
            || (part.contains(':') && part.chars().filter(|c| c.is_ascii_digit()).count() > 5)
        {
            address_idx = Some(idx);
            break;
        }
    }

    let address_idx = address_idx?;
    if address_idx < 5 {
        return None;
    }

    let address = parts[address_idx].to_string();

    // Name is everything from index 3 to address_idx - 1 (excluding lastmsg which is right before address)
    // lastmsg is at address_idx - 1
    // qport is the last element (if present)
    let name_parts: Vec<&str> = parts[3..address_idx - 1].to_vec();
    let name = name_parts.join(" ");

    Some(Player {
        client_num,
        score,
        address,
        name,
        ping,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_status_output() {
        // q2pro format: num score ping name lastmsg address qport
        let output = r#"
--- server status ----------------------------------------------------------------
 rate loss    checks   mss  port    client            idletm   ping
    0.0    0.0       0   512   27015 192.168.1.100:27910    0.0     45
    0.0    0.0       0   512   27015 192.168.1.101:27910    0.0     78

 num score ping   name            lastmsg address              qport
   0    15    45   PlayerOne           0  192.168.1.100:27     27
   1     8    78   PlayerTwo           0  192.168.1.101:27     27
---------------------------------------------------------------------------
map              : q2dm1
\dmflags\256\timelimit\20\fraglimit\50
"#;

        let result = parse_status_output(output).unwrap();
        assert_eq!(result.map, Some("q2dm1".to_string()));
        assert_eq!(result.players.len(), 2);
        assert_eq!(result.players[0].name, "PlayerOne");
        assert_eq!(result.players[0].score, 15);
        assert_eq!(result.players[0].ping, 45);
        assert_eq!(result.players[0].address, "192.168.1.100:27");
        assert_eq!(result.players[1].name, "PlayerTwo");
        assert_eq!(result.players[1].score, 8);
        assert_eq!(result.players[1].ping, 78);
    }

    #[test]
    fn test_parse_empty_status() {
        let output = "No players connected\nmap              : test_map";
        let result = parse_status_output(output).unwrap();
        assert_eq!(result.map, Some("test_map".to_string()));
        assert_eq!(result.dmflags, None);
        assert_eq!(result.timelimit, None);
        assert_eq!(result.fraglimit, None);
        assert_eq!(result.maxclients, None);
        assert_eq!(result.players.len(), 0);
    }

    #[test]
    fn test_parse_serverinfo() {
        let output = r#"
--- server status ----------------------------------------------------------------
map              : q2dm1
\dmflags\256\timelimit\20\fraglimit\50\maxclients\64
"#;

        let result = parse_status_output(output).unwrap();
        assert_eq!(result.map, Some("q2dm1".to_string()));
        assert_eq!(result.dmflags, Some(256));
        assert_eq!(result.timelimit, Some(20));
        assert_eq!(result.fraglimit, Some(50));
        assert_eq!(result.maxclients, Some(64));
        assert_eq!(result.players.len(), 0);
    }

    #[test]
    fn test_parse_yquake2_status() {
        let output = r#"
--- server status ----------------------------------------------------------------
 num score ping name            lastmsg address               qport 
--- ----- ---- --------------- ------- --------------------- ------
 0    15    45 Player One          0  192.168.1.100:27     27
 1     8    78 Player Two          0  192.168.1.101:27     27
---------------------------------------------------------------------------
map              : q2dm1
\dmflags\256\timelimit\20\fraglimit\50
"#;

        let result = parse_status_output(output).unwrap();
        assert_eq!(result.map, Some("q2dm1".to_string()));
        assert_eq!(result.players.len(), 2);
        assert_eq!(result.players[0].name, "Player One");
        assert_eq!(result.players[0].score, 15);
        assert_eq!(result.players[0].ping, 45);
        assert_eq!(result.players[0].address, "192.168.1.100:27");
    }
}
