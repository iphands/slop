use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware,
    routing::{delete, get, post, put},
    Json, Router,
};
use qctrl_rcon::{RconClient, ServerQuery};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::services::ServeDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod clock;
mod config;
mod favorites;
mod frames;
mod logs;
mod maps;
mod oob;
mod rotation;
mod rotator;
mod status;
mod status_cache;

use config::Config;
use favorites::Favorites;
use logs::LogStream;
use maps::MapCache;
use rotation::{AddMapRequest, QueueResponse, QueueStatusResponse, RotationMode, RotationQueue};
use rotator::{decide, RotationDecision, Tick};
use status::{parse_status_output, StatusResponse};
use status_cache::StatusCache;

#[derive(Clone)]
struct SharedState {
    config: Config,
    rcon_client: Arc<RconClient>,
    /// Password-free OOB status query. Read-only, and metered by the server under
    /// its own rate limit — see `qctrl_rcon::ServerQuery`.
    server_query: Arc<ServerQuery>,
    status_cache: StatusCache,
    map_cache: MapCache,
    log_stream: Arc<LogStream>,
    favorites: Favorites,
    rotation_queue: Arc<tokio::sync::Mutex<RotationQueue>>,
    rotation_enabled: Arc<tokio::sync::Mutex<bool>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::load("config.yaml").unwrap_or_else(|_| {
        tracing::warn!("Failed to load config.yaml, using defaults");
        Config::default()
    });

    let rcon_client = Arc::new(RconClient::new(
        &config.server.host,
        config.server.port,
        &config.server.rcon_password,
    ));

    let server_query = Arc::new(ServerQuery::new(&config.server.host, config.server.port));
    let status_cache = StatusCache::new();

    let map_cache = MapCache::new(&config.paths.baseq2);
    let log_stream = Arc::new(LogStream::new(1000, 200));
    let favorites = Favorites::new("favorites.json").unwrap_or_else(|e| {
        tracing::warn!(
            "Failed to initialize favorites: {}, using empty favorites",
            e
        );
        Favorites::new("favorites.json").unwrap_or_else(|_| {
            // Last resort: create empty favorites
            let _ = std::fs::write("favorites.json", "[]");
            Favorites::new("favorites.json").unwrap()
        })
    });

    let rotation_queue = Arc::new(tokio::sync::Mutex::new({
        // Try to load existing queue from disk, or create new with persistence
        match RotationQueue::load("rotation.yaml") {
            Ok(queue) => queue,
            Err(e) => {
                tracing::warn!("Failed to load rotation queue: {}, creating new one", e);
                RotationQueue::new_with_persistence(RotationMode::Sequential, "rotation.yaml")
            }
        }
    }));

    let rotation_enabled = Arc::new(tokio::sync::Mutex::new(true));

    let state = SharedState {
        config,
        rcon_client,
        server_query,
        status_cache,
        map_cache,
        log_stream,
        favorites,
        rotation_queue,
        rotation_enabled,
    };

    // Push the persisted rotation queue to the server on startup so the server's
    // own end-of-match rotation is correct from the first map (sv_maplist is
    // lost on server restart).
    {
        let queue = state.rotation_queue.lock().await;
        spawn_sv_maplist_sync(state.clone(), &queue);
    }

    // ...and keep it pushed: a Q2 server restart wipes the cvar, which re-arms
    // the empty-map crash until something pushes it again.
    spawn_sv_maplist_watchdog(state.clone());

    // The only thing that touches the server on a schedule for reads. Everything
    // serving /api/status now reads its cache instead of opening an rcon call.
    spawn_status_poller(state.clone());

    // ...and the thing that acts on what the poller sees. Q2 never leaves
    // intermission on its own, so without an owner here an unattended server
    // stops rotating the moment the last browser tab closes.
    spawn_rotator(state.clone());

    // Optional (Plan 13): a qbots fleet on this host can relay the server's own frame counter,
    // which is the exact age of the running map — something neither RCON nor the OOB status query
    // will ever tell us. Absent a `frames:` block, NOTHING here is spawned and the map clock keeps
    // inferring from map-name edges exactly as before.
    if state.config.frames.enabled() {
        spawn_frame_listener(state.clone()).await;
    }

    let api_routes = Router::new()
        .route("/health", get(health))
        .route("/config", get(get_config))
        .route("/rcon/execute", post(rcon_execute))
        .route("/maps", get(list_maps))
        .route("/favorites", get(get_favorites))
        .route("/favorites", post(add_favorite))
        .route("/favorites/:map_name", delete(remove_favorite))
        .route("/status", get(get_status))
        .route("/rotation", get(get_rotation))
        .route("/rotation", post(add_to_rotation))
        .route("/rotation", put(update_rotation))
        .route("/rotation/:map_name", delete(remove_from_rotation))
        .route("/rotation/toggle", post(toggle_rotation))
        .route("/logs/ws", get(logs_ws))
        .with_state(state);

    let static_files = ServeDir::new("frontend/dist");

    async fn spa_fallback(
        request: Request<Body>,
        next: middleware::Next,
    ) -> Result<axum::response::Response, StatusCode> {
        let path = request.uri().path().to_string();

        if path.starts_with("/api/") {
            return Ok(next.run(request).await);
        }

        let response = next.run(request).await;

        if response.status() == StatusCode::NOT_FOUND {
            let index_html = tokio::fs::read("frontend/dist/index.html")
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            Ok(axum::response::Response::new(Body::from(index_html)))
        } else {
            Ok(response)
        }
    }

