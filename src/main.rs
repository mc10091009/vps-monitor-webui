use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod auth;
mod collectors;
mod config;
mod db;
mod error;
mod handlers;
mod state;

#[derive(Parser)]
#[command(name = "vps-monitor", version, about = "Lightweight VPS monitoring WebUI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the web server
    Serve {
        #[arg(short, long, default_value = "/etc/vps-monitor/config.toml")]
        config: PathBuf,
    },
    /// Apply database migrations
    Migrate {
        #[arg(long, default_value = "/var/lib/vps-monitor/db.sqlite")]
        db: PathBuf,
    },
    /// Add a user (interactive password prompt)
    UserAdd {
        username: String,
        #[arg(long, default_value = "/var/lib/vps-monitor/db.sqlite")]
        db: PathBuf,
    },
    /// List users
    UserList {
        #[arg(long, default_value = "/var/lib/vps-monitor/db.sqlite")]
        db: PathBuf,
    },
    /// Change a user's password
    UserPasswd {
        username: String,
        #[arg(long, default_value = "/var/lib/vps-monitor/db.sqlite")]
        db: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,vps_monitor=debug")),
        )
        .init();

    let cli = Cli::parse();

    match cli.cmd {
        Cmd::Serve { config } => {
            let cfg = config::Config::load(&config)?;
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?
                .block_on(serve(cfg))
        }
        Cmd::Migrate { db } => db::migrate(&db),
        Cmd::UserAdd { username, db } => cmd_user_add(&db, &username),
        Cmd::UserList { db } => cmd_user_list(&db),
        Cmd::UserPasswd { username, db } => cmd_user_passwd(&db, &username),
    }
}

async fn serve(cfg: config::Config) -> anyhow::Result<()> {
    cfg.validate_bind()?;

    db::migrate(&cfg.db_path)?;

    let app_state = state::AppState::new(cfg.clone()).await?;
    let app = handlers::router(app_state);

    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    let local_addr = listener.local_addr()?;
    tracing::info!("vps-monitor listening on http://{}", local_addr);
    tracing::info!(
        "access via SSH tunnel: ssh -L {port}:localhost:{port} <user>@<vps-host>",
        port = local_addr.port()
    );

    let shutdown = async {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("shutting down");
    };

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await?;

    Ok(())
}

fn cmd_user_add(db_path: &std::path::Path, username: &str) -> anyhow::Result<()> {
    db::migrate(db_path)?;
    let pool = db::open_pool(db_path)?;
    let pw1 = rpassword::prompt_password(format!("Password for {username}: "))?;
    let pw2 = rpassword::prompt_password("Confirm: ")?;
    if pw1 != pw2 {
        anyhow::bail!("passwords do not match");
    }
    if pw1.len() < 8 {
        anyhow::bail!("password must be at least 8 characters");
    }
    let hash = auth::password::hash(&pw1)?;
    db::users::create(&pool.get()?, username, &hash)?;
    println!("user '{}' created", username);
    Ok(())
}

fn cmd_user_list(db_path: &std::path::Path) -> anyhow::Result<()> {
    let pool = db::open_pool(db_path)?;
    let users = db::users::list(&pool.get()?)?;
    if users.is_empty() {
        println!("(no users)");
    }
    for u in users {
        println!("{:>4}  {:<20}  created {}", u.id, u.username, u.created_at);
    }
    Ok(())
}

fn cmd_user_passwd(db_path: &std::path::Path, username: &str) -> anyhow::Result<()> {
    let pool = db::open_pool(db_path)?;
    let pw1 = rpassword::prompt_password(format!("New password for {username}: "))?;
    let pw2 = rpassword::prompt_password("Confirm: ")?;
    if pw1 != pw2 {
        anyhow::bail!("passwords do not match");
    }
    if pw1.len() < 8 {
        anyhow::bail!("password must be at least 8 characters");
    }
    let hash = auth::password::hash(&pw1)?;
    db::users::set_password(&pool.get()?, username, &hash)?;
    println!("password for '{}' updated", username);
    Ok(())
}
