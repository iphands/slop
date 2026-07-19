//! Splitting a read buffer into whole lines.
//!
//! The reader `pread`s a bounded chunk of a log file that nginx is still
//! appending to, so the tail of that chunk is very often a partial line. The
//! rule is absolute: **never consume bytes past the last newline.** Whatever
//! follows it is picked up next tick, once the writer has finished it.

/// Split a buffer into the complete-lines prefix and the number of bytes that
/// may be marked consumed.
///
/// Returns `(complete_lines, bytes_consumed)`. Bytes after the LAST `\n` are a
/// partial line: they are excluded from both halves of the return value, so the
/// caller's checkpoint offset never advances past them.
///
/// A buffer with no newline at all yields `(&[], 0)` — nothing is consumed, and
/// the caller retries with a larger read next tick. See
/// [`crate::chunk::PATHOLOGICAL_LINE_NOTE`] for the escape hatch that keeps
/// that from becoming a permanent stall.
pub fn split_complete_lines(buf: &[u8]) -> (&[u8], usize) {
    match buf.iter().rposition(|&b| b == b'\n') {
        Some(i) => (&buf[..=i], i + 1),
        None => (&[], 0),
    }
}

/// Why a caller must not simply loop when `split_complete_lines` returns 0.
///
/// If `bytes_consumed == 0` **and** the buffer is at the reader's read cap, a
/// single line has no newline within the cap — a corrupt or truncated file.
/// Retrying forever would stall that file and every file behind it. The reader
/// must log an error, count one parse error, and consume the whole cap.
pub const PATHOLOGICAL_LINE_NOTE: &str =
    "consumed==0 at the read cap means a line longer than the cap: skip it, do not stall";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_buffer_consumes_nothing() {
        assert_eq!(split_complete_lines(b""), (&b""[..], 0));
    }

    #[test]
    fn a_buffer_with_no_newline_consumes_nothing() {
        // The writer may still be mid-line; we must not guess where it ends.
        assert_eq!(split_complete_lines(b"partial line"), (&b""[..], 0));
    }

    #[test]
    fn a_single_complete_line_is_fully_consumed() {
        let (lines, n) = split_complete_lines(b"one\n");
        assert_eq!(lines, b"one\n");
        assert_eq!(n, 4);
    }

    #[test]
    fn a_trailing_partial_line_is_left_unconsumed() {
        let (lines, n) = split_complete_lines(b"one\ntwo\nthree-partial");
        assert_eq!(lines, b"one\ntwo\n");
        assert_eq!(n, 8);
    }

    #[test]
    fn consumed_count_always_matches_the_returned_slice() {
        // This invariant is what keeps the checkpoint offset in step with the
        // aggregates: whatever we return, we consume exactly that many bytes.
        for buf in [
            &b""[..],
            &b"\n"[..],
            &b"a\n"[..],
            &b"a\nb"[..],
            &b"a\nb\n"[..],
            &b"no newline"[..],
        ] {
            let (lines, n) = split_complete_lines(buf);
            assert_eq!(lines.len(), n, "mismatch on {buf:?}");
        }
    }

    #[test]
    fn a_lone_newline_is_a_complete_empty_line() {
        assert_eq!(split_complete_lines(b"\n"), (&b"\n"[..], 1));
    }
}
