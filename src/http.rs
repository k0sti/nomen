//! HTTP server: REST API + static file serving for the web UI.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::Router;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use surrealdb::engine::local::Db;
use surrealdb::Surreal;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::info;

use crate::consolidate;
use crate::db;
use crate::embed::Embedder;
use crate::entities;
use crate::groups;
use crate::groups::GroupStore;
use crate::ingest;
use crate::search;
use crate::send;

// ── Shared state ─────────────────────────────────────────────────

pub struct AppState {
    pub db: Surreal<Db>,
    pub embedder: Box<dyn Embedder>,
    pub relay: Option<crate::relay::RelayManager>,
    pub groups: GroupStore,
    pub default_channel: String,
}

type SharedState = Arc<AppState>;

// ── Request / response types ─────────────────────────────────────

#[derive(Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub tier: Option<String>,
    pub scope: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
struct SearchResultJson {
    tier: String,
    topic: String,
    confidence: String,
    summary: String,
    created_at: u64,
    score: f64,
    match_type: String,
}

#[derive(Deserialize)]
pub struct StoreRequest {
    pub topic: String,
    pub summary: String,
    pub detail: Option<String>,
    pub tier: Option<String>,
    pub scope: Option<String>,
    pub confidence: Option<f64>,
}

#[derive(Deserialize)]
pub struct IngestRequest {
    pub source: String,
    pub sender: String,
    pub channel: Option<String>,
    pub content: String,
}

