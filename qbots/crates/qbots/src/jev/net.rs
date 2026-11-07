//! # qbots::jev::net — one HTTP/1.1 `POST`, by hand (Plan 72 T4)
//!
//! The lockfile has no HTTP stack (no `hyper`/`reqwest`/`http`), and the endpoint needs exactly
//! one verb, so this is a small client over `tokio::net::TcpStream`:
//!
//! - **`Connection: close` always** — one connection per request. It deletes the whole
//!   keep-alive-pool bug class for ~zero cost on a LAN (the endpoint is loopback on the dev box).
//! - **One deadline wraps resolve + connect + write + read.** A hung box costs at most `deadline`.
//! - Response framing: `Content-Length`, `Transfer-Encoding: chunked`, or close-delimited (read to
//!   EOF — legal because we sent `Connection: close`). Bodies over [`MAX_BODY`] are refused.
//! - A non-200 status is returned as soon as the status line is read.

use std::time::Duration;

use brain::jev::decision::{MalformedKind, TransportErr};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Response body cap (1 MiB). A real answer is a few hundred bytes.
pub const MAX_BODY: usize = 1 << 20;
/// Header block cap.
const MAX_HEAD: usize = 16 << 10;

/// A `host:port` endpoint (from config — never from the wire).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// Host name or IP literal.
    pub host: String,
    /// TCP port.
    pub port: u16,
}

impl Endpoint {
    /// Parse `host:port`.
    pub fn parse(s: &str) -> Result<Self, String> {
        let (host, port) = s
            .rsplit_once(':')
            .ok_or_else(|| format!("endpoint '{s}' is not host:port"))?;
        let port = port
            .parse()
            .map_err(|_| format!("endpoint '{s}': bad port '{port}'"))?;
        if host.is_empty() {
            return Err(format!("endpoint '{s}': empty host"));
        }
        Ok(Self {
            host: host.to_string(),
            port,
        })
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.host, self.port)
    }
}

/// Why a `POST` produced no 200 body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostError {
    /// Nothing usable came back.
    Transport(TransportErr),
    /// Bytes came back but broke HTTP framing or the size cap.
    Malformed(MalformedKind),
}

/// `POST path` with a JSON `body`; returns the 200 response body.
pub async fn post_json(
    ep: &Endpoint,
    path: &str,
    body: &str,
    deadline: Duration,
) -> Result<Vec<u8>, PostError> {
    match tokio::time::timeout(deadline, exchange(ep, path, body)).await {
        Ok(r) => r,
        Err(_) => Err(PostError::Transport(TransportErr::Timeout)),
    }
}

async fn exchange(ep: &Endpoint, path: &str, body: &str) -> Result<Vec<u8>, PostError> {
    let addrs: Vec<_> = tokio::net::lookup_host((ep.host.as_str(), ep.port))
        .await
        .map_err(|_| PostError::Transport(TransportErr::Resolve))?
        .collect();
    if addrs.is_empty() {
        return Err(PostError::Transport(TransportErr::Resolve));
    }
    let mut stream = connect_any(&addrs).await?;
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: {ep}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await.map_err(io_err)?;
    stream.write_all(body.as_bytes()).await.map_err(io_err)?;
    read_response(&mut stream).await
}

async fn connect_any(addrs: &[std::net::SocketAddr]) -> Result<TcpStream, PostError> {
    let mut last = TransportErr::Io;
    for a in addrs {
        match TcpStream::connect(a).await {
            Ok(s) => return Ok(s),
            Err(e) => last = io_kind(&e),
        }
    }
    Err(PostError::Transport(last))
}

fn io_kind(e: &std::io::Error) -> TransportErr {
    match e.kind() {
        std::io::ErrorKind::ConnectionRefused => TransportErr::Refused,
        _ => TransportErr::Io,
    }
}

fn io_err(e: std::io::Error) -> PostError {
    PostError::Transport(io_kind(&e))
}

