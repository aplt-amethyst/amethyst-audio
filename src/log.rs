use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::sync::Mutex;

use chrono::Local;

use crate::config::LoggingConfig;

pub struct AppLogger {
    writer: Option<Mutex<BufWriter<std::fs::File>>>,
    enabled: bool,
}

impl AppLogger {
    pub fn new(config: &LoggingConfig) -> Self {
        if !config.enabled {
            return Self {
                writer: None,
                enabled: false,
            };
        }

        let file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config.file)
        {
            Ok(f) => f,
            Err(e) => {
                eprintln!("failed to open log file {}: {e}", config.file);
                return Self {
                    writer: None,
                    enabled: false,
                };
            }
        };

        Self {
            writer: Some(Mutex::new(BufWriter::new(file))),
            enabled: true,
        }
    }

    pub fn log(&self, level: &str, user: &str, ip: &str, msg: &str, result: &str) {
        if !self.enabled {
            return;
        }
        let ts = Local::now().format("%Y-%m-%d %H:%M:%S");
        let user_display = if user.is_empty() { "-" } else { user };
        let line = format!(
            "[{ts}]{level}/server<{user_display}@{ip}>:{msg}({result})\n"
        );
        if let Some(ref writer) = self.writer {
            if let Ok(mut w) = writer.lock() {
                let _ = w.write_all(line.as_bytes());
                let _ = w.flush();
            }
        }
    }

    pub fn info(&self, msg: &str) {
        self.log("INFO", "", "", msg, "ok");
    }

    pub fn request(&self, method: &str, path: &str, user: &str, ip: &str, result: &str) {
        self.log("REQUEST", user, ip, &format!("{method} {path}"), result);
    }
}
