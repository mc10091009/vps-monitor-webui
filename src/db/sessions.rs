use rusqlite::{params, OptionalExtension};

use super::{now_unix, Conn};

pub struct SessionRecord {
    pub user_id: i64,
    pub username: String,
    pub expires_at: i64,
}

pub fn create(
    conn: &Conn,
    token: &str,
    user_id: i64,
    ttl_secs: i64,
    ip: Option<&str>,
) -> anyhow::Result<()> {
    let now = now_unix();
    conn.execute(
        "INSERT INTO sessions (token, user_id, created_at, expires_at, last_seen, ip)
         VALUES (?1, ?2, ?3, ?4, ?3, ?5)",
        params![token, user_id, now, now + ttl_secs, ip],
    )?;
    Ok(())
}

pub fn find_active(conn: &Conn, token: &str) -> anyhow::Result<Option<SessionRecord>> {
    let now = now_unix();
    let r = conn
        .query_row(
            "SELECT s.user_id, u.username, s.expires_at
             FROM sessions s JOIN users u ON u.id = s.user_id
             WHERE s.token = ?1 AND s.expires_at > ?2",
            params![token, now],
            |row| {
                Ok(SessionRecord {
                    user_id: row.get(0)?,
                    username: row.get(1)?,
                    expires_at: row.get(2)?,
                })
            },
        )
        .optional()?;
    Ok(r)
}

pub fn touch(conn: &Conn, token: &str, slide_to: Option<i64>) -> anyhow::Result<()> {
    if let Some(new_exp) = slide_to {
        conn.execute(
            "UPDATE sessions SET last_seen = ?1, expires_at = ?2 WHERE token = ?3",
            params![now_unix(), new_exp, token],
        )?;
    } else {
        conn.execute(
            "UPDATE sessions SET last_seen = ?1 WHERE token = ?2",
            params![now_unix(), token],
        )?;
    }
    Ok(())
}

pub fn delete(conn: &Conn, token: &str) -> anyhow::Result<()> {
    conn.execute("DELETE FROM sessions WHERE token = ?1", params![token])?;
    Ok(())
}

pub fn purge_expired(conn: &Conn) -> anyhow::Result<u64> {
    let n = conn.execute(
        "DELETE FROM sessions WHERE expires_at < ?1",
        params![now_unix()],
    )?;
    Ok(n as u64)
}
