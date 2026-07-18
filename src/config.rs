use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_segment_duration")]
    pub segment_duration_sec: u64,
    #[serde(default = "default_max_live_segments")]
    pub max_live_segments: usize,
    #[serde(default = "default_output_dir")]
    pub output_dir: String,
    #[serde(default)]
    pub logging: LoggingConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub rtmp: RtmpConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            segment_duration_sec: default_segment_duration(),
            max_live_segments: default_max_live_segments(),
            output_dir: default_output_dir(),
            logging: LoggingConfig::default(),
            auth: AuthConfig::default(),
            rtmp: RtmpConfig::default(),
        }
    }
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}
fn default_port() -> u16 {
    7024
}
fn default_segment_duration() -> u64 {
    10
}
fn default_max_live_segments() -> usize {
    5
}
fn default_output_dir() -> String {
    "output".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_server_log")]
    pub file: String,
    #[serde(default = "default_log_level")]
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            file: default_server_log(),
            level: default_log_level(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret: String,
    #[serde(default = "default_jwt_expiry")]
    pub jwt_expiry_sec: u64,
    #[serde(default = "default_true")]
    pub allow_registration: bool,
    #[serde(default)]
    pub max_streams_per_user: usize,
    #[serde(default = "default_audit_log")]
    pub audit_log: String,
    #[serde(default)]
    pub invite_code: InviteCodeConfig,
    #[serde(default)]
    pub admin_user: AdminUserConfig,
    #[serde(default)]
    pub api_key: ApiKeyConfig,
    #[serde(default = "default_users_file")]
    pub users_file: String,
    #[serde(default = "default_sources_file")]
    pub sources_file: String,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            jwt_secret: default_jwt_secret(),
            jwt_expiry_sec: default_jwt_expiry(),
            allow_registration: true,
            max_streams_per_user: 0,
            audit_log: default_audit_log(),
            invite_code: InviteCodeConfig::default(),
            admin_user: AdminUserConfig::default(),
            api_key: ApiKeyConfig::default(),
            users_file: default_users_file(),
            sources_file: default_sources_file(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteCodeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_invite_display")]
    pub display_name: String,
    #[serde(default = "default_code_length")]
    pub code_length: usize,
    #[serde(default = "default_expiry_days")]
    pub default_expiry_days: u32,
}

impl Default for InviteCodeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            display_name: default_invite_display(),
            code_length: default_code_length(),
            default_expiry_days: default_expiry_days(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUserConfig {
    #[serde(default = "default_admin_username")]
    pub username: String,
    #[serde(default = "default_admin_password")]
    pub password: String,
}

impl Default for AdminUserConfig {
    fn default() -> Self {
        Self {
            username: default_admin_username(),
            password: default_admin_password(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyConfig {
    #[serde(default = "default_api_key_prefix")]
    pub prefix: String,
    #[serde(default = "default_api_key_length")]
    pub key_length: usize,
}

impl Default for ApiKeyConfig {
    fn default() -> Self {
        Self {
            prefix: default_api_key_prefix(),
            key_length: default_api_key_length(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_server_log() -> String {
    "server.log".to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_jwt_secret() -> String {
    "change-me-in-production".to_string()
}
fn default_jwt_expiry() -> u64 {
    3600
}
fn default_audit_log() -> String {
    "auth.log".to_string()
}
fn default_users_file() -> String {
    "users.json".to_string()
}
fn default_sources_file() -> String {
    "sources.json".to_string()
}
fn default_invite_display() -> String {
    "Invite Code".to_string()
}
fn default_code_length() -> usize {
    8
}
fn default_expiry_days() -> u32 {
    7
}
fn default_admin_username() -> String {
    "admin".to_string()
}
fn default_admin_password() -> String {
    "admin12345".to_string()
}
fn default_api_key_prefix() -> String {
    "amt_".to_string()
}
fn default_api_key_length() -> usize {
    32
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RtmpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_rtmp_port")]
    pub port: u16,
    #[serde(default = "default_host")]
    pub bind_address: String,
}

impl Default for RtmpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: default_rtmp_port(),
            bind_address: default_host(),
        }
    }
}

fn default_rtmp_port() -> u16 {
    1935
}
