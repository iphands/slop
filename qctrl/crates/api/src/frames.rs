//! The serverframe beacon — the map clock's only *measured* input (Plan 13).
//!
//! # Why this exists
//!
//! Everything else feeding [`crate::clock`] is inference: poll the map name, watch for an
//! edge, guess when the map started. This is not. yquake2 zeroes `sv.framenum` on every map
//! spawn (`memset(&sv, 0, sizeof(sv))`, `sv_init.c:267`), advances it at exactly 10 Hz
//! (`sv.framenum++; sv.time = sv.framenum * 100`, `sv_main.c:343`), and writes it to **every
//! connected client every frame** (`svc_frame`, `sv_entities.c:425`).
//!
//! So `serverframe / 10` **is** the age of the running map, to the frame.
//!
//! qctrl cannot see it. It speaks RCON and the connectionless OOB `status` query, and neither
//! carries a frame counter — hence `clock.rs`'s (entirely correct) claim that there is nothing
//! to read. But a *client* has the number for free, and qbots has up to 32 of them, already
//! decoding this exact field and discarding it.
//!
//! This module reads it off a unix socket qbots optionally publishes (its Plan 66). qctrl
//! therefore learns the exact map age **without connecting a Q2 client of its own** — no player
//! slot, no scoreboard entry, no handshake.
//!
//! # It is a source, never a dependency
//!
//! Absent the socket, nothing here is spawned and the clock behaves exactly as it did before:
//! the map-edge inference, the `sv_maplist` watchdog, all of it. The beacon only ever *improves*
//! the anchor.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;

/// Wire schema we speak. qbots stamps every line with its own; a mismatch is rejected loudly
/// rather than mis-parsed, because a silently misread frame counter would produce a confidently
/// wrong countdown — the exact failure this whole plan exists to eliminate.
pub const SCHEMA_VERSION: u32 = 1;

/// A line longer than this is not a beacon; the peer is confused, so we drop the connection and
/// reconnect. (Note this bounds what we *process*, not what a pathological peer could make us
/// buffer — the peer is our own qbots over a local socket, so that is the right tradeoff.)
const MAX_LINE_BYTES: usize = 8 * 1024;

