use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::collectors::systemd::{self, SystemdService};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ServicesQuery {
    pub state: Option<String>,
}

pub async fn list(
    _user: AuthUser,
    State(_state): State<AppState>,
    Query(q): Query<ServicesQuery>,
) -> AppResult<Json<Vec<SystemdService>>> {
    let mut svcs = systemd::list().await.map_err(AppError::Other)?;
    if let Some(filter) = q.state.as_deref() {
        let f = filter.to_lowercase();
        svcs.retain(|s| s.active.eq_ignore_ascii_case(&f) || s.sub.eq_ignore_ascii_case(&f));
    }
    Ok(Json(svcs))
}
