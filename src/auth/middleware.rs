use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::header;

use crate::error::AppError;
use crate::server::AppState;

use super::models::{AuthUser, CredType, UserRole};

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        if !state.config.auth.enabled {
            return Ok(AuthUser {
                user_id: "anonymous".to_string(),
                username: "anonymous".to_string(),
                role: UserRole::Admin,
                cred_type: CredType::Anonymous,
            });
        }

        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let token = auth_header.strip_prefix("Bearer ").unwrap_or("");
        if token.is_empty() {
            return Err(AppError::Unauthorized("missing authorization header".to_string()));
        }

        let auth_state = &state.auth;

        if token.starts_with(&auth_state.config.api_key.prefix) {
            let hash = super::sha256_hex(token);
            let api_keys = auth_state
                .api_keys
                .try_read()
                .map_err(|_| AppError::Internal("lock error".to_string()))?;

            let api_key = api_keys
                .get(&hash)
                .ok_or_else(|| AppError::Unauthorized("invalid api key".to_string()))?;

            let user = {
                let users = auth_state
                    .users
                    .try_read()
                    .map_err(|_| AppError::Internal("lock error".to_string()))?;
                users
                    .get(&api_key.user_id)
                    .cloned()
                    .ok_or_else(|| AppError::Unauthorized("api key owner not found".to_string()))?
            };

            return Ok(AuthUser {
                user_id: user.id,
                username: user.username,
                role: user.role,
                cred_type: CredType::ApiKey {
                    key_name: api_key.name.clone(),
                },
            });
        }

        let claims = auth_state
            .verify_jwt(token)
            .map_err(|e| AppError::Unauthorized(format!("invalid token: {e}")))?;

        let role = match claims.role.as_str() {
            "admin" => UserRole::Admin,
            _ => UserRole::User,
        };

        Ok(AuthUser {
            user_id: claims.sub,
            username: claims.username,
            role,
            cred_type: CredType::Jwt,
        })
    }
}

pub async fn check_ingest_auth(
    state: &AppState,
    auth_header: Option<&str>,
    push_password_header: Option<&str>,
    source_id: &str,
) -> Result<AuthUser, AppError> {
    if !state.config.auth.enabled {
        return Ok(AuthUser {
            user_id: "anonymous".to_string(),
            username: "anonymous".to_string(),
            role: UserRole::Admin,
            cred_type: CredType::Anonymous,
        });
    }

    let source_info = state.service.get_source(source_id).await;

    if let Some(ref source) = source_info {
        if source.is_public_ingest {
            return Ok(AuthUser {
                user_id: "public_ingest".to_string(),
                username: "public".to_string(),
                role: UserRole::User,
                cred_type: CredType::Anonymous,
            });
        }
    }

    if let Some(pw) = push_password_header {
        if let Some(ref source) = source_info {
            if !source.push_password_hash.is_empty() {
                let valid = state
                    .auth
                    .verify_password(pw, &source.push_password_hash)
                    .unwrap_or(false);
                if valid {
                    return Ok(AuthUser {
                        user_id: source.owner_id.clone(),
                        username: format!("owner_of:{}", source.id),
                        role: UserRole::User,
                        cred_type: CredType::Anonymous,
                    });
                }
            }
        }
        return Err(AppError::Unauthorized("invalid push password".to_string()));
    }

    if let Some(header) = auth_header {
        let token = header.strip_prefix("Bearer ").unwrap_or("");
        if token.is_empty() {
            return Err(AppError::Unauthorized("missing token".to_string()));
        }

        let auth_state = &state.auth;

        let user = if token.starts_with(&auth_state.config.api_key.prefix) {
            let hash = super::sha256_hex(token);
            let api_keys = auth_state.api_keys.read().await;
            let api_key = api_keys
                .get(&hash)
                .ok_or_else(|| AppError::Unauthorized("invalid api key".to_string()))?;
            let users = auth_state.users.read().await;
            users
                .get(&api_key.user_id)
                .cloned()
                .ok_or_else(|| AppError::Unauthorized("api key owner not found".to_string()))?
        } else {
            let claims = auth_state
                .verify_jwt(token)
                .map_err(|e| AppError::Unauthorized(format!("invalid token: {e}")))?;
            let users = auth_state.users.read().await;
            users
                .get(&claims.sub)
                .cloned()
                .ok_or_else(|| AppError::Unauthorized("user not found".to_string()))?
        };

        let is_admin = user.role == UserRole::Admin;
        let is_owner = source_info
            .as_ref()
            .map(|s| s.owner_id == user.id)
            .unwrap_or(false);

        if is_owner || is_admin {
            return Ok(AuthUser {
                user_id: user.id,
                username: user.username,
                role: user.role,
                cred_type: CredType::Jwt,
            });
        }

        return Err(AppError::Forbidden("not authorized to push to this source".to_string()));
    }

    Err(AppError::Unauthorized("authentication required".to_string()))
}

pub async fn check_source_ownership(
    state: &AppState,
    auth_user: &AuthUser,
    source_id: &str,
) -> Result<(), AppError> {
    if !state.config.auth.enabled {
        return Ok(());
    }

    let source = state
        .service
        .get_source(source_id)
        .await
        .ok_or_else(|| AppError::NotFound(format!("source not found: {source_id}")))?;

    if auth_user.role == UserRole::Admin {
        return Ok(());
    }

    if source.owner_id == auth_user.user_id {
        return Ok(());
    }

    Err(AppError::Forbidden("not authorized for this source".to_string()))
}

pub async fn check_access_key(
    state: &AppState,
    source_id: &str,
    key: Option<&str>,
) -> Result<bool, AppError> {
    if !state.config.auth.enabled {
        return Ok(true);
    }

    let source = match state.service.get_source(source_id).await {
        Some(s) => s,
        None => return Ok(true),
    };

    if source.is_public_playback {
        return Ok(true);
    }

    let key = key.unwrap_or("");
    if key.is_empty() {
        return Err(AppError::Unauthorized("access key required for private stream".to_string()));
    }

    if state.auth.validate_access_key(source_id, key).await.unwrap_or(false) {
        return Ok(true);
    }

    Err(AppError::Unauthorized("invalid or expired access key".to_string()))
}
