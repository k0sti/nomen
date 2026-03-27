//! Unix domain socket server for the Nomen wire protocol.
//!
//! Supports per-connection identity via `identity.auth` action.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use nomen_wire::{Event, Frame, NomenCodec, Response};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, RwLock};
use tokio_util::codec::Framed;
use tracing::{debug, error, info, warn};

use nomen_api::NomenBackend;
use nomen_core::config::SocketConfig;
use nomen_core::signer::NomenSigner;

/// A connection ID for tracking subscriptions.
type ConnId = u64;

/// Socket server state shared across connection tasks.
struct ServerState {
    nomen: Arc<dyn NomenBackend>,
    default_channel: String,
    /// Subscription registry: connection ID -> subscribed event types.
    subscriptions: RwLock<HashMap<ConnId, HashSet<String>>>,
    /// Broadcast channel for push events.
    event_tx: broadcast::Sender<Event>,
    /// Connection ID counter.
    next_conn_id: AtomicU64,
}

pub struct SocketServer {
    state: Arc<ServerState>,
    socket_path: PathBuf,
    max_connections: usize,
    max_frame_size: usize,
}

impl SocketServer {
    pub fn new(
        nomen: Arc<dyn NomenBackend>,
        config: &SocketConfig,
        default_channel: String,
        event_tx: Option<broadcast::Sender<Event>>,
    ) -> Self {
        let event_tx = event_tx.unwrap_or_else(|| {
            let (tx, _) = broadcast::channel(1024);
            tx
        });
        Self {
            state: Arc::new(ServerState {
                nomen,
                default_channel,
                subscriptions: RwLock::new(HashMap::new()),
                event_tx,
                next_conn_id: AtomicU64::new(1),
            }),
            socket_path: PathBuf::from(&config.path),
            max_connections: config.max_connections,
            max_frame_size: config.max_frame_size,
        }
    }

    /// Get the broadcast sender for sharing with other components.
    pub fn event_sender(&self) -> broadcast::Sender<Event> {
        self.state.event_tx.clone()
    }

    /// Broadcast a push event to subscribed connections.
    pub fn emit(&self, event: Event) {
        // Only send if there are subscribers
        let _ = self.state.event_tx.send(event);
    }

    /// Accept connections and dispatch requests.
    pub async fn run(&self) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.socket_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Remove stale socket file if it exists
        if self.socket_path.exists() {
            tokio::fs::remove_file(&self.socket_path).await?;
        }

        let listener = UnixListener::bind(&self.socket_path)?;

