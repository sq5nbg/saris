//! SARIS - System Archiwizacji Raportów i Sprawozdań.
//! Web application for students to upload reports into an on-disk archive.

mod admin;
mod config;
mod mail;
mod storage;
mod upload;
mod upload_log;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderValue, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::config::{Config, DOC_TYPES};
use crate::upload_log::UploadLogger;

const INDEX_HTML: &str = include_str!("../static/index.html");
const STYLE_CSS: &str = include_str!("../static/style.css");
const APP_JS: &str = include_str!("../static/app.js");

/// Extra room in the request body for the text fields of the form.
const FORM_FIELDS_MARGIN: usize = 1024 * 1024;

/// Shared application state.
pub struct AppState {
    pub config: Config,
    /// Canonical absolute path of the archive root directory.
    pub storage_root: PathBuf,
    pub logger: UploadLogger,
}

fn asset(content_type: &'static str, body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        ],
        body,
    )
        .into_response()
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn style_css() -> Response {
    asset("text/css; charset=utf-8", STYLE_CSS)
}

async fn app_js() -> Response {
    asset("text/javascript; charset=utf-8", APP_JS)
}

/// `GET /api/config` — public configuration for the form (no passwords).
async fn api_config(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let cfg = &state.config;
    Json(json!({
        "subjects": cfg.subjects.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        "max_file_size_mb": cfg.server.max_file_size_mb,
        "allowed_extensions": cfg.server.allowed_extensions,
        "doc_types": DOC_TYPES,
    }))
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let body_limit = usize::try_from(state.config.max_file_bytes())
        .unwrap_or(usize::MAX)
        .saturating_add(FORM_FIELDS_MARGIN);
    Router::new()
        .route("/", get(index))
        .route("/static/style.css", get(style_css))
        .route("/static/app.js", get(app_js))
        .route("/api/config", get(api_config))
        .route(
            "/api/upload",
            post(upload::upload_handler).layer(DefaultBodyLimit::max(body_limit)),
        )
        .route("/admin", get(admin::admin_index))
        .route(
            "/admin/download/{subject}/{year}/{filename}",
            get(admin::admin_download),
        )
        .with_state(state)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    if let Err(e) = run().await {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let config_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("saris.toml"));
    let config = Config::load(&config_path)?;
    tracing::info!("loaded configuration from {}", config_path.display());

    if config.admin.password == "change-me" || config.admin.password.is_empty() {
        tracing::warn!("admin password is the default/empty one - change it in the config file");
    }

    tokio::fs::create_dir_all(&config.server.storage_dir)
        .await
        .map_err(|e| {
            format!(
                "cannot create storage dir {}: {e}",
                config.server.storage_dir.display()
            )
        })?;
    let storage_root = tokio::fs::canonicalize(&config.server.storage_dir)
        .await
        .map_err(|e| format!("cannot resolve storage dir: {e}"))?;

    // Make the log path absolute so it does not depend on later cwd changes.
    let log_path = if config.server.log_file.is_absolute() {
        config.server.log_file.clone()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("cannot determine current dir: {e}"))?
            .join(&config.server.log_file)
    };
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("cannot create log dir {}: {e}", parent.display()))?;
    }

    let bind = config.server.bind.clone();
    let state = Arc::new(AppState {
        config,
        storage_root,
        logger: UploadLogger::new(log_path),
    });
    tracing::info!("archive directory: {}", state.storage_root.display());
    tracing::info!("upload log: {}", state.logger.path().display());
    tracing::info!(
        "e-mail confirmations: {}",
        if state.config.smtp.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );

    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|e| format!("cannot bind {bind}: {e}"))?;
    tracing::info!("SARIS listening on http://{bind}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutting down");
    })
    .await
    .map_err(|e| format!("server error: {e}"))
}
