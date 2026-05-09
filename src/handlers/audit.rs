use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::db::audit::{self, AuditEntry};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct AuditQuery {
    pub limit: Option<i64>,
}

pub async fn list(
    _user: AuthUser,
    State(state): State<AppState>,
    Query(q): Query<AuditQuery>,
) -> AppResult<Json<Vec<AuditEntry>>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let pool = state.db.clone();
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<AuditEntry>, AppError> {
        let conn = pool.get()?;
        Ok(audit::list(&conn, limit).map_err(AppError::Other)?)
    })
    .await
    .map_err(|e| AppError::Other(e.into()))??;
    Ok(Json(rows))
}
