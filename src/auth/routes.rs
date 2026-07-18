use std::sync::Arc;

use axum::extract::{ConnectInfo, State};
use axum::response::Json;
use axum::routing::{delete, get, post, put};
use axum::Router;
use chrono::Utc;
use uuid::Uuid;

use crate::auth::models::*;
use crate::auth::AuthState;
use crate::error::AppError;

use std::net::SocketAddr;

pub fn auth_routes() -> Router<crate::server::AppState> {
    Router::new()
        .route("/api/auth/login", post(login_handler))
        .route("/api/auth/register", post(register_handler))
        .route("/api/invite-codes/validate/{code}", get(validate_invite_code_handler))
        .route("/api/invite-codes", get(list_invite_codes_handler).post(create_invite_code_handler))
        .route("/api/invite-codes/{id}", delete(delete_invite_code_handler))
        .route("/api/users", get(list_users_handler).post(create_user_handler))
        .route("/api/users/{id}", put(update_user_handler).delete(delete_user_handler))
        .route("/api/users/me", get(get_me_handler).put(change_password_handler).delete(delete_account_handler))
        .route("/api/users/me/api-keys", get(list_api_keys_handler).post(create_api_key_handler))
        .route("/api/users/me/api-keys/{id}", delete(delete_api_key_handler))
}

fn client_ip(addr: &SocketAddr) -> String {
    addr.ip().to_string()
}

async fn login_handler(
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip = client_ip(&addr);

    if req.username.is_empty() || req.password.is_empty() {
        auth.audit.login(&req.username, &ip, "failed:Empty credentials");
        return Err(AppError::BadRequest("username and password required".to_string()));
    }

    let user = auth
        .find_user_by_username(&req.username)
        .await
        .ok_or_else(|| AppError::Unauthorized("invalid credentials".to_string()))?;

    let valid = auth
        .verify_password(&req.password, &user.password_hash)
        .unwrap_or(false);

    if !valid {
        auth.audit.login(&req.username, &ip, "failed:Invalid password");
        return Err(AppError::Unauthorized("invalid credentials".to_string()));
    }

    let token = auth
        .create_jwt(&user.id, &user.username, &user.role.to_string())
        .map_err(|e| AppError::Internal(format!("token generation failed: {e}")))?;

    auth.audit.login(&req.username, &ip, "ok");

    Ok(Json(serde_json::json!({
        "token": token,
        "token_type": "Bearer",
        "expires_in": auth.config.jwt_expiry_sec,
        "user": {
            "id": user.id,
            "username": user.username,
            "role": user.role.to_string(),
            "created_at": user.created_at,
        }
    })))
}

async fn register_handler(
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip = client_ip(&addr);

    if !auth.config.allow_registration {
        auth.audit.register(&req.username, &ip, "failed:Registration disabled");
        return Err(AppError::Forbidden("registration is disabled".to_string()));
    }

    if req.username.trim().is_empty() {
        return Err(AppError::BadRequest("username cannot be empty".to_string()));
    }

    if req.username.len() < 3 || req.username.len() > 64 {
        return Err(AppError::BadRequest("username must be 3-64 characters".to_string()));
    }

    if req.password.len() < 8 {
        return Err(AppError::BadRequest("password must be at least 8 characters".to_string()));
    }

    if req.password != req.password_confirm {
        return Err(AppError::BadRequest("passwords do not match".to_string()));
    }

    if auth.find_user_by_username(&req.username).await.is_some() {
        auth.audit.register(&req.username, &ip, "failed:Duplicate username");
        return Err(AppError::BadRequest("username already taken".to_string()));
    }

    if auth.config.invite_code.enabled {
        let code = req.invite_code.as_deref().unwrap_or("");
        if code.is_empty() {
            return Err(AppError::BadRequest(
                format!("{} is required", auth.config.invite_code.display_name)
            ));
        }
        auth.validate_invite_code(code).await.map_err(|e| {
            AppError::BadRequest(format!("invalid {}: {e}", auth.config.invite_code.display_name))
        })?;
    }

    let id = Uuid::new_v4().to_string();
    let password_hash = auth
        .hash_password(&req.password)
        .map_err(|e| AppError::Internal(format!("password hashing failed: {e}")))?;
    let now = Utc::now().timestamp();

    let user = User {
        id: id.clone(),
        username: req.username.clone(),
        password_hash,
        role: UserRole::User,
        created_at: now,
        updated_at: now,
    };

    auth.users.write().await.insert(id.clone(), user);
    auth.users_by_username
        .write()
        .await
        .insert(req.username.clone(), id.clone());

    if auth.config.invite_code.enabled {
        if let Some(ref code) = req.invite_code {
            let _ = auth.consume_invite_code(code, &id).await;
        }
    }

    auth.save_users().await.map_err(|e| {
        AppError::Internal(format!("failed to save user data: {e}"))
    })?;

    auth.audit.register(&req.username, &ip, "ok");

    Ok(Json(serde_json::json!({
        "status": "registered",
        "id": id,
        "username": req.username,
    })))
}

