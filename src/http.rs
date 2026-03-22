//! HTTP server: REST API + static file serving for the web UI.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::State;
use axum::response::{IntoResponse, Json, Redirect};
use axum::routing::{get, post};
use axum::{Extension, Router};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;
use tracing::info;

use crate::auth::CallerContext;
use crate::config::Config;

// ── Shared state ─────────────────────────────────────────────────

pub struct AppState {
    pub nomen: Arc<crate::Nomen>,
    pub default_channel: String,
    pub config: Arc<RwLock<Config>>,
}

type SharedState = Arc<AppState>;

// ── Router ───────────────────────────────────────────────────────

pub fn build_router(
    state: AppState,
    static_dir: Option<PathBuf>,
    landing_dir: Option<PathBuf>,
) -> Router {
    let shared = Arc::new(state);

    let api = Router::new()
        .route("/dispatch", post(api_dispatch))
        .route("/health", get(api_health))
        .route("/stats", get(api_stats))
        .route("/config", get(api_get_config))
        .route("/config/reload", post(api_reload_config))
        .route("/auth/info", get(api_auth_info))
        .layer(axum::middleware::from_fn_with_state(
            shared.clone(),
            crate::auth::nip98_middleware,
        ))
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(|origin, _| {
                    let o = origin.as_bytes();
                    // Allow localhost (any port) for development
                    o.starts_with(b"http://localhost:") || o.starts_with(b"http://127.0.0.1:")
                    // Allow same-origin (requests from the served UI have no Origin or matching origin)
                    || o.ends_with(b".atlantislabs.space") || o == b"https://nomen.atlantislabs.space"
                }))
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST, axum::http::Method::OPTIONS])
                .allow_headers([axum::http::header::CONTENT_TYPE, axum::http::header::AUTHORIZATION])
        )
        .with_state(shared.clone());

    let mut app = Router::new().nest("/memory/api", api);

    // Serve static files at /memory/ if the directory exists
    if let Some(dir) = static_dir {
        if dir.is_dir() {
            info!("Serving static files from {}", dir.display());
            let index_path = dir.join("index.html");
            let serve = ServeDir::new(&dir)
                .append_index_html_on_directories(true)
                .fallback(tower_http::services::ServeFile::new(&index_path));
            app = app.nest_service("/memory", serve);
        } else {
            tracing::warn!(
                "Static directory {} not found, skipping static file serving",
                dir.display()
            );
        }
    }

    // Serve landing page at / if the directory exists, otherwise redirect to /memory/
    if let Some(ref dir) = landing_dir {
        if dir.is_dir() {
            info!("Serving landing page from {}", dir.display());
            let serve = ServeDir::new(dir).append_index_html_on_directories(true);
            app = app.fallback_service(serve);
        } else {
            tracing::warn!(
                "Landing directory {} not found, falling back to redirect",
                dir.display()
            );
            app = app.route("/", get(|| async { Redirect::permanent("/memory/") }));
        }
    } else {
        app = app.route("/", get(|| async { Redirect::permanent("/memory/") }));
    }

    app
}

/// Start the HTTP server on the given address.
pub async fn serve(
    addr: &str,
    state: AppState,
    static_dir: Option<PathBuf>,
    landing_dir: Option<PathBuf>,
) -> Result<()> {
    let app = build_router(state, static_dir, landing_dir);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("HTTP server listening on {addr}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

// ── Dispatch endpoint ─────────────────────────────────────────────

async fn api_dispatch(
    State(state): State<SharedState>,
    Extension(caller): Extension<CallerContext>,
    Json(req): Json<crate::api::types::ApiRequest>,
) -> impl IntoResponse {
    let request_id = req.meta.as_ref().and_then(|m| m.request_id.clone());
    let resp = crate::api::dispatch(
        &state.nomen,
        &state.default_channel,
        &req.action,
        &req.params,
        &caller,
    )
    .await
    .with_request_id(request_id);
    Json(serde_json::to_value(&resp).unwrap_or_default())
}

async fn api_health(State(state): State<SharedState>) -> impl IntoResponse {
    let count = state.nomen.count_memories().await;
    let (total, _, _) = count.unwrap_or((0, 0, 0));
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "memory_count": total,
    }))
}

