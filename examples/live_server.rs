use amethyst_audio::config::ServerConfig;
use amethyst_audio::server::run_server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
        .init();

    let config = ServerConfig {
        host: "0.0.0.0".to_string(),
        port: 3001,
        segment_duration_sec: 4,
        max_live_segments: 8,
        output_dir: "live_output".to_string(),
    };

    tracing::info!("amethyst-audio Live server starting on port 3001");
    tracing::info!("register a live source: POST /api/sources {{\"id\": \"stream\", \"file_path\": \"/path/to/audio.aac\"}}");
    tracing::info!("then access: GET /streams/level/stream/playlist.m3u8 (no EXT-X-ENDLIST)");

    run_server(config).await
}
