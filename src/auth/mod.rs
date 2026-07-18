pub mod audit;
pub mod middleware;
pub mod models;
pub mod routes;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chrono::Utc;
use jsonwebtoken::{encode, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::AuthConfig;
use crate::config::AuthConfig as FullAuthConfig;

use self::audit::AuditLogger;
use self::models::*;

pub struct AuthState {
    pub config: AuthConfig,
    pub jwt_secret: String,
    pub users: RwLock<HashMap<String, User>>,
    pub users_by_username: RwLock<HashMap<String, String>>,
    pub api_keys: RwLock<HashMap<String, ApiKey>>,
    pub invite_codes: RwLock<HashMap<String, InviteCode>>,
    pub invite_codes_by_code: RwLock<HashMap<String, String>>,
    pub access_keys: RwLock<HashMap<String, StreamAccessKey>>,
    pub access_keys_by_key: RwLock<HashMap<String, String>>,
    pub users_file: String,
    pub sources_file: String,
    pub audit: Arc<AuditLogger>,
}

impl AuthState {
    pub async fn new_async(config: AuthConfig) -> Result<Self> {
        let audit = Arc::new(AuditLogger::new(&config.audit_log));
        if !audit.is_writable() {
            tracing::warn!(
                path = %config.audit_log,
                "audit log file not writable, audit events will be discarded"
            );
        }

        let state = Self {
            config: config.clone(),
            jwt_secret: config.jwt_secret.clone(),
            users: RwLock::new(HashMap::new()),
            users_by_username: RwLock::new(HashMap::new()),
            api_keys: RwLock::new(HashMap::new()),
            invite_codes: RwLock::new(HashMap::new()),
            invite_codes_by_code: RwLock::new(HashMap::new()),
            access_keys: RwLock::new(HashMap::new()),
            access_keys_by_key: RwLock::new(HashMap::new()),
            users_file: config.users_file.clone(),
            sources_file: config.sources_file.clone(),
            audit,
        };

        if let Err(e) = state.load_users().await {
            tracing::warn!("could not load users file, creating new: {}", e);
        }

        if state.users.read().await.is_empty() {
            state.create_admin_user(&config).await?;
        }

        Ok(state)
    }

    pub async fn config(&self) -> &FullAuthConfig {
        &self.config
    }

    pub fn hash_password(&self, password: &str) -> Result<String> {
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))?;
        Ok(hash.to_string())
    }

    pub fn verify_password(&self, password: &str, hash_str: &str) -> Result<bool> {
        let parsed_hash = PasswordHash::new(hash_str)
            .map_err(|e| anyhow::anyhow!("invalid password hash: {e}"))?;
        let argon2 = Argon2::default();
        Ok(argon2
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    }

    pub fn create_jwt(&self, user_id: &str, username: &str, role: &str) -> Result<String> {
        let now = Utc::now().timestamp() as usize;
        let claims = JwtClaims {
            sub: user_id.to_string(),
            username: username.to_string(),
            role: role.to_string(),
            exp: now + self.config.jwt_expiry_sec as usize,
            iat: now,
        };
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )
        .map_err(|e| anyhow::anyhow!("JWT encoding failed: {e}"))
    }

    pub fn verify_jwt(&self, token: &str) -> Result<JwtClaims> {
        let token_data = jsonwebtoken::decode::<JwtClaims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|e| anyhow::anyhow!("JWT validation failed: {e}"))?;
        Ok(token_data.claims)
    }

    pub fn generate_api_key(&self) -> (String, String) {
        let random_part = random_alphanumeric(self.config.api_key.key_length);
        let full_key = format!("{}{}", self.config.api_key.prefix, random_part);
        let hash = sha256_hex(&full_key);
        (full_key, hash)
    }

    pub fn verify_api_key_hash(&self, key: &str) -> String {
        sha256_hex(key)
    }

    pub fn generate_invite_code(&self) -> String {
        random_alphanumeric(self.config.invite_code.code_length)
    }

    pub fn generate_access_key(&self) -> String {
        random_alphanumeric(24)
    }

    async fn create_admin_user(&self, config: &AuthConfig) -> Result<()> {
        let id = Uuid::new_v4().to_string();
        let password_hash = self.hash_password(&config.admin_user.password)?;
        let now = Utc::now().timestamp();
        let admin = User {
            id: id.clone(),
            username: config.admin_user.username.clone(),
            password_hash,
            role: UserRole::Admin,
            created_at: now,
            updated_at: now,
        };
        self.users.write().await.insert(id.clone(), admin);
        self.users_by_username
            .write()
            .await
            .insert(config.admin_user.username.clone(), id);
        self.save_users().await?;
        tracing::info!(
            username = %config.admin_user.username,
            "admin user created from config"
        );
        Ok(())
    }

    pub async fn find_user_by_username(&self, username: &str) -> Option<User> {
        let by_username = self.users_by_username.read().await;
        let user_id = by_username.get(username)?;
        let users = self.users.read().await;
        users.get(user_id).cloned()
    }

    pub async fn find_user_by_id(&self, user_id: &str) -> Option<User> {
        let users = self.users.read().await;
        users.get(user_id).cloned()
    }

    pub async fn validate_invite_code(&self, code: &str) -> Result<InviteCode> {
        let by_code = self.invite_codes_by_code.read().await;
        let code_id = by_code
            .get(code)
            .with_context(|| "invalid invite code".to_string())?;

        let codes = self.invite_codes.read().await;
        let invite = codes
            .get(code_id)
            .cloned()
            .with_context(|| "invite code not found".to_string())?;

        let now = Utc::now().timestamp();
        if let Some(exp) = invite.expires_at {
            if now > exp {
                anyhow::bail!("invite code expired");
            }
        }
        if invite.max_uses > 0 && invite.used_by.len() >= invite.max_uses as usize {
            anyhow::bail!("invite code usage limit reached");
        }

        Ok(invite)
    }

    pub async fn consume_invite_code(&self, code: &str, user_id: &str) -> Result<()> {
        let by_code = self.invite_codes_by_code.read().await;
        let code_id = by_code
            .get(code)
            .cloned()
            .with_context(|| "invalid invite code".to_string())?;
        drop(by_code);

        let mut codes = self.invite_codes.write().await;
        if let Some(invite) = codes.get_mut(&code_id) {
            invite.used_by.push(user_id.to_string());
        }
        self.save_users().await?;
        Ok(())
    }

    pub async fn validate_access_key(&self, source_id: &str, key: &str) -> Result<bool> {
        let by_key = self.access_keys_by_key.read().await;
        let key_id = match by_key.get(key) {
            Some(id) => id.clone(),
            None => return Ok(false),
        };
        drop(by_key);

        let mut keys = self.access_keys.write().await;
        let access_key = match keys.get_mut(&key_id) {
            Some(k) => k,
            None => return Ok(false),
        };

        if access_key.source_id != source_id {
            return Ok(false);
        }

        let now = Utc::now().timestamp();
        if let Some(exp) = access_key.expires_at {
            if now > exp {
                return Ok(false);
            }
        }
        if access_key.max_uses > 0 && access_key.use_count >= access_key.max_uses {
            return Ok(false);
        }

        access_key.use_count += 1;
        self.save_sources().await?;
        Ok(true)
    }

    pub async fn is_admin(&self, user_id: &str) -> bool {
        self.find_user_by_id(user_id)
            .await
            .map(|u| u.role == UserRole::Admin)
            .unwrap_or(false)
    }

    pub async fn count_admin_users(&self) -> usize {
        self.users
            .read()
            .await
            .values()
            .filter(|u| u.role == UserRole::Admin)
            .count()
    }

    pub async fn count_user_sources(&self, user_id: &str) -> usize {
        self.load_sources_cache()
            .await
            .values()
            .filter(|s| s.owner_id == user_id)
            .count()
    }
}

