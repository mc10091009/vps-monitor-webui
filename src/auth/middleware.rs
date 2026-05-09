use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum_extra::extract::cookie::CookieJar;

use crate::error::AppError;
use crate::state::AppState;

use super::session::COOKIE_NAME;

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: i64,
    pub username: String,
    pub token: String,
    pub ip: Option<String>,
}

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(COOKIE_NAME)
            .map(|c| c.value().to_string())
            .ok_or(AppError::Unauthorized)?;

        let ip = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string())
            .or_else(|| {
                parts
                    .extensions
                    .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                    .map(|c| c.0.ip().to_string())
            });

        // Validate session
        let pool = state.db.clone();
        let token_clone = token.clone();
        let ttl = state.cfg.session_ttl_secs as i64;
        let session = tokio::task::spawn_blocking(move || -> Result<_, AppError> {
            let conn = pool.get()?;
            let s = crate::db::sessions::find_active(&conn, &token_clone)
                .map_err(AppError::Other)?
                .ok_or(AppError::Unauthorized)?;

            // Sliding expiry: extend if near expiration
            let now = crate::db::now_unix();
            let new_exp = if s.expires_at - now < ttl / 2 {
                Some(now + ttl)
            } else {
                None
            };
            crate::db::sessions::touch(&conn, &token_clone, new_exp).map_err(AppError::Other)?;
            Ok(s)
        })
        .await
        .map_err(|e| AppError::Other(e.into()))??;

        Ok(AuthUser {
            user_id: session.user_id,
            username: session.username,
            token,
            ip,
        })
    }
}

/// Verifies an X-Requested-With header is present (CSRF mitigation for mutating endpoints).
pub fn ensure_xhr(parts: &Parts) -> Result<(), AppError> {
    let ok = parts
        .headers
        .get("x-requested-with")
        .and_then(|h| h.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("fetch") || v.eq_ignore_ascii_case("xmlhttprequest"))
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}
