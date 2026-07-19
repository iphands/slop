//! The HTTP layer.
//!
//! `GET /api/stats` never touches SQLite. The ingest tick builds the payload,
//! serializes it, gzips it and stamps an ETag once; a request is an `Arc` clone
//! and a socket write. `Bytes::clone` is an atomic refcount increment, not a
//! memcpy.

use std::sync::{Arc, RwLock};

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use bytes::Bytes;
use rusqlite::Connection;

use crate::snapshot::Payload;

/// A payload, pre-serialized every way a client might want it.
#[derive(Debug)]
pub struct Rendered {
    pub json: Bytes,
    pub gzip: Bytes,
    pub etag: HeaderValue,
    pub generated_at: i64,
    pub last_tick_at: i64,
}

impl Rendered {
    pub fn build(p: &Payload) -> Self {
        let json = serde_json::to_vec(p).unwrap_or_else(|_| b"{}".to_vec());
        // Pre-gzip once per tick rather than using a CompressionLayer, which
        // would re-compress the same bytes on every single request -- the same
        // reasoning that motivated the snapshot, applied consistently.
        let gzip = gzip(&json);
        // Cheap, stable, and changes exactly when the content does.
        let etag = HeaderValue::from_str(&format!("\"{}-{}\"", p.generated_at, fnv1a(&json)))
            .unwrap_or_else(|_| HeaderValue::from_static("\"0\""));
        Self {
            json: Bytes::from(json),
            gzip: Bytes::from(gzip),
            etag,
            generated_at: p.generated_at,
            last_tick_at: p.ingest.last_tick_at,
        }
    }
}

fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn gzip(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    if e.write_all(data).is_err() {
        return Vec::new();
    }
    e.finish().unwrap_or_default()
}

/// The shared snapshot cell.
///
/// `RwLock<Arc<_>>` rather than `arc-swap`: one write per tick against a handful
/// of reads is not contention, and it matches the house style. A lock-free
/// primitive here would be performance theatre.
#[derive(Clone)]
pub struct Snapshot(Arc<RwLock<Arc<Rendered>>>);

impl Snapshot {
    pub fn new(r: Rendered) -> Self {
        Self(Arc::new(RwLock::new(Arc::new(r))))
    }
    pub fn load(&self) -> Arc<Rendered> {
        self.0.read().expect("snapshot lock poisoned").clone()
    }
    pub fn store(&self, r: Rendered) {
        *self.0.write().expect("snapshot lock poisoned") = Arc::new(r);
    }
}

/// Shared state for the router. The DB connection is only for the on-demand
/// drilldown, which is not polled.
#[derive(Clone)]
pub struct AppState {
    pub snapshot: Snapshot,
    pub db: Arc<std::sync::Mutex<Connection>>,
}

#[derive(rust_embed::Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../frontend/dist"]
struct Assets;

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/stats", get(get_stats))
        .route("/stats/client/{ip}", get(get_client))
        // A typo under /api must 404, not silently return the SPA shell --
        // which is what qctrl's from_fn middleware did.
        .fallback(|| async { StatusCode::NOT_FOUND })
        .with_state(state.clone());

    Router::new()
        .route("/healthz", get(healthz))
        .nest("/api", api)
        .fallback(get(assets))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state)
}

async fn get_stats(State(st): State<AppState>, headers: HeaderMap) -> Response {
    let p = st.snapshot.load();

    // An idle dashboard polling every 5s then transfers ~150 bytes of headers
    // and nothing else.
    if headers
        .get(header::IF_NONE_MATCH)
        .is_some_and(|v| v == p.etag)
    {
        return (StatusCode::NOT_MODIFIED, [(header::ETAG, p.etag.clone())]).into_response();
    }

    let wants_gzip = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("gzip"));

    let mut resp = Response::builder()
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ETAG, p.etag.clone());
    if wants_gzip {
        resp = resp.header(header::CONTENT_ENCODING, "gzip");
    }
    // Bytes::clone is a refcount bump, not a copy.
    let body = if wants_gzip {
        p.gzip.clone()
    } else {
        p.json.clone()
    };
    resp.body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

#[derive(serde::Deserialize)]
pub struct WindowQuery {
    window: Option<String>,
}

