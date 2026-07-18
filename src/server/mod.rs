pub mod routes;

use std::sync::Arc;

use crate::auth::AuthState;
use crate::config::ServerConfig;
use crate::service::HlsService;

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<HlsService>,
    pub auth: Arc<AuthState>,
    pub config: Arc<ServerConfig>,
}

impl axum::extract::FromRef<AppState> for Arc<HlsService> {
    fn from_ref(state: &AppState) -> Self {
        state.service.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<AuthState> {
    fn from_ref(state: &AppState) -> Self {
        state.auth.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<ServerConfig> {
    fn from_ref(state: &AppState) -> Self {
        state.config.clone()
    }
}

pub use routes::run_server;
