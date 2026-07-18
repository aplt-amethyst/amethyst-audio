use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::sync::Mutex;

use chrono::Local;

pub struct AuditLogger {
    writer: Option<Mutex<BufWriter<std::fs::File>>>,
}

impl AuditLogger {
    pub fn new(path: &str) -> Self {
        let file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(f) => f,
            Err(e) => {
                eprintln!("failed to open audit log file {path}: {e}");
                return Self { writer: None };
            }
        };

        Self {
            writer: Some(Mutex::new(BufWriter::new(file))),
        }
    }

    pub fn disabled() -> Self {
        Self { writer: None }
    }

    pub fn is_writable(&self) -> bool {
        self.writer.is_some()
    }

    fn write(&self, event: &str, username: &str, ip: &str, msg: &str, result: &str) {
        let writer = match &self.writer {
            Some(w) => w,
            None => return,
        };
        let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
        let line = format!(
            "[{ts}]{event}/server<{username}@{ip}>:{msg}({result})\n"
        );
        if let Ok(mut w) = writer.lock() {
            let _ = w.write_all(line.as_bytes());
            let _ = w.flush();
        }
    }

    pub fn login(&self, username: &str, ip: &str, result: &str) {
        let role = if username == "admin" { "Admin" } else { "User" };
        self.write("LOGIN", username, ip, &format!("{role} login"), result);
    }

    pub fn register(&self, username: &str, ip: &str, result: &str) {
        self.write("REGISTER", username, ip, "User registered", result);
    }

    pub fn api_key_create(&self, username: &str, ip: &str, key_name: &str, result: &str) {
        self.write(
            "APIKEY",
            username,
            ip,
            &format!("API key created \"{key_name}\""),
            result,
        );
    }

    pub fn api_key_delete(&self, username: &str, ip: &str, key_id: &str, result: &str) {
        self.write(
            "APIKEY_DELETE",
            username,
            ip,
            &format!("API key deleted {key_id}"),
            result,
        );
    }

    pub fn user_create(&self, actor: &str, ip: &str, target: &str, result: &str) {
        self.write(
            "USER_CREATE",
            actor,
            ip,
            &format!("User created {target}"),
            result,
        );
    }

    pub fn user_update(&self, actor: &str, ip: &str, target: &str, result: &str) {
        self.write(
            "USER_UPDATE",
            actor,
            ip,
            &format!("User updated {target}"),
            result,
        );
    }

    pub fn user_delete(&self, actor: &str, ip: &str, target: &str, result: &str) {
        self.write(
            "DELETE_USER",
            actor,
            ip,
            &format!("User deleted {target}"),
            result,
        );
    }

    pub fn account_delete(&self, username: &str, ip: &str, result: &str) {
        self.write(
            "DELETE_ACCOUNT",
            username,
            ip,
            "Account self-deleted",
            result,
        );
    }

    pub fn password_change(&self, username: &str, ip: &str, result: &str) {
        self.write(
            "PASSWORD_CHANGE",
            username,
            ip,
            "Password changed",
            result,
        );
    }

    pub fn invite_create(&self, actor: &str, ip: &str, count: u32, result: &str) {
        self.write(
            "INVITE",
            actor,
            ip,
            &format!("Invite code(s) created count={count}"),
            result,
        );
    }

    pub fn invite_revoke(&self, actor: &str, ip: &str, code: &str, result: &str) {
        self.write(
            "INVITE_REVOKE",
            actor,
            ip,
            &format!("Invite code revoked {code}"),
            result,
        );
    }

    pub fn source_ingest(&self, username: &str, ip: &str, source_id: &str, result: &str) {
        self.write(
            "SOURCE_INGEST",
            username,
            ip,
            &format!("Ingest to {source_id}"),
            result,
        );
    }

    pub fn source_delete(&self, username: &str, ip: &str, source_id: &str, result: &str) {
        self.write(
            "SOURCE_DELETE",
            username,
            ip,
            &format!("Source deleted {source_id}"),
            result,
        );
    }

    pub fn source_register(&self, username: &str, ip: &str, source_id: &str, result: &str) {
        self.write(
            "SOURCE_REGISTER",
            username,
            ip,
            &format!("Source registered {source_id}"),
            result,
        );
    }

    pub fn source_update(&self, username: &str, ip: &str, source_id: &str, result: &str) {
        self.write(
            "SOURCE_UPDATE",
            username,
            ip,
            &format!("Source updated {source_id}"),
            result,
        );
    }

    pub fn access_key_create(&self, username: &str, ip: &str, source_id: &str, result: &str) {
        self.write(
            "ACCESS_KEY",
            username,
            ip,
            &format!("Access key created for {source_id}"),
            result,
        );
    }

    pub fn access_key_revoke(&self, username: &str, ip: &str, source_id: &str, key_id: &str, result: &str) {
        self.write(
            "ACCESS_KEY_REVOKE",
            username,
            ip,
            &format!("Access key {key_id} revoked for {source_id}"),
            result,
        );
    }
}
