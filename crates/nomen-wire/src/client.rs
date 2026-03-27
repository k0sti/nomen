//! NomenClient — async client for connecting to Nomen socket server.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::UnixStream;
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
use tokio_util::codec::Framed;

use crate::codec::NomenCodec;
use crate::types::{Event, Frame, Request, Response};

/// Stream of push events from the server.
pub type EventStream = broadcast::Receiver<Event>;

/// Async client for the Nomen socket protocol.
pub struct NomenClient {
    /// Channel for sending frames to the writer task.
    tx: mpsc::Sender<Frame>,
    /// Pending request map: id → oneshot sender for the response.
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<Response>>>>,
    /// Broadcast channel for push events.
    events_tx: broadcast::Sender<Event>,
    /// Counter for generating unique request IDs.
    id_counter: AtomicU64,
    /// Handle to the background reader task.
    _reader_handle: tokio::task::JoinHandle<()>,
    /// Handle to the background writer task.
    _writer_handle: tokio::task::JoinHandle<()>,
}

impl NomenClient {
    /// Connect to a Nomen socket server at the given path.
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self> {
        let stream = UnixStream::connect(path.as_ref()).await.with_context(|| {
            format!("Failed to connect to socket at {}", path.as_ref().display())
        })?;

        let framed = Framed::new(stream, NomenCodec::new());
        let (sink, stream) = framed.split();

        let pending: Arc<Mutex<HashMap<String, oneshot::Sender<Response>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let (events_tx, _) = broadcast::channel(256);

        // Writer task: receives frames from mpsc and writes them to the socket.
        let (tx, mut rx) = mpsc::channel::<Frame>(64);
        let writer_handle = tokio::spawn(async move {
            let mut sink = sink;
            while let Some(frame) = rx.recv().await {
                if let Err(e) = sink.send(frame).await {
                    tracing::error!("Socket write error: {e}");
                    break;
                }
            }
        });

        // Reader task: reads frames from socket and dispatches them.
        let pending_clone = pending.clone();
        let events_tx_clone = events_tx.clone();
        let reader_handle = tokio::spawn(async move {
            let mut stream = stream;
            while let Some(result) = stream.next().await {
                match result {
                    Ok(frame) => match frame {
                        Frame::Response(resp) => {
                            let mut map = pending_clone.lock().await;
                            if let Some(sender) = map.remove(&resp.id) {
                                let _ = sender.send(resp);
                            }
                        }
                        Frame::Event(event) => {
                            let _ = events_tx_clone.send(event);
                        }
                        Frame::Request(_) => {
                            tracing::warn!("Received unexpected request frame from server");
                        }
                    },
                    Err(e) => {
                        tracing::error!("Socket read error: {e}");
                        break;
                    }
                }
            }
            // Connection closed — wake up all pending requests with an error.
            let mut map = pending_clone.lock().await;
            for (id, sender) in map.drain() {
                let _ = sender.send(Response::error(
                    id,
                    "connection_closed",
                    "Socket connection closed",
                ));
            }
        });

        Ok(Self {
            tx,
            pending,
            events_tx,
            id_counter: AtomicU64::new(1),
            _reader_handle: reader_handle,
            _writer_handle: writer_handle,
        })
    }

    /// Generate a unique request ID.
    fn next_id(&self) -> String {
        let n = self.id_counter.fetch_add(1, Ordering::Relaxed);
        format!("req-{n}")
    }

    /// Send a request and await the correlated response.
    pub async fn request(&self, action: &str, params: Value) -> Result<Response> {
        self.request_with_timeout(action, params, Duration::from_secs(30))
            .await
    }

