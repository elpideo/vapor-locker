mod csrf;
mod db;
mod handlers;
mod logging;
mod models;
mod security;
mod version;

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

/// CLI arguments for `vapor` (parsed by `clap`).
#[derive(Debug, Parser)]
#[command(name = "vapor")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// Available subcommands.
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

/// Shared application state injected into Axum handlers.
///
/// Holds the database handle, CSRF configuration, IP abuse limiter, and proxy
/// trust flag.
#[derive(Clone)]
pub(crate) struct AppState {
    db: db::Db,
    abuse_limiter: security::AbuseLimiter,
    csrf: csrf::CsrfConfig,
    trust_proxy: bool,
}

/// Entry point for the `vapor` binary.
///
/// This function performs the global process initialization before delegating
/// to the requested subcommand:
///
/// - loads environment variables from `.env` if the file exists;
/// - parses the command line with `clap`;
/// - initializes structured logging and keeps its lifetime guard alive;
/// - opens the database connection;
/// - applies SQL migrations at startup;
/// - then executes `serve`, `purge once`, or `purge loop`.
///
/// # Errors
///
/// Returns an error if logging initialization fails, if the database
/// connection or migrations fail, or if the selected subcommand itself
/// returns an error.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    let log_guard = logging::init_logging_from_env().context("init logging")?;
    info!(event = "startup", "vapor starting");

    info!(event = "db_connect", "connecting to database");
    let db = db::Db::connect_from_env().await.context("connect db")?;
    info!(event = "db_connected", "database connected");

    info!(event = "migrate", "running migrations");
    db.migrate().await.context("run migrations")?;
    info!(event = "migrate_done", "migrations complete");

    let cmd = cli.command.unwrap_or(Command::Serve);
    info!(event = "command", cmd = ?cmd, "executing command");

    match cmd {
        Command::Serve => serve(db, log_guard).await,
        Command::PurgeOnce => {
            info!(event = "purge_once_start", "starting one-shot purge");
            let stats = db.purge_expired().await.context("purge expired")?;
            info!(
                event = "purge_once",
                entries_deleted = stats.entries_deleted,
                salts_deleted = stats.salts_deleted,
                "purge finished"
            );
            Ok(())
        }
        Command::PurgeLoop { interval_seconds } => {
            info!(event = "purge_loop_start", interval_seconds, "starting purge loop");
            purge_loop(db, Duration::from_secs(interval_seconds)).await
        }
    }
}

/// Builds the Axum application and starts the HTTP server.
///
/// This function:
///
/// - reads the listen address from `APP_ADDR` (default: `0.0.0.0:3000`);
/// - enables or disables reverse-proxy trust through `TRUST_PROXY`;
/// - loads the CSRF configuration from the environment;
/// - creates the IP abuse limiter with a retention duration from
///   `ABUSE_TTL_SECS`;
/// - configures static file serving;
/// - registers API routes and Axum/Tower middleware;
/// - starts the TCP listener and serves requests until a shutdown signal is
///   received.
///
/// The `_log_guard` parameter is intentionally kept alive for the full server
/// lifetime so the logging backend remains initialized while the process is
/// serving requests.
///
/// # Errors
///
/// Returns an error if the listen address is invalid, if the CSRF
/// configuration cannot be loaded, if TCP bind fails, or if the HTTP serving
/// loop exits with an error.
async fn serve(db: db::Db, _log_guard: logging::LogGuard) -> anyhow::Result<()> {
    info!(event = "serve_start", "building router and binding");
    let addr: SocketAddr = std::env::var("APP_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3000".to_string())
        .parse()
        .context("APP_ADDR parse")?;

    let trust_proxy = std::env::var("TRUST_PROXY")
        .unwrap_or_else(|_| "false".to_string())
        .to_lowercase()
        == "true";

    let csrf = csrf::CsrfConfig::from_env()?;
    let abuse_ttl = Duration::from_secs(
        std::env::var("ABUSE_TTL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(86400),
    );
    let abuse_limiter = security::AbuseLimiter::new(abuse_ttl);

    // Use an absolute path based on the crate directory to serve static
    // assets, avoiding 404s when the binary is launched from a different
    // working directory.
    let static_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("static");
    let index_html = static_root.join("index.html");

    let state = AppState {
        db,
        abuse_limiter,
        csrf,
        trust_proxy,
    };

    let app = Router::new()
        .nest_service("/static", ServeDir::new(static_root))
        .route_service("/", ServeFile::new(index_html))
        .route("/api/csrf", get(handlers::api_csrf))
        .route("/api/salts", get(handlers::api_salts))
        .route("/api/get", post(handlers::api_get))
        .route("/api/set", post(handlers::api_set))
        .route("/api/version", get(handlers::api_version))
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

/// Runs an infinite periodic purge of expired data.
///
/// On each iteration, this function deletes expired entries and salts through
/// the database layer, logs how many items were removed, then waits for
/// `interval` before the next pass.
///
/// Under normal operation, this loop is not expected to return.
///
/// # Parameters
///
/// - `db`: database handle used to execute the purge.
/// - `interval`: delay between two successive purge passes.
///
/// # Errors
///
/// Returns an error if a database purge operation fails.
async fn purge_loop(db: db::Db, interval: Duration) -> anyhow::Result<()> {
    loop {
        let stats = db.purge_expired().await.context("purge expired")?;
        info!(
            event = "purge_loop",
            entries_deleted = stats.entries_deleted,
            salts_deleted = stats.salts_deleted,
            interval_seconds = interval.as_secs(),
            "purge finished"
        );
        tokio::time::sleep(interval).await;
    }
}

/// Waits for a graceful process shutdown signal.
///
/// For now, only `Ctrl-C` is supported. This future is used by
/// `axum::serve(...).with_graceful_shutdown(...)` to stop the HTTP server
/// cleanly instead of abruptly terminating in-flight connections.
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

