//! Connectionless ("out-of-band") packet framing.
//!
//! Connectionless Q2 packets (the `getchallenge` / `connect` / `client_connect`
//! handshake, plus `info`/`ping` queries) are prefixed with four `0xff` bytes,
//! followed by an ASCII command line. The marker doubles as the i32 `-1`
//! (`0xFFFFFFFF`): in-band netchan packets never start with it, so it discriminates
//! the two. See `server/sv_conless.c:385` ("four leading 0xff").

use crate::Writer;

/// The four leading bytes of a connectionless packet.
pub const OOB_PREFIX: [u8; 4] = [0xff; 4];

/// The marker read as a little-endian i32 (`0xFFFFFFFF == -1`).
pub const OOB_MARKER: i32 = -1;

/// Write a connectionless command line: the 4-byte marker followed by `line` (the
/// caller includes any trailing `\n`, as Q2 commands conventionally have).
pub fn write_oob(w: &mut Writer, line: &str) {
    w.write_bytes(&OOB_PREFIX);
    w.write_bytes(line.as_bytes());
}

/// Whether `buf` begins with the connectionless marker.
pub fn is_oob(buf: &[u8]) -> bool {
    buf.first_chunk::<4>() == Some(&OOB_PREFIX)
}

/// Strip the 4-byte marker, returning the payload, or `None` if `buf` isn't OOB.
pub fn oob_payload(buf: &[u8]) -> Option<&[u8]> {
    buf.strip_prefix(OOB_PREFIX.as_slice())
}

/// Split a command line into argv, honoring double-quoted tokens (quotes removed),
/// mirroring Q2's `Cmd_TokenizeString`. Whitespace (including `\n`) separates tokens.
///
/// E.g. `connect 34 12345 67890 "\\name\\x"` →
/// `["connect", "34", "12345", "67890", "\\name\\x"]`.
pub fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        if c == '"' {
            chars.next(); // opening quote
            let mut tok = String::new();
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
                tok.push(c);
            }
            out.push(tok);
        } else {
            let mut tok = String::new();
            for c in chars.by_ref() {
                if c.is_whitespace() {
                    break;
                }
                tok.push(c);
            }
            out.push(tok);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Reader;

    #[test]
    fn write_and_detect_oob() {
        let mut w = Writer::new();
        write_oob(&mut w, "getchallenge\n");
        let bytes = w.freeze();
        assert!(is_oob(&bytes));
        assert_eq!(oob_payload(&bytes), Some(&b"getchallenge\n"[..]));
    }

    #[test]
    fn non_oob_packet_is_detected() {
        // In-band netchan packets start with a sequence number, never 0xff×4.
        assert!(!is_oob(&[0x00, 0x01, 0x02, 0x03]));
        assert_eq!(oob_payload(&[0x00, 0x01, 0x02]), None);
    }

    #[test]
    fn connect_line_tokenizes_to_four_argv() {
        let userinfo = "\\name\\qbots\\rate\\25000";
        let line = format!("connect 34 12345 67890 \"{userinfo}\"\n");
        let argv = tokenize(&line);
        assert_eq!(argv.len(), 5);
        assert_eq!(argv[0], "connect");
        assert_eq!(argv[1], "34");
        assert_eq!(argv[2], "12345");
        assert_eq!(argv[3], "67890");
        assert_eq!(argv[4], userinfo);
    }

    #[test]
    fn challenge_reply_tokenizes() {
        let argv = tokenize("challenge 98765 p=34\n");
        assert_eq!(argv, ["challenge", "98765", "p=34"]);
    }

    #[test]
    fn oob_round_trip_via_reader() {
        // A bot receives a connectionless reply; read the marker then the command line.
        let mut w = Writer::new();
        write_oob(&mut w, "client_connect\n");
        let bytes = w.freeze();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.read_i32().unwrap(), OOB_MARKER);
        let cmd = r.read_string_line().unwrap();
        assert_eq!(cmd, "client_connect");
    }
}
