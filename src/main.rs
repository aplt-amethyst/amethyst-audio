use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use amethyst_audio::auth::AuthState;
use amethyst_audio::config::ServerConfig;
use amethyst_audio::log::AppLogger;
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
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn"));

    let fmt_layer = tracing_subscriber::fmt::layer()
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

    let app_logger = Arc::new(AppLogger::new(&config.logging));

    app_logger.info(&format!(
        "Server starting on {}:{}",
        config.host, config.port
    ));

    info!(
        host = %config.host,
        port = config.port,
        segment_duration_sec = config.segment_duration_sec,
        max_live_segments = config.max_live_segments,
        output_dir = %config.output_dir,
        auth_enabled = config.auth.enabled,
        "amethyst-audio initializing"
    );

    let auth_state = if config.auth.enabled {
        match AuthState::new_async(config.auth.clone()).await {
            Ok(state) => Arc::new(state),
            Err(e) => {
                tracing::error!(error = %e, "failed to initialize auth state, disabling auth");
                Arc::new(AuthState::new_async(Default::default()).await?)
            }
        }
    } else {
        Arc::new(AuthState::new_async(Default::default()).await?)
    };

    if config.auth.enabled {
        app_logger.info(&format!(
            "Auth enabled, admin user: {}",
            config.auth.admin_user.username
        ));
    } else {
        app_logger.info("Auth disabled, running in open mode");
    }

    run_server(config, auth_state).await
}