/// Read until the response is complete per its framing, or EOF.
async fn read_response(s: &mut TcpStream) -> Result<Vec<u8>, PostError> {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 8192];
    loop {
        if let Some(done) = try_complete(&buf, false)? {
            return Ok(done);
        }
        let n = s.read(&mut tmp).await.map_err(io_err)?;
        if n == 0 {
            if buf.is_empty() {
                return Err(PostError::Transport(TransportErr::Closed));
            }
            return try_complete(&buf, true)?.ok_or(PostError::Malformed(MalformedKind::Framing));
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

/// The parsed head of a response.
struct Head {
    status: u16,
    body_start: usize,
    framing: Framing,
}

enum Framing {
    Length(usize),
    Chunked,
    UntilClose,
}

/// `Ok(Some(body))` when complete, `Ok(None)` when more bytes are needed, `Err` on a framing or
/// size violation or a non-200 status. `eof`: the peer has closed.
fn try_complete(buf: &[u8], eof: bool) -> Result<Option<Vec<u8>>, PostError> {
    let Some(head) = parse_head(buf)? else {
        return Ok(None);
    };
    if head.status != 200 {
        return Err(PostError::Transport(TransportErr::Http(head.status)));
    }
    let body = &buf[head.body_start..];
    let malformed = |k| Err(PostError::Malformed(k));
    match head.framing {
        Framing::Length(n) if n > MAX_BODY => malformed(MalformedKind::TooLarge),
        Framing::Length(n) if body.len() >= n => Ok(Some(body[..n].to_vec())),
        Framing::Length(_) => Ok(None),
        Framing::Chunked => match decode_chunked(body) {
            Err(k) => malformed(k),
            Ok(Some(b)) => Ok(Some(b)),
            Ok(None) if body.len() > MAX_BODY + MAX_HEAD => malformed(MalformedKind::TooLarge),
            Ok(None) => Ok(None),
        },
        Framing::UntilClose if body.len() > MAX_BODY => malformed(MalformedKind::TooLarge),
        Framing::UntilClose if eof => Ok(Some(body.to_vec())),
        Framing::UntilClose => Ok(None),
    }
}

/// Parse the status line and headers once the blank line has arrived.
fn parse_head(buf: &[u8]) -> Result<Option<Head>, PostError> {
    let framing_err = PostError::Malformed(MalformedKind::Framing);
    let Some(end) = find(buf, b"\r\n\r\n") else {
        return if buf.len() > MAX_HEAD {
            Err(framing_err)
        } else {
            Ok(None)
        };
    };
    let text = std::str::from_utf8(&buf[..end]).map_err(|_| framing_err)?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let mut parts = status_line.split(' ');
    let (Some(version), Some(code)) = (parts.next(), parts.next()) else {
        return Err(framing_err);
    };
    if !version.starts_with("HTTP/1.") {
        return Err(framing_err);
    }
    let status: u16 = code.parse().map_err(|_| framing_err)?;
    let mut framing = Framing::UntilClose;
    for line in lines {
        let Some((k, v)) = line.split_once(':') else {
            return Err(framing_err);
        };
        let (k, v) = (k.trim(), v.trim());
        if k.eq_ignore_ascii_case("transfer-encoding") && v.eq_ignore_ascii_case("chunked") {
            framing = Framing::Chunked;
        } else if k.eq_ignore_ascii_case("content-length") && !matches!(framing, Framing::Chunked) {
            framing = Framing::Length(v.parse().map_err(|_| framing_err)?);
        }
    }
    Ok(Some(Head {
        status,
        body_start: end + 4,
        framing,
    }))
}

/// Decode a chunked body. `Ok(None)` = incomplete; trailers are ignored.
fn decode_chunked(mut data: &[u8]) -> Result<Option<Vec<u8>>, MalformedKind> {
    let mut out = Vec::new();
    loop {
        let Some(eol) = find(data, b"\r\n") else {
            return Ok(None);
        };
        let size_txt = std::str::from_utf8(&data[..eol]).map_err(|_| MalformedKind::Framing)?;
        let size_hex = size_txt.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_hex, 16).map_err(|_| MalformedKind::Framing)?;
        data = &data[eol + 2..];
        if size == 0 {
            return Ok(Some(out));
        }
        if out.len() + size > MAX_BODY {
            return Err(MalformedKind::TooLarge);
        }
        if data.len() < size + 2 {
            return Ok(None);
        }
        if &data[size..size + 2] != b"\r\n" {
            return Err(MalformedKind::Framing);
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
