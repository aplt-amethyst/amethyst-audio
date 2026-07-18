use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::Multipart;
use tracing::{debug, info, warn};

use crate::config::UploadConfig;
use crate::error::AppError;
use crate::service::HlsService;

const ALLOWED_EXTENSIONS: &[&str] = &["aac", "mp3", "wav", "flac", "adts"];

pub async fn handle_upload(
    mut multipart: Multipart,
    service: Arc<HlsService>,
    upload_config: &UploadConfig,
    owner_id: String,
) -> Result<serde_json::Value, AppError> {
    let upload_dir = PathBuf::from(&upload_config.dir);
    std::fs::create_dir_all(&upload_dir)
        .context("failed to create upload directory")
        .map_err(|e| AppError::Internal(format!("upload directory error: {e}")))?;

    let mut source_id = String::new();
    let mut saved_path: Option<PathBuf> = None;
    let mut file_size: u64 = 0;
    let max_size = (upload_config.max_size_mb as u64) * 1024 * 1024;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "id" => {
                source_id = field.text().await.unwrap_or_default();
            }
            "file" => {
                let filename = field
                    .file_name()
                    .unwrap_or("unknown.bin")
                    .to_string();

                validate_filename(&filename)
                    .map_err(AppError::BadRequest)?;

                let ext = PathBuf::from(&filename)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");

                if !ALLOWED_EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
                    return Err(AppError::BadRequest(format!(
                        "unsupported format: {ext}. supported: {}",
                        ALLOWED_EXTENSIONS.join(", ")
                    )));
                }

                let dest_name = if source_id.is_empty() {
                    let stem = PathBuf::from(&filename)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("upload");
                    source_id = stem.to_string();
                    format!("{stem}.{ext}")
                } else {
                    format!("{source_id}.{ext}")
                };

                let dest_path = upload_dir.join(&dest_name);

                let data = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("failed to read upload: {e}")))?;

                file_size = data.len() as u64;
                if file_size > max_size {
                    return Err(AppError::BadRequest(format!(
                        "file too large: {} bytes (max {} MB)",
                        file_size, upload_config.max_size_mb
                    )));
                }

                std::fs::write(&dest_path, &data)
                    .context("failed to write uploaded file")
                    .map_err(|e| AppError::Internal(format!("write error: {e}")))?;

                saved_path = Some(dest_path);
                info!(filename = %filename, size = file_size, path = %saved_path.as_ref().unwrap().display(), "file uploaded");
            }
            _ => {
                let _ = field.text().await;
            }
        }
    }

    let file_path = saved_path.ok_or_else(|| AppError::BadRequest("no file provided".to_string()))?;

    let source_id = if source_id.is_empty() {
        file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("upload")
            .to_string()
    } else {
        source_id
    };

    let source_id = sanitize_id(&source_id);

    let source = match service
        .register_source(source_id.clone(), file_path.clone(), owner_id)
        .await
    {
        Ok(info) => {
            match service.generate_vod_segments(&source_id).await {
                Ok(segments) => {
                    info!(source_id = %source_id, segment_count = segments.len(), "vod segments generated from upload");
                }
                Err(e) => {
                    warn!(source_id = %source_id, error = %e, "vod segment generation had issues");
                }
            }
            Some(info)
        }
        Err(e) => {
            warn!(error = %e, "register source failed, removing uploaded file");
            let _ = std::fs::remove_file(&file_path);
            return Err(AppError::Internal(format!("failed to register source: {e}")));
        }
    };

    Ok(serde_json::json!({
        "status": "uploaded",
        "id": source_id,
        "filename": file_path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown"),
        "file_path": file_path.to_string_lossy(),
        "size": file_size,
        "source": {
            "id": source.id,
            "format": format!("{:?}", source.format),
            "sample_rate": source.sample_rate,
            "bitrate_bps": source.bitrate_bps,
            "duration_sec": source.duration_sec,
            "live": false,
    }
    }))
}

fn validate_filename(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
    {
        return Err("invalid filename (path traversal detected)".to_string());
    }
    Ok(())
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}
