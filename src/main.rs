use clap::Parser;
use tracing::info;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use amethyst_audio::config::ServerConfig;
use amethyst_audio::server::run_server;

#[derive(Parser)]
#[command(
    name = "amethyst-audio",
    version,
    about = "HLS stream server with pure Rust MPEG-TS muxer"
)]
struct Cli {
    #[arg(short, long, default_value = "0.0.0.0")]
    host: String,

    #[arg(short, long, default_value = "3000")]
    port: u16,

    #[arg(short, long, default_value_t = 10)]
    segment_duration: u64,

    #[arg(short, long, default_value = "output")]
    output_dir: String,
}

fn init_logging() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_span_events(FmtSpan::CLOSE)
        .with_target(true)
        .with_file(true)
        .with_line_number(true);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();

    let cli = Cli::parse();

    let config = ServerConfig {
        host: cli.host,
        port: cli.port,
        segment_duration_sec: cli.segment_duration,
        output_dir: cli.output_dir,
        ..ServerConfig::default()
    };

    info!(
        host = %config.host,
        port = config.port,
        segment_duration_sec = config.segment_duration_sec,
        "amethyst-audio initializing"
    );

    run_server(config).await
}