async fn validate_invite_code_handler(
    State(auth): State<Arc<AuthState>>,
    axum::extract::Path(code): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    match auth.validate_invite_code(&code).await {
        Ok(invite) => {
            let now = Utc::now().timestamp();
            let is_expired = invite.expires_at.map(|e| now > e).unwrap_or(false);
            let is_exhausted = invite.max_uses > 0 && invite.used_by.len() >= invite.max_uses as usize;
            Ok(Json(serde_json::json!({
                "valid": !is_expired && !is_exhausted,
                "expires_at": invite.expires_at,
                "remaining_uses": if invite.max_uses == 0 { None } else {
                    Some(invite.max_uses.saturating_sub(invite.used_by.len() as u32))
                }
            })))
        }
        Err(_) => Ok(Json(serde_json::json!({ "valid": false }))),
    }
}

async fn create_invite_code_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<CreateInviteCodeRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if auth_user.role != UserRole::Admin {
        return Err(AppError::Forbidden("admin only".to_string()));
    }

    let ip = client_ip(&addr);
    let count = req.count.unwrap_or(1);
    if count == 0 || count > 100 {
        return Err(AppError::BadRequest("count must be 1-100".to_string()));
    }

    let now = Utc::now().timestamp();
    let days = req.expires_in_days.unwrap_or(auth.config.invite_code.default_expiry_days);
    let expires_at = if days == 0 {
        None
    } else {
        Some(now + (days as i64) * 86400)
    };
    let max_uses = req.max_uses.unwrap_or(1);

    let mut codes = Vec::new();

    for _ in 0..count {
        let code_str = auth.generate_invite_code();
        let code_id = Uuid::new_v4().to_string();
        let invite = InviteCode {
            id: code_id.clone(),
            code: code_str.clone(),
            created_by: auth_user.user_id.clone(),
            created_at: now,
            expires_at,
            max_uses,
            used_by: Vec::new(),
        };

        auth.invite_codes.write().await.insert(code_id.clone(), invite);
        auth.invite_codes_by_code
            .write()
            .await
            .insert(code_str.clone(), code_id);
        codes.push(code_str);
    }

    auth.save_users().await.map_err(|e| {
        AppError::Internal(format!("failed to save: {e}"))
    })?;

    auth.audit.invite_create(&auth_user.username, &ip, count, "ok");

    Ok(Json(serde_json::json!({
        "status": "created",
        "codes": codes,
    })))
}

async fn list_invite_codes_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    if auth_user.role != UserRole::Admin {
        return Err(AppError::Forbidden("admin only".to_string()));
    }

    let codes = auth.invite_codes.read().await;
    let now = Utc::now().timestamp();

    let list: Vec<_> = codes
        .values()
        .map(|c| {
            let is_expired = c.expires_at.map(|e| now > e).unwrap_or(false);
            let is_exhausted = c.max_uses > 0 && c.used_by.len() >= c.max_uses as usize;
            let creator = {
                let users = auth.users.try_read();
                users.map(|u| {
                    u.get(&c.created_by)
                        .map(|u| u.username.clone())
                        .unwrap_or_default()
                })
                .unwrap_or_default()
            };
            serde_json::json!({
                "id": c.id,
                "code": c.code,
                "created_by": creator,
                "created_at": c.created_at,
                "expires_at": c.expires_at,
                "max_uses": c.max_uses,
                "used_count": c.used_by.len(),
                "is_valid": !is_expired && !is_exhausted,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "invite_codes": list })))
}

