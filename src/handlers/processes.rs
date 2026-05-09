use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::collectors::system::{ProcessRow, SortBy};
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ProcQuery {
    pub sort: Option<String>,
    pub limit: Option<usize>,
}

pub async fn list(
    _user: AuthUser,
    State(state): State<AppState>,
    Query(q): Query<ProcQuery>,
) -> AppResult<Json<Vec<ProcessRow>>> {
    let sort = SortBy::parse(q.sort.as_deref().unwrap_or("cpu"));
    let limit = q.limit.unwrap_or(20).clamp(1, 200);
    Ok(Json(state.system.top_processes(sort, limit).await))
}
