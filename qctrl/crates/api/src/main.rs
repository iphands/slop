use axum::{
    extract::State,
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use qctrl_rcon::RconClient;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod favorites;
mod logs;
mod maps;
mod status;

use config::Config;
use favorites::Favorites;
use logs::LogStream;
use maps::MapCache;
use status::{parse_status_output, PlayerList};

#[derive(Clone)]
struct SharedState {
    config: Config,
    rcon_client: Arc<RconClient>,
    map_cache: MapCache,
    log_stream: Arc<LogStream>,
    favorites: Favorites,
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

    let map_cache = MapCache::new(&config.paths.baseq2);
    let log_stream = Arc::new(LogStream::new(1000));
    let favorites = Favorites::new("favorites.json").unwrap_or_else(|e| {
        tracing::warn!("Failed to initialize favorites: {}, using empty favorites", e);
        Favorites::new("favorites.json").unwrap_or_else(|_| {
            // Last resort: create empty favorites
            let _ = std::fs::write("favorites.json", "[]");
            Favorites::new("favorites.json").unwrap()
        })
    });

    let state = SharedState {
        config,
        rcon_client,
        map_cache,
        log_stream,
        favorites,
    };

    let api_routes = Router::new()
        .route("/health", get(health))
        .route("/config", get(get_config))
        .route("/rcon/execute", post(rcon_execute))
        .route("/maps", get(list_maps))
        .route("/favorites", get(get_favorites))
        .route("/favorites", post(add_favorite))
        .route("/favorites/:map_name", delete(remove_favorite))
        .route("/status", get(get_status))
        .route("/logs/ws", get(logs_ws))
        .with_state(state);

    let static_files = ServeDir::new("frontend/dist")
        .not_found_service(ServeDir::new("frontend/dist").append_index_html_on_directories(true));

    let app = Router::new()
        .nest("/api", api_routes)
        .nest_service("/", static_files)
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

async fn get_status(State(state): State<SharedState>) -> Result<Json<PlayerList>, StatusCode> {
    match state.rcon_client.execute("status").await {
        Ok(output) => match parse_status_output(&output) {
            Ok(players) => Ok(Json(players)),
            Err(e) => {
                tracing::error!("Failed to parse status: {}", e);
                Err(StatusCode::INTERNAL_SERVER_ERROR)
            }
        },
        Err(e) => {
            tracing::error!("Failed to get status: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn rcon_execute(
    State(state): State<SharedState>,
    Json(payload): Json<ExecutePayload>,
) -> Result<Json<ExecuteResponse>, StatusCode> {
    match state.rcon_client.execute(&payload.command).await {
        Ok(output) => {
            state.log_stream.broadcast("INFO", &format!("Executing: {}", payload.command));
            Ok(Json(ExecuteResponse {
                success: true,
                output,
            }))
        }
        Err(e) => {
            state.log_stream.broadcast("ERROR", &format!("Command failed: {}", e));
            tracing::error!("RCON command failed: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn logs_ws(
    State(state): State<SharedState>,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| handle_websocket(socket, state.log_stream.subscribe()))
}

async fn handle_websocket(
    mut socket: axum::extract::ws::WebSocket,
    mut rx: logs::LogReceiver,
) {
    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(entry) => {
                        let json = serde_json::to_string(&entry).unwrap();
                        if socket.send(axum::extract::ws::Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            _ = socket.recv() => {
                break;
            }
        }
    }
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
