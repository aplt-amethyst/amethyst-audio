use clap::Parser;
use std::path::PathBuf;
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
    #[arg(short, long, default_value = "/etc/amethyst-audio/config.yaml")]
    config: PathBuf,

    #[arg(long)]
    host: Option<String>,

    #[arg(long)]
    port: Option<u16>,

    #[arg(long = "segment-duration")]
    segment_duration: Option<u64>,

    #[arg(long = "max-live-segments")]
    max_live_segments: Option<usize>,

    #[arg(long = "output-dir")]
    output_dir: Option<String>,
}

fn load_config(cli: &Cli) -> anyhow::Result<ServerConfig> {
    let mut config = if cli.config.exists() {
        let content = std::fs::read_to_string(&cli.config).unwrap_or_else(|e| {
            tracing::warn!(
                path = %cli.config.display(),
                error = %e,
                "failed to read config file, using defaults"
            );
            String::new()
        });
        if content.is_empty() {
            ServerConfig::default()
        } else {
            serde_yaml::from_str::<ServerConfig>(&content).unwrap_or_else(|e| {
                tracing::warn!(
                    path = %cli.config.display(),
                    error = %e,
                    "failed to parse config file, using defaults"
                );
                ServerConfig::default()
            })
        }
    } else {
        ServerConfig::default()
    };

    if let Some(ref host) = cli.host {
        config.host = host.clone();
    }
    if let Some(port) = cli.port {
        config.port = port;
    }
    if let Some(seg) = cli.segment_duration {
        config.segment_duration_sec = seg;
    }
    if let Some(mls) = cli.max_live_segments {
        config.max_live_segments = mls;
    }
    if let Some(ref od) = cli.output_dir {
        config.output_dir = od.clone();
    }

    Ok(config)
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
    let config = load_config(&cli)?;

    info!(
        host = %config.host,
        port = config.port,
        segment_duration_sec = config.segment_duration_sec,
        max_live_segments = config.max_live_segments,
        output_dir = %config.output_dir,
        "amethyst-audio initializing"
    );

    run_server(config).await
}