async fn delete_invite_code_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if auth_user.role != UserRole::Admin {
        return Err(AppError::Forbidden("admin only".to_string()));
    }

    let ip = client_ip(&addr);

    let code_value = {
        let mut codes = auth.invite_codes.write().await;
        let code = codes
            .remove(&id)
            .ok_or_else(|| AppError::NotFound("invite code not found".to_string()))?;
        code.code.clone()
    };

    {
        let mut by_code = auth.invite_codes_by_code.write().await;
        by_code.remove(&code_value);
    }

    auth.save_users().await.map_err(|e| {
        AppError::Internal(format!("failed to save: {e}"))
    })?;

    auth.audit.invite_revoke(&auth_user.username, &ip, &code_value, "ok");

    Ok(Json(serde_json::json!({
        "status": "revoked",
        "id": id,
    })))
}

async fn list_users_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    if auth_user.role != UserRole::Admin {
        return Err(AppError::Forbidden("admin only".to_string()));
    }

    let users = auth.users.read().await;
    let list: Vec<_> = users
        .values()
        .map(|u| {
            serde_json::json!({
                "id": u.id,
                "username": u.username,
                "role": u.role.to_string(),
                "created_at": u.created_at,
                "updated_at": u.updated_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "users": list })))
}

async fn create_user_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<CreateUserRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if auth_user.role != UserRole::Admin {
        return Err(AppError::Forbidden("admin only".to_string()));
    }

    let ip = client_ip(&addr);

    if req.username.trim().is_empty() {
        return Err(AppError::BadRequest("username cannot be empty".to_string()));
    }
    if req.password.len() < 8 {
        return Err(AppError::BadRequest("password must be at least 8 characters".to_string()));
    }
    if auth.find_user_by_username(&req.username).await.is_some() {
        return Err(AppError::BadRequest("username already taken".to_string()));
    }

    let role = match req.role.as_deref().unwrap_or("user") {
        "admin" => UserRole::Admin,
        _ => UserRole::User,
    };

    let id = Uuid::new_v4().to_string();
    let role_for_response = role.clone();
    let id_for_response = id.clone();
    let password_hash = auth
        .hash_password(&req.password)
        .map_err(|e| AppError::Internal(format!("password hashing failed: {e}")))?;
    let now = Utc::now().timestamp();

    let user = User {
        id: id.clone(),
        username: req.username.clone(),
        password_hash,
        role,
        created_at: now,
        updated_at: now,
    };

    auth.users.write().await.insert(id.clone(), user);
    auth.users_by_username
        .write()
        .await
        .insert(req.username.clone(), id);
    auth.save_users().await.map_err(|e| {
        AppError::Internal(format!("failed to save: {e}"))
    })?;

    auth.audit.user_create(&auth_user.username, &ip, &req.username, "ok");

    Ok(Json(serde_json::json!({
        "status": "created",
        "id": id_for_response,
        "username": req.username,
        "role": role_for_response.to_string(),
    })))
}

