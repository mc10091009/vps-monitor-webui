use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use bollard::container::LogsOptions;
use futures_util::StreamExt;
use once_cell::sync::Lazy;
use regex::Regex;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader, SeekFrom};

use crate::auth::AuthUser;
use crate::collectors::{docker as docker_c, pm2, systemd};
use crate::db;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

static PM2_NAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9._\-]{1,128}$").unwrap());

fn audit_log(state: &AppState, user: &AuthUser, action: &str, target: &str) {
    let pool = state.db.clone();
    let user_id = user.user_id;
    let username = user.username.clone();
    let ip = user.ip.clone();
    let action = action.to_string();
    let target = target.to_string();
    tokio::task::spawn_blocking(move || {
        if let Ok(c) = pool.get() {
            let _ = db::audit::write(
                &c,
                &db::audit::AuditWrite {
                    user_id: Some(user_id),
                    username: Some(&username),
                    ip: ip.as_deref(),
                    action: &action,
                    target: Some(&target),
                    detail: None,
                },
            );
        }
    });
}

pub async fn ws_pm2(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    user: AuthUser,
    Path(name): Path<String>,
) -> AppResult<Response> {
    if !PM2_NAME_RE.is_match(&name) {
        return Err(AppError::BadRequest("invalid pm2 name".into()));
    }
    let app = pm2::find(&name, &state.cfg.pm2_homes)
        .await
        .map_err(AppError::Other)?
        .ok_or(AppError::NotFound)?;
    let log_path = app.log_out.clone().ok_or_else(|| {
        AppError::BadRequest("pm2 app has no out_log_path; check pm2 describe".into())
    })?;
    audit_log(&state, &user, "view_logs", &format!("pm2:{name}"));
    Ok(ws.on_upgrade(move |sock| async move {
        if let Err(e) = tail_file(sock, &log_path).await {
            tracing::debug!("pm2 tail closed: {e:?}");
        }
    }))
}

pub async fn ws_docker(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Response> {
    if !docker_c::validate_id(&id) {
        return Err(AppError::BadRequest("invalid container id".into()));
    }
    let docker = state
        .docker
        .clone()
        .ok_or_else(|| AppError::BadRequest("docker not available".into()))?;
    audit_log(&state, &user, "view_logs", &format!("docker:{id}"));
    Ok(ws.on_upgrade(move |sock| async move {
        if let Err(e) = stream_docker(sock, &docker, &id).await {
            tracing::debug!("docker logs closed: {e:?}");
        }
    }))
}

pub async fn ws_journal(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    user: AuthUser,
    Path(unit): Path<String>,
) -> AppResult<Response> {
    if !systemd::validate_unit(&unit) {
        return Err(AppError::BadRequest("invalid unit name".into()));
    }
    audit_log(&state, &user, "view_logs", &format!("journal:{unit}"));
    Ok(ws.on_upgrade(move |sock| async move {
        if let Err(e) = stream_journal(sock, &unit).await {
            tracing::debug!("journal closed: {e:?}");
        }
    }))
}

const TAIL_INITIAL_BYTES: u64 = 8 * 1024;

async fn tail_file(mut sock: WebSocket, path: &std::path::Path) -> anyhow::Result<()> {
    let mut file = tokio::fs::File::open(path).await?;
    let metadata = file.metadata().await?;
    let len = metadata.len();
    let start = len.saturating_sub(TAIL_INITIAL_BYTES);
    file.seek(SeekFrom::Start(start)).await?;

    // Read tail and send as one chunk
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).await?;
    if !buf.is_empty() {
        if sock
            .send(Message::Text(String::from_utf8_lossy(&buf).into_owned()))
            .await
            .is_err()
        {
            return Ok(());
        }
    }

    let mut pos = len;
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let mut f = match tokio::fs::File::open(path).await {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                let new_len = f.metadata().await?.len();
                if new_len < pos {
                    // truncated/rotated
                    pos = 0;
                }
                if new_len > pos {
                    f.seek(SeekFrom::Start(pos)).await?;
                    let mut chunk = vec![0u8; (new_len - pos).min(1 << 20) as usize];
                    let n = f.read(&mut chunk).await?;
                    chunk.truncate(n);
                    pos += n as u64;
                    if !chunk.is_empty() {
                        if sock
                            .send(Message::Text(String::from_utf8_lossy(&chunk).into_owned()))
                            .await
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                }
            }
            msg = sock.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Err(_)) => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}

async fn stream_docker(
    mut sock: WebSocket,
    docker: &bollard::Docker,
    id: &str,
) -> anyhow::Result<()> {
    let opts = LogsOptions::<String> {
        follow: true,
        stdout: true,
        stderr: true,
        timestamps: false,
        tail: "200".into(),
        ..Default::default()
    };
    let stream = docker.logs(id, Some(opts));
    tokio::pin!(stream);
    loop {
        tokio::select! {
            chunk = stream.next() => {
                let Some(chunk) = chunk else { return Ok(()); };
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = sock.send(Message::Text(format!("\n[stream error] {e}\n"))).await;
                        return Ok(());
                    }
                };
                let bytes = chunk.into_bytes();
                if !bytes.is_empty() {
                    if sock.send(Message::Text(String::from_utf8_lossy(&bytes).into_owned())).await.is_err() {
                        return Ok(());
                    }
                }
            }
            msg = sock.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => return Ok(()),
                    Some(Err(_)) => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}

async fn stream_journal(mut sock: WebSocket, unit: &str) -> anyhow::Result<()> {
    use std::process::Stdio;
    let mut child = tokio::process::Command::new("journalctl")
        .args(["-fu", unit, "--no-pager", "--output=short-iso", "-n", "200"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("no stdout"))?;
    let mut reader = BufReader::new(stdout).lines();

    loop {
        tokio::select! {
            line = reader.next_line() => {
                match line? {
                    Some(line) => {
                        if sock.send(Message::Text(format!("{line}\n"))).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            msg = sock.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
    let _ = child.kill().await;
    Ok(())
}