impl AuthState {
    pub async fn load_users(&self) -> Result<()> {
        let content = match std::fs::read_to_string(&self.users_file) {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };
        if content.trim().is_empty() {
            return Ok(());
        }
        let persisted: PersistedUsers = serde_json::from_str(&content)
            .context("failed to parse users file")?;

        let mut users = self.users.write().await;
        let mut by_username = self.users_by_username.write().await;
        for user in &persisted.users {
            by_username.insert(user.username.clone(), user.id.clone());
            users.insert(user.id.clone(), user.clone());
        }

        let mut api_keys = self.api_keys.write().await;
        for key in &persisted.api_keys {
            api_keys.insert(key.key_hash.clone(), key.clone());
        }

        let mut codes = self.invite_codes.write().await;
        let mut codes_by_code = self.invite_codes_by_code.write().await;
        for code in &persisted.invite_codes {
            codes_by_code.insert(code.code.clone(), code.id.clone());
            codes.insert(code.id.clone(), code.clone());
        }

        tracing::info!(
            users = persisted.users.len(),
            api_keys = persisted.api_keys.len(),
            invite_codes = persisted.invite_codes.len(),
            "loaded users state"
        );
        Ok(())
    }

    pub async fn save_users(&self) -> Result<()> {
        let users = self.users.read().await.values().cloned().collect();
        let api_keys = self.api_keys.read().await.values().cloned().collect();
        let invite_codes = self.invite_codes.read().await.values().cloned().collect();

        let persisted = PersistedUsers {
            users,
            api_keys,
            invite_codes,
        };
        let json = serde_json::to_string_pretty(&persisted)?;
        std::fs::write(&self.users_file, json)?;
        Ok(())
    }

