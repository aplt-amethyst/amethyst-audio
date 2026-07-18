use std::net::SocketAddr;

use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{delete, get, post, put};
use axum::Router;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};

use crate::auth::models::*;
use crate::auth::AuthState;
use crate::config::ServerConfig;
use crate::error::AppError;
use crate::rtmp::RtmpServer;
use crate::service::HlsService;

use super::AppState;

pub async fn run_server(config: ServerConfig, auth_state: Arc<AuthState>) -> anyhow::Result<()> {
    let service = Arc::new(HlsService::new(config.clone()));

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    if config.rtmp.enabled {
        let rtmp_auth = if config.auth.enabled { Some(auth_state.clone()) } else { None };
        let rtmp_server = RtmpServer::new(config.rtmp.clone(), service.clone(), rtmp_auth, shutdown_rx);
        let _rtmp_handle = rtmp_server.spawn();
        info!(
            rtmp_port = config.rtmp.port,
            "RTMP server started"
        );
    }

    let app_state = AppState {
        service: service.clone(),
        auth: auth_state.clone(),
        config: Arc::new(config.clone()),
    };

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/streams/level/{id}/playlist.m3u8", get(playlist_handler))
        .route("/streams/level/{id}/{segment}", get(segment_handler))
        .route(
            "/streams/level/{id}/ingest",
            post(ingest_handler).layer(DefaultBodyLimit::max(100 * 1024 * 1024)),
        )
        .route("/api/sources", get(list_sources_handler).post(register_source_handler))
        .route("/api/sources/{id}", put(update_source_handler).delete(delete_source_handler))
        .route("/api/sources/{id}/flush", post(flush_source_handler))
        .route("/api/sources/{id}/access-keys", get(list_access_keys_handler).post(create_access_key_handler))
        .route("/api/sources/{id}/access-keys/{kid}", delete(delete_access_key_handler))
        .route("/api/upload", post(upload_handler)
            .layer(DefaultBodyLimit::max(500 * 1024 * 1024)))
        .route("/api/playlists", get(playlists_handler))
        .merge(crate::auth::routes::auth_routes())
        .layer(CorsLayer::permissive())
        .with_state(app_state.clone());

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    info!(%addr, "amethyst-audio server starting");

    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    );

    let shutdown = async move {
        shutdown_signal().await;
        let _ = shutdown_tx.send(true);
    };

    server.with_graceful_shutdown(shutdown).await?;

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