    /// Send a request with a custom timeout.
    pub async fn request_with_timeout(
        &self,
        action: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Response> {
        let id = self.next_id();
        let (resp_tx, resp_rx) = oneshot::channel();

        // Register pending request.
        {
            let mut map = self.pending.lock().await;
            map.insert(id.clone(), resp_tx);
        }

        let frame = Frame::Request(Request {
            id: id.clone(),
            action: action.to_string(),
            params,
        });

        // Send the request frame.
        self.tx
            .send(frame)
            .await
            .map_err(|_| anyhow::anyhow!("Socket writer channel closed"))?;

        // Await response with timeout.
        match tokio::time::timeout(timeout, resp_rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_)) => anyhow::bail!("Response channel dropped (connection closed)"),
            Err(_) => {
                // Clean up pending request on timeout.
                let mut map = self.pending.lock().await;
                map.remove(&id);
                anyhow::bail!("Request timed out after {}ms", timeout.as_millis())
            }
        }
    }

    /// Subscribe to push events. Returns a stream of events.
    ///
    /// Sends a `subscribe` action to the server and returns a broadcast receiver.
    pub async fn subscribe(&self, events: &[&str]) -> Result<EventStream> {
        let event_list: Vec<Value> = events.iter().map(|e| json!(*e)).collect();
        let resp = self
            .request("subscribe", json!({"events": event_list}))
            .await?;
        if !resp.ok {
            let msg = resp
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "Unknown error".to_string());
            anyhow::bail!("Subscribe failed: {msg}");
        }
        Ok(self.events_tx.subscribe())
    }

    /// Unsubscribe from push events.
    pub async fn unsubscribe(&self, events: &[&str]) -> Result<()> {
        let event_list: Vec<Value> = events.iter().map(|e| json!(*e)).collect();
        let resp = self
            .request("unsubscribe", json!({"events": event_list}))
            .await?;
        if !resp.ok {
            let msg = resp
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| "Unknown error".to_string());
            anyhow::bail!("Unsubscribe failed: {msg}");
        }
        Ok(())
    }

    /// One-shot request: connect, send, receive, disconnect.
    pub async fn oneshot(path: impl AsRef<Path>, action: &str, params: Value) -> Result<Response> {
        let client = Self::connect(path).await?;
        let resp = client.request(action, params).await?;
        client.close().await;
        Ok(resp)
    }

    /// Graceful disconnect.
    pub async fn close(self) {
        drop(self.tx); // Close writer channel, which stops writer task.
                       // Reader task will stop when the connection drops.
    }
}

// ── ReconnectingClient ──────────────────────────────────────────────

use std::path::PathBuf;

/// A reconnecting wrapper around `NomenClient`.
///
/// Automatically reconnects if the socket server restarts or the connection drops.
pub struct ReconnectingClient {
    path: PathBuf,
    client: Mutex<Option<NomenClient>>,
    max_retries: usize,
}

impl ReconnectingClient {
    /// Create a new reconnecting client (does not connect immediately).
    pub fn new(path: impl AsRef<Path>, max_retries: usize) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            client: Mutex::new(None),
            max_retries,
        }
    }

    /// Ensure we have a connected client, reconnecting if needed.
    async fn ensure_connected(&self) -> Result<()> {
        let mut guard = self.client.lock().await;
        if guard.is_some() {
            return Ok(());
        }
        let client = NomenClient::connect(&self.path).await?;
        *guard = Some(client);
        Ok(())
    }

    /// Send a request, with automatic reconnection on failure.
    pub async fn request(&self, action: &str, params: Value) -> Result<Response> {
        for attempt in 0..=self.max_retries {
            // Ensure connected
            if let Err(e) = self.ensure_connected().await {
                if attempt == self.max_retries {
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(100 * (1 << attempt.min(5)))).await;
                continue;
            }

            // Try the request
            let result = {
                let guard = self.client.lock().await;
                if let Some(ref client) = *guard {
                    client.request(action, params.clone()).await
                } else {
                    Err(anyhow::anyhow!("No client"))
                }
            };

            match result {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    // Connection likely dead — drop the client so next attempt reconnects
                    let mut guard = self.client.lock().await;
                    *guard = None;

                    if attempt == self.max_retries {
                        return Err(e.context("All reconnection attempts failed"));
                    }
                    tracing::debug!(
                        "Request failed (attempt {}/{}): {e}, reconnecting...",
                        attempt + 1,
                        self.max_retries
                    );
                    tokio::time::sleep(Duration::from_millis(100 * (1 << attempt.min(5)))).await;
                }
            }
        }
        unreachable!()
    }

    /// Subscribe to push events with reconnection support.
    pub async fn subscribe(&self, events: &[&str]) -> Result<EventStream> {
        self.ensure_connected().await?;
        let guard = self.client.lock().await;
        if let Some(ref client) = *guard {
            client.subscribe(events).await
        } else {
            anyhow::bail!("Not connected")
        }
    }

    /// Graceful disconnect.
    pub async fn close(&self) {
        let mut guard = self.client.lock().await;
        if let Some(client) = guard.take() {
            client.close().await;
        }
    }
}
