use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct SystemdService {
    pub unit: String,
    pub load: String,
    pub active: String,
    pub sub: String,
    pub description: String,
}

static UNIT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9@:._-]+\.(service|socket|target|timer|mount|path)$").unwrap());

/// Strict allow-list to prevent command injection via unit names.
pub fn validate_unit(name: &str) -> bool {
    !name.is_empty() && name.len() <= 256 && UNIT_RE.is_match(name)
}

pub async fn list() -> anyhow::Result<Vec<SystemdService>> {
    let out = Command::new("systemctl")
        .args([
            "list-units",
            "--type=service",
            "--all",
            "--no-pager",
            "--plain",
            "--no-legend",
        ])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("run systemctl: {}", e))?;
    if !out.status.success() {
        anyhow::bail!(
            "systemctl failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut rows = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(5, char::is_whitespace).filter(|s| !s.is_empty());
        let unit = match parts.next() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let load = parts.next().unwrap_or("").to_string();
        let active = parts.next().unwrap_or("").to_string();
        let sub = parts.next().unwrap_or("").to_string();
        let description = parts.next().unwrap_or("").to_string();
        rows.push(SystemdService {
            unit,
            load,
            active,
            sub,
            description,
        });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_unit_names() {
        assert!(validate_unit("nginx.service"));
        assert!(validate_unit("docker.socket"));
        assert!(validate_unit("getty@tty1.service"));
        assert!(!validate_unit("nginx.service; rm -rf /"));
        assert!(!validate_unit("../etc/passwd"));
        assert!(!validate_unit("nginx"));
        assert!(!validate_unit(""));
    }
}