async fn update_user_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if auth_user.role != UserRole::Admin {
        return Err(AppError::Forbidden("admin only".to_string()));
    }

    let ip = client_ip(&addr);

    let target_username = {
        let users = auth.users.read().await;
        users
            .get(&user_id)
            .map(|u| u.username.clone())
            .ok_or_else(|| AppError::NotFound("user not found".to_string()))?
    };

    if let Some(ref new_username) = req.username {
        if new_username.trim().is_empty() {
            return Err(AppError::BadRequest("username cannot be empty".to_string()));
        }
        if *new_username != target_username
            && auth.find_user_by_username(new_username).await.is_some()
        {
            return Err(AppError::BadRequest("username already taken".to_string()));
        }
    }

    if let Some(ref new_password) = req.password {
        if new_password.len() < 8 {
            return Err(AppError::BadRequest("password must be at least 8 characters".to_string()));
        }
    }

    let needs_admin_check = if let Some(ref new_role) = req.role {
        let target = match new_role.as_str() {
            "admin" => UserRole::Admin,
            "user" => UserRole::User,
            _ => return Err(AppError::BadRequest("invalid role".to_string())),
        };
        if target == UserRole::User {
            let users = auth.users.read().await;
            let user = users.get(&user_id).unwrap();
            user.role == UserRole::Admin
        } else {
            false
        }
    } else {
        false
    };

    if needs_admin_check && auth.count_admin_users().await <= 1 {
        return Err(AppError::BadRequest("cannot demote the last admin".to_string()));
    }

    let now = Utc::now().timestamp();
    {
        let mut users = auth.users.write().await;
        let user = users
            .get_mut(&user_id)
            .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

        if let Some(ref new_username) = req.username {
            if *new_username != user.username {
                let mut by_username = auth.users_by_username.write().await;
                by_username.remove(&user.username);
                by_username.insert(new_username.clone(), user.id.clone());
                user.username = new_username.clone();
            }
        }

        if let Some(ref new_password) = req.password {
            user.password_hash = auth
                .hash_password(new_password)
                .map_err(|e| AppError::Internal(format!("password hashing failed: {e}")))?;
        }

        if let Some(ref new_role) = req.role {
            user.role = match new_role.as_str() {
                "admin" => UserRole::Admin,
                "user" => UserRole::User,
                _ => unreachable!(),
            };
        }

        user.updated_at = now;
    }

    auth.save_users().await.map_err(|e| {
        AppError::Internal(format!("failed to save: {e}"))
    })?;

    auth.audit.user_update(&auth_user.username, &ip, &target_username, "ok");

    Ok(Json(serde_json::json!({
        "status": "updated",
        "id": user_id,
    })))
}

async fn delete_user_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if auth_user.role != UserRole::Admin {
        return Err(AppError::Forbidden("admin only".to_string()));
    }

    let ip = client_ip(&addr);

    if user_id == auth_user.user_id {
        auth.audit.user_delete(&auth_user.username, &ip, &user_id, "failed:Cannot delete self");
        return Err(AppError::BadRequest("cannot delete yourself, use account deletion instead".to_string()));
    }

    let user = {
        let users = auth.users.read().await;
        users
            .get(&user_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound("user not found".to_string()))?
    };

    if user.role == UserRole::Admin {
        let admin_count = auth.count_admin_users().await;
        if admin_count <= 1 {
            auth.audit.user_delete(&auth_user.username, &ip, &user.username, "failed:Last admin");
            return Err(AppError::BadRequest("cannot delete the last admin user".to_string()));
        }
    }

    let target_username = user.username.clone();

    auth.users.write().await.remove(&user_id);
    auth.users_by_username.write().await.remove(&target_username);

    {
        let mut api_keys = auth.api_keys.write().await;
        api_keys.retain(|_, k| k.user_id != user_id);
    }

    auth.save_users().await.map_err(|e| {
        AppError::Internal(format!("failed to save: {e}"))
    })?;

    auth.audit.user_delete(&auth_user.username, &ip, &target_username, "ok");

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "id": user_id,
    })))
}

async fn get_me_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = auth
        .find_user_by_id(&auth_user.user_id)
        .await
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    Ok(Json(serde_json::json!({
        "id": user.id,
        "username": user.username,
        "role": user.role.to_string(),
        "created_at": user.created_at,
        "updated_at": user.updated_at,
    })))
}

async fn change_password_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip = client_ip(&addr);

    if req.new_password != req.new_password_confirm {
        return Err(AppError::BadRequest("new passwords do not match".to_string()));
    }

    if req.new_password.len() < 8 {
        return Err(AppError::BadRequest("new password must be at least 8 characters".to_string()));
    }

    let mut users = auth.users.write().await;
    let user = users
        .get_mut(&auth_user.user_id)
        .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    let valid = auth
        .verify_password(&req.old_password, &user.password_hash)
        .unwrap_or(false);

    if !valid {
        auth.audit.password_change(&auth_user.username, &ip, "failed:Wrong old password");
        return Err(AppError::Unauthorized("current password is incorrect".to_string()));
    }

    user.password_hash = auth
        .hash_password(&req.new_password)
        .map_err(|e| AppError::Internal(format!("password hashing failed: {e}")))?;
    user.updated_at = Utc::now().timestamp();
    drop(users);

    auth.save_users().await.map_err(|e| {
        AppError::Internal(format!("failed to save: {e}"))
    })?;

    auth.audit.password_change(&auth_user.username, &ip, "ok");

    Ok(Json(serde_json::json!({
        "status": "password_changed",
    })))
}

