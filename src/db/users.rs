use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use super::{now_unix, Conn};

#[derive(Debug, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub created_at: i64,
}

#[derive(Debug)]
pub struct UserAuth {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub failed_count: u32,
    pub locked_until: Option<i64>,
}

pub fn create(conn: &Conn, username: &str, password_hash: &str) -> anyhow::Result<i64> {
    conn.execute(
        "INSERT INTO users (username, password_hash, created_at) VALUES (?1, ?2, ?3)",
        params![username, password_hash, now_unix()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list(conn: &Conn) -> anyhow::Result<Vec<User>> {
    let mut stmt = conn.prepare("SELECT id, username, created_at FROM users ORDER BY id")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(User {
                id: r.get(0)?,
                username: r.get(1)?,
                created_at: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn find_for_auth(conn: &Conn, username: &str) -> anyhow::Result<Option<UserAuth>> {
    let r = conn
        .query_row(
            "SELECT id, username, password_hash, failed_count, locked_until
             FROM users WHERE username = ?1",
            params![username],
            |row| {
                Ok(UserAuth {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    failed_count: row.get::<_, i64>(3)? as u32,
                    locked_until: row.get(4)?,
                })
            },
        )
        .optional()?;
    Ok(r)
}

pub fn record_failed(
    conn: &Conn,
    user_id: i64,
    max_failures: u32,
    lockout_secs: i64,
) -> anyhow::Result<()> {
    let next: i64 = conn.query_row(
        "SELECT failed_count + 1 FROM users WHERE id = ?1",
        params![user_id],
        |r| r.get(0),
    )?;
    let lock = if (next as u32) >= max_failures {
        Some(now_unix() + lockout_secs)
    } else {
        None
    };
    conn.execute(
        "UPDATE users SET failed_count = ?1, locked_until = ?2 WHERE id = ?3",
        params![next, lock, user_id],
    )?;
    Ok(())
}

pub fn reset_failed(conn: &Conn, user_id: i64) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE users SET failed_count = 0, locked_until = NULL WHERE id = ?1",
        params![user_id],
    )?;
    Ok(())
}

pub fn set_password(conn: &Conn, username: &str, password_hash: &str) -> anyhow::Result<()> {
    let n = conn.execute(
        "UPDATE users SET password_hash = ?1, failed_count = 0, locked_until = NULL
         WHERE username = ?2",
        params![password_hash, username],
    )?;
    if n == 0 {
        anyhow::bail!("user '{}' not found", username);
    }
    Ok(())
}