/// The full per-client drilldown. This one DOES hit SQLite — a human clicking a
/// table row is not a hot path, and keeping the full list out of the snapshot is
/// what stops it exploding on dist-upgrade day.
async fn get_client(
    State(st): State<AppState>,
    Path(ip): Path<String>,
    Query(q): Query<WindowQuery>,
) -> Response {
    if ip.len() > 64 || ip.is_empty() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let days = match q.window.as_deref() {
        Some("7d") => 7,
        Some("30d") => 30,
        _ => 1,
    };
    let since = crate::now_secs() - days * 86_400;

    let conn = match st.db.lock() {
        Ok(c) => c,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let packages = crate::snapshot::client_paths(&conn, &ip, since, "package", 500);
    let metadata = crate::snapshot::client_paths(&conn, &ip, since, "metadata", 100);
    match (packages, metadata) {
        (Ok(p), Ok(m)) => axum::Json(serde_json::json!({
            "ip": ip, "window": q.window.unwrap_or_else(|| "24h".into()),
            "packages": p, "metadata": m,
        }))
        .into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// 503 when the ingest tick has gone stale.
///
/// A stats service whose ingest task died but whose HTTP server is still up is
/// **worse** than one that is down: the dashboard keeps rendering frozen numbers
/// and nothing alerts. This works only because the tick timestamp advances even
/// when zero lines were ingested, so an idle cache stays healthy.
pub const STALE_AFTER_SECS: i64 = 60;

async fn healthz(State(st): State<AppState>) -> Response {
    let p = st.snapshot.load();
    let lag = crate::now_secs() - p.last_tick_at;
    let stale = lag > STALE_AFTER_SECS;
    let body = serde_json::json!({
        "status": if stale { "stale" } else { "ok" },
        "last_tick_at": p.last_tick_at,
        "lag_seconds": lag,
    });
    let code = if stale {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    (code, axum::Json(body)).into_response()
}

/// Serve the embedded SPA.
async fn assets(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(f) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            // vite hashes asset filenames, so they are immutable; index.html
            // must NOT be, or a cached shell pins dead asset hashes after a
            // deploy and users see a broken page.
            let cache = if path.starts_with("assets/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            (
                [
                    (header::CONTENT_TYPE, mime.as_ref()),
                    (header::CACHE_CONTROL, cache),
                ],
                f.data.into_owned(),
            )
                .into_response()
        }
        // SPA fallback: an unknown path is a client-side route.
        None => match Assets::get("index.html") {
            Some(f) => (
                [
                    (header::CONTENT_TYPE, "text/html"),
                    (header::CACHE_CONTROL, "no-cache"),
                ],
                f.data.into_owned(),
            )
                .into_response(),
            None => (StatusCode::NOT_FOUND, "frontend not built").into_response(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{self, Ingest};

    fn rendered() -> Rendered {
        let mut c = Connection::open_in_memory().unwrap();
        crate::db::migrate_for_tests(&mut c);
        let p = snapshot::build(&c, crate::now_secs(), Ingest::default(), None).unwrap();
        Rendered::build(&p)
    }

    #[test]
    fn a_rendered_payload_carries_json_gzip_and_an_etag() {
        let r = rendered();
        assert!(!r.json.is_empty());
        assert!(!r.gzip.is_empty());
        assert!(r.gzip.len() < r.json.len(), "gzip should be smaller");
        assert!(r.etag.to_str().unwrap().starts_with('"'));
    }

    #[test]
    fn the_etag_changes_when_the_content_does() {
        let mut c = Connection::open_in_memory().unwrap();
        crate::db::migrate_for_tests(&mut c);
        let a = Rendered::build(&snapshot::build(&c, 1000, Ingest::default(), None).unwrap());
        let b = Rendered::build(&snapshot::build(&c, 2000, Ingest::default(), None).unwrap());
        assert_ne!(a.etag, b.etag);
    }

    #[test]
    fn the_snapshot_cell_swaps_atomically() {
        let s = Snapshot::new(rendered());
        let before = s.load().generated_at;
        s.store(rendered());
        assert!(s.load().generated_at >= before);
    }

    #[test]
    fn gzip_round_trips() {
        use std::io::Read;
        let data = b"{\"hello\":\"world\"}".repeat(50);
        let z = gzip(&data);
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(&z[..])
            .read_to_end(&mut out)
            .unwrap();
        assert_eq!(out, data);
    }
}
