use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct Pm2App {
    pub name: String,
    pub pid: u32,
    pub status: String,
    pub cpu: f32,
    pub memory: u64,
    pub uptime_ms: i64,
    pub restart_count: u32,
    pub script: Option<String>,
    pub cwd: Option<String>,
    pub user: Option<String>,
    pub log_out: Option<PathBuf>,
    pub log_err: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct RawApp {
    name: String,
    pid: Option<u32>,
    pm2_env: RawEnv,
    monit: Option<RawMonit>,
    pm_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RawEnv {
    status: String,
    pm_uptime: Option<i64>,
    restart_time: Option<u32>,
    pm_out_log_path: Option<String>,
    pm_err_log_path: Option<String>,
    pm_exec_path: Option<String>,
    pm_cwd: Option<String>,
    username: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawMonit {
    cpu: Option<f32>,
    memory: Option<u64>,
}

pub async fn list() -> anyhow::Result<Vec<Pm2App>> {
    let out = Command::new("pm2")
        .arg("jlist")
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("run pm2 jlist: {}. Is PM2 installed?", e))?;
    if !out.status.success() {
        anyhow::bail!(
            "pm2 jlist failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return Ok(vec![]);
    }
    let raw: Vec<RawApp> = serde_json::from_str(trimmed)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let apps = raw
        .into_iter()
        .map(|a| {
            let uptime_ms = a
                .pm2_env
                .pm_uptime
                .map(|u| (now - u).max(0))
                .unwrap_or(0);
            Pm2App {
                pid: a.pid.unwrap_or(0),
                name: a.name,
                status: a.pm2_env.status,
                cpu: a.monit.as_ref().and_then(|m| m.cpu).unwrap_or(0.0),
                memory: a.monit.as_ref().and_then(|m| m.memory).unwrap_or(0),
                uptime_ms,
                restart_count: a.pm2_env.restart_time.unwrap_or(0),
                script: a.pm2_env.pm_exec_path,
                cwd: a.pm2_env.pm_cwd,
                user: a.pm2_env.username,
                log_out: a.pm2_env.pm_out_log_path.map(PathBuf::from),
                log_err: a.pm2_env.pm_err_log_path.map(PathBuf::from),
            }
        })
        .collect();
    Ok(apps)
}

pub async fn find(name: &str) -> anyhow::Result<Option<Pm2App>> {
    Ok(list().await?.into_iter().find(|a| a.name == name))
}
