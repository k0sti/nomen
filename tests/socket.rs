//! Integration tests for the Nomen socket server and NomenClient.
//!
//! These tests verify the Unix domain socket protocol end-to-end:
//! server startup, client connection, request/response, subscriptions,
//! and concurrent client handling.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use surrealdb::engine::local::{Db, SurrealKv};
use surrealdb::Surreal;
use tempfile::TempDir;
use tokio::time::timeout;

// ── Test helpers ────────────────────────────────────────────────────

/// Spin up a fresh SurrealDB with the nomen schema.
async fn init_test_db() -> anyhow::Result<(Surreal<Db>, TempDir)> {
    let tmp = tempfile::tempdir()?;
    let db = Surreal::new::<SurrealKv>(tmp.path()).await?;
    db.use_ns("nomen_test").use_db("nomen_test").await?;
    db.query(nomen::db::SCHEMA).await?.check()?;
    Ok((db, tmp))
}

/// Create a SocketServer bound to a temp path, returning the server and all
/// handles the caller needs to keep alive.
async fn setup_server() -> (
    nomen::socket::SocketServer,
    std::path::PathBuf,
    TempDir, // DB temp dir — must outlive the test
    TempDir, // Socket temp dir — must outlive the test
) {
    let (db, db_tmp) = init_test_db().await.expect("init_test_db");
    let nomen = nomen::Nomen::from_db(db);

    let sock_tmp = TempDir::new().expect("tempdir for socket");
    let sock_path = sock_tmp.path().join("test.sock");

    let config = nomen::config::SocketConfig {
        enabled: true,
        path: sock_path.to_string_lossy().to_string(),
        max_connections: 32,
        max_frame_size: 16 * 1024 * 1024,
    };

    let server = nomen::socket::SocketServer::new(
        Arc::new(nomen),
        &config,
        "test".to_string(),
        None,
    );

    (server, sock_path, db_tmp, sock_tmp)
}

/// Spawn server from an Arc and connect a client.
async fn spawn_and_connect(
    server: Arc<nomen::socket::SocketServer>,
    sock_path: &std::path::Path,
) -> (
    nomen_wire::NomenClient,
    tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let handle = tokio::spawn({
        let server = server.clone();
        async move { server.run().await }
    });

    // Wait for server to start listening
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = nomen_wire::NomenClient::connect(sock_path)
        .await
        .expect("client connect");

    (client, handle)
}

// ════════════════════════════════════════════════════════════════════
// Socket server + client integration tests
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t_sock_01_basic_request_response() {
    let (server, sock_path, _db_tmp, _sock_tmp) = setup_server().await;
    let server = Arc::new(server);
    let (client, _handle) = spawn_and_connect(server, &sock_path).await;

    let resp = timeout(Duration::from_secs(5), client.request("memory.list", json!({})))
        .await
        .expect("timeout")
        .expect("request");

    assert!(resp.ok, "memory.list should succeed: {:?}", resp.error);
    assert!(resp.result.is_some());

    client.close().await;
}

#[tokio::test]
async fn t_sock_02_unknown_action_returns_error() {
    let (server, sock_path, _db_tmp, _sock_tmp) = setup_server().await;
    let server = Arc::new(server);
    let (client, _handle) = spawn_and_connect(server, &sock_path).await;

    let resp = timeout(
        Duration::from_secs(5),
        client.request("bogus.action", json!({})),
    )
    .await
    .expect("timeout")
    .expect("request");

    assert!(!resp.ok, "bogus.action should fail");
    let err = resp.error.expect("should have error body");
    assert_eq!(err.code, "unknown_action");

    client.close().await;
}

