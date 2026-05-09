use std::path::Path;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

pub mod audit;
pub mod sessions;
pub mod users;

pub type SqlitePool = Pool<SqliteConnectionManager>;
pub type Conn = r2d2::PooledConnection<SqliteConnectionManager>;

const SCHEMA: &str = include_str!("../../migrations/001_init.sql");

pub fn open_pool(path: &Path) -> anyhow::Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).ok();
        }
    }
    let manager = SqliteConnectionManager::file(path).with_init(|c| {
        c.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )
    });
    let pool = Pool::builder().max_size(8).build(manager)?;
    Ok(pool)
}

pub fn migrate(path: &Path) -> anyhow::Result<()> {
    let pool = open_pool(path)?;
    let conn = pool.get()?;
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
