use std::sync::Arc;

use amethyst_audio::auth::AuthState;
use amethyst_audio::config::ServerConfig;
use amethyst_audio::server::run_server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
        .init();

    let mut config = ServerConfig {
        host: "0.0.0.0".to_string(),
        port: 3001,
        segment_duration_sec: 4,
        max_live_segments: 8,
        output_dir: "live_output".to_string(),
        ..ServerConfig::default()
    };
    config.auth.enabled = false;

    let auth_state = Arc::new(AuthState::new_async(config.auth.clone()).await?);

    tracing::info!("amethyst-audio Live server starting on port 3001 (auth disabled)");
    tracing::info!("register a live source: POST /api/sources {{\"id\": \"stream\", \"live\": true, \"bitrate\": 128000}}");
    tracing::info!("then access: GET /streams/level/stream/playlist.m3u8 (no EXT-X-ENDLIST)");

    run_server(config, auth_state).await
}
