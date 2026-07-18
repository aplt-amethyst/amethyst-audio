use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UserRole {
    Admin,
    User,
}

impl std::fmt::Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Admin => write!(f, "admin"),
            Self::User => write!(f, "user"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub role: UserRole,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub key_hash: String,
    pub key_prefix: String,
    pub created_at: i64,
    pub last_used_at: i64,
}

impl ApiKey {
    pub fn new(id: String, user_id: String, name: String, key_hash: String, key_prefix: String) -> Self {
        let now = Utc::now().timestamp();
        Self {
            id,
            user_id,
            name,
            key_hash,
            key_prefix,
            created_at: now,
            last_used_at: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteCode {
    pub id: String,
    pub code: String,
    pub created_by: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub max_uses: u32,
    pub used_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamAccessKey {
    pub id: String,
    pub source_id: String,
    pub key: String,
    pub created_by: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub max_uses: u32,
    pub use_count: u32,
    pub used_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    pub username: String,
    pub role: String,
    pub exp: usize,
    pub iat: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedUsers {
    pub users: Vec<User>,
    pub api_keys: Vec<ApiKey>,
    pub invite_codes: Vec<InviteCode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSources {
    pub sources: Vec<PersistedSourceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSourceInfo {
    pub id: String,
    pub owner_id: String,
    pub push_password_hash: String,
    pub is_public_ingest: bool,
    pub is_public_playback: bool,
    pub format: String,
    pub sample_rate: u32,
    pub bitrate_bps: u64,
    pub is_live: bool,
    pub access_keys: Vec<StreamAccessKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub password_confirm: String,
    #[serde(default)]
    pub invite_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub user: UserInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub role: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyResponse {
    pub id: String,
    pub name: String,
    pub api_key: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub created_at: i64,
    pub last_used_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInviteCodeRequest {
    pub count: Option<u32>,
    pub expires_in_days: Option<u32>,
    pub max_uses: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteCodeInfo {
    pub id: String,
    pub code: String,
    pub created_by: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub max_uses: u32,
    pub used_count: usize,
    pub is_valid: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateUserRequest {
    pub username: Option<String>,
    pub password: Option<String>,
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
    pub new_password_confirm: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteAccountRequest {
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterSourceRequest {
    pub id: String,
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub live: bool,
    #[serde(default = "default_source_bitrate")]
    pub bitrate: u64,
    #[serde(default)]
    pub push_password: Option<String>,
    #[serde(default = "default_true_bool")]
    pub is_public_ingest: bool,
    #[serde(default = "default_true_bool")]
    pub is_public_playback: bool,
}

fn default_source_bitrate() -> u64 {
    128_000
}

fn default_true_bool() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSourceRequest {
    pub push_password: Option<String>,
    pub is_public_ingest: Option<bool>,
    pub is_public_playback: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccessKeyRequest {
    pub count: Option<u32>,
    pub expires_in_days: Option<u32>,
    pub max_uses: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessKeyInfo {
    pub id: String,
    pub key: String,
    pub created_by: String,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub max_uses: u32,
    pub use_count: u32,
    pub is_valid: bool,
}

#[derive(Debug, Clone)]
pub enum CredType {
    Jwt,
    ApiKey { key_name: String },
    Anonymous,
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: String,
    pub username: String,
    pub role: UserRole,
    pub cred_type: CredType,
}
