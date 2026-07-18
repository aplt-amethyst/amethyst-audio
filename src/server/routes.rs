use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};

use crate::config::ServerConfig;
use crate::service::HlsService;

pub async fn run_server(config: ServerConfig) -> anyhow::Result<()> {
    let service = Arc::new(HlsService::new(config.clone()));

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/streams/level/{id}/playlist.m3u8", get(playlist_handler))
        .route("/streams/level/{id}/{segment}", get(segment_handler))
        .route(
            "/streams/level/{id}/ingest",
            post(ingest_handler).layer(DefaultBodyLimit::max(100 * 1024 * 1024)),
        )
        .route(
            "/api/sources",
            get(list_sources_handler).post(register_source_handler),
        )
        .route("/api/sources/{id}/flush", post(flush_source_handler))
        .layer(CorsLayer::permissive())
        .with_state(service);

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!(%addr, "amethyst-audio server starting");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("amethyst-audio server shut down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {
            warn!("received SIGINT, shutting down gracefully");
        }
        () = terminate => {
            warn!("received SIGTERM, shutting down gracefully");
        }
    }
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "amethyst-audio",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn playlist_handler(
    State(service): State<Arc<HlsService>>,
    Path(source_id): Path<String>,
) -> Result<Response, AppError> {
    match service.get_playlist(&source_id).await {
        Ok(playlist) => Ok((
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")],
            playlist,
        )
            .into_response()),
        Err(e) => {
            error!(source_id = %source_id, error = %e, "playlist not found");
            Err(AppError::NotFound(format!(
                "playlist not found: {source_id}"
            )))
        }
    }
}

async fn segment_handler(
    State(service): State<Arc<HlsService>>,
    Path((source_id, segment)): Path<(String, String)>,
) -> Result<Response, AppError> {
    if !is_safe_segment_name(&segment) {
        return Err(AppError::BadRequest("invalid segment name".to_string()));
    }
    match service.get_segment_data(&source_id, &segment).await {
        Ok(data) => {
            Ok((StatusCode::OK, [(header::CONTENT_TYPE, "video/mp2t")], data).into_response())
        }
        Err(e) => {
            error!(source_id = %source_id, segment = %segment, error = %e, "segment not found");
            Err(AppError::NotFound(format!("segment not found: {segment}")))
        }
    }
}

async fn ingest_handler(
    State(service): State<Arc<HlsService>>,
    Path(source_id): Path<String>,
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    let sources = service.list_sources().await;
    let exists = sources.iter().any(|s| s.id == source_id);

    if !exists {
        let default_bitrate = 128_000u64;
        service
            .register_live_source(source_id.clone(), default_bitrate)
            .await;
        info!(source_id = %source_id, "auto-created live source on first ingest");
    }

    match service.ingest_chunk(&source_id, &body).await {
        Ok(()) => Ok(Json(serde_json::json!({
            "status": "ok",
            "bytes_received": body.len()
        }))),
        Err(e) => {
            error!(source_id = %source_id, error = %e, "ingest failed");
            Err(AppError::Internal(e.to_string()))
        }
    }
}

#[derive(Debug, Deserialize)]
struct RegisterSourceRequest {
    id: String,
    #[serde(default)]
    file_path: String,
    #[serde(default)]
    live: bool,
    #[serde(default = "default_bitrate")]
    bitrate: u64,
}

fn default_bitrate() -> u64 {
    128_000
}

async fn register_source_handler(
    State(service): State<Arc<HlsService>>,
    Json(req): Json<RegisterSourceRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if req.live {
        info!(source_id = %req.id, bitrate = req.bitrate, "registering live source");
        let info = service
            .register_live_source(req.id.clone(), req.bitrate)
            .await;
        return Ok(Json(serde_json::json!({
            "status": "registered",
            "source": {
                "id": info.id,
                "format": format!("{:?}", info.format),
                "sample_rate": info.sample_rate,
                "bitrate_bps": info.bitrate_bps,
                "live": true,
            }
        })));
    }

    info!(source_id = %req.id, file_path = %req.file_path, "registering source");

    let file_path = std::path::PathBuf::from(&req.file_path);
    if !file_path.exists() {
        return Err(AppError::BadRequest(format!(
            "file not found: {}",
            req.file_path
        )));
    }

    let info = service
        .register_source(req.id.clone(), file_path)
        .await
        .map_err(|e| {
            error!(error = %e, "failed to register source");
            AppError::Internal(e.to_string())
        })?;

    match service.generate_vod_segments(&req.id).await {
        Ok(segments) => {
            info!(source_id = %req.id, segment_count = segments.len(), "vod segments generated");
        }
        Err(e) => {
            warn!(source_id = %req.id, error = %e, "vod segment generation had issues");
        }
    }

    Ok(Json(serde_json::json!({
        "status": "registered",
        "source": {
            "id": info.id,
            "format": format!("{:?}", info.format),
            "sample_rate": info.sample_rate,
            "bitrate_bps": info.bitrate_bps,
            "duration_sec": info.duration_sec,
        }
    })))
}

async fn list_sources_handler(State(service): State<Arc<HlsService>>) -> Json<serde_json::Value> {
    let sources = service.list_sources().await;
    Json(serde_json::json!({
        "sources": sources.iter().map(|s| {
            serde_json::json!({
                "id": s.id,
                "format": format!("{:?}", s.format),
                "sample_rate": s.sample_rate,
                "duration_sec": s.duration_sec,
                "live": s.is_live,
            })
        }).collect::<Vec<_>>()
    }))
}

async fn flush_source_handler(
    State(service): State<Arc<HlsService>>,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    match service.flush_live_buffer(&source_id).await {
        Ok(()) => Ok(Json(serde_json::json!({
            "status": "flushed",
            "source_id": source_id,
        }))),
        Err(e) => {
            error!(source_id = %source_id, error = %e, "flush failed");
            Err(AppError::NotFound(format!("source not found: {source_id}")))
        }
    }
}

fn is_safe_segment_name(name: &str) -> bool {
    if name.is_empty()
        || name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

#[derive(Debug)]
enum AppError {
    NotFound(String),
    BadRequest(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            Self::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}