        // Set socket file permissions to 0660
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o660);
            std::fs::set_permissions(&self.socket_path, perms)?;
        }

        info!("Socket server listening on {}", self.socket_path.display());

        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.max_connections));
        let max_frame_size = self.max_frame_size;

        loop {
            let (stream, _addr) = listener.accept().await?;
            let state = self.state.clone();
            let permit = semaphore.clone().acquire_owned().await?;
            let conn_id = state.next_conn_id.fetch_add(1, Ordering::Relaxed);

            debug!("Socket client connected (id={conn_id})");

            // Notify subscribers about new agent connection
            let connect_event = Event {
                event: "agent.connected".to_string(),
                ts: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                data: serde_json::json!({"agent_id": conn_id}),
            };
            let _ = state.event_tx.send(connect_event);

            tokio::spawn(async move {
                let codec = NomenCodec::with_max_frame_size(max_frame_size);
                let framed = Framed::new(stream, codec);
                let (mut sink, mut stream) = framed.split();

                // Subscribe to events broadcast for this connection
                let mut event_rx = state.event_tx.subscribe();

                // Per-connection backend override (set by identity.auth)
                let mut conn_backend: Option<Arc<dyn NomenBackend>> = None;

                loop {
                    tokio::select! {
                        // Handle incoming frames from client
                        frame = stream.next() => {
                            match frame {
                                Some(Ok(Frame::Request(req))) => {
                                    // Intercept identity.auth to set per-connection signer
                                    if req.action == "identity.auth" {
                                        let (response, new_backend) =
                                            handle_identity_auth(&state, conn_id, &req).await;
                                        if let Some(b) = new_backend {
                                            conn_backend = Some(b);
                                        }
                                        if let Err(e) = sink.send(Frame::Response(response)).await {
                                            error!("Failed to send response to conn {conn_id}: {e}");
                                            break;
                                        }
                                    } else if conn_backend.is_none() {
                                        // Require authentication before any other action
                                        let response = nomen_wire::Response {
                                            id: req.id,
                                            ok: false,
                                            result: None,
                                            error: Some(nomen_wire::ErrorBody {
                                                code: "auth_required".to_string(),
                                                message: "Authentication required. Send identity.auth first.".to_string(),
                                            }),
                                            meta: serde_json::json!({"version": "v2"}),
                                        };
                                        if let Err(e) = sink.send(Frame::Response(response)).await {
                                            error!("Failed to send response to conn {conn_id}: {e}");
                                            break;
                                        }
                                    } else {
                                        let backend: &dyn NomenBackend = &**conn_backend.as_ref().unwrap();
                                        let response = handle_request_with_backend(backend, &state, conn_id, req).await;
                                        if let Err(e) = sink.send(Frame::Response(response)).await {
                                            error!("Failed to send response to conn {conn_id}: {e}");
                                            break;
                                        }
                                    }
                                }
                                Some(Ok(_)) => {
                                    warn!("Unexpected frame type from conn {conn_id}");
                                }
                                Some(Err(e)) => {
                                    warn!("Frame error from conn {conn_id}: {e}");
                                    break;
                                }
                                None => {
                                    debug!("Client disconnected (id={conn_id})");
                                    break;
                                }
                            }
                        }
                        // Forward push events to subscribed clients
                        event = event_rx.recv() => {
                            match event {
                                Ok(evt) => {
                                    if is_subscribed(&state, conn_id, &evt.event).await {
                                        if let Err(e) = sink.send(Frame::Event(evt)).await {
                                            error!("Failed to send event to conn {conn_id}: {e}");
                                            break;
                                        }
                                    }
                                }
                                Err(broadcast::error::RecvError::Lagged(n)) => {
                                    warn!("Conn {conn_id} lagged behind by {n} events");
                                }
                                Err(broadcast::error::RecvError::Closed) => break,
                            }
                        }
                    }
                }

                // Cleanup: remove subscriptions
                {
                    let mut subs = state.subscriptions.write().await;
                    subs.remove(&conn_id);
                }

                // Notify about disconnection
                let disconnect_event = Event {
                    event: "agent.disconnected".to_string(),
                    ts: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                    data: serde_json::json!({"agent_id": conn_id}),
                };
                let _ = state.event_tx.send(disconnect_event);

                drop(permit); // Release connection slot
            });
        }
    }

    /// Clean up socket file on shutdown.
    pub async fn cleanup(&self) {
        if self.socket_path.exists() {
            let _ = tokio::fs::remove_file(&self.socket_path).await;
        }
    }
}

/// Check if a connection is subscribed to a given event type.
async fn is_subscribed(state: &ServerState, conn_id: ConnId, event_type: &str) -> bool {
    let subs = state.subscriptions.read().await;
    match subs.get(&conn_id) {
        Some(events) => events.contains("*") || events.contains(event_type),
        None => false,
    }
}

