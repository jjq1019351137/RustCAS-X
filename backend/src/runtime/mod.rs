pub mod deferred_delete;
pub mod drama;
pub mod sync;

use crate::state::AppState;
use std::time::Duration;
use tokio::time::interval;

/// Background loops for scheduler / deferred deletes (Wave 3–4 skeleton).
pub fn spawn_background(state: AppState) {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(30));
        loop {
            tick.tick().await;
            if let Err(err) = deferred_delete::process_due(&state).await {
                tracing::warn!(error = %err, "deferred sync delete tick failed");
            }
        }
    });
}