async fn delete_account_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<DeleteAccountRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip = client_ip(&addr);

    let user = {
        let users = auth.users.read().await;
        users
            .get(&auth_user.user_id)
            .cloned()
            .ok_or_else(|| AppError::NotFound("user not found".to_string()))?
    };

    let valid = auth
        .verify_password(&req.password, &user.password_hash)
        .unwrap_or(false);

    if !valid {
        auth.audit.account_delete(&auth_user.username, &ip, "failed:Wrong password");
        return Err(AppError::Unauthorized("password is incorrect".to_string()));
    }

    if user.role == UserRole::Admin {
        let admin_count = auth.count_admin_users().await;
        if admin_count <= 1 {
            auth.audit.account_delete(&auth_user.username, &ip, "failed:Last admin");
            return Err(AppError::BadRequest("cannot delete the last admin account".to_string()));
        }
    }

    let username = user.username.clone();
    auth.users.write().await.remove(&auth_user.user_id);
    auth.users_by_username.write().await.remove(&username);

    {
        let mut api_keys = auth.api_keys.write().await;
        api_keys.retain(|_, k| k.user_id != auth_user.user_id);
    }

    auth.save_users().await.map_err(|e| {
        AppError::Internal(format!("failed to save: {e}"))
    })?;

    auth.audit.account_delete(&username, &ip, "ok");

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "message": "account has been deleted",
    })))
}

async fn create_api_key_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<CreateApiKeyRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip = client_ip(&addr);

    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("api key name cannot be empty".to_string()));
    }

    let (full_key, key_hash) = auth.generate_api_key();
    let key_id = Uuid::new_v4().to_string();

    let api_key = ApiKey::new(
        key_id.clone(),
        auth_user.user_id.clone(),
        req.name.clone(),
        key_hash.clone(),
        auth.config.api_key.prefix.clone(),
    );

    auth.api_keys.write().await.insert(key_hash, api_key);
    auth.save_users().await.map_err(|e| {
        AppError::Internal(format!("failed to save: {e}"))
    })?;

    auth.audit.api_key_create(&auth_user.username, &ip, &req.name, "ok");

    Ok(Json(serde_json::json!({
        "id": key_id,
        "name": req.name,
        "api_key": full_key,
        "prefix": auth.config.api_key.prefix,
        "created_at": Utc::now().timestamp(),
        "warning": "store this key securely, it will not be shown again",
    })))
}

async fn list_api_keys_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let api_keys = auth.api_keys.read().await;
    let list: Vec<_> = api_keys
        .values()
        .filter(|k| k.user_id == auth_user.user_id)
        .map(|k| {
            serde_json::json!({
                "id": k.id,
                "name": k.name,
                "prefix": k.key_prefix,
                "created_at": k.created_at,
                "last_used_at": k.last_used_at,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "api_keys": list })))
}

async fn delete_api_key_handler(
    auth_user: AuthUser,
    State(auth): State<Arc<AuthState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::extract::Path(key_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let ip = client_ip(&addr);

    let mut api_keys = auth.api_keys.write().await;
    let keys_to_remove: Vec<_> = api_keys
        .iter()
        .filter(|(_, k)| k.id == key_id && k.user_id == auth_user.user_id)
        .map(|(hash, _)| hash.clone())
        .collect();

    if keys_to_remove.is_empty() {
        return Err(AppError::NotFound("api key not found".to_string()));
    }

    for hash in &keys_to_remove {
        api_keys.remove(hash);
    }
    drop(api_keys);

    auth.save_users().await.map_err(|e| {
        AppError::Internal(format!("failed to save: {e}"))
    })?;

    auth.audit.api_key_delete(&auth_user.username, &ip, &key_id, "ok");

    Ok(Json(serde_json::json!({
        "status": "deleted",
        "id": key_id,
    })))
}
