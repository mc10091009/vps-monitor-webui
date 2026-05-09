use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;

use crate::auth::AuthUser;
use crate::collectors::system::MetricsSnapshot;
use crate::error::AppResult;
use crate::state::AppState;

pub async fn current(
    _user: AuthUser,
    State(state): State<AppState>,
) -> AppResult<Json<MetricsSnapshot>> {
    Ok(Json(state.system.snapshot().await))
}

pub async fn stream(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let s = stream! {
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let snap = state.system.snapshot().await;
            let payload = serde_json::to_string(&snap).unwrap_or_else(|_| "{}".into());
            yield Ok::<_, Infallible>(Event::default().event("metrics").data(payload));
        }
    };
    Sse::new(s).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