/// Auth info endpoint — lets the UI know the caller's role and pubkey.
async fn api_auth_info(Extension(caller): Extension<CallerContext>) -> impl IntoResponse {
    Json(json!({
        "role": format!("{:?}", caller.role).to_lowercase(),
        "pubkey": caller.pubkey,
        "can_write": caller.can_write(),
        "allowed_visibilities": caller.allowed_visibilities(),
    }))
}

// ── Settings dashboard endpoints (kept outside dispatch) ─────────

fn strip_config_secrets(config: &Config) -> Value {
    let embedding = config.embedding.as_ref().map(|e| {
        json!({
            "provider": e.provider,
            "model": e.model,
            "dimensions": e.dimensions,
        })
    });

    // Resolve consolidation config
    let consolidation = config
        .memory
        .as_ref()
        .and_then(|m| m.consolidation.as_ref())
        .map(|c| {
            json!({
                "enabled": c.enabled,
                "interval_hours": c.interval_hours,
                "ephemeral_ttl_minutes": c.ephemeral_ttl_minutes,
                "max_ephemeral_count": c.max_ephemeral_count,
                "provider": c.provider,
                "model": c.model,
                "dry_run": c.dry_run,
            })
        })
        .or_else(|| {
            config.consolidation.as_ref().map(|c| {
                json!({
                    "enabled": true,
                    "provider": c.provider,
                    "model": c.model,
                })
            })
        });

    let groups: Vec<Value> = config
        .groups
        .iter()
        .map(|g| {
            json!({
                "id": g.id,
                "name": g.name,
                "member_count": g.members.len(),
            })
        })
        .collect();

    json!({
        "relay": config.relay,
        "embedding": embedding,
        "consolidation": consolidation,
        "groups": groups,
        "config_path": Config::path().to_string_lossy(),
    })
}

async fn api_get_config(
    Extension(caller): Extension<CallerContext>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    if !caller.is_owner() {
        return Json(json!({"ok": false, "error": {"code": "unauthorized", "message": "Owner access required"}}));
    }
    let config = state.config.read().await;
    Json(strip_config_secrets(&config))
}

async fn api_reload_config(
    Extension(caller): Extension<CallerContext>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    if !caller.is_owner() {
        return Json(json!({"ok": false, "error": {"code": "unauthorized", "message": "Owner access required"}}));
    }
    match Config::load() {
        Ok(new_config) => {
            let stripped = strip_config_secrets(&new_config);
            let mut config = state.config.write().await;
            *config = new_config;
            Json(stripped)
        }
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

async fn api_stats(
    Extension(caller): Extension<CallerContext>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    if !caller.is_owner() {
        return Json(json!({"ok": false, "error": {"code": "unauthorized", "message": "Owner access required"}}));
    }
    let (total, named, pending) = state
        .nomen
        .count_memories()
        .await
        .unwrap_or((0, 0, 0));
    let entities = crate::db::list_entities(state.nomen.db(), None)
        .await
        .map(|e| e.len())
        .unwrap_or(0);
    let groups = state
        .nomen
        .group_list()
        .await
        .map(|g| g.len())
        .unwrap_or(0);
    let last_consolidation =
        crate::db::get_meta(state.nomen.db(), "last_consolidation_run")
            .await
            .unwrap_or(None);
    let last_prune = crate::db::get_meta(state.nomen.db(), "last_prune_run")
        .await
        .unwrap_or(None);

    let db_size_bytes = estimate_db_size();

    Json(json!({
        "total_memories": total,
        "named_memories": named,
        "ephemeral_messages": pending,
        "entities": entities,
        "groups": groups,
        "last_consolidation": last_consolidation,
        "last_prune": last_prune,
        "db_size_bytes": db_size_bytes,
    }))
}

fn estimate_db_size() -> u64 {
    let db_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".nomen")
        .join("db");
    if !db_dir.exists() {
        return 0;
    }
    walkdir(db_dir)
}

fn walkdir(path: std::path::PathBuf) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(&path) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                if meta.is_file() {
                    total += meta.len();
                } else if meta.is_dir() {
                    total += walkdir(entry.path());
                }
            }
        }
    }
    total
}
