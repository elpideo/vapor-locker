mod csrf;
mod db;
mod handlers;
mod logging;
mod models;
mod security;

use std::{net::SocketAddr, time::Duration};

use anyhow::Context;
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use clap::{Parser, Subcommand};
use tower::ServiceBuilder;
use tower_cookies::CookieManagerLayer;
use tower_http::services::{ServeDir, ServeFile};
use tracing::info;

#[derive(Debug, Parser)]
#[command(name = "vapor")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Runs the HTTP server
    Serve,
    /// Purge expired DB entries once
    PurgeOnce,
    /// Purge expired DB entries in a loop
    PurgeLoop {
        /// Interval between purges in seconds
        #[arg(long, env = "PURGE_INTERVAL_SECONDS", default_value_t = 3600)]
        interval_seconds: u64,
    },
}

#[derive(Clone)]
pub(crate) struct AppState {
    db: db::Db,
    ip_cache: security::IpCache,
    csrf: csrf::CsrfConfig,
    trust_proxy: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    let log_guard = logging::init_logging_from_env().context("init logging")?;
    info!(event = "startup", "vapor starting");

    let db = db::Db::connect_from_env().await.context("connect db")?;
    db.migrate().await.context("run migrations")?;

    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => serve(db, log_guard).await,
        Command::PurgeOnce => {
            let deleted = db.purge_expired().await.context("purge expired")?;
            info!(event = "purge_once", deleted, "purge finished");
            Ok(())
        }
        Command::PurgeLoop { interval_seconds } => {
            purge_loop(db, Duration::from_secs(interval_seconds)).await
        }
    }
}

async fn serve(db: db::Db, _log_guard: logging::LogGuard) -> anyhow::Result<()> {
    let addr: SocketAddr = std::env::var("APP_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
        .parse()
        .context("APP_ADDR parse")?;

    let trust_proxy = std::env::var("TRUST_PROXY")
        .unwrap_or_else(|_| "false".to_string())
        .to_lowercase()
        == "true";

    let csrf = csrf::CsrfConfig::from_env()?;
    let ip_cache = security::IpCache::new(Duration::from_secs(3));

    let state = AppState {
        db,
        ip_cache,
        csrf,
        trust_proxy,
    };

    let app = Router::new()
        .nest_service("/static", ServeDir::new("static"))
        .route_service("/", ServeFile::new("static/index.html"))
        .route("/api/csrf", get(handlers::api_csrf))
        .route("/api/get", get(handlers::api_get))
        .route("/api/set", post(handlers::api_set))
        .layer(DefaultBodyLimit::max(250_000))
        .layer(CookieManagerLayer::new())
        .layer(
            ServiceBuilder::new()
                .layer(tower_http::trace::TraceLayer::new_for_http()),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    info!(event = "listen", addr = %addr, "listening");

    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve")?;

    Ok(())
}

async fn purge_loop(db: db::Db, interval: Duration) -> anyhow::Result<()> {
    loop {
        let deleted = db.purge_expired().await.context("purge expired")?;
        info!(event = "purge_loop", deleted, interval_seconds = interval.as_secs(), "purge finished");
        tokio::time::sleep(interval).await;
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