    pub async fn load_sources_cache(&self) -> HashMap<String, PersistedSourceInfo> {
        let content = match std::fs::read_to_string(&self.sources_file) {
            Ok(c) => c,
            Err(_) => return HashMap::new(),
        };
        if content.trim().is_empty() {
            return HashMap::new();
        }
        let persisted: PersistedSources = match serde_json::from_str(&content) {
            Ok(p) => p,
            Err(_) => return HashMap::new(),
        };
        let mut map = HashMap::new();
        for src in persisted.sources {
            map.insert(src.id.clone(), src);
        }
        map
    }

    pub async fn save_sources(&self) -> Result<()> {
        let all_keys = self.access_keys.read().await;
        let mut source_keys: HashMap<String, Vec<StreamAccessKey>> = HashMap::new();
        for key in all_keys.values() {
            source_keys
                .entry(key.source_id.clone())
                .or_default()
                .push(key.clone());
        }
        drop(all_keys);

        match std::fs::read_to_string(&self.sources_file) {
            Ok(content) if !content.trim().is_empty() => {
                if let Ok(mut persisted) = serde_json::from_str::<PersistedSources>(&content) {
                    for key_list in source_keys {
                        for src in &mut persisted.sources {
                            if src.id == key_list.0 {
                                src.access_keys = key_list.1.clone();
                            }
                        }
                    }
                    let json = serde_json::to_string_pretty(&persisted)?;
                    std::fs::write(&self.sources_file, json)?;
                    return Ok(());
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub async fn persist_source(&self, info: &PersistedSourceInfo) -> Result<()> {
        let mut persisted = match std::fs::read_to_string(&self.sources_file) {
            Ok(content) if !content.trim().is_empty() => {
                serde_json::from_str::<PersistedSources>(&content).unwrap_or(PersistedSources {
                    sources: vec![],
                })
            }
            _ => PersistedSources { sources: vec![] },
        };

        persisted.sources.retain(|s| s.id != info.id);
        persisted.sources.push(info.clone());
        let json = serde_json::to_string_pretty(&persisted)?;
        std::fs::write(&self.sources_file, json)?;
        Ok(())
    }

    pub async fn remove_persisted_source(&self, source_id: &str) -> Result<()> {
        let mut persisted = match std::fs::read_to_string(&self.sources_file) {
            Ok(content) if !content.trim().is_empty() => {
                serde_json::from_str::<PersistedSources>(&content).unwrap_or(PersistedSources {
                    sources: vec![],
                })
            }
            _ => return Ok(()),
        };

        persisted.sources.retain(|s| s.id != source_id);
        let json = serde_json::to_string_pretty(&persisted)?;
        std::fs::write(&self.sources_file, json)?;
        Ok(())
    }
}

fn random_alphanumeric(length: usize) -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}

pub fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}
