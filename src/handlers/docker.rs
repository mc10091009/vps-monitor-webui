use axum::extract::State;
use axum::Json;

use crate::auth::AuthUser;
use crate::collectors::docker::{self, DockerContainer};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

pub async fn list(
    _user: AuthUser,
    State(state): State<AppState>,
) -> AppResult<Json<Vec<DockerContainer>>> {
    let Some(d) = state.docker.as_ref() else {
        return Ok(Json(vec![]));
    };
    let cs = docker::list(d).await.map_err(AppError::Other)?;
    Ok(Json(cs))
}
