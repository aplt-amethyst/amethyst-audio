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
        port: 3000,
        segment_duration_sec: 10,
        max_live_segments: 5,
        output_dir: "output".to_string(),
        ..ServerConfig::default()
    };
    config.auth.enabled = false;

    let auth_state = Arc::new(AuthState::new_async(config.auth.clone()).await?);

    tracing::info!("amethyst-audio VOD server starting on port 3000 (auth disabled)");
    tracing::info!(
        "register a source: POST /api/sources {{\"id\": \"song\", \"file_path\": \"/path/to/audio.aac\"}}"
    );
    tracing::info!("then access: GET /streams/level/song/playlist.m3u8");

    run_server(config, auth_state).await
}