/// One decoded beacon line.
///
/// Unknown fields are **ignored**, not rejected: a newer qbots may add fields within schema v1,
/// and a qctrl that refused to start over an unrecognised key would be needlessly brittle across
/// two independently-versioned repos.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Beacon {
    pub v: u32,
    /// The resolved `ip:port` the bots are actually connected to.
    pub server: String,
    /// The server as *named* in qbots' config, e.g. `noir.lan:27910`.
    #[serde(default)]
    pub server_name: String,
    pub map: String,
    pub servercount: i32,
    pub serverframe: i32,
    /// How stale the reading already was when it left qbots. This is why qbots' 1 Hz publish
    /// rate costs **zero** accuracy — we subtract it back out when deriving the anchor.
    #[serde(default)]
    pub age_ms: u64,
    #[serde(default)]
    pub bots: u32,
    /// Advances by exactly 1 per published line, so a gap means we dropped one.
    #[serde(default)]
    pub seq: u64,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BeaconError {
    #[error("malformed beacon line: {0}")]
    Malformed(String),
    #[error("unsupported beacon schema v{got} (this qctrl speaks v{SCHEMA_VERSION})")]
    Version { got: u32 },
    #[error("nonsensical beacon: {0}")]
    Nonsense(&'static str),
}

/// Parse one NDJSON beacon line.
///
/// Pure, so it is tested against canned strings and never a socket — the same discipline as
/// `parse_oob_status` / `parse_status_output` / `parse_cvar_echo`.
pub fn parse_beacon_line(line: &str) -> Result<Beacon, BeaconError> {
    let beacon: Beacon =
        serde_json::from_str(line.trim()).map_err(|e| BeaconError::Malformed(e.to_string()))?;

    if beacon.v != SCHEMA_VERSION {
        return Err(BeaconError::Version { got: beacon.v });
    }
    if beacon.serverframe < 0 {
        return Err(BeaconError::Nonsense("negative serverframe"));
    }
    if beacon.map.is_empty() {
        // qbots is supposed to withhold these, but a frame with no map cannot be attributed to
        // a map, and attributing it to the wrong one is worse than ignoring it.
        return Err(BeaconError::Nonsense("empty map"));
    }
    Ok(beacon)
}

/// Is this beacon about the server **we** manage?
///
/// Without this check, a qbots fleet pointed at a *different* Q2 server — a test box, a second
/// instance — would silently drive this qctrl's map clock with a foreign map's age. The countdown
/// would look perfectly healthy and be entirely wrong, and the rotator would act on it.
///
/// Matched on the resolved socket address, with the configured name as a fallback: the two sides
/// routinely spell the same host differently (`noir.lan:27910` vs `192.168.1.10:27910`), and DNS
/// may hand back several addresses.
pub fn beacon_matches(beacon: &Beacon, want_addrs: &[SocketAddr], want_name: &str) -> bool {
    if want_addrs.iter().any(|a| a.to_string() == beacon.server) {
        return true;
    }
    !want_name.is_empty()
        && (beacon.server_name.eq_ignore_ascii_case(want_name)
            || beacon.server.eq_ignore_ascii_case(want_name))
}

/// Where the beacon comes from.
///
/// A seam, not a flourish: a unix socket requires qbots on the same host, and the day that stops
/// being true the fix must be a new variant here — **not** a change to the clock.
#[derive(Debug, Clone)]
pub enum Transport {
    Unix(PathBuf),
}

impl Transport {
    async fn connect(&self) -> std::io::Result<BufReader<UnixStream>> {
        match self {
            Transport::Unix(path) => Ok(BufReader::new(UnixStream::connect(path).await?)),
        }
    }

    fn describe(&self) -> String {
        match self {
            Transport::Unix(path) => path.display().to_string(),
        }
    }
}

/// Backoff schedule for reconnecting to the beacon.
#[derive(Debug, Clone, Copy)]
pub struct Backoff {
    pub min: Duration,
    pub max: Duration,
}

impl Backoff {
    /// Double, capped. Used by the reader loop so a qbots that is simply not running yet costs a
    /// slow retry rather than a hot spin.
    pub fn next(&self, current: Duration) -> Duration {
        (current * 2).min(self.max)
    }
}

/// What the reader does with each accepted beacon, and with the link coming and going. Keeps this
/// module free of `SharedState`, so it stays unit-testable and `main.rs` owns the wiring.
pub trait BeaconSink: Send + Sync + 'static {
    fn apply(&self, beacon: &Beacon, now: Instant);
    /// The socket opened or closed. Reported from where it is actually *known*, rather than
    /// inferred downstream from whether frames happen to be arriving — a live socket with an
    /// idle fleet looks exactly like a dead one from the outside, and they are not the same
    /// thing to anyone reading the UI.
    fn set_connected(&self, connected: bool);
}

/// Connect, read lines, reconnect forever.
///
/// Tolerates every ordering: qbots not running, qbots starting *after* qctrl, qbots restarting
/// underneath it. None of those are errors — they are the normal life of an optional feature.
pub async fn run_reader<S: BeaconSink>(
    transport: Transport,
    sink: S,
    want_addrs: Vec<SocketAddr>,
    want_name: String,
    require_match: bool,
    backoff: Backoff,
) {
    let mut delay = backoff.min;
    let mut warned_unreachable = false;

    loop {
        let stream = match transport.connect().await {
            Ok(stream) => stream,
            Err(e) => {
                // Log the first failure, then go quiet: a qbots that is simply not running must
                // not flood the log for as long as qctrl is up.
                if !warned_unreachable {
                    tracing::warn!(
                        beacon = %transport.describe(),
                        error = %e,
                        "serverframe beacon unreachable; the map clock falls back to map-edge \
                         inference. Retrying in the background."
                    );
                    warned_unreachable = true;
                }
                tokio::time::sleep(delay).await;
                delay = backoff.next(delay);
                continue;
            }
        };

        tracing::info!(beacon = %transport.describe(), "serverframe beacon connected");
        warned_unreachable = false;
        delay = backoff.min;
        sink.set_connected(true);

        read_lines(stream, &sink, &want_addrs, &want_name, require_match).await;

        sink.set_connected(false);
        tracing::warn!(
            beacon = %transport.describe(),
            "serverframe beacon disconnected; the map clock falls back to map-edge inference"
        );
    }
}