#[derive(Deserialize)]
pub struct MessagesQuery {
    pub source: Option<String>,
    pub channel: Option<String>,
    pub sender: Option<String>,
    pub since: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct EntitiesQuery {
    pub kind: Option<String>,
    pub query: Option<String>,
}

#[derive(Deserialize)]
pub struct MemoriesQuery {
    pub tier: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct ConsolidateRequest {
    pub channel: Option<String>,
    pub since: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateGroupRequest {
    pub id: String,
    pub name: String,
    pub members: Option<Vec<String>>,
    pub nostr_group: Option<String>,
}

#[derive(Deserialize)]
pub struct SendRequest {
    pub recipient: String,
    pub content: String,
    pub channel: Option<String>,
}

// ── Error helper ─────────────────────────────────────────────────

struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": self.0.to_string() })),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

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
        .route("/entities", get(api_entities))
        .route("/consolidate", post(api_consolidate))
        .route("/consolidate/status", get(api_consolidation_status))
        .route("/memories", get(api_list_memories))
        .route("/memories/{topic}", delete(api_delete_memory))
        .route("/groups", get(api_list_groups))
        .route("/groups", post(api_create_group))
        .route("/send", post(api_send))
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

// ── API handlers ─────────────────────────────────────────────────

async fn api_search(
    State(state): State<SharedState>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<Value>, AppError> {
    let opts = search::SearchOptions {
        query: req.query,
        tier: req.tier,
        allowed_scopes: req.scope.map(|s| vec![s]),
        limit: req.limit.unwrap_or(10),
        ..Default::default()
    };

    let results = search::search(&state.db, state.embedder.as_ref(), &opts).await?;

    let items: Vec<SearchResultJson> = results
        .into_iter()
        .map(|r| SearchResultJson {
            tier: r.tier,
            topic: r.topic,
            confidence: r.confidence,
            summary: r.summary,
            created_at: r.created_at.as_u64(),
            score: r.score,
            match_type: format!("{:?}", r.match_type),
        })
        .collect();

    Ok(Json(json!({ "results": items })))
}

async fn api_store(
    State(state): State<SharedState>,
    Json(req): Json<StoreRequest>,
) -> Result<Json<Value>, AppError> {
    let mem = crate::NewMemory {
        topic: req.topic.clone(),
        summary: req.summary,
        detail: req.detail.unwrap_or_default(),
        tier: req.tier.unwrap_or_else(|| "public".to_string()),
        confidence: req.confidence.unwrap_or(0.8),
        source: Some("http".to_string()),
        model: Some("http/api".to_string()),
    };

    let d_tag = crate::Nomen::store_direct(&state.db, state.embedder.as_ref(), mem).await?;
    Ok(Json(json!({ "stored": req.topic, "d_tag": d_tag })))
}

async fn api_ingest(
    State(state): State<SharedState>,
    Json(req): Json<IngestRequest>,
) -> Result<Json<Value>, AppError> {
    let msg = ingest::RawMessage {
        source: req.source,
        source_id: None,
        sender: req.sender,
        channel: req.channel,
        content: req.content,
        metadata: None,
        created_at: None,
    };

    let id = ingest::ingest_message(&state.db, &msg).await?;
    Ok(Json(json!({ "id": id })))
}

async fn api_messages(
    State(state): State<SharedState>,
    Query(q): Query<MessagesQuery>,
) -> Result<Json<Value>, AppError> {
    let opts = ingest::MessageQuery {
        source: q.source,
        channel: q.channel,
        sender: q.sender,
        since: q.since,
        limit: Some(q.limit.unwrap_or(50)),
        consolidated_only: false,
    };

    let messages = ingest::get_messages(&state.db, &opts).await?;
    Ok(Json(json!({ "messages": messages })))
}

async fn api_entities(
    State(state): State<SharedState>,
    Query(q): Query<EntitiesQuery>,
) -> Result<Json<Value>, AppError> {
    let kind = q.kind.as_deref().and_then(entities::EntityKind::from_str);
    let mut entity_list = db::list_entities(&state.db, kind.as_ref()).await?;

    // Filter by name query if provided
    if let Some(ref query) = q.query {
        let q_lower = query.to_lowercase();
        entity_list.retain(|e| e.name.to_lowercase().contains(&q_lower));
    }

    Ok(Json(json!({ "entities": entity_list })))
}

async fn api_consolidation_status(
    State(state): State<SharedState>,
) -> Result<Json<Value>, AppError> {
    // Use default consolidation config values for the check
    let config = crate::config::MemoryConsolidationConfig {
        enabled: true,
        interval_hours: 4,
        ephemeral_ttl_minutes: 60,
        max_ephemeral_count: 200,
        dry_run: false,
        provider: None,
        model: None,
        api_key_env: None,
        base_url: None,
    };
    let status = consolidate::check_consolidation_due(&state.db, &config).await?;
    Ok(Json(serde_json::to_value(status)?))
}

async fn api_consolidate(
    State(state): State<SharedState>,
    Json(_req): Json<ConsolidateRequest>,
) -> Result<Json<Value>, AppError> {
    let config = consolidate::ConsolidationConfig::default();
    let report = consolidate::consolidate(&state.db, state.embedder.as_ref(), &config, state.relay.as_ref()).await?;

    Ok(Json(json!({
        "messages_processed": report.messages_processed,
        "memories_created": report.memories_created,
        "channels": report.channels,
    })))
}

async fn api_list_memories(
    State(state): State<SharedState>,
    Query(q): Query<MemoriesQuery>,
) -> Result<Json<Value>, AppError> {
    let limit = q.limit.unwrap_or(200);
    let memories = db::list_memories(&state.db, q.tier.as_deref(), limit).await?;
    Ok(Json(json!({ "memories": memories })))
}

async fn api_delete_memory(
    State(state): State<SharedState>,
    Path(topic): Path<String>,
) -> Result<Json<Value>, AppError> {
    let d_tag = format!("snow:memory:{topic}");
    db::delete_memory_by_dtag(&state.db, &d_tag).await?;
    Ok(Json(json!({ "deleted": topic })))
}

async fn api_list_groups(
    State(state): State<SharedState>,
) -> Result<Json<Value>, AppError> {
    let group_list = groups::list_groups(&state.db).await?;
    Ok(Json(json!({ "groups": group_list })))
}

async fn api_create_group(
    State(state): State<SharedState>,
    Json(req): Json<CreateGroupRequest>,
) -> Result<Json<Value>, AppError> {
    let members = req.members.unwrap_or_default();
    groups::create_group(
        &state.db,
        &req.id,
        &req.name,
        &members,
        req.nostr_group.as_deref(),
        None,
    )
    .await?;
    Ok(Json(json!({ "created": req.id })))
}

async fn api_send(
    State(state): State<SharedState>,
    Json(req): Json<SendRequest>,
) -> Result<Json<Value>, AppError> {
    let relay = state
        .relay
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No relay configured — cannot send messages"))?;

    let target = send::parse_recipient(&req.recipient)?;
    let opts = send::SendOptions {
        target,
        content: req.content,
        channel: req.channel,
        metadata: None,
    };

    let result = send::send_message(relay, &state.db, &state.groups, opts).await?;
    Ok(Json(json!({
        "event_id": result.event_id,
        "accepted": result.accepted,
        "rejected": result.rejected,
    })))
}