#[tokio::test]
async fn t_sock_03_multiple_concurrent_clients() {
    let (server, sock_path, _db_tmp, _sock_tmp) = setup_server().await;
    let server = Arc::new(server);

    let handle = tokio::spawn({
        let s = server.clone();
        async move { s.run().await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Connect 3 clients
    let c1 = nomen_wire::NomenClient::connect(&sock_path)
        .await
        .expect("c1");
    let c2 = nomen_wire::NomenClient::connect(&sock_path)
        .await
        .expect("c2");
    let c3 = nomen_wire::NomenClient::connect(&sock_path)
        .await
        .expect("c3");

    // Each sends a request concurrently
    let (r1, r2, r3) = tokio::join!(
        c1.request("memory.list", json!({})),
        c2.request("memory.list", json!({})),
        c3.request("memory.list", json!({})),
    );

    assert!(r1.expect("r1").ok);
    assert!(r2.expect("r2").ok);
    assert!(r3.expect("r3").ok);

    c1.close().await;
    c2.close().await;
    c3.close().await;
    handle.abort();
}

#[tokio::test]
async fn t_sock_04_request_id_correlation() {
    let (server, sock_path, _db_tmp, _sock_tmp) = setup_server().await;
    let server = Arc::new(server);
    let (client, _handle) = spawn_and_connect(server, &sock_path).await;

    // Send 3 requests concurrently; NomenClient handles correlation internally.
    // Each should resolve to its own response.
    let (r1, r2, r3) = tokio::join!(
        client.request("memory.list", json!({})),
        client.request("memory.list", json!({"limit": 5})),
        client.request("memory.list", json!({"limit": 10})),
    );

    let r1 = r1.expect("r1");
    let r2 = r2.expect("r2");
    let r3 = r3.expect("r3");

    // All should succeed — correlation is verified by the fact that
    // each future resolved (the client matches response IDs to pending requests).
    assert!(r1.ok);
    assert!(r2.ok);
    assert!(r3.ok);

    // IDs should all be different
    assert_ne!(r1.id, r2.id);
    assert_ne!(r2.id, r3.id);

    client.close().await;
}

#[tokio::test]
async fn t_sock_05_subscription_opt_in() {
    let (server, sock_path, _db_tmp, _sock_tmp) = setup_server().await;
    let server = Arc::new(server);

    let handle = tokio::spawn({
        let s = server.clone();
        async move { s.run().await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Client A subscribes to memory.updated
    let client_a = nomen_wire::NomenClient::connect(&sock_path)
        .await
        .expect("client_a");
    let mut events_a = client_a
        .subscribe(&["memory.updated"])
        .await
        .expect("subscribe");

    // Client B does NOT subscribe
    let _client_b = nomen_wire::NomenClient::connect(&sock_path)
        .await
        .expect("client_b");

    // Emit a memory.updated event via the server
    server.emit(nomen_wire::Event {
        event: "memory.updated".to_string(),
        ts: 12345,
        data: json!({"topic": "test-topic"}),
    });

    // Client A should receive the event
    let evt = timeout(Duration::from_secs(2), events_a.recv())
        .await
        .expect("timeout waiting for event")
        .expect("recv event");
    assert_eq!(evt.event, "memory.updated");
    assert_eq!(evt.data["topic"], "test-topic");

    client_a.close().await;
    handle.abort();
}

#[tokio::test]
async fn t_sock_06_wildcard_subscription() {
    let (server, sock_path, _db_tmp, _sock_tmp) = setup_server().await;
    let server = Arc::new(server);

    let handle = tokio::spawn({
        let s = server.clone();
        async move { s.run().await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = nomen_wire::NomenClient::connect(&sock_path)
        .await
        .expect("client");
    let mut events = client.subscribe(&["*"]).await.expect("subscribe wildcard");

    // Emit different event types
    server.emit(nomen_wire::Event {
        event: "memory.updated".to_string(),
        ts: 1,
        data: json!({"n": 1}),
    });
    server.emit(nomen_wire::Event {
        event: "sync.complete".to_string(),
        ts: 2,
        data: json!({"n": 2}),
    });
    server.emit(nomen_wire::Event {
        event: "consolidation.complete".to_string(),
        ts: 3,
        data: json!({"n": 3}),
    });

    // Collect all 3 events
    let mut received = Vec::new();
    for _ in 0..3 {
        let evt = timeout(Duration::from_secs(2), events.recv())
            .await
            .expect("timeout")
            .expect("recv");
        received.push(evt.event.clone());
    }

    assert!(received.contains(&"memory.updated".to_string()));
    assert!(received.contains(&"sync.complete".to_string()));
    assert!(received.contains(&"consolidation.complete".to_string()));

    client.close().await;
    handle.abort();
}

#[tokio::test]
async fn t_sock_07_unsubscribe_stops_events() {
    let (server, sock_path, _db_tmp, _sock_tmp) = setup_server().await;
    let server = Arc::new(server);

    let handle = tokio::spawn({
        let s = server.clone();
        async move { s.run().await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let client = nomen_wire::NomenClient::connect(&sock_path)
        .await
        .expect("client");
    let mut events = client
        .subscribe(&["memory.updated", "sync.complete"])
        .await
        .expect("subscribe");

    // Unsubscribe from memory.updated
    client
        .unsubscribe(&["memory.updated"])
        .await
        .expect("unsubscribe");

    // Small delay to let unsubscribe propagate
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Emit both event types
    server.emit(nomen_wire::Event {
        event: "memory.updated".to_string(),
        ts: 1,
        data: json!({"should": "not_receive"}),
    });
    server.emit(nomen_wire::Event {
        event: "sync.complete".to_string(),
        ts: 2,
        data: json!({"should": "receive"}),
    });

    // Should receive sync.complete but NOT memory.updated
    let evt = timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("timeout")
        .expect("recv");
    assert_eq!(evt.event, "sync.complete");

    // Verify no more events arrive (memory.updated should have been filtered)
    let no_more = timeout(Duration::from_millis(300), events.recv()).await;
    assert!(no_more.is_err(), "should not receive any more events");

    client.close().await;
    handle.abort();
}

#[tokio::test]
async fn t_sock_08_client_disconnect_cleanup() {
    // Test that a subscribed client receives agent.disconnected events.
    //
    // Note: NomenClient::close() detaches background tasks rather than aborting
    // them, so the Unix socket may not close immediately. Instead, we test the
    // disconnect notification path by having client B connect via a raw
    // UnixStream which we can fully shut down, triggering the server's
    // disconnect handler.
    let (server, sock_path, _db_tmp, _sock_tmp) = setup_server().await;
    let server = Arc::new(server);

    let handle = tokio::spawn({
        let s = server.clone();
        async move { s.run().await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Client A subscribes to wildcard to catch all events
    let client_a = nomen_wire::NomenClient::connect(&sock_path)
        .await
        .expect("client_a");
    let mut events_a = client_a
        .subscribe(&["*"])
        .await
        .expect("subscribe");

    // Connect client B using a raw UnixStream so we can fully close it
    {
        let raw_stream = tokio::net::UnixStream::connect(&sock_path)
            .await
            .expect("raw connect");

        // Wait for B's connection to be established on the server side
        tokio::time::sleep(Duration::from_millis(200)).await;

        // We should have received agent.connected for B
        let evt = timeout(Duration::from_secs(2), events_a.recv())
            .await
            .expect("timeout waiting for agent.connected")
            .expect("recv");
        assert_eq!(evt.event, "agent.connected");

        // Drop the raw stream — this fully closes the socket
        drop(raw_stream);
    }

    // Client A should receive the disconnection event
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline - tokio::time::Instant::now();
        match tokio::time::timeout(remaining, events_a.recv()).await {
            Ok(Ok(evt)) if evt.event == "agent.disconnected" => {
                assert!(evt.data.get("agent_id").is_some());
                break;
            }
            Ok(Ok(_other)) => {
                continue;
            }
            Ok(Err(e)) => panic!("broadcast recv error: {e}"),
            Err(_) => panic!("timed out waiting for agent.disconnected event"),
        }
    }

    client_a.close().await;
    handle.abort();
}

// ════════════════════════════════════════════════════════════════════
// NomenClient-focused tests
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn t_client_01_connect_and_request() {
    let (server, sock_path, _db_tmp, _sock_tmp) = setup_server().await;
    let server = Arc::new(server);
    let (client, _handle) = spawn_and_connect(server, &sock_path).await;

    let resp = client
        .request("memory.list", json!({}))
        .await
        .expect("request");
    assert!(resp.ok);

    client.close().await;
}

#[tokio::test]
async fn t_client_05_timeout() {
    let (server, sock_path, _db_tmp, _sock_tmp) = setup_server().await;
    let server = Arc::new(server);
    let (client, _handle) = spawn_and_connect(server, &sock_path).await;

    // Verify that a normal request completes within a generous timeout
    let resp = client
        .request_with_timeout("memory.list", json!({}), Duration::from_secs(5))
        .await
        .expect("request_with_timeout");
    assert!(resp.ok);

    client.close().await;
}

#[tokio::test]
async fn t_client_06_concurrent_requests() {
    let (server, sock_path, _db_tmp, _sock_tmp) = setup_server().await;
    let server = Arc::new(server);
    let (client, _handle) = spawn_and_connect(server, &sock_path).await;

    let client = Arc::new(client);

    // Send 10 requests concurrently
    let mut handles = Vec::new();
    for i in 0..10 {
        let c = client.clone();
        handles.push(tokio::spawn(async move {
            c.request("memory.list", json!({"_tag": i})).await
        }));
    }

    let mut ok_count = 0;
    for h in handles {
        let resp = timeout(Duration::from_secs(5), h)
            .await
            .expect("task timeout")
            .expect("task join")
            .expect("request");
        if resp.ok {
            ok_count += 1;
        }
    }

    assert_eq!(ok_count, 10, "all 10 concurrent requests should succeed");

    // Arc<NomenClient> doesn't have close(), so just drop it
    drop(client);
}
