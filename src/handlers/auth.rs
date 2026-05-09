use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::auth::password;
use crate::auth::session::{new_token, COOKIE_NAME};
use crate::auth::AuthUser;
use crate::db;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

// Pre-computed argon2 hash, used to neutralise timing differences when
// the requested user does not exist. Lazy so the cost is paid once.
static DUMMY_HASH: Lazy<String> = Lazy::new(|| {
    password::hash("dummy-not-a-real-password-please-ignore").unwrap_or_default()
});

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub username: String,
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> AppResult<(CookieJar, Json<LoginResponse>)> {
    require_xhr(&headers)?;
    if req.username.is_empty() || req.password.is_empty() {
        return Err(AppError::BadRequest("missing credentials".into()));
    }

    let ip_str = addr.ip().to_string();
    let max_failed = state.cfg.max_failed_logins;
    let lockout = (state.cfg.lockout_minutes * 60) as i64;
    let ttl = state.cfg.session_ttl_secs as i64;

    let pool = state.db.clone();
    let username = req.username.clone();
    let password = req.password.clone();
    let ip_for_blocking = ip_str.clone();

    let result = tokio::task::spawn_blocking(move || -> Result<(i64, String), AppError> {
        let conn = pool.get()?;
        let user = db::users::find_for_auth(&conn, &username).map_err(AppError::Other)?;
        let user = match user {
            Some(u) => u,
            None => {
                // dummy verify to neutralise timing differences
                let _ = password::verify(&password, &DUMMY_HASH);
                let _ = db::audit::write(
                    &conn,
                    &db::audit::AuditWrite {
                        user_id: None,
                        username: Some(&username),
                        ip: Some(&ip_for_blocking),
                        action: "login_fail",
                        target: None,
                        detail: Some("unknown user"),
                    },
                );
                return Err(AppError::BadCredentials);
            }
        };

        if let Some(until) = user.locked_until {
            let now = db::now_unix();
            if until > now {
                let _ = db::audit::write(
                    &conn,
                    &db::audit::AuditWrite {
                        user_id: Some(user.id),
                        username: Some(&user.username),
                        ip: Some(&ip_for_blocking),
                        action: "login_locked",
                        target: None,
                        detail: None,
                    },
                );
                return Err(AppError::AccountLocked((until - now) as u64));
            }
        }

        if !password::verify(&password, &user.password_hash) {
            db::users::record_failed(&conn, user.id, max_failed, lockout)
                .map_err(AppError::Other)?;
            let _ = db::audit::write(
                &conn,
                &db::audit::AuditWrite {
                    user_id: Some(user.id),
                    username: Some(&user.username),
                    ip: Some(&ip_for_blocking),
                    action: "login_fail",
                    target: None,
                    detail: Some("bad password"),
                },
            );
            return Err(AppError::BadCredentials);
        }

        db::users::reset_failed(&conn, user.id).map_err(AppError::Other)?;
        let token = new_token();
        db::sessions::create(&conn, &token, user.id, ttl, Some(&ip_for_blocking))
            .map_err(AppError::Other)?;
        let _ = db::audit::write(
            &conn,
            &db::audit::AuditWrite {
                user_id: Some(user.id),
                username: Some(&user.username),
                ip: Some(&ip_for_blocking),
                action: "login_ok",
                target: None,
                detail: None,
            },
        );
        Ok((user.id, token))
    })
    .await
    .map_err(|e| AppError::Other(e.into()))??;

    let (_user_id, token) = result;

    let cookie = Cookie::build((COOKIE_NAME, token))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(false) // localhost via SSH tunnel; browser allows non-secure on localhost
        .max_age(time::Duration::seconds(ttl))
        .build();

    Ok((
        jar.add(cookie),
        Json(LoginResponse {
            username: req.username,
        }),
    ))
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    user: AuthUser,
    jar: CookieJar,
) -> AppResult<(CookieJar, Json<serde_json::Value>)> {
    require_xhr(&headers)?;
    let pool = state.db.clone();
    let token = user.token.clone();
    let username = user.username.clone();
    let user_id = user.user_id;
    let ip = user.ip.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = pool.get()?;
        db::sessions::delete(&conn, &token).map_err(AppError::Other)?;
        let _ = db::audit::write(
            &conn,
            &db::audit::AuditWrite {
                user_id: Some(user_id),
                username: Some(&username),
                ip: ip.as_deref(),
                action: "logout",
                target: None,
                detail: None,
            },
        );
        Ok(())
    })
    .await
    .map_err(|e| AppError::Other(e.into()))??;
    let cookie = Cookie::build((COOKIE_NAME, ""))
        .path("/")
        .max_age(time::Duration::ZERO)
        .build();
    Ok((jar.remove(cookie), Json(serde_json::json!({"ok": true}))))
}

pub async fn me(user: AuthUser) -> Json<serde_json::Value> {
    Json(serde_json::json!({"username": user.username, "user_id": user.user_id}))
}

fn require_xhr(headers: &HeaderMap) -> AppResult<()> {
    let ok = headers
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