/// Pump one connection until it closes. Returns on EOF or a fatal read error.
async fn read_lines<S: BeaconSink>(
    stream: BufReader<UnixStream>,
    sink: &S,
    want_addrs: &[SocketAddr],
    want_name: &str,
    require_match: bool,
) {
    let mut lines = stream.lines();
    // The config-mismatch footgun deserves to be loud, but exactly once — otherwise a
    // misconfigured fleet writes a warning per second forever.
    let mut warned_mismatch = false;
    let mut warned_parse = false;

    loop {
        let line = match lines.next_line().await {
            Ok(Some(line)) => line,
            Ok(None) => return, // qbots exited
            Err(e) => {
                tracing::warn!(error = %e, "serverframe beacon read failed");
                return;
            }
        };
        if line.len() > MAX_LINE_BYTES {
            tracing::warn!(
                bytes = line.len(),
                "beacon line absurdly long; dropping the connection"
            );
            return;
        }
        if line.trim().is_empty() {
            continue;
        }

        let beacon = match parse_beacon_line(&line) {
            Ok(beacon) => beacon,
            Err(e) => {
                if !warned_parse {
                    tracing::warn!(error = %e, "ignoring unparseable beacon line");
                    warned_parse = true;
                }
                continue;
            }
        };

        if require_match && !beacon_matches(&beacon, want_addrs, want_name) {
            if !warned_mismatch {
                tracing::warn!(
                    beacon_server = %beacon.server,
                    beacon_server_name = %beacon.server_name,
                    our_server = %want_name,
                    "IGNORING a serverframe beacon for a DIFFERENT server — the qbots fleet is \
                     pointed somewhere else. Trusting it would drive this map clock with a \
                     foreign map's age."
                );
                warned_mismatch = true;
            }
            continue;
        }

        sink.apply(&beacon, Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact line qbots emits. This string is COPIED VERBATIM from qbots'
    /// `beacon::tests::encode_pins_the_wire_format`.
    ///
    /// The two repos share no crate by design — a shared crate would be a hard build dependency
    /// between them and would defeat the whole point of an *optional* coupling. So this golden
    /// pair is the ONLY thing keeping the wire format honest. Change one, change the other, and
    /// bump `SCHEMA_VERSION` on both sides.
    const GOLDEN: &str = r#"{"v":1,"server":"192.168.1.10:27910","server_name":"noir.lan:27910","map":"q2dm1","servercount":1234,"serverframe":4210,"age_ms":250,"bots":32,"seq":99}"#;

    #[test]
    fn the_wire_format_matches_what_qbots_emits() {
        let beacon = parse_beacon_line(GOLDEN).expect("qbots' own output must parse");
        assert_eq!(beacon.v, 1);
        assert_eq!(beacon.server, "192.168.1.10:27910");
        assert_eq!(beacon.server_name, "noir.lan:27910");
        assert_eq!(beacon.map, "q2dm1");
        assert_eq!(beacon.servercount, 1234);
        assert_eq!(beacon.serverframe, 4210);
        assert_eq!(beacon.age_ms, 250);
        assert_eq!(beacon.bots, 32);
        assert_eq!(beacon.seq, 99);
    }

    /// 4210 frames at 10 Hz is 421 seconds. This is the whole idea in one assertion.
    #[test]
    fn serverframe_over_ten_is_the_map_age_in_seconds() {
        let beacon = parse_beacon_line(GOLDEN).unwrap();
        assert_eq!(beacon.serverframe / 10, 421);
    }

    /// Forward-compat: a newer qbots may add fields within schema v1. Refusing to start over an
    /// unrecognised key would be needlessly brittle across two independently-versioned repos.
    #[test]
    fn unknown_fields_are_accepted() {
        let line = r#"{"v":1,"server":"1.2.3.4:27910","map":"q2dm1","servercount":1,"serverframe":10,"something_new":"from a future qbots"}"#;
        let beacon = parse_beacon_line(line).expect("unknown fields must not break us");
        assert_eq!(beacon.map, "q2dm1");
    }

    #[test]
    fn a_future_schema_version_is_rejected_not_guessed_at() {
        let line =
            r#"{"v":2,"server":"1.2.3.4:27910","map":"q2dm1","servercount":1,"serverframe":10}"#;
        assert_eq!(
            parse_beacon_line(line),
            Err(BeaconError::Version { got: 2 })
        );
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(matches!(
            parse_beacon_line("not json at all"),
            Err(BeaconError::Malformed(_))
        ));
        assert!(matches!(
            parse_beacon_line(""),
            Err(BeaconError::Malformed(_))
        ));
        // Truncated mid-line (qbots killed while writing).
        assert!(matches!(
            parse_beacon_line(r#"{"v":1,"server":"1.2.3.4:279"#),
            Err(BeaconError::Malformed(_))
        ));
    }

    #[test]
    fn a_negative_serverframe_is_rejected() {
        let line =
            r#"{"v":1,"server":"1.2.3.4:27910","map":"q2dm1","servercount":1,"serverframe":-5}"#;
        assert_eq!(
            parse_beacon_line(line),
            Err(BeaconError::Nonsense("negative serverframe"))
        );
    }

    #[test]
    fn a_frame_with_no_map_is_rejected() {
        let line = r#"{"v":1,"server":"1.2.3.4:27910","map":"","servercount":1,"serverframe":10}"#;
        assert_eq!(
            parse_beacon_line(line),
            Err(BeaconError::Nonsense("empty map"))
        );
    }

    fn beacon_from(server: &str, server_name: &str) -> Beacon {
        Beacon {
            v: 1,
            server: server.to_string(),
            server_name: server_name.to_string(),
            map: "q2dm1".into(),
            servercount: 1,
            serverframe: 10,
            age_ms: 0,
            bots: 1,
            seq: 1,
        }
    }

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn a_beacon_for_our_resolved_address_matches() {
        let beacon = beacon_from("192.168.1.10:27910", "noir.lan:27910");
        assert!(beacon_matches(
            &beacon,
            &[addr("192.168.1.10:27910")],
            "noir.lan:27910"
        ));
    }

    /// DNS may hand back several addresses; any of ours is ours.
    #[test]
    fn a_beacon_matching_any_resolved_address_matches() {
        let beacon = beacon_from("192.168.1.11:27910", "noir.lan:27910");
        assert!(beacon_matches(
            &beacon,
            &[addr("192.168.1.10:27910"), addr("192.168.1.11:27910")],
            "noir.lan:27910"
        ));
    }

    /// The two sides routinely spell the same host differently, and qctrl may fail to resolve at
    /// all. The configured name is the fallback so a working setup isn't rejected on a spelling.
    #[test]
    fn a_beacon_matching_only_the_configured_name_still_matches() {
        let beacon = beacon_from("192.168.1.10:27910", "noir.lan:27910");
        assert!(beacon_matches(&beacon, &[], "noir.lan:27910"));
    }

    /// THE guard. A qbots fleet pointed at another server must never drive our map clock: the
    /// countdown would look healthy, be completely wrong, and the rotator would act on it.
    #[test]
    fn a_beacon_from_a_foreign_server_is_rejected() {
        let beacon = beacon_from("10.9.9.9:27910", "testbox.lan:27910");
        assert!(!beacon_matches(
            &beacon,
            &[addr("192.168.1.10:27910")],
            "noir.lan:27910"
        ));
    }

    #[test]
    fn backoff_doubles_and_caps() {
        let b = Backoff {
            min: Duration::from_millis(500),
            max: Duration::from_secs(10),
        };
        assert_eq!(b.next(b.min), Duration::from_secs(1));
        assert_eq!(b.next(Duration::from_secs(8)), Duration::from_secs(10));
        assert_eq!(
            b.next(Duration::from_secs(10)),
            Duration::from_secs(10),
            "must cap, not grow forever"
        );
    }
}
