use std::path::{Path, PathBuf};

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
    /// Which PM2_HOME this app was discovered in.
    pub pm2_home: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct RawApp {
    name: String,
    pid: Option<u32>,
    pm2_env: RawEnv,
    monit: Option<RawMonit>,
    #[allow(dead_code)]
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

/// List PM2 apps across one or more PM2_HOME directories.
/// If `homes` is empty, scan well-known locations.
pub async fn list(homes: &[PathBuf]) -> anyhow::Result<Vec<Pm2App>> {
    let targets: Vec<PathBuf> = if homes.is_empty() {
        discover_homes()
    } else {
        homes.to_vec()
    };

    if targets.is_empty() {
        // No homes found at all. Try a plain `pm2 jlist` (uses caller's $HOME).
        return list_for_home(None).await;
    }

    let mut all = Vec::new();
    let mut errs = Vec::new();
    for home in &targets {
        match list_for_home(Some(home)).await {
            Ok(mut apps) => {
                for a in &mut apps {
                    a.pm2_home = Some(home.clone());
                }
                all.extend(apps);
            }
            Err(e) => {
                tracing::debug!("pm2 jlist for PM2_HOME={}: {}", home.display(), e);
                errs.push(format!("{}: {}", home.display(), e));
            }
        }
    }

    if all.is_empty() && !errs.is_empty() {
        anyhow::bail!("pm2 jlist failed for all homes: {}", errs.join("; "));
    }
    Ok(all)
}

async fn list_for_home(home: Option<&Path>) -> anyhow::Result<Vec<Pm2App>> {
    let mut cmd = Command::new("pm2");
    cmd.arg("jlist");
    if let Some(h) = home {
        cmd.env("PM2_HOME", h);
    }
    let out = cmd
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("spawn pm2: {}. Is PM2 installed?", e))?;
    if !out.status.success() {
        anyhow::bail!("pm2 jlist exited {}: {}", out.status, String::from_utf8_lossy(&out.stderr).trim());
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() || trimmed == "[]" {
        return Ok(vec![]);
    }
    let raw: Vec<RawApp> = serde_json::from_str(trimmed)
        .map_err(|e| anyhow::anyhow!("parse pm2 jlist json: {}", e))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Ok(raw
        .into_iter()
        .map(|a| {
            let uptime_ms = a.pm2_env.pm_uptime.map(|u| (now - u).max(0)).unwrap_or(0);
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
                pm2_home: None,
            }
        })
        .collect())
}

/// Find common PM2_HOME locations on disk.
/// Returns directories that contain a PM2 daemon pid file (`pm2.pid`),
/// indicating an active or previously-active PM2 instance.
pub fn discover_homes() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut candidates: Vec<PathBuf> = Vec::new();

    candidates.push(PathBuf::from("/root/.pm2"));
    if let Ok(entries) = std::fs::read_dir("/home") {
        for e in entries.flatten() {
            candidates.push(e.path().join(".pm2"));
        }
    }

    for c in candidates {
        if c.is_dir() {
            // Even better signal: pm2 daemon ever ran here.
            let has_pid = c.join("pm2.pid").exists();
            let has_dump = c.join("dump.pm2").exists();
            let has_module = c.join("module_conf.json").exists();
            if has_pid || has_dump || has_module {
                found.push(c);
            }
        }
    }
    found
}

pub async fn find(name: &str, homes: &[PathBuf]) -> anyhow::Result<Option<Pm2App>> {
    Ok(list(homes).await?.into_iter().find(|a| a.name == name))
}
