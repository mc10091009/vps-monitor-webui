use std::sync::Arc;

use crate::collectors::system::SystemCollector;
use crate::config::Config;
use crate::db::SqlitePool;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub db: SqlitePool,
    pub system: Arc<SystemCollector>,
    pub docker: Option<Arc<bollard::Docker>>,
}

impl AppState {
    pub async fn new(cfg: Config) -> anyhow::Result<Self> {
        let db = crate::db::open_pool(&cfg.db_path)?;
        let system = Arc::new(SystemCollector::new());
        system.clone().start_refresher();

        let docker = match cfg.docker_socket.as_ref() {
            Some(path) => bollard::Docker::connect_with_unix(
                path.to_string_lossy().as_ref(),
                120,
                bollard::API_DEFAULT_VERSION,
            )
            .ok()
            .map(Arc::new),
            None => bollard::Docker::connect_with_local_defaults()
                .ok()
                .map(Arc::new),
        };
        if docker.is_none() {
            tracing::warn!("docker not available — /api/docker will be empty");
        }

        let state = AppState {
            cfg: Arc::new(cfg),
            db,
            system,
            docker,
        };

        state.spawn_session_purge();
        state.spawn_audit_purge();

        Ok(state)
    }

    fn spawn_session_purge(&self) {
        let pool = self.db.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
            loop {
                tick.tick().await;
                let pool = pool.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(c) = pool.get() {
                        let _ = crate::db::sessions::purge_expired(&c);
                    }
                })
                .await;
            }
        });
    }

    fn spawn_audit_purge(&self) {
        let pool = self.db.clone();
        let retain = self.cfg.audit_retention_days;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(86400));
            loop {
                tick.tick().await;
                let pool = pool.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(c) = pool.get() {
                        let _ = crate::db::audit::purge_older_than(&c, retain);
                    }
                })
                .await;
            }
        });
    }
}
