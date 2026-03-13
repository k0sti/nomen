//! HTTP server: REST API + static file serving for the web UI.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Json, Redirect};
use axum::routing::{delete, get, post};
use axum::Router;
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::info;

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
        .route("/search", post(api_search))
        .route("/store", post(api_store))
        .route("/ingest", post(api_ingest))
        .route("/messages", get(api_messages))
        .route("/messages/{id}/context", get(api_message_context))
        .route("/entities", get(api_entities))
        .route("/entities/relationships", get(api_entity_relationships))
        .route("/consolidate", post(api_consolidate))
        .route("/memories", get(api_list_memories))
        .route("/memories/{topic}", get(api_get_memory))
        .route("/memories/{topic}", delete(api_delete_memory))
        .route("/groups", get(api_list_groups))
        .route("/groups", post(api_create_group))
        .route("/groups/{id}/members", get(api_group_members))
        .route("/groups/{id}/members", post(api_group_add_member))
        .route("/groups/{id}/members/{npub}", delete(api_group_remove_member))
        .route("/send", post(api_send))
        .route("/config", get(api_get_config))
        .route("/config/reload", post(api_reload_config))
        .route("/stats", get(api_stats))
        .route("/prune", post(api_prune))
        .route("/embed", post(api_embed))
        .route("/sync", post(api_sync))
        .route("/cluster", post(api_cluster))
        .route("/health", get(api_health))
        .layer(CorsLayer::permissive())
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
    axum::serve(listener, app).await?;
    Ok(())
}

// ── Dispatch helpers ─────────────────────────────────────────────

/// Call dispatch and return the ApiResponse as JSON.
async fn call_dispatch(state: &AppState, action: &str, params: &Value) -> impl IntoResponse {
    let resp =
        crate::api::dispatch(&state.nomen, &state.default_channel, action, params).await;
    Json(serde_json::to_value(&resp).unwrap_or_default())
}

// ── API handlers (thin dispatch wrappers) ────────────────────────

async fn api_search(State(state): State<SharedState>, Json(body): Json<Value>) -> impl IntoResponse {
    call_dispatch(&state, "memory.search", &body).await
}

async fn api_store(State(state): State<SharedState>, Json(body): Json<Value>) -> impl IntoResponse {
    call_dispatch(&state, "memory.put", &body).await
}

async fn api_ingest(State(state): State<SharedState>, Json(body): Json<Value>) -> impl IntoResponse {
    call_dispatch(&state, "message.ingest", &body).await
}

async fn api_messages(
    State(state): State<SharedState>,
    Query(q): Query<Value>,
) -> impl IntoResponse {
    call_dispatch(&state, "message.list", &q).await
}

async fn api_message_context(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Query(q): Query<Value>,
) -> impl IntoResponse {
    let mut params = q;
    params
        .as_object_mut()
        .map(|o| o.insert("id".to_string(), json!(id)));
    call_dispatch(&state, "message.context", &params).await
}

async fn api_entities(
    State(state): State<SharedState>,
    Query(q): Query<Value>,
) -> impl IntoResponse {
    call_dispatch(&state, "entity.list", &q).await
}

async fn api_entity_relationships(
    State(state): State<SharedState>,
    Query(q): Query<Value>,
) -> impl IntoResponse {
    call_dispatch(&state, "entity.relationships", &q).await
}

async fn api_consolidate(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    call_dispatch(&state, "memory.consolidate", &body).await
}

async fn api_list_memories(
    State(state): State<SharedState>,
    Query(q): Query<Value>,
) -> impl IntoResponse {
    call_dispatch(&state, "memory.list", &q).await
}

async fn api_get_memory(
    State(state): State<SharedState>,
    Path(topic): Path<String>,
) -> impl IntoResponse {
    call_dispatch(&state, "memory.get", &json!({"topic": topic})).await
}

async fn api_delete_memory(
    State(state): State<SharedState>,
    Path(topic): Path<String>,
) -> impl IntoResponse {
    call_dispatch(&state, "memory.delete", &json!({"topic": topic})).await
}

async fn api_list_groups(State(state): State<SharedState>) -> impl IntoResponse {
    call_dispatch(&state, "group.list", &json!({})).await
}

async fn api_create_group(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    call_dispatch(&state, "group.create", &body).await
}

async fn api_group_members(
    State(state): State<SharedState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    call_dispatch(&state, "group.members", &json!({"id": id})).await
}

async fn api_group_add_member(
    State(state): State<SharedState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let mut params = body;
    params
        .as_object_mut()
        .map(|o| o.insert("id".to_string(), json!(id)));
    call_dispatch(&state, "group.add_member", &params).await
}

async fn api_group_remove_member(
    State(state): State<SharedState>,
    Path((id, npub)): Path<(String, String)>,
) -> impl IntoResponse {
    call_dispatch(
        &state,
        "group.remove_member",
        &json!({"id": id, "npub": npub}),
    )
    .await
}

async fn api_send(State(state): State<SharedState>, Json(body): Json<Value>) -> impl IntoResponse {
    call_dispatch(&state, "message.send", &body).await
}

async fn api_prune(State(state): State<SharedState>, Json(body): Json<Value>) -> impl IntoResponse {
    call_dispatch(&state, "memory.prune", &body).await
}

async fn api_embed(State(state): State<SharedState>, Json(body): Json<Value>) -> impl IntoResponse {
    call_dispatch(&state, "memory.embed", &body).await
}

async fn api_sync(State(state): State<SharedState>) -> impl IntoResponse {
    call_dispatch(&state, "memory.sync", &json!({})).await
}

async fn api_cluster(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    call_dispatch(&state, "memory.cluster", &body).await
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

async fn api_get_config(State(state): State<SharedState>) -> impl IntoResponse {
    let config = state.config.read().await;
    Json(strip_config_secrets(&config))
}

async fn api_reload_config(State(state): State<SharedState>) -> impl IntoResponse {
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

async fn api_stats(State(state): State<SharedState>) -> impl IntoResponse {
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
