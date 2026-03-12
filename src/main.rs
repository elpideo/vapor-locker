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

/// Arguments CLI de `vapor` (piloté par `clap`).
#[derive(Debug, Parser)]
#[command(name = "vapor")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// Sous-commandes disponibles.
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

/// État partagé injecté dans les handlers Axum.
///
/// Contient la DB, la config CSRF, le limiteur d’abus par IP et le flag proxy.
#[derive(Clone)]
pub(crate) struct AppState {
    db: db::Db,
    abuse_limiter: security::AbuseLimiter,
    csrf: csrf::CsrfConfig,
    trust_proxy: bool,
}

/// Point d’entrée du binaire `vapor`.
///
/// Cette fonction effectue l’initialisation globale du processus avant de
/// déléguer à la sous-commande demandée :
///
/// - charge les variables d’environnement depuis `.env` si le fichier existe ;
/// - parse la ligne de commande avec `clap` ;
/// - initialise le logging structuré et conserve son garde-fou de durée de vie ;
/// - ouvre la connexion à la base de données ;
/// - applique les migrations SQL au démarrage ;
/// - exécute ensuite `serve`, `purge once` ou `purge loop`.
///
/// # Errors
///
/// Retourne une erreur si l’initialisation du logging échoue, si la connexion
/// ou les migrations de base de données échouent, ou si la sous-commande
/// appelée retourne elle-même une erreur.
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

/// Construit l’application Axum puis démarre le serveur HTTP.
///
/// Cette fonction :
///
/// - lit l’adresse d’écoute depuis `APP_ADDR` (défaut : `0.0.0.0:3000`) ;
/// - active ou non la confiance envers un proxy inverse via `TRUST_PROXY` ;
/// - charge la configuration CSRF depuis l’environnement ;
/// - crée le limiteur d’abus IP avec une durée de rétention issue de
///   `ABUSE_TTL_SECS` ;
/// - configure le service des fichiers statiques ;
/// - enregistre les routes API et les middlewares Axum/Tower ;
/// - lance l’écoute TCP puis sert les requêtes jusqu’au signal d’arrêt.
///
/// Le paramètre `_log_guard` est volontairement conservé jusqu’à la fin de vie
/// du serveur afin de garantir que le backend de logging reste initialisé tant
/// que le processus sert des requêtes.
///
/// # Errors
///
/// Retourne une erreur si l’adresse d’écoute est invalide, si la configuration
/// CSRF ne peut pas être chargée, si le bind TCP échoue, ou si la boucle de
/// service HTTP se termine sur une erreur.
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

    // Utilise un chemin absolu basé sur le répertoire du crate pour servir les
    // assets statiques, afin d'éviter les 404 si le binaire est lancé depuis
    // un répertoire de travail différent.
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

/// Exécute une purge périodique infinie des données expirées.
///
/// À chaque itération, la fonction supprime les entrées et sels expirés via la
/// base de données, journalise le nombre d’éléments supprimés, puis attend la
/// durée `interval` avant la passe suivante.
///
/// Cette boucle ne retourne normalement jamais en cas de fonctionnement
/// nominal.
///
/// # Parameters
///
/// - `db` : handle de base de données utilisé pour lancer la purge.
/// - `interval` : durée d’attente entre deux purges successives.
///
/// # Errors
///
/// Retourne une erreur si une opération de purge base de données échoue.
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

/// Attend un signal d’arrêt gracieux du processus.
///
/// Pour l’instant, seul `Ctrl-C` est pris en charge. Cette future est utilisée
/// par `axum::serve(...).with_graceful_shutdown(...)` pour arrêter
/// proprement le serveur HTTP sans couper brutalement les connexions en cours.
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

