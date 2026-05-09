use rusqlite::params;
use serde::Serialize;

use super::{now_unix, Conn};

#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub ts: i64,
    pub username: Option<String>,
    pub ip: Option<String>,
    pub action: String,
    pub target: Option<String>,
    pub detail: Option<String>,
}

pub struct AuditWrite<'a> {
    pub user_id: Option<i64>,
    pub username: Option<&'a str>,
    pub ip: Option<&'a str>,
    pub action: &'a str,
    pub target: Option<&'a str>,
    pub detail: Option<&'a str>,
}

pub fn write(conn: &Conn, e: &AuditWrite<'_>) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO audit_log (ts, user_id, username, ip, action, target, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            now_unix(),
            e.user_id,
            e.username,
            e.ip,
            e.action,
            e.target,
            e.detail,
        ],
    )?;
    Ok(())
}

pub fn list(conn: &Conn, limit: i64) -> anyhow::Result<Vec<AuditEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, ts, username, ip, action, target, detail
         FROM audit_log ORDER BY ts DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit], |r| {
            Ok(AuditEntry {
                id: r.get(0)?,
                ts: r.get(1)?,
                username: r.get(2)?,
                ip: r.get(3)?,
                action: r.get(4)?,
                target: r.get(5)?,
                detail: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn purge_older_than(conn: &Conn, retain_days: u64) -> anyhow::Result<u64> {
    let cutoff = now_unix() - (retain_days as i64) * 86400;
    let n = conn.execute("DELETE FROM audit_log WHERE ts < ?1", params![cutoff])?;
    Ok(n as u64)
}
