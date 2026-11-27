mod auth;
mod dashboard;
mod drive_accounts;
mod health;
mod magic_regex;
mod media;
mod notifications;
mod openlist;
mod resource_search;
mod roles;
mod setup;
mod stubs;
mod sync_tasks;
mod system_settings;
mod tasks;
mod tmdb;
mod tmdb_cache;
mod users;

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
        .nest("/drive-accounts", drive_accounts::router())
        .nest("/users", users::router())
        .nest("/roles", roles::roles_router())
        .nest("/permissions", roles::permissions_router())
        .nest("/system-settings", system_settings::router())
        .nest("/tmdb", tmdb::router().merge(tmdb_cache::router()))
        .nest("/magic-regex", magic_regex::router())
        .nest("/openlist", openlist::router())
        .nest("/notifications", notifications::router())
        .nest("/resource-search", resource_search::router())
        .nest("/media", media::router())
        .merge(stubs::router())
        .with_state(state)
}
