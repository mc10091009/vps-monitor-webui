use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub bind: String,
    pub db_path: PathBuf,

    #[serde(default = "default_session_ttl")]
    pub session_ttl_secs: u64,

    #[serde(default = "default_audit_retention")]
    pub audit_retention_days: u64,

    #[serde(default = "default_max_failed_logins")]
    pub max_failed_logins: u32,

    #[serde(default = "default_lockout_minutes")]
    pub lockout_minutes: u64,

    #[serde(default)]
    pub pm2_log_dirs: Vec<PathBuf>,

    pub docker_socket: Option<PathBuf>,
}

fn default_session_ttl() -> u64 {
    7 * 24 * 3600
}
fn default_audit_retention() -> u64 {
    90
}
fn default_max_failed_logins() -> u32 {
    5
}
fn default_lockout_minutes() -> u64 {
    15
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let s = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("read {}: {}", path.display(), e))?;
        let cfg: Config = toml::from_str(&s)?;
        Ok(cfg)
    }

    /// Hard-fail if bind address is publicly routable. We *only* serve over loopback / private nets.
    pub fn validate_bind(&self) -> anyhow::Result<()> {
        let addr: std::net::SocketAddr = self
            .bind
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid bind '{}': {}", self.bind, e))?;
        let ip = addr.ip();
        if !is_safe_bind(ip) {
            anyhow::bail!(
                "refusing to bind to public address {}. \
                 Only loopback or private addresses are allowed. \
                 Use SSH tunnel: ssh -L {p}:localhost:{p} <user>@<host>",
                ip,
                p = addr.port()
            );
        }
        Ok(())
    }
}

fn is_safe_bind(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified() == false && {
                    // tailscale CGNAT
                    let o = v4.octets();
                    o[0] == 100 && (64..128).contains(&o[1])
                }
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 unique-local
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn rejects_public_v4() {
        assert!(!is_safe_bind(IpAddr::from_str("8.8.8.8").unwrap()));
        assert!(!is_safe_bind(IpAddr::from_str("0.0.0.0").unwrap()));
    }

    #[test]
    fn allows_loopback_and_private() {
        assert!(is_safe_bind(IpAddr::from_str("127.0.0.1").unwrap()));
        assert!(is_safe_bind(IpAddr::from_str("10.0.0.5").unwrap()));
        assert!(is_safe_bind(IpAddr::from_str("192.168.1.1").unwrap()));
        assert!(is_safe_bind(IpAddr::from_str("100.64.0.1").unwrap())); // tailscale
        assert!(is_safe_bind(IpAddr::from_str("::1").unwrap()));
    }
}