    let app = Router::new()
        .nest("/api", api_routes)
        .nest_service("/", static_files)
        .layer(middleware::from_fn(spa_fallback))
        .fallback_service(ServeDir::new("frontend/dist").append_index_html_on_directories(true));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Starting qctrl API + frontend on {}", addr);
    tracing::info!("Frontend: http://localhost:3000");
    tracing::info!("API: http://localhost:3000/api/*");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn get_config(State(state): State<SharedState>) -> Json<config::Config> {
    Json(state.config.clone())
}

async fn list_maps(State(state): State<SharedState>) -> Result<Json<MapList>, StatusCode> {
    match state.map_cache.get_maps() {
        Ok(maps) => Ok(Json(MapList { maps })),
        Err(e) => {
            tracing::error!("Failed to list maps: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_favorites(State(state): State<SharedState>) -> Json<FavoritesResponse> {
    let favorites = state.favorites.get_favorites();
    Json(FavoritesResponse { favorites })
}

async fn add_favorite(
    State(state): State<SharedState>,
    Json(payload): Json<AddFavoritePayload>,
) -> Result<Json<AddFavoriteResponse>, StatusCode> {
    match state.favorites.add_favorite(&payload.map_name) {
        Ok(_) => Ok(Json(AddFavoriteResponse { success: true })),
        Err(e) => {
            tracing::error!("Failed to add favorite: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn remove_favorite(
    State(state): State<SharedState>,
    axum::extract::Path(map_name): axum::extract::Path<String>,
) -> Result<Json<RemoveFavoriteResponse>, StatusCode> {
    match state.favorites.remove_favorite(&map_name) {
        Ok(_) => Ok(Json(RemoveFavoriteResponse { success: true })),
        Err(e) => {
            tracing::error!("Failed to remove favorite: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Serve the cached status. A lock read — this touches the network not at all.
///
/// This used to do a live rcon round-trip per request, which meant an rcon
/// `status` every 2 seconds once the frontend was open (six components poll the
/// `['status']` query key and react-query dedupes them to the shortest
/// interval). `sv_rcon_limit` defaults to 1/sec, and a throttled Q2 server
/// answers every command with `Bad rcon_password` — see `context/pitfalls.md`.
async fn get_status(State(state): State<SharedState>) -> Json<StatusResponse> {
    Json(state.status_cache.snapshot())
}

async fn get_rotation(
    State(state): State<SharedState>,
) -> Result<Json<QueueStatusResponse>, StatusCode> {
    let queue = state.rotation_queue.lock().await;
    let enabled = *state.rotation_enabled.lock().await;
    let maps = queue.get_maps();
    let current_map = if !maps.is_empty() {
        Some(maps[0].clone())
    } else {
        None
    };

    tracing::info!(
        "Rotation status: enabled={}, maps={:?}, current={:?}",
        enabled,
        maps,
        current_map
    );

    Ok(Json(QueueStatusResponse {
        maps,
        mode: queue.mode(),
        current_map,
        enabled,
    }))
}

async fn add_to_rotation(
    State(state): State<SharedState>,
    Json(payload): Json<AddMapRequest>,
) -> Result<Json<QueueResponse>, StatusCode> {
    tracing::info!("Adding to rotation: {}", payload.map_name);

    if !valid_map_name(&payload.map_name) {
        tracing::warn!("Rejected rotation map name '{}'", payload.map_name);
        return Err(StatusCode::BAD_REQUEST);
    }

    match state.map_cache.get_maps() {
        Ok(available_maps) => {
            let map_exists = available_maps.iter().any(|m| m.name == payload.map_name);

            if !map_exists {
                tracing::warn!("Map '{}' not found in baseq2/maps/", payload.map_name);
                return Ok(Json(QueueResponse {
                    success: false,
                    message: format!("Map '{}' not found in baseq2/maps/", payload.map_name),
                    queue_size: 0,
                }));
            }

            let mut queue = state.rotation_queue.lock().await;
            queue.add_map(payload.map_name.clone());

            if let Err(e) = queue.save() {
                tracing::error!("Failed to save rotation queue: {}", e);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }

            tracing::info!(
                "Added '{}' to rotation queue. Queue size: {}",
                payload.map_name,
                queue.len()
            );
            spawn_sv_maplist_sync(state.clone(), &queue);

            Ok(Json(QueueResponse {
                success: true,
                message: format!("Added '{}' to rotation queue", payload.map_name),
                queue_size: queue.len(),
            }))
        }
        Err(e) => {
            tracing::error!("Failed to list maps for validation: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn update_rotation(
    State(state): State<SharedState>,
    Json(payload): Json<QueueStatusResponse>,
) -> Result<Json<QueueResponse>, StatusCode> {
    tracing::info!(
        "Updating rotation: mode={:?}, maps={:?}",
        payload.mode,
        payload.maps
    );

    if let Some(bad) = payload.maps.iter().find(|m| !valid_map_name(m)) {
        tracing::warn!("Rejected rotation update: invalid map name '{}'", bad);
        return Err(StatusCode::BAD_REQUEST);
    }

    let mut queue = state.rotation_queue.lock().await;

    queue.set_maps(payload.maps);
    queue.set_mode(payload.mode);

    if let Err(e) = queue.save() {
        tracing::error!("Failed to save rotation queue: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    tracing::info!("Rotation queue updated. Queue size: {}", queue.len());
    spawn_sv_maplist_sync(state.clone(), &queue);

    Ok(Json(QueueResponse {
        success: true,
        message: "Rotation queue updated".to_string(),
        queue_size: queue.len(),
    }))
}

async fn remove_from_rotation(
    State(state): State<SharedState>,
    axum::extract::Path(map_name): axum::extract::Path<String>,
) -> Result<Json<QueueResponse>, StatusCode> {
    tracing::info!("Removing from rotation: {}", map_name);

    let mut queue = state.rotation_queue.lock().await;

    let was_present = queue.get_maps().contains(&map_name);
    queue.remove_map(&map_name);

    if let Err(e) = queue.save() {
        tracing::error!("Failed to save rotation queue: {}", e);
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    tracing::info!(
        "Removed '{}' from rotation. Was present: {}, queue size: {}",
        map_name,
        was_present,
        queue.len()
    );
    spawn_sv_maplist_sync(state.clone(), &queue);

    Ok(Json(QueueResponse {
        success: was_present,
        message: if was_present {
            format!("Removed '{}' from rotation queue", map_name)
        } else {
            format!("Map '{}' was not in rotation queue", map_name)
        },
        queue_size: queue.len(),
    }))
}

#[derive(Debug, Serialize, Deserialize)]
struct ToggleRotationResponse {
    success: bool,
    enabled: bool,
    message: String,
}

async fn toggle_rotation(
    State(state): State<SharedState>,
) -> Result<Json<ToggleRotationResponse>, StatusCode> {
    let mut enabled = state.rotation_enabled.lock().await;
    *enabled = !*enabled;

    let message = if *enabled {
        "Map rotation enabled".to_string()
    } else {
        "Map rotation disabled".to_string()
    };

    tracing::info!("Rotation toggled: enabled={}, {}", *enabled, message);

    Ok(Json(ToggleRotationResponse {
        success: true,
        enabled: *enabled,
        message,
    }))
}

/// Push the current rotation queue to the server's `sv_maplist` cvar.
///
/// The Quake 2 game logic (`EndDMLevel`) advances through `sv_maplist` when a
/// match ends on timelimit/fraglimit. With it empty the server resolves its
/// next map to an empty string and dies on `maps/.bsp`. Syncing our queue makes
/// the server's own rotation a correct fallback that always lands on a real
/// map — even if qctrl's frontend is closed when the limit hits.
///
/// Runs on a background task (rcon may take a few seconds) and is best-effort:
/// a failure to reach the server never fails the API request. An empty queue is
/// intentionally a no-op so we never clear a good `sv_maplist` and reintroduce
/// the empty-map crash.
fn spawn_sv_maplist_sync(state: SharedState, queue: &RotationQueue) {
    let maps = queue.get_maps();
    if maps.is_empty() {
        return;
    }
    tokio::spawn(async move {
        match push_sv_maplist(&state.rcon_client, &maps).await {
            Ok(_) => tracing::info!("Synced sv_maplist ({} maps) to server", maps.len()),
            Err(e) => tracing::warn!("Failed to sync sv_maplist to server: {}", e),
        }
    });
}

/// Join map names into an `sv_maplist` value that can survive rcon.
///
/// A quoted value with spaces CANNOT: the server tokenizes the rcon packet, then
/// rebuilds the command by re-joining argv with spaces (`sv_conless.c`,
/// `SVC_RemoteCommand`). The quotes are gone by then, so
/// `set sv_maplist "q2dm1 q2dm2"` reaches the command buffer as
/// `set sv_maplist q2dm1 q2dm2` — too many arguments, and `set` answers
/// `usage: set <variable> <value> [u / s]` while the cvar stays empty.
///
/// Commas dodge this: `EndDMLevel` tokenizes `sv_maplist` on `" ,\n\r"`
/// (`g_main.c`), and a comma-joined value is a single unquoted argv token.
fn sv_maplist_value(maps: &[String]) -> String {
    maps.join(",")
}

/// The map names in an `sv_maplist` value, however it happens to be separated.
/// The game accepts spaces and commas interchangeably, so a value that differs
/// from ours only in separators is not drift.
fn sv_maplist_maps(value: &str) -> Vec<&str> {
    value
        .split([' ', ',', '\n', '\r'])
        .filter(|s| !s.is_empty())
        .collect()
}

/// Push `maps` to the server as `sv_maplist`. An empty list is a no-op: clearing
/// a good `sv_maplist` would reintroduce the `maps/.bsp` crash this module exists
/// to prevent.
async fn push_sv_maplist(rcon: &RconClient, maps: &[String]) -> Result<(), String> {
    if maps.is_empty() {
        return Ok(());
    }
    let command = format!("set sv_maplist {}", sv_maplist_value(maps));
    rcon.execute(&command)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Extract the value from a cvar echo line: `"sv_maplist" is "q2dm1 q2dm2"`.
///
/// Returns `None` when the reply doesn't have the echo shape — e.g. the server
/// is throttling rcon and replied `Bad rcon_password`.
fn parse_cvar_echo<'a>(reply: &'a str, cvar: &str) -> Option<&'a str> {
    let needle = format!("\"{cvar}\" is ");
    let line = reply.lines().find(|l| l.contains(&needle))?;
    // The value is the last quoted span on the line.
    let mut parts = line.rsplitn(3, '"');
    let _after_closing_quote = parts.next()?;
    let value = parts.next()?;
    Some(value)
}

/// True when the server's live `sv_maplist` doesn't match the queue we want.
///
/// Compared map-by-map rather than as a string, so a value someone set from the
/// server console with spaces reads as equivalent to our comma-joined one — the
/// game treats them the same, and re-pushing over a working list every minute
/// would be pure console noise.
///
/// An unparseable reply (`None`) counts as *not* drifted: pushing on garbage
/// would hammer the server exactly when it is throttling us.
fn maplist_drifted(live: Option<&str>, wanted: &[String]) -> bool {
    match live {
        None => false,
        Some(v) => sv_maplist_maps(v) != wanted.iter().map(String::as_str).collect::<Vec<_>>(),
    }
}

/// Re-check `sv_maplist` every 60 s and re-push the rotation queue on drift.
///
/// The startup and rotation-CRUD pushes are lost whenever the Q2 *server*
/// restarts (cvars reset to empty), which silently re-arms the `maps/.bsp` crash
/// at the next match end. Check-then-push keeps the server console quiet and
/// stays well under rcon flood protection: one cheap query a minute, and a
/// second command only when the cvar actually drifted.
fn spawn_sv_maplist_watchdog(state: SharedState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Set once an engine proves it ignores sv_uptime, so we stop probing it
        // (and stop writing a cvar it does nothing with) every minute.
        let mut uptime_unsupported = false;
        loop {
            tick.tick().await;

            let wanted = state.rotation_queue.lock().await.get_maps();
            if wanted.is_empty() {
                continue; // Nothing to protect the server with.
            }

            match state.rcon_client.execute("sv_maplist").await {
                Ok(reply) => {
                    let live = parse_cvar_echo(&reply, "sv_maplist");
                    if maplist_drifted(live, &wanted) {
                        tracing::warn!(
                            live = live.unwrap_or("<unparseable>"),
                            "sv_maplist drifted (server restarted?) — re-pushing {} maps",
                            wanted.len()
                        );
                        if let Err(e) = push_sv_maplist(&state.rcon_client, &wanted).await {
                            tracing::warn!("sv_maplist re-push failed: {}", e);
                        }
                        // A wiped sv_maplist means the server process restarted, which
                        // makes our map-start anchor meaningless. Say we don't know
                        // rather than keep counting from a dead anchor.
                        state.status_cache.invalidate_clock();
                    }
                }
                // Server down or unreachable: an outage must not become a log flood.
                Err(e) => tracing::debug!("sv_maplist check skipped: {}", e),
            }

            if state.config.poll.manage_sv_uptime && !uptime_unsupported {
                uptime_unsupported = ensure_sv_uptime(&state).await == SvUptime::Unsupported;
            }
        }
    });
}

#[derive(Debug, PartialEq, Eq)]
enum SvUptime {
    /// The server reports uptime — restart detection via the clock is armed.
    Working,
    /// This engine has no `sv_uptime`. Stop asking; fall back to `sv_maplist` drift.
    Unsupported,
    /// Just set the cvar (or couldn't reach the server); re-check next tick.
    Pending,
}

/// Try to keep `sv_uptime 1` set, so the OOB status reply carries the server's uptime.
///
/// Uptime lets the map clock notice a server restart onto the *same* map — such a
/// restart changes no map name, so without it the clock keeps counting from an
/// anchor belonging to a dead process. `sv_uptime 1` is the second-resolution form
/// (`MM.SS` / `H:MM.SS` / `D+HH:MM.SS`); `2` is a prose form we cannot parse.
///
/// **This only works on q2pro/q2repro.** yquake2 has no `sv_uptime` cvar at all —
/// and crucially, setting it there still *appears* to succeed, because Q2 creates an
/// inert user cvar for any unknown name and dutifully echoes `"sv_uptime" is "1"`
/// back. Trusting that echo would mean believing restart detection was armed when it
/// was doing nothing. So the cvar's value is not the test: the test is whether an
/// `uptime` key ever shows up in an actual status reply.
///
/// On an engine that ignores it, we say so once and stop writing a junk cvar to
/// someone's server. Restart detection then rests on the `sv_maplist` watchdog,
/// which already spots a restart (a restart wipes the cvar) and invalidates the clock.
async fn ensure_sv_uptime(state: &SharedState) -> SvUptime {
    let reply = match state.rcon_client.execute("sv_uptime").await {
        Ok(reply) => reply,
        Err(e) => {
            tracing::debug!("sv_uptime check skipped: {}", e);
            return SvUptime::Pending;
        }
    };

    if parse_cvar_echo(&reply, "sv_uptime") != Some("1") {
        tracing::info!("Setting sv_uptime 1 (lets qctrl detect server restarts)");
        if let Err(e) = state.rcon_client.execute("set sv_uptime 1").await {
            tracing::warn!("Failed to set sv_uptime: {}", e);
        }
        // The next status poll will show whether the engine honored it.
        return SvUptime::Pending;
    }

    // The cvar says 1. Did the server actually act on it?
    if state.status_cache.saw_uptime_key() {
        return SvUptime::Working;
    }

    tracing::warn!(
        "Server accepts `sv_uptime 1` but never reports uptime — this engine \
         (yquake2?) does not support it. Map-clock restart detection will rely on \
         sv_maplist drift instead, so a server restart onto the same map may take \
         up to a minute to notice."
    );
    SvUptime::Unsupported
}

/// The map name a `map`/`gamemap` command will load, if it is one.
///
/// qctrl issuing a map command is the only anchor available for a restart onto
/// the map already running — that produces no map-name change, so the poller's
/// edge detector will never fire for it.
fn map_command_target(command: &str) -> Option<String> {
    let mut it = command.split_whitespace();
    let head = it.next()?.to_ascii_lowercase();
    if head != "map" && head != "gamemap" {
        return None;
    }
    let arg = it.next()?.trim_matches('"').trim();
    (!arg.is_empty()).then(|| arg.to_string())
}

/// Poll the server for status and keep `StatusCache` current.
///
/// Two very different cadences, because they hit two different rate limits:
///
/// * The **OOB status query** runs every `poll.status_interval_ms` (1s default).
///   It is unauthenticated and metered under `sv_status_limit` (default 15/sec),
///   so 1 Hz costs the rcon budget nothing. It supplies the map, the cvars, and
///   every player's frags/ping/name — and it is what anchors the map clock, so
///   its interval is also the accuracy bound on the countdown.
///
/// * **RCON `status`** runs only when the identity table is stale or a new player
///   name appears, because it is the sole source of client numbers and addresses
///   (the OOB reply has neither) and rcon's budget is 1/sec.
///
/// Neither of them carries a map clock — see `spawn_frame_listener` for the one thing that does.
fn spawn_status_poller(state: SharedState) {
    tokio::spawn(async move {
        let status_interval = Duration::from_millis(state.config.poll.status_interval_ms.max(200));
        let identity_max_age = Duration::from_millis(state.config.poll.rcon_identity_interval_ms);

        let mut tick = tokio::time::interval(status_interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // rcon `status` is rate-limited at 1/sec server-side; never queue two
        // back-to-back even if players are churning.
        const MIN_RCON_IDENTITY_GAP: Duration = Duration::from_secs(2);
        let mut last_identity_attempt: Option<Instant> = None;

        loop {
            tokio::select! {
                _ = tick.tick() => {}
                _ = state.status_cache.refresh_requested() => {}
            }

            match state.server_query.status(Duration::from_secs(1)).await {
                Ok(reply) => match oob::parse_oob_status(&reply) {
                    Ok(status) => {
                        state.status_cache.apply_oob(status, Instant::now());
                    }
                    Err(e) => {
                        let failures = state.status_cache.note_failure();
                        if failures == 1 {
                            tracing::warn!("Unparseable OOB status reply: {}", e);
                        }
                    }
                },
                Err(e) => {
                    // An outage must not become a log flood: say it once, then go quiet.
                    let failures = state.status_cache.note_failure();
                    if failures == 1 {
                        tracing::warn!("Status poll failed (server down?): {}", e);
                    } else {
                        tracing::debug!("Status poll failed ({} in a row): {}", failures, e);
                    }
                    continue;
                }
            }

            let now = Instant::now();
            let gap_ok = last_identity_attempt
                .map(|t| now.saturating_duration_since(t) >= MIN_RCON_IDENTITY_GAP)
                .unwrap_or(true);

            if gap_ok
                && state
                    .status_cache
                    .needs_rcon_identity(now, identity_max_age)
            {
                last_identity_attempt = Some(now);
                match state.rcon_client.execute("status").await {
                    Ok(output) => match parse_status_output(&output) {
                        Ok(parsed) => state
                            .status_cache
                            .apply_rcon_identity(parsed.players, Instant::now()),
                        Err(e) => tracing::debug!("Failed to parse rcon status: {}", e),
                    },
                    Err(e) => tracing::debug!("rcon identity poll skipped: {}", e),
                }
            }
        }
    });
}

/// Bridge the beacon reader to the status cache. Keeps `frames.rs` free of `SharedState`.
struct ClockSink(StatusCache);

impl frames::BeaconSink for ClockSink {
    fn apply(&self, beacon: &frames::Beacon, now: Instant) {
        self.0.apply_beacon(beacon, now);
    }
}

/// Spawn the serverframe beacon reader (Plan 13). Only called when `frames:` is configured.
///
/// This is the only input that *measures* the map clock rather than inferring it. See
/// `crate::frames`.
///
/// Resolves our own server address up front so a beacon about a **different** Q2 server can be
/// rejected. A qbots fleet pointed at a test box would otherwise silently drive this map clock
/// with a foreign map's age — and the countdown would look perfectly healthy while being wrong.
async fn spawn_frame_listener(state: SharedState) {
    let want_name = format!("{}:{}", state.config.server.host, state.config.server.port);

    // Best-effort: a DNS failure is not fatal, it just means we fall back to matching on the
    // configured name. Not worth re-resolving on a timer — the server address is static in practice.
    let want_addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host(&want_name)
        .await
        .map(|it| it.collect())
        .unwrap_or_default();
    if want_addrs.is_empty() {
        tracing::warn!(
            server = %want_name,
            "could not resolve our own server address; beacons will be matched on name alone"
        );
    }

    let path = state.config.frames.socket_path.trim().to_string();
    let backoff = frames::Backoff {
        min: Duration::from_millis(state.config.frames.reconnect_min_ms.max(100)),
        max: Duration::from_millis(state.config.frames.reconnect_max_ms.max(1000)),
    };

    tracing::info!(
        beacon = %path,
        server = %want_name,
        "serverframe beacon enabled — the map clock will be measured, not inferred"
    );

    tokio::spawn(frames::run_reader(
        frames::Transport::Unix(path.into()),
        ClockSink(state.status_cache.clone()),
        want_addrs,
        want_name,
        state.config.frames.require_server_match,
        backoff,
    ));
}

/// Don't retry a failed rotation faster than this. A `map` command that the
/// server refuses (or never answers) would otherwise be resent once a second,
/// which blows straight through `sv_rcon_limit` and gets every later command
/// answered with `Bad rcon_password` — see `context/pitfalls.md`.
const ROTATION_COOLDOWN: Duration = Duration::from_secs(15);

/// A random `u64` for `Random` rotation mode, without pulling in a dependency for
/// it. SipHash of the current nanos under a per-process random key: not
/// cryptographic, but well-distributed, which is all "pick a map" needs.
fn random_pick() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    hasher.finish()
}

/// Advance the map when a match ends.
///
/// This has to live in the backend. Quake 2 never leaves intermission on its own
/// — the only exit is a connected client pressing a button five seconds in
/// (`yquake2 game/player/client.c:2122`) — and `sv_maplist` only picks the
/// destination, it does not fire the exit. Rotation used to run in a React hook,
/// so an unattended server stopped rotating the moment the last tab closed and
/// sat in intermission until someone opened the frontend. See `rotator.rs` for
/// the full story and the trigger policy.
///
/// Reads the status cache the poller already fills, so this costs no extra server
/// I/O on a tick that decides to do nothing — which is almost all of them.
fn spawn_rotator(state: SharedState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // The rotator's own view of how long the current map has been up. This is
        // deliberately separate from the clock's anchor: it exists even when the
        // clock has none, and that is exactly when the rescue trigger needs it.
        let mut seen: Option<(String, Instant)> = None;
        let mut cooldown_until: Option<Instant> = None;
        let mut was_online = false;

        loop {
            tick.tick().await;

            let status = state.status_cache.snapshot();
            let now = Instant::now();

            // A server that just came back is a server that may have restarted, and
            // a restart onto the *same* map changes no map name — so the arm below
            // would happily keep counting from before the outage and rescue-rotate a
            // map that has been running for ten seconds. Treat the outage as a fresh
            // start and re-arm the window.
            let came_back = status.server_online && !was_online;
            was_online = status.server_online;
            if came_back {
                seen = None;
            }

            // Re-arm the rescue window on every map change, however it happened.
            match (&seen, status.map.as_deref()) {
                (Some((last, _)), Some(current)) if last == current => {}
                (_, Some(current)) => seen = Some((current.to_string(), now)),
                (_, None) => seen = None,
            }

            let cooling_down = cooldown_until.is_some_and(|t| now < t);

            let decision = {
                let queue = state.rotation_queue.lock().await;
                let enabled = *state.rotation_enabled.lock().await;
                let maps = queue.get_maps();

                decide(&Tick {
                    clock: &status.clock,
                    timelimit_minutes: status.timelimit,
                    current_map: status.map.as_deref(),
                    enabled,
                    maps: &maps,
                    mode: queue.mode(),
                    cooling_down,
                    map_seen_for: seen
                        .as_ref()
                        .map(|(_, since)| now.saturating_duration_since(*since))
                        .unwrap_or_default(),
                    pick: random_pick(),
                })
            };

            let RotationDecision::Rotate(next) = decision else {
                continue;
            };

            // Start the cooldown before the await, not after: the rcon call can
            // take seconds, and nothing else guards this loop from firing again
            // underneath it.
            cooldown_until = Some(now + ROTATION_COOLDOWN);

            tracing::info!(
                "Rotating {} -> {} (elapsed={:?}s, timelimit={:?}m, anchor={:?})",
                status.map.as_deref().unwrap_or("?"),
                next,
                status.clock.elapsed_seconds,
                status.timelimit,
                status.clock.anchor,
            );
            state
                .log_stream
                .broadcast("INFO", &format!("Rotating to {}", next));

            match execute_rcon_checked(&state, &format!("map {}", next)).await {
                Ok(_) => {
                    // `note_own_map_command` already re-anchored the clock. Re-anchor
                    // our own view too, so the rescue window restarts from here even
                    // if the map name did not change.
                    seen = Some((next.clone(), Instant::now()));
                }
                Err(RconFailure::Rejected(msg)) => {
                    tracing::error!("Refused to rotate to '{}': {}", next, msg);
                    state.log_stream.broadcast("ERROR", &msg);
                }
                Err(RconFailure::Failed(e)) => {
                    tracing::warn!("Rotation to '{}' failed, will retry: {}", next, e);
                    state
                        .log_stream
                        .broadcast("ERROR", &format!("Rotation to {} failed: {}", next, e));
                }
            }
        }
    });
}

/// Reject rcon commands that would make the server load an empty map name:
/// `map` / `gamemap` with a blank argument resolves to `maps/.bsp`, which is a
/// fatal error that shuts the game down. Everything else passes through
/// untouched — this is a tripwire, not a filter.
fn validate_rcon_command(command: &str) -> Result<(), String> {
    let mut it = command.split_whitespace();
    let head = it.next().unwrap_or("").to_ascii_lowercase();
    if head == "map" || head == "gamemap" {
        let arg = it.next().unwrap_or("").trim_matches('"').trim();
        if arg.is_empty() {
            return Err(format!(
                "refusing '{head}' with an empty map name (this crashes the server on maps/.bsp)"
            ));
        }
    }
    Ok(())
}

/// Q2 map names as they appear on disk: letters, digits, underscore, hyphen.
/// Anything else could escape the quoting in `set sv_maplist "…"`.
fn valid_map_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// An rcon command we refused to send, or one the server refused to run.
enum RconFailure {
    /// Blocked before it left the process. A caller bug, not a server problem.
    Rejected(String),
    /// Sent, but the server did not answer.
    Failed(String),
}

/// Run an rcon command with the rails every caller needs.
///
/// * [`validate_rcon_command`] — the empty-map-name tripwire. `map ""` resolves
///   to `maps/.bsp`, which is fatal and shuts the game down.
/// * [`map_command_target`] + `note_own_map_command` — the only clock anchor
///   available for a restart onto the map already running, which produces no
///   map-name edge for the poller to detect.
/// * `request_refresh` — the cache is now behind the server; wake the poller so
///   the next read isn't pre-change state.
///
/// Both the admin's command box and the rotator go through here, so neither can
/// forget one.
async fn execute_rcon_checked(state: &SharedState, command: &str) -> Result<String, RconFailure> {
    validate_rcon_command(command).map_err(RconFailure::Rejected)?;

    let output = state
        .rcon_client
        .execute(command)
        .await
        .map_err(|e| RconFailure::Failed(e.to_string()))?;

    if let Some(map) = map_command_target(command) {
        state
            .status_cache
            .note_own_map_command(&map, Instant::now());
    }
    state.status_cache.request_refresh();

    Ok(output)
}

async fn rcon_execute(
    State(state): State<SharedState>,
    Json(payload): Json<ExecutePayload>,
) -> Result<Json<ExecuteResponse>, StatusCode> {
    tracing::info!("Received RCON command: {}", payload.command);

    match execute_rcon_checked(&state, &payload.command).await {
        Ok(output) => {
            tracing::info!("Command executed successfully, broadcasting logs");

            state
                .log_stream
                .broadcast("INFO", &format!("Executing: {}", payload.command));

            let cleaned_output = output.replace('\0', "").trim().to_string();

            let display_output = if cleaned_output.len() > 500 {
                format!(
                    "{}... (truncated {} chars)",
                    &cleaned_output[..500],
                    cleaned_output.len() - 500
                )
            } else {
                cleaned_output
            };

            state.log_stream.broadcast("RESPONSE", &display_output);
            tracing::info!("Logs broadcast complete");
            Ok(Json(ExecuteResponse {
                success: true,
                output,
            }))
        }
        Err(RconFailure::Rejected(msg)) => {
            tracing::warn!("Rejected RCON command '{}': {}", payload.command, msg);
            state.log_stream.broadcast("ERROR", &msg);
            Err(StatusCode::BAD_REQUEST)
        }
        Err(RconFailure::Failed(e)) => {
            state
                .log_stream
                .broadcast("ERROR", &format!("Command failed: {}", e));
            tracing::error!("RCON command failed: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn logs_ws(
    State(state): State<SharedState>,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> axum::response::Response {
    tracing::info!("WebSocket connection requested to /api/logs/ws");
    ws.on_upgrade(move |socket| {
        tracing::info!("WebSocket connected to /api/logs/ws");
        handle_websocket(socket, state.log_stream.subscribe())
    })
}

async fn handle_websocket(
    mut socket: axum::extract::ws::WebSocket,
    (mut rx, history): (logs::LogReceiver, Vec<logs::LogEntry>),
) {
    tracing::info!(
        "WebSocket handler started, sending {} history entries",
        history.len()
    );

    for entry in history {
        let json = serde_json::to_string(&entry).unwrap();
        if socket
            .send(axum::extract::ws::Message::Text(json))
            .await
            .is_err()
        {
            tracing::info!("WebSocket disconnected during history send");
            return;
        }
    }

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(entry) => {
                        let json = serde_json::to_string(&entry).unwrap();
                        tracing::debug!("Sending log entry via WebSocket: {:?}", entry);
                        if socket.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                            tracing::info!("WebSocket disconnected");
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Error receiving from log stream: {:?}", e);
                        break;
                    }
                }
            }
            _ = socket.recv() => {
                tracing::info!("WebSocket client disconnected");
                break;
            }
        }
    }
    tracing::info!("WebSocket handler ended");
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct MapList {
    maps: Vec<crate::maps::MapInfo>,
}

#[derive(Serialize)]
struct FavoritesResponse {
    favorites: Vec<String>,
}

#[derive(Deserialize)]
struct AddFavoritePayload {
    map_name: String,
}

#[derive(Serialize)]
struct AddFavoriteResponse {
    success: bool,
}

#[derive(Serialize)]
struct RemoveFavoriteResponse {
    success: bool,
}

#[derive(Deserialize)]
struct ExecutePayload {
    command: String,
}

#[derive(Serialize)]
struct ExecuteResponse {
    success: bool,
    output: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maps(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_cvar_echo() {
        assert_eq!(
            parse_cvar_echo("\"sv_maplist\" is \"q2dm1 q2dm2\"", "sv_maplist"),
            Some("q2dm1 q2dm2")
        );
    }

    #[test]
    fn parses_empty_cvar_value() {
        assert_eq!(
            parse_cvar_echo("\"sv_maplist\" is \"\"", "sv_maplist"),
            Some("")
        );
    }

    #[test]
    fn throttle_reply_is_unparseable() {
        assert_eq!(parse_cvar_echo("Bad rcon_password.", "sv_maplist"), None);
    }

    #[test]
    fn finds_echo_line_in_multiline_reply() {
        assert_eq!(
            parse_cvar_echo("foo\n\"sv_maplist\" is \"a b\"\n", "sv_maplist"),
            Some("a b")
        );
    }

    #[test]
    fn empty_live_maplist_is_drift() {
        assert!(maplist_drifted(Some(""), &maps(&["q2dm1", "q2dm2"])));
    }

    #[test]
    fn exact_match_is_not_drift() {
        assert!(!maplist_drifted(
            Some("q2dm1 q2dm2"),
            &maps(&["q2dm1", "q2dm2"])
        ));
    }

    #[test]
    fn unparseable_reply_is_never_drift() {
        assert!(!maplist_drifted(None, &maps(&["q2dm1"])));
    }

    #[test]
    fn different_order_is_drift() {
        assert!(maplist_drifted(
            Some("q2dm2 q2dm1"),
            &maps(&["q2dm1", "q2dm2"])
        ));
    }

    #[test]
    fn surrounding_whitespace_is_not_drift() {
        assert!(!maplist_drifted(
            Some(" q2dm1 q2dm2 "),
            &maps(&["q2dm1", "q2dm2"])
        ));
    }

    // The value must be comma-joined and unquoted: rcon strips quotes and re-joins
    // argv with spaces, so a quoted multi-word value reaches the server as extra
    // arguments and `set` rejects the whole command with its usage line.
    #[test]
    fn maplist_value_is_comma_joined_and_unquoted() {
        let value = sv_maplist_value(&maps(&["q2dm1", "q2dm2", "q2dm3"]));
        assert_eq!(value, "q2dm1,q2dm2,q2dm3");
        assert!(!value.contains(' '));
        assert!(!value.contains('"'));
    }

    #[test]
    fn comma_separated_live_value_is_not_drift() {
        assert!(!maplist_drifted(
            Some("q2dm1,q2dm2"),
            &maps(&["q2dm1", "q2dm2"])
        ));
    }

    // Someone setting the cvar from the server console gets spaces; the game reads
    // both the same, so that is not drift and must not trigger a re-push loop.
    #[test]
    fn separator_style_alone_is_not_drift() {
        assert!(!maplist_drifted(
            Some("q2dm1 q2dm2"),
            &maps(&["q2dm1", "q2dm2"])
        ));
    }

    #[test]
    fn rejects_map_commands_without_a_map_name() {
        for command in ["map", "map   ", "map \"\"", "gamemap", "MAP", "  Gamemap  "] {
            assert!(
                validate_rcon_command(command).is_err(),
                "should have rejected {command:?}"
            );
        }
    }

    #[test]
    fn allows_map_commands_with_a_map_name() {
        assert!(validate_rcon_command("map q2dm1").is_ok());
        assert!(validate_rcon_command("gamemap q2dm3").is_ok());
    }

    #[test]
    fn leaves_unrelated_commands_alone() {
        for command in [
            "status",
            "fraglimit 5",
            "set sv_maplist \"a b\"",
            "kick 3",
            "mapcycle foo", // "map" as a prefix of another word is not a map command
        ] {
            assert!(
                validate_rcon_command(command).is_ok(),
                "should have allowed {command:?}"
            );
        }
    }

    #[test]
    fn recognizes_map_commands_and_their_target() {
        assert_eq!(map_command_target("map q2dm1").as_deref(), Some("q2dm1"));
        assert_eq!(
            map_command_target("gamemap \"the_edge\"").as_deref(),
            Some("the_edge")
        );
        assert_eq!(map_command_target("MAP q2dm3").as_deref(), Some("q2dm3"));
    }

    #[test]
    fn non_map_commands_have_no_target() {
        for command in ["status", "kick 3", "mapcycle foo", "set sv_maplist a,b"] {
            assert_eq!(
                map_command_target(command),
                None,
                "{command:?} is not a map command"
            );
        }
        // Guarded elsewhere by validate_rcon_command, but must not anchor either.
        assert_eq!(map_command_target("map"), None);
    }

    #[test]
    fn accepts_real_map_names() {
        for name in ["q2dm1", "the_edge", "ztn2dm3-b"] {
            assert!(valid_map_name(name), "should have accepted {name:?}");
        }
    }

    #[test]
    fn rejects_map_names_that_could_escape_the_quoting() {
        for name in [
            "",
            "q2dm1\"; quit",
            "maps/q2dm1",
            "q2 dm1",
            "a;quit",
            "$foo",
        ] {
            assert!(!valid_map_name(name), "should have rejected {name:?}");
        }
        assert!(!valid_map_name(&"a".repeat(65)));
        assert!(valid_map_name(&"a".repeat(64)));
    }
}
