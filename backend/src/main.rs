mod adapters;
mod auth_ctx;
mod config;
mod db;
mod error;
mod routes;
mod runtime;
mod security;
mod services;
mod settings_store;
mod state;

use crate::config::Settings;
use crate::db::{connect, ensure_permissions_and_roles};
use crate::state::AppState;
use axum::Router;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cas_backend=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let settings = Settings::load()?;
    tracing::info!(
        app = %settings.app_name,
        listen = %settings.listen_addr,
        db = %settings.database_url,
        "starting RustCAS-X backend"
    );

    let pool = connect(&settings.database_url).await?;
    ensure_permissions_and_roles(&pool).await?;
    match services::drive_probe::normalize_legacy_runtime_status(&pool).await {
        Ok(n) if n > 0 => tracing::info!(updated = n, "normalized drive runtime_status ok→active"),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "normalize drive runtime_status failed"),
    }

    let state = AppState {
        db: pool,
        settings: Arc::new(settings.clone()),
    };
    runtime::spawn_background(state.clone());

    let api = routes::api_router(state);
    let app = Router::new()
        .nest(&settings.api_prefix, api)
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    let listener = tokio::net::TcpListener::bind(&settings.listen_addr).await?;
    tracing::info!("listening on {}", settings.listen_addr);
    axum::serve(listener, app).await?;
    Ok(())
}
