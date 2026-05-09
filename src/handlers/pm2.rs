use axum::extract::State;
use axum::Json;

use crate::auth::AuthUser;
use crate::collectors::pm2::{self, Pm2App};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub async fn list(
    _user: AuthUser,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<Pm2App>>> {
    let apps = pm2::list(&state.cfg.pm2_homes).await.map_err(AppError::Other)?;
    Ok(Json(apps))
}
