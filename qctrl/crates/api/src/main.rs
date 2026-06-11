use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use qctrl_rcon::RconClient;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod logs;
mod maps;
mod routes;
mod status;

use config::Config;
use logs::LogStream;
use maps::MapCache;
use routes::AppState;
use status::{parse_status_output, PlayerList};

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

    let state = AppState { config, rcon_client, map_cache };

    let app = Router::new()
        .route("/health", get(health))
        .route("/config", get(get_config))
        .route("/rcon/execute", post(rcon_execute))
        .route("/maps", get(list_maps))
        .route("/status", get(get_status))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Starting qctrl API on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn get_config(State(state): State<AppState>) -> Json<config::Config> {
    Json(state.config.clone())
}

async fn list_maps(State(state): State<AppState>) -> Result<Json<MapList>, StatusCode> {
    match state.map_cache.get_maps() {
        Ok(maps) => Ok(Json(MapList { maps })),
        Err(e) => {
            tracing::error!("Failed to list maps: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_status(State(state): State<AppState>) -> Result<Json<PlayerList>, StatusCode> {
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
    State(state): State<AppState>,
    Json(payload): Json<ExecutePayload>,
) -> Result<Json<ExecuteResponse>, StatusCode> {
    match state.rcon_client.execute(&payload.command).await {
        Ok(output) => Ok(Json(ExecuteResponse {
            success: true,
            output,
        })),
        Err(e) => {
            tracing::error!("RCON command failed: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[derive(Serialize)]
struct MapList {
    maps: Vec<crate::maps::MapInfo>,
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
