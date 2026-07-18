use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub segment_duration_sec: u64,
    pub max_live_segments: usize,
    pub output_dir: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
            segment_duration_sec: 10,
            max_live_segments: 5,
            output_dir: "output".to_string(),
        }
    }
}
