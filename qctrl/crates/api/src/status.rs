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
pub struct PlayerList {
    pub players: Vec<Player>,
}

/// Full status response including map, server settings, and players.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub map: Option<String>,
    pub dmflags: Option<i32>,
    pub timelimit: Option<i32>,
    pub fraglimit: Option<i32>,
    pub players: Vec<Player>,
}

/// Parse the `status` command output into a StatusResponse.
pub fn parse_status_output(output: &str) -> Result<StatusResponse, StatusParseError> {
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

    // Second pass: parse server settings (dmflags, timelimit, fraglimit)
    let mut dmflags: Option<i32> = None;
    let mut timelimit: Option<i32> = None;
    let mut fraglimit: Option<i32> = None;

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
    }

    // Third pass: parse players (existing logic)
    let mut in_players = false;
    for line in &lines {
        let line = line.trim();

        // Find the header line and start parsing from the next line
        if line.starts_with("num") && line.contains("score") {
            in_players = true;
            continue;
        }

        // Skip empty lines
        if line.is_empty() {
            continue;
        }

        // Stop at footer
        if line.starts_with("-----") || line.starts_with("connection") {
            break;
        }

        if in_players {
            if let Some(player) = parse_player_line(line) {
                players.push(player);
            }
        }
    }

    // Sort by score (descending)
    use std::cmp::Reverse;
    players.sort_by_key(|p| Reverse(p.score));

    Ok(StatusResponse {
        map,
        dmflags,
        timelimit,
        fraglimit,
        players,
    })
}

/// Parse a single player line from status output.
fn parse_player_line(line: &str) -> Option<Player> {
    // Format: "num score address name ping"
    // Example: " 0    15   192.168.1.100:27  PlayerName     45"

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }

    let client_num = i32::from_str(parts[0]).ok()?;
    let score = i32::from_str(parts[1]).ok()?;
    let address = parts[2].to_string();
    let ping = i32::from_str(parts[parts.len() - 1]).ok()?;

    // Name is everything between address and ping
    let name = parts[3..parts.len() - 1].join(" ");

    Some(Player {
        client_num,
        score,
        address,
        name,
        ping,
    })
}

/// Parse an integer value from RCON command output like: "dmflags" is "17424"
pub fn parse_rcon_int(output: &str, command: &str) -> Option<i32> {
    // Output format: "command" is "value"
    let pattern = format!("\"{}\" is \"", command);
    if let Some(start) = output.find(&pattern) {
        let value_start = start + pattern.len();
        let rest = &output[value_start..];
        if let Some(end) = rest.find('"') {
            if let Ok(val) = rest[..end].trim().parse() {
                return Some(val);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_status_output() {
        let output = r#"
--- server status ----------------------------------------------------------------
 rate loss    checks   mss  port    client            idletm   ping
    0.0    0.0       0   512   27015 192.168.1.100:27910    0.0     45
    0.0    0.0       0   512   27015 192.168.1.101:27910    0.0     78

 num score address              name            ping
   0    15   192.168.1.100:27   PlayerOne      45
   1     8   192.168.1.101:27   PlayerTwo      78
---------------------------------------------------------------------------
map              : q2dm1
\dmflags\256\timelimit\20\fraglimit\50
"#;

        let result = parse_status_output(output).unwrap();
        assert_eq!(result.map, Some("q2dm1".to_string()));
        assert_eq!(result.players.len(), 2);
        assert_eq!(result.players[0].name, "PlayerOne");
        assert_eq!(result.players[0].score, 15);
        assert_eq!(result.players[1].name, "PlayerTwo");
        assert_eq!(result.players[1].score, 8);
    }

    #[test]
    fn test_parse_empty_status() {
        let output = "No players connected\nmap              : test_map";
        let result = parse_status_output(output).unwrap();
        assert_eq!(result.map, Some("test_map".to_string()));
        assert_eq!(result.dmflags, None);
        assert_eq!(result.timelimit, None);
        assert_eq!(result.fraglimit, None);
        assert_eq!(result.players.len(), 0);
    }

    #[test]
    fn test_parse_serverinfo() {
        let output = r#"
--- server status ----------------------------------------------------------------
map              : q2dm1
\dmflags\256\timelimit\20\fraglimit\50
"#;

        let result = parse_status_output(output).unwrap();
        assert_eq!(result.map, Some("q2dm1".to_string()));
        assert_eq!(result.dmflags, Some(256));
        assert_eq!(result.timelimit, Some(20));
        assert_eq!(result.fraglimit, Some(50));
        assert_eq!(result.players.len(), 0);
    }
}
