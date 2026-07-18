use std::sync::Arc;

use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::{error, info};

use crate::auth::AuthState;
use crate::config::RtmpConfig;
use crate::service::HlsService;

use super::session::run_session;

pub struct RtmpServer {
    config: RtmpConfig,
    service: Arc<HlsService>,
    auth: Option<Arc<AuthState>>,
    shutdown_rx: watch::Receiver<bool>,
}

impl RtmpServer {
    pub fn new(
        config: RtmpConfig,
        service: Arc<HlsService>,
        auth: Option<Arc<AuthState>>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            config,
            service,
            auth,
            shutdown_rx,
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        if !self.config.enabled {
            info!("RTMP server disabled, skipping");
            return Ok(());
        }

        let addr = format!("{}:{}", self.config.bind_address, self.config.port);
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                error!(%addr, error = %e, "failed to bind RTMP listener");
                return Ok(());
            }
        };

        info!(%addr, "RTMP server listening");

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, peer)) => {
                            let svc = self.service.clone();
                            let auth = self.auth.clone();
                            info!(%peer, "RTMP connection accepted");
                            tokio::spawn(async move {
                                if let Err(e) = run_session(stream, svc, auth).await {
                                    error!(%peer, error = %e, "RTMP session error");
                                }
                                info!(%peer, "RTMP session ended");
                            });
                        }
                        Err(e) => {
                            error!(error = %e, "RTMP accept error");
                        }
                    }
                }
                _ = self.shutdown_rx.changed() => {
                    info!("RTMP server shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            if let Err(e) = self.run().await {
                error!(error = %e, "RTMP server fatal error");
            }
        })
    }
}