/// Handle a single request frame using a specific backend.
///
/// Request routing:
/// 1. **Transport capabilities** — `subscribe` and `unsubscribe` are socket-specific
///    transport features for push event management. They are NOT canonical API actions
///    and are handled directly by the socket layer.
/// 2. **Canonical dispatch** — all other actions are routed through `api::dispatch()`,
///    producing the same canonical request/response envelope as HTTP, MCP, and CVM.
async fn handle_request_with_backend(
    backend: &dyn NomenBackend,
    state: &ServerState,
    conn_id: ConnId,
    req: nomen_wire::Request,
) -> Response {
    match req.action.as_str() {
        // Transport-specific: event subscription management
        "subscribe" => handle_subscribe(state, conn_id, &req).await,
        "unsubscribe" => handle_unsubscribe(state, conn_id, &req).await,
        // Canonical dispatch — same semantics as HTTP/MCP/CVM
        _ => {
            let api_resp =
                nomen_api::dispatch(backend, &state.default_channel, &req.action, &req.params)
                    .await;

            // Map canonical ApiResponse to wire Response envelope
            let resp_value = serde_json::to_value(&api_resp).unwrap_or_default();
            Response {
                id: req.id,
                ok: api_resp.ok,
                result: api_resp.result,
                error: api_resp.error.map(|e| nomen_wire::ErrorBody {
                    code: e.code,
                    message: e.message,
                }),
                meta: resp_value.get("meta").cloned().unwrap_or_default(),
            }
        }
    }
}

/// Handle identity.auth: create per-connection SessionBackend.
async fn handle_identity_auth(
    state: &ServerState,
    conn_id: ConnId,
    req: &nomen_wire::Request,
) -> (Response, Option<Arc<dyn NomenBackend>>) {
    // Dispatch to validate the nsec
    let api_resp = nomen_api::dispatch(
        &*state.nomen,
        &state.default_channel,
        "identity.auth",
        &req.params,
    )
    .await;

    if !api_resp.ok {
        let resp_value = serde_json::to_value(&api_resp).unwrap_or_default();
        return (
            Response {
                id: req.id.clone(),
                ok: api_resp.ok,
                result: api_resp.result,
                error: api_resp.error.map(|e| nomen_wire::ErrorBody {
                    code: e.code,
                    message: e.message,
                }),
                meta: resp_value.get("meta").cloned().unwrap_or_default(),
            },
            None,
        );
    }

    // Create the session backend
    let nsec = req
        .params
        .get("nsec")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match nostr_sdk::Keys::parse(nsec) {
        Ok(keys) => {
            let signer = Arc::new(nomen_relay::signer::KeysSigner::new(keys));
            info!(conn_id, pubkey = %signer.public_key().to_hex(), "Socket: session identity set");
            let session_backend: Arc<dyn NomenBackend> =
                Arc::new(nomen_api::SessionBackend::new(state.nomen.clone(), signer));

            let resp_value = serde_json::to_value(&api_resp).unwrap_or_default();
            (
                Response {
                    id: req.id.clone(),
                    ok: api_resp.ok,
                    result: api_resp.result,
                    error: None,
                    meta: resp_value.get("meta").cloned().unwrap_or_default(),
                },
                Some(session_backend),
            )
        }
        Err(e) => {
            warn!(conn_id, "Socket: identity.auth failed to parse nsec: {e}");
            (
                Response::error(
                    req.id.clone(),
                    "invalid_params",
                    &format!("invalid nsec: {e}"),
                ),
                None,
            )
        }
    }
}

/// Handle subscribe action.
async fn handle_subscribe(
    state: &ServerState,
    conn_id: ConnId,
    req: &nomen_wire::Request,
) -> Response {
    let events: Vec<String> = req
        .params
        .get("events")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    if events.is_empty() {
        return Response::error(req.id.clone(), "invalid_params", "events array required");
    }

    let mut subs = state.subscriptions.write().await;
    let entry = subs.entry(conn_id).or_insert_with(HashSet::new);
    for event in &events {
        entry.insert(event.clone());
    }

    Response::success(
        req.id.clone(),
        serde_json::json!({
            "subscribed": events,
        }),
    )
}

/// Handle unsubscribe action.
async fn handle_unsubscribe(
    state: &ServerState,
    conn_id: ConnId,
    req: &nomen_wire::Request,
) -> Response {
    let events: Vec<String> = req
        .params
        .get("events")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut subs = state.subscriptions.write().await;
    if let Some(entry) = subs.get_mut(&conn_id) {
        for event in &events {
            entry.remove(event);
        }
    }

    Response::success(
        req.id.clone(),
        serde_json::json!({
            "unsubscribed": events,
        }),
    )
}