fn client_ip(addr: &SocketAddr) -> String {
    addr.ip().to_string()
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "amethyst-audio",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

#[derive(serde::Deserialize)]
struct PlaylistQuery {
    access_key: Option<String>,
}

async fn playlist_handler(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
    Query(query): Query<PlaylistQuery>,
) -> Result<Response, AppError> {
    crate::auth::middleware::check_access_key(
        &state,
        &source_id,
        query.access_key.as_deref(),
    )
    .await?;

    match state.service.get_playlist(&source_id).await {
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

#[derive(serde::Deserialize)]
struct SegmentQuery {
    access_key: Option<String>,
}

async fn segment_handler(
    State(state): State<AppState>,
    Path((source_id, segment)): Path<(String, String)>,
    Query(query): Query<SegmentQuery>,
) -> Result<Response, AppError> {
    if !is_safe_segment_name(&segment) {
        return Err(AppError::BadRequest("invalid segment name".to_string()));
    }

    crate::auth::middleware::check_access_key(
        &state,
        &source_id,
        query.access_key.as_deref(),
    )
    .await?;

    match state.service.get_segment_data(&source_id, &segment).await {
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
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(source_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip = client_ip(&addr);

    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let push_pw = headers
        .get("x-push-password")
        .and_then(|v| v.to_str().ok());

    let auth_user = crate::auth::middleware::check_ingest_auth(
        &state,
        auth_header,
        push_pw,
        &source_id,
    )
    .await?;

    let sources = state.service.list_sources().await;
    let exists = sources.iter().any(|s| s.id == source_id);

    if !exists {
        if !state.config.auth.enabled {
            let default_bitrate = 128_000u64;
            state
                .service
                .register_live_source(
                    source_id.clone(),
                    default_bitrate,
                    String::new(),
                    String::new(),
                    true,
                    true,
                )
                .await;
            info!(source_id = %source_id, "auto-created live source on first ingest");
        } else {
            return Err(AppError::NotFound(format!(
                "source not found: {source_id}. register the source first before ingesting."
            )));
        }
    }

    match state.service.ingest_chunk(&source_id, &body).await {
        Ok(()) => {
            state.auth.audit.source_ingest(&auth_user.username, &ip, &source_id, "ok");
            Ok(Json(serde_json::json!({
                "status": "ok",
                "bytes_received": body.len()
            })))
        }
        Err(e) => {
            state.auth.audit.source_ingest(&auth_user.username, &ip, &source_id, "failed");
            error!(source_id = %source_id, error = %e, "ingest failed");
            Err(AppError::Internal(e.to_string()))
        }
    }
}

async fn register_source_handler(
    auth_user: AuthUser,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<RegisterSourceRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip = client_ip(&addr);

    if state.config.auth.enabled {
        let existing = state.service.list_sources().await;
        if existing.iter().any(|s| s.id == req.id) {
            return Err(AppError::BadRequest(format!(
                "source id '{}' is already taken",
                req.id
            )));
        }

        if state.config.auth.max_streams_per_user > 0 {
            let count = state.auth.count_user_sources(&auth_user.user_id).await;
            if count >= state.config.auth.max_streams_per_user {
                return Err(AppError::BadRequest(format!(
                    "maximum streams per user reached ({})",
                    state.config.auth.max_streams_per_user
                )));
            }
        }
    }

    let push_password_hash = if let Some(ref pw) = req.push_password {
        if pw.is_empty() {
            String::new()
        } else {
            state
                .auth
                .hash_password(pw)
                .map_err(|e| AppError::Internal(format!("password hashing failed: {e}")))?;
            state
                .auth
                .hash_password(pw)
                .map_err(|e| AppError::Internal(format!("password hashing failed: {e}")))?
        }
    } else {
        String::new()
    };

    let owner_id = if state.config.auth.enabled {
        auth_user.user_id.clone()
    } else {
        String::new()
    };

    if req.live {
        info!(source_id = %req.id, bitrate = req.bitrate, "registering live source");
        let info = state
            .service
            .register_live_source(
                req.id.clone(),
                req.bitrate,
                owner_id.clone(),
                push_password_hash.clone(),
                req.is_public_ingest,
                req.is_public_playback,
            )
            .await;

        let persisted = PersistedSourceInfo {
            id: info.id.clone(),
            owner_id,
            push_password_hash,
            is_public_ingest: req.is_public_ingest,
            is_public_playback: req.is_public_playback,
            format: format!("{:?}", info.format),
            sample_rate: info.sample_rate,
            bitrate_bps: info.bitrate_bps,
            is_live: true,
            access_keys: Vec::new(),
        };
        let _ = state.auth.persist_source(&persisted).await;

        state.auth.audit.source_register(&auth_user.username, &ip, &req.id, "ok");

        return Ok(Json(serde_json::json!({
            "status": "registered",
            "source": {
                "id": info.id,
                "format": format!("{:?}", info.format),
                "sample_rate": info.sample_rate,
                "bitrate_bps": info.bitrate_bps,
                "live": true,
                "owner_id": info.owner_id,
                "is_public_ingest": info.is_public_ingest,
                "is_public_playback": info.is_public_playback,
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

    let owner = owner_id.clone();
    let info = state
        .service
        .register_source(req.id.clone(), file_path, owner)
        .await
        .map_err(|e| {
            error!(error = %e, "failed to register source");
            AppError::Internal(e.to_string())
        })?;

    match state.service.generate_vod_segments(&req.id).await {
        Ok(segments) => {
            info!(source_id = %req.id, segment_count = segments.len(), "vod segments generated");
        }
        Err(e) => {
            warn!(source_id = %req.id, error = %e, "vod segment generation had issues");
        }
    }

    let persisted = PersistedSourceInfo {
        id: info.id.clone(),
        owner_id,
        push_password_hash,
        is_public_ingest: req.is_public_ingest,
        is_public_playback: req.is_public_playback,
        format: format!("{:?}", info.format),
        sample_rate: info.sample_rate,
        bitrate_bps: info.bitrate_bps,
        is_live: false,
        access_keys: Vec::new(),
    };
    let _ = state.auth.persist_source(&persisted).await;

    state.auth.audit.source_register(&auth_user.username, &ip, &req.id, "ok");

    Ok(Json(serde_json::json!({
        "status": "registered",
        "source": {
            "id": info.id,
            "format": format!("{:?}", info.format),
            "sample_rate": info.sample_rate,
            "bitrate_bps": info.bitrate_bps,
            "duration_sec": info.duration_sec,
            "owner_id": info.owner_id,
            "is_public_ingest": info.is_public_ingest,
            "is_public_playback": info.is_public_playback,
        }
    })))
}

async fn list_sources_handler(
    auth_user: AuthUser,
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let sources = state.service.list_sources().await;
    let filtered: Vec<_> = if state.config.auth.enabled && auth_user.role != UserRole::Admin {
        sources
            .into_iter()
            .filter(|s| s.owner_id == auth_user.user_id)
            .collect()
    } else {
        sources
    };

    Json(serde_json::json!({
        "sources": filtered.iter().map(|s| {
            let mut j = serde_json::json!({
                "id": s.id,
                "format": format!("{:?}", s.format),
                "sample_rate": s.sample_rate,
                "duration_sec": s.duration_sec,
                "live": s.is_live,
                "owner_id": s.owner_id,
                "is_public_ingest": s.is_public_ingest,
                "is_public_playback": s.is_public_playback,
            });
            if s.is_live {
                j.as_object_mut().unwrap().remove("duration_sec");
            }
            j
        }).collect::<Vec<_>>()
    }))
}

async fn update_source_handler(
    auth_user: AuthUser,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(source_id): Path<String>,
    Json(req): Json<UpdateSourceRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip = client_ip(&addr);

    crate::auth::middleware::check_source_ownership(&state, &auth_user, &source_id).await?;

    let push_password_hash = if let Some(ref pw) = req.push_password {
        if pw.is_empty() {
            Some(String::new())
        } else {
            let hash = state
                .auth
                .hash_password(pw)
                .map_err(|e| AppError::Internal(format!("password hashing failed: {e}")))?;
            Some(hash)
        }
    } else {
        None
    };

    state
        .service
        .update_live_source_config(
            &source_id,
            push_password_hash.clone(),
            req.is_public_ingest,
            req.is_public_playback,
        )
        .await
        .map_err(|e| AppError::NotFound(format!("source not found: {source_id}: {e}")))?;

    if let Some(ref hash) = push_password_hash {
        if let Some(mut persisted_source) = state
            .auth
            .load_sources_cache()
            .await
            .remove(&source_id)
        {
            persisted_source.push_password_hash = hash.clone();
            let _ = state.auth.persist_source(&persisted_source).await;
        }
    }

    state.auth.audit.source_update(&auth_user.username, &ip, &source_id, "ok");

    Ok(Json(serde_json::json!({
        "status": "updated",
        "source_id": source_id,
    })))
}

async fn delete_source_handler(
    auth_user: AuthUser,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip = client_ip(&addr);

    crate::auth::middleware::check_source_ownership(&state, &auth_user, &source_id).await?;

    state
        .service
        .cleanup_source_files(&source_id)
        .await
        .unwrap_or_else(|e| {
            error!(source_id = %source_id, error = %e, "failed to cleanup source files");
        });

    state.service.remove_source(&source_id).await;
    let _ = state.auth.remove_persisted_source(&source_id).await;

    state.auth.audit.source_delete(&auth_user.username, &ip, &source_id, "ok");

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "source_id": source_id,
    })))
}

async fn flush_source_handler(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    crate::auth::middleware::check_source_ownership(&state, &auth_user, &source_id).await?;

    match state.service.flush_live_buffer(&source_id).await {
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

async fn create_access_key_handler(
    auth_user: AuthUser,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path(source_id): Path<String>,
    Json(req): Json<CreateAccessKeyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip = client_ip(&addr);

    crate::auth::middleware::check_source_ownership(&state, &auth_user, &source_id).await?;

    let count = req.count.unwrap_or(1);
    if count == 0 || count > 100 {
        return Err(AppError::BadRequest("count must be 1-100".to_string()));
    }

    let now = chrono::Utc::now().timestamp();
    let days = req.expires_in_days.unwrap_or(0);
    let expires_at = if days == 0 {
        None
    } else {
        Some(now + (days as i64) * 86400)
    };
    let max_uses = req.max_uses.unwrap_or(0);

    let mut keys = Vec::new();

    for _ in 0..count {
        let key_str = state.auth.generate_access_key();
        let key_id = uuid::Uuid::new_v4().to_string();
        let access_key = StreamAccessKey {
            id: key_id.clone(),
            source_id: source_id.clone(),
            key: key_str.clone(),
            created_by: auth_user.user_id.clone(),
            created_at: now,
            expires_at,
            max_uses,
            use_count: 0,
            used_by: Vec::new(),
        };

        state
            .auth
            .access_keys
            .write()
            .await
            .insert(key_id.clone(), access_key);
        state
            .auth
            .access_keys_by_key
            .write()
            .await
            .insert(key_str.clone(), key_id);
        keys.push(key_str);
    }

    let _ = state.auth.save_sources().await;

    state.auth.audit.access_key_create(&auth_user.username, &ip, &source_id, "ok");

    Ok(Json(serde_json::json!({
        "status": "created",
        "keys": keys,
    })))
}

async fn list_access_keys_handler(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    crate::auth::middleware::check_source_ownership(&state, &auth_user, &source_id).await?;

    let all_keys = state.auth.access_keys.read().await;
    let now = chrono::Utc::now().timestamp();

    let list: Vec<_> = all_keys
        .values()
        .filter(|k| k.source_id == source_id)
        .map(|k| {
            let is_expired = k.expires_at.map(|e| now > e).unwrap_or(false);
            let is_exhausted = k.max_uses > 0 && k.use_count >= k.max_uses;
            serde_json::json!({
                "id": k.id,
                "key": k.key,
                "created_at": k.created_at,
                "expires_at": k.expires_at,
                "max_uses": k.max_uses,
                "use_count": k.use_count,
                "is_valid": !is_expired && !is_exhausted,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "access_keys": list })))
}

async fn delete_access_key_handler(
    auth_user: AuthUser,
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Path((source_id, kid)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip = client_ip(&addr);

    crate::auth::middleware::check_source_ownership(&state, &auth_user, &source_id).await?;

    let mut all_keys = state.auth.access_keys.write().await;
    let key_value = if let Some(k) = all_keys.get(&kid) {
        k.key.clone()
    } else {
        return Err(AppError::NotFound("access key not found".to_string()));
    };

    if let Some(k) = all_keys.get(&kid) {
        if k.source_id != source_id {
            return Err(AppError::NotFound("access key not found for this source".to_string()));
        }
    }

    all_keys.remove(&kid);
    drop(all_keys);

    state.auth.access_keys_by_key.write().await.remove(&key_value);
    let _ = state.auth.save_sources().await;

    state.auth.audit.access_key_revoke(&auth_user.username, &ip, &source_id, &kid, "ok");

    Ok(Json(serde_json::json!({
        "status": "revoked",
        "id": kid,
    })))
}

async fn upload_handler(
    auth_user: AuthUser,
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = crate::upload::handle_upload(
        multipart,
        state.service.clone(),
        &state.config.upload,
        auth_user.user_id.clone(),
    )
    .await?;
    Ok(Json(result))
}

async fn playlists_handler(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let sources = state.service.list_sources().await;
    let base_url = "https://stream.aplcexenicesetrl.com";

    let list: Vec<_> = sources
        .iter()
        .filter(|s| {
            if state.config.auth.enabled {
                s.is_public_playback
            } else {
                true
            }
        })
        .map(|s| {
            let url = format!("{base_url}/streams/level/{}/playlist.m3u8", s.id);
            serde_json::json!({
                "id": s.id,
                "format": format!("{:?}", s.format),
                "live": s.is_live,
                "duration_sec": if s.is_live { serde_json::Value::Null } else { serde_json::json!(s.duration_sec) },
                "sample_rate": s.sample_rate,
                "is_public_playback": s.is_public_playback,
                "owner_id": s.owner_id,
                "url": url,
            })
        })
        .collect();

    Json(serde_json::json!({
        "base_url": base_url,
        "playlists": list,
        "total": list.len(),
    }))
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
