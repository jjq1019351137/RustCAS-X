mod auth;
mod dashboard;
mod health;
mod setup;
mod stubs;
mod sync_tasks;
mod tasks;

use crate::state::AppState;
use axum::Router;

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .merge(health::router())
        .nest("/auth", auth::router())
        .nest("/setup", setup::router())
        .nest("/dashboard", dashboard::router())
        .nest("/tasks", tasks::router())
        .nest("/sync-tasks", sync_tasks::router())
        .merge(stubs::router())
        .with_state(state)
}
