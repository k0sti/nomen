//! Transport conformance tests — verify semantic equivalence across transports.
//!
//! All Nomen transports (direct dispatch, CVM direct action, CVM tools/call, MCP, HTTP, socket)
//! route through `api::dispatch`. These tests confirm that the same operation
//! produces equivalent results regardless of which transport carries it.
//!
//! ## Structure
//!
//! - **Fixtures**: reusable `(action, params)` pairs shared across transports.
//! - **Transport helpers**: dispatch a fixture through a specific transport and
//!   return the canonical `Value` envelope (`{ ok, result, error, meta }`).
//! - **Tests**: exercise individual transports, cross-transport equivalence,
//!   error parity, and a live HTTP smoke test.

use anyhow::Result;
use contextvm_sdk::{JsonRpcMessage, JsonRpcRequest};
use serde_json::{json, Value};
use surrealdb::engine::local::{Db, SurrealKv};
use surrealdb::Surreal;

use nomen::cvm::CvmHandler;
use nomen::mcp::McpServer;
use nomen::socket::SocketServer;

// ── Fixture requests ──────────────────────────────────────────────────
//
// Reusable `(action, params)` pairs. Each test picks the fixtures it needs
// and dispatches them through one or more transports.

/// A fixture request: canonical action name + params.
struct Fixture {
    action: &'static str,
    params: Value,
}

/// Fixtures for common operations.
mod fixtures {
    use serde_json::{json, Value};
    use super::Fixture;

    pub fn memory_list() -> Fixture {
        Fixture { action: "memory.list", params: json!({}) }
    }

    #[allow(dead_code)]
    pub fn memory_list_limited(limit: u64) -> Fixture {
        Fixture { action: "memory.list", params: json!({ "limit": limit }) }
    }

    pub fn memory_put(topic: &str, content: &str) -> Fixture {
        Fixture {
            action: "memory.put",
            params: json!({
                "topic": topic,
                "content": content,
                "visibility": "public",
            }),
        }
    }

    pub fn memory_get(d_tag: &str) -> Fixture {
        Fixture { action: "memory.get", params: json!({ "d_tag": d_tag }) }
    }

    pub fn memory_search(query: &str) -> Fixture {
        Fixture {
            action: "memory.search",
            params: json!({ "query": query, "limit": 10 }),
        }
    }

    pub fn memory_search_missing_query() -> Fixture {
        Fixture { action: "memory.search", params: json!({}) }
    }

    pub fn unknown_action() -> Fixture {
        Fixture { action: "bogus.action", params: json!({}) }
    }

    #[allow(dead_code)]
    pub fn group_list() -> Fixture {
        Fixture { action: "group.list", params: json!({}) }
    }

    /// The MCP tool name for a canonical action (e.g. "memory.list" → "memory_list").
    pub fn mcp_tool_name(action: &str) -> String {
        action.replacen('.', "_", 1)
    }

    /// Build MCP tools/call params from a fixture.
    pub fn mcp_tools_call(f: &Fixture) -> Value {
        json!({
            "name": mcp_tool_name(f.action),
            "arguments": f.params,
        })
    }
}

// ── Test infrastructure ──────────────────────────────────────────────

async fn init_test_db() -> Result<(Surreal<Db>, tempfile::TempDir)> {
    let tmp = tempfile::tempdir()?;
    let db = Surreal::new::<SurrealKv>(tmp.path()).await?;
    db.use_ns("nomen_test").use_db("nomen_test").await?;
    db.query(nomen::db::SCHEMA).await?.check()?;
    Ok((db, tmp))
}

async fn test_nomen() -> Result<(nomen::Nomen, tempfile::TempDir)> {
    let (db, tmp) = init_test_db().await?;
    Ok((nomen::Nomen::from_db(db), tmp))
}

/// Store a test memory via direct dispatch and return the serialized ApiResponse.
async fn store_test_memory(nomen: &nomen::Nomen, topic: &str, summary: &str) -> Value {
    let f = fixtures::memory_put(topic, summary);
    dispatch_direct(nomen, &f).await
}

// ── Transport dispatch helpers ───────────────────────────────────────
//
// Each helper dispatches a Fixture through a specific transport and returns
// the canonical `Value` envelope.

/// Direct `api::dispatch()`.
async fn dispatch_direct(nomen: &dyn nomen_api::NomenBackend, f: &Fixture) -> Value {
    let resp = nomen::api::dispatch(nomen, "test", f.action, &f.params).await;
    serde_json::to_value(&resp).unwrap()
}

/// HTTP dispatch via test router (in-process, no TCP).
async fn dispatch_http(router: axum::Router, f: &Fixture) -> Value {
    use tower::ServiceExt;

    let body = json!({ "action": f.action, "params": f.params });
    let req = http::Request::builder()
        .method("POST")
        .uri("/memory/api/dispatch")
        .header("content-type", "application/json")
        .body(axum::body::Body::from(serde_json::to_string(&body).unwrap()))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap_or(json!(null))
}

/// CVM direct action dispatch.
fn make_cvm_request(f: &Fixture) -> JsonRpcMessage {
    JsonRpcMessage::Request(JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: json!(1),
        method: f.action.to_string(),
        params: Some(f.params.clone()),
    })
}

async fn dispatch_cvm(handler: &CvmHandler, f: &Fixture) -> Value {
    let resp = handler.handle_message(&make_cvm_request(f)).await;
    match resp {
        JsonRpcMessage::Response(r) => r.result.clone(),
        _ => panic!("Expected Response, got: {:?}", resp),
    }
}

/// CVM tools/call dispatch — returns the inner ApiResponse envelope.
async fn dispatch_cvm_tools_call(handler: &CvmHandler, f: &Fixture) -> Value {
    let req = JsonRpcMessage::Request(JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: json!(1),
        method: "tools/call".to_string(),
        params: Some(fixtures::mcp_tools_call(f)),
    });
    let resp = handler.handle_message(&req).await;
    let result = match &resp {
        JsonRpcMessage::Response(r) => &r.result,
        _ => panic!("Expected Response, got: {:?}", resp),
    };
    let text = result["content"][0]["text"].as_str().expect("content[0].text");
    serde_json::from_str(text).expect("valid JSON ApiResponse")
}

/// MCP tools/call dispatch — returns the inner ApiResponse envelope.
async fn dispatch_mcp(mcp: &McpServer<'_>, f: &Fixture) -> Value {
    let req = nomen::mcp::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: fixtures::mcp_tools_call(f),
    };
    let resp = mcp.handle_request(&req).await;
    let result = resp.result.as_ref().expect("MCP response should have result");
    let text = result["content"][0]["text"].as_str().expect("content[0].text");
    serde_json::from_str(text).expect("valid JSON ApiResponse")
}

/// Socket dispatch — returns the canonical envelope as Value.
async fn dispatch_socket(
    client: &nomen_wire::NomenClient,
    f: &Fixture,
) -> Value {
    let resp = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.request(f.action, f.params.clone()),
    )
    .await
    .expect("socket timeout")
    .expect("socket request");

    // Convert wire Response to canonical Value envelope
    json!({
        "ok": resp.ok,
        "result": resp.result,
        "error": resp.error.map(|e| json!({ "code": e.code, "message": e.message })),
        "meta": resp.meta,
    })
}

// ── Transport factory helpers ────────────────────────────────────────

fn make_handler(nomen: nomen::Nomen) -> CvmHandler {
    CvmHandler::new(Box::new(nomen), vec![], 100, "test".to_string())
}

fn make_mcp_server(nomen: nomen::Nomen) -> McpServer<'static> {
    // Leak the nomen instance to get a 'static reference for tests
    let nomen_ref: &'static dyn nomen_api::NomenBackend = Box::leak(Box::new(nomen));
    McpServer { nomen: nomen_ref, default_channel: "test".to_string() }
}

fn build_test_router(nomen: nomen::Nomen) -> axum::Router {
    let state = nomen::http::AppState {
        nomen: std::sync::Arc::new(nomen) as std::sync::Arc<dyn nomen_api::NomenBackend>,
        default_channel: "test".to_string(),
        config: std::sync::Arc::new(tokio::sync::RwLock::new(nomen::config::Config::default())),
    };
    nomen::http::build_router(state, None, None)
}

async fn setup_socket_server(
    db: Surreal<Db>,
) -> (
    std::sync::Arc<SocketServer>,
    std::path::PathBuf,
    tempfile::TempDir,
    tokio::task::JoinHandle<Result<()>>,
) {
    let nomen = nomen::Nomen::from_db(db);
    let sock_tmp = tempfile::tempdir().expect("tempdir for socket");
    let sock_path = sock_tmp.path().join("conformance.sock");

    let config = nomen::config::SocketConfig {
        enabled: true,
        path: sock_path.to_string_lossy().to_string(),
        max_connections: 32,
        max_frame_size: 16 * 1024 * 1024,
    };

    let server = std::sync::Arc::new(SocketServer::new(
        std::sync::Arc::new(nomen) as std::sync::Arc<dyn nomen_api::NomenBackend>,
        &config,
        "test".to_string(),
        None,
    ));

    let handle = tokio::spawn({
        let s = server.clone();
        async move { s.run().await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    (server, sock_path, sock_tmp, handle)
}

// ── Assertion helpers ────────────────────────────────────────────────

/// Assert that a canonical envelope reports success.
fn assert_ok(val: &Value, label: &str) {
    assert!(val["ok"].as_bool().unwrap(), "{label} should succeed");
}

/// Assert that a canonical envelope reports failure.
fn assert_err(val: &Value, label: &str) {
    assert!(!val["ok"].as_bool().unwrap(), "{label} should fail");
}

/// Assert two envelopes have the same ok/error/meta.version and optionally the same result key.
fn assert_envelope_eq(a: &Value, b: &Value, a_label: &str, b_label: &str) {
    assert_eq!(
        a["ok"], b["ok"],
        "ok should match between {a_label} and {b_label}"
    );
    assert_eq!(
        a["meta"]["version"], b["meta"]["version"],
        "meta.version should match between {a_label} and {b_label}"
    );
    if !a["ok"].as_bool().unwrap_or(true) {
        assert_eq!(
            a["error"]["code"], b["error"]["code"],
            "error.code should match between {a_label} and {b_label}"
        );
        assert_eq!(
            a["error"]["message"], b["error"]["message"],
            "error.message should match between {a_label} and {b_label}"
        );
    }
}

// ════════════════════════════════════════════════════════════════════
// CVM transport tests
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn dispatch_vs_cvm_memory_list() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let direct = dispatch_direct(&nomen, &fixtures::memory_list()).await;
    let handler = make_handler(nomen);
    let cvm = dispatch_cvm(&handler, &fixtures::memory_list()).await;

    assert_ok(&direct, "direct");
    assert_ok(&cvm, "CVM");
    assert_envelope_eq(&direct, &cvm, "direct", "CVM");
    assert_eq!(direct["result"]["count"], cvm["result"]["count"]);
}

#[tokio::test]
async fn dispatch_vs_cvm_memory_put_get() {
    let (nomen, _tmp) = test_nomen().await.unwrap();

    let put = store_test_memory(&nomen, "conformance/put-test", "Test memory for conformance").await;
    assert_ok(&put, "put");
    let d_tag = put["result"]["d_tag"].as_str().unwrap().to_string();

    let handler = make_handler(nomen);
    let get = dispatch_cvm(&handler, &fixtures::memory_get(&d_tag)).await;
    assert_ok(&get, "CVM get");
    assert_eq!(get["result"]["topic"], "conformance/put-test");
    assert_eq!(get["result"]["content"], "Test memory for conformance");

    // Store via CVM, get via CVM (both go through dispatch)
    let cvm_put = dispatch_cvm(&handler, &fixtures::memory_put("conformance/cvm-put", "Stored via CVM")).await;
    assert_ok(&cvm_put, "CVM put");
    let cvm_d_tag = cvm_put["result"]["d_tag"].as_str().unwrap();
    let cvm_get = dispatch_cvm(&handler, &fixtures::memory_get(cvm_d_tag)).await;
    assert_ok(&cvm_get, "CVM get roundtrip");
    assert_eq!(cvm_get["result"]["topic"], "conformance/cvm-put");
}

#[tokio::test]
async fn dispatch_vs_cvm_unknown_action() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let direct = dispatch_direct(&nomen, &fixtures::unknown_action()).await;
    let handler = make_handler(nomen);
    let cvm = dispatch_cvm(&handler, &fixtures::unknown_action()).await;

    assert_err(&direct, "direct");
    assert_err(&cvm, "CVM");
    assert_envelope_eq(&direct, &cvm, "direct", "CVM");
}

#[tokio::test]
async fn dispatch_vs_cvm_tools_call() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let direct = dispatch_direct(&nomen, &fixtures::memory_list()).await;
    let handler = make_handler(nomen);
    let tools = dispatch_cvm_tools_call(&handler, &fixtures::memory_list()).await;

    assert_envelope_eq(&direct, &tools, "direct", "CVM tools/call");
    assert_eq!(direct["result"]["count"], tools["result"]["count"]);
}

// ════════════════════════════════════════════════════════════════════
// Multi-transport equivalence tests
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn all_transports_memory_list_equivalence() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    store_test_memory(&nomen, "conformance/all-transports", "Multi-transport test").await;

    let f = fixtures::memory_list();
    let direct = dispatch_direct(&nomen, &f).await;
    let handler = make_handler(nomen);
    let cvm_direct = dispatch_cvm(&handler, &f).await;
    let cvm_tools = dispatch_cvm_tools_call(&handler, &f).await;

    for (label, val) in [("CVM direct", &cvm_direct), ("CVM tools/call", &cvm_tools)] {
        assert_ok(val, label);
        assert_envelope_eq(&direct, val, "direct", label);
        assert_eq!(direct["result"]["count"], val["result"]["count"]);
    }

    // All should have the same topic
    let direct_topic = direct["result"]["memories"][0]["topic"].as_str().unwrap();
    assert_eq!(direct_topic, "conformance/all-transports");
    assert_eq!(direct_topic, cvm_direct["result"]["memories"][0]["topic"].as_str().unwrap());
    assert_eq!(direct_topic, cvm_tools["result"]["memories"][0]["topic"].as_str().unwrap());
}

#[tokio::test]
async fn all_transports_error_equivalence() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let f = fixtures::memory_search_missing_query();

    let direct = dispatch_direct(&nomen, &f).await;
    let handler = make_handler(nomen);
    let cvm = dispatch_cvm(&handler, &f).await;

    assert_err(&direct, "direct");
    assert_err(&cvm, "CVM");
    assert_envelope_eq(&direct, &cvm, "direct", "CVM");
    assert_eq!(direct["error"]["code"], "invalid_params");
}

#[tokio::test]
async fn all_transports_memory_search_equivalence() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    store_test_memory(&nomen, "conformance/search-target", "Unique searchable content for conformance testing").await;

    let f = fixtures::memory_search("conformance searchable content");
    let direct = dispatch_direct(&nomen, &f).await;
    let handler = make_handler(nomen);
    let cvm = dispatch_cvm(&handler, &f).await;

    assert_ok(&direct, "direct");
    assert_ok(&cvm, "CVM");
    assert_eq!(direct["result"]["count"], cvm["result"]["count"]);

    if direct["result"]["count"].as_u64().unwrap() > 0 {
        let direct_topics: Vec<&str> = direct["result"]["results"].as_array().unwrap()
            .iter().map(|r| r["topic"].as_str().unwrap()).collect();
        let cvm_topics: Vec<&str> = cvm["result"]["results"].as_array().unwrap()
            .iter().map(|r| r["topic"].as_str().unwrap()).collect();
        assert_eq!(direct_topics, cvm_topics);
    }
}

// ════════════════════════════════════════════════════════════════════
// HTTP transport tests
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn http_dispatch_success() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let router = build_test_router(nomen);
    let val = dispatch_http(router, &fixtures::memory_list()).await;

    assert_ok(&val, "HTTP");
    assert_eq!(val["meta"]["version"], "v2");
}

#[tokio::test]
async fn http_dispatch_error_missing_query() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let router = build_test_router(nomen);
    let val = dispatch_http(router, &fixtures::memory_search_missing_query()).await;

    assert_err(&val, "HTTP");
    assert_eq!(val["error"]["code"], "invalid_params");
}

#[tokio::test]
async fn http_dispatch_unknown_action() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let router = build_test_router(nomen);
    let val = dispatch_http(router, &fixtures::unknown_action()).await;

    assert_err(&val, "HTTP");
    assert_eq!(val["error"]["code"], "unknown_action");
}

#[tokio::test]
async fn http_dispatch_malformed_request() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let router = build_test_router(nomen);

    use tower::ServiceExt;
    let req = http::Request::builder()
        .method("POST")
        .uri("/memory/api/dispatch")
        .header("content-type", "application/json")
        .body(axum::body::Body::from("not json"))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert!(
        resp.status() == http::StatusCode::BAD_REQUEST || resp.status() == http::StatusCode::UNPROCESSABLE_ENTITY,
        "malformed JSON should return 4xx, got {}", resp.status()
    );
}

#[tokio::test]
async fn http_dispatch_vs_direct_equivalence() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    store_test_memory(&nomen, "conformance/http-equiv", "HTTP equivalence test").await;

    let f = fixtures::memory_list();
    let direct = dispatch_direct(&nomen, &f).await;
    let router = build_test_router(nomen);
    let http_val = dispatch_http(router, &f).await;

    assert_envelope_eq(&direct, &http_val, "direct", "HTTP");
    assert_eq!(direct["result"]["count"], http_val["result"]["count"]);
}

#[tokio::test]
async fn http_health_endpoint() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let router = build_test_router(nomen);

    use tower::ServiceExt;
    let req = http::Request::builder()
        .method("GET")
        .uri("/memory/api/health")
        .body(axum::body::Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), http::StatusCode::OK);

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let val: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(val["status"], "ok");
}

// ════════════════════════════════════════════════════════════════════
// MCP transport tests
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn mcp_tools_call_memory_list() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let direct = dispatch_direct(&nomen, &fixtures::memory_list()).await;
    let mcp = make_mcp_server(nomen);
    let mcp_val = dispatch_mcp(&mcp, &fixtures::memory_list()).await;

    assert_ok(&direct, "direct");
    assert_ok(&mcp_val, "MCP");
    assert_envelope_eq(&direct, &mcp_val, "direct", "MCP");
    assert_eq!(direct["result"]["count"], mcp_val["result"]["count"]);
}

#[tokio::test]
async fn mcp_tools_call_memory_put_get() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let mcp = make_mcp_server(nomen);

    let put = dispatch_mcp(&mcp, &fixtures::memory_put("conformance/mcp-put", "Stored via MCP adapter")).await;
    assert_ok(&put, "MCP put");
    let d_tag = put["result"]["d_tag"].as_str().unwrap();

    let get = dispatch_mcp(&mcp, &fixtures::memory_get(d_tag)).await;
    assert_ok(&get, "MCP get");
    assert_eq!(get["result"]["topic"], "conformance/mcp-put");
    assert_eq!(get["result"]["content"], "Stored via MCP adapter");

    // Cross-transport: get via direct dispatch
    let direct = dispatch_direct(mcp.nomen, &fixtures::memory_get(d_tag)).await;
    assert_ok(&direct, "direct get");
    assert_eq!(direct["result"]["topic"], "conformance/mcp-put");
}

#[tokio::test]
async fn mcp_tools_call_unknown_tool() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let mcp = make_mcp_server(nomen);

    let req = nomen::mcp::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "tools/call".to_string(),
        params: json!({ "name": "bogus_tool", "arguments": {} }),
    };
    let resp = mcp.handle_request(&req).await;
    let result = resp.result.as_ref().unwrap();
    assert!(result["isError"].as_bool().unwrap_or(false), "unknown tool should return isError");
}

#[tokio::test]
async fn mcp_tools_call_error_equivalence() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let f = fixtures::memory_search_missing_query();

    let direct = dispatch_direct(&nomen, &f).await;
    let mcp = make_mcp_server(nomen);
    let mcp_val = dispatch_mcp(&mcp, &f).await;

    assert_err(&direct, "direct");
    assert_err(&mcp_val, "MCP");
    assert_envelope_eq(&direct, &mcp_val, "direct", "MCP");
}

#[tokio::test]
async fn mcp_tools_list_returns_all_actions() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    let mcp = make_mcp_server(nomen);

    let req = nomen::mcp::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "tools/list".to_string(),
        params: json!({}),
    };
    let resp = mcp.handle_request(&req).await;
    let tools = resp.result.as_ref().unwrap()["tools"].as_array().expect("tools array");

    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            nomen::api::dispatch::mcp_tool_to_action(name).is_some(),
            "MCP tool '{name}' should map to a canonical action"
        );
    }

    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for expected in ["memory_search", "memory_put", "memory_list", "message_ingest", "entity_list", "group_list"] {
        assert!(names.contains(&expected), "should have {expected}");
    }
}

// ════════════════════════════════════════════════════════════════════
// All-transport equivalence (direct, MCP, CVM, HTTP)
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn mcp_vs_cvm_vs_http_memory_list() {
    let (db, _tmp) = init_test_db().await.unwrap();

    let nomen1 = nomen::Nomen::from_db(db.clone());
    let nomen2 = nomen::Nomen::from_db(db.clone());
    let nomen3 = nomen::Nomen::from_db(db.clone());
    let nomen4 = nomen::Nomen::from_db(db);

    store_test_memory(&nomen1, "conformance/all-four", "All transports test").await;

    let f = fixtures::memory_list();
    let direct = dispatch_direct(&nomen1, &f).await;
    let mcp_val = dispatch_mcp(&make_mcp_server(nomen2), &f).await;
    let cvm_val = dispatch_cvm(&make_handler(nomen3), &f).await;
    let http_val = dispatch_http(build_test_router(nomen4), &f).await;

    let count = direct["result"]["count"].as_u64().unwrap();
    for (label, val) in [("MCP", &mcp_val), ("CVM", &cvm_val), ("HTTP", &http_val)] {
        assert_ok(val, label);
        assert_envelope_eq(&direct, val, "direct", label);
        assert_eq!(count, val["result"]["count"].as_u64().unwrap(), "{label} count should match");
    }
}

// ════════════════════════════════════════════════════════════════════
// Socket transport conformance tests
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn socket_vs_direct_memory_list() {
    let (db, _db_tmp) = init_test_db().await.unwrap();
    let nomen_direct = nomen::Nomen::from_db(db.clone());
    store_test_memory(&nomen_direct, "conformance/socket-list", "Socket conformance").await;

    let f = fixtures::memory_list();
    let direct = dispatch_direct(&nomen_direct, &f).await;

    let (_server, sock_path, _sock_tmp, _handle) = setup_socket_server(db).await;
    let client = nomen_wire::NomenClient::connect(&sock_path).await.expect("connect");
    let sock = dispatch_socket(&client, &f).await;

    assert_ok(&direct, "direct");
    assert_ok(&sock, "socket");
    assert_envelope_eq(&direct, &sock, "direct", "socket");
    assert_eq!(direct["result"]["count"], sock["result"]["count"]);

    client.close().await;
}

#[tokio::test]
async fn socket_vs_direct_memory_put_get() {
    let (db, _db_tmp) = init_test_db().await.unwrap();
    let (_server, sock_path, _sock_tmp, _handle) = setup_socket_server(db.clone()).await;
    let client = nomen_wire::NomenClient::connect(&sock_path).await.expect("connect");

    let put = dispatch_socket(&client, &fixtures::memory_put("conformance/socket-put", "Stored via socket")).await;
    assert_ok(&put, "socket put");
    let d_tag = put["result"]["d_tag"].as_str().unwrap();

    let nomen_direct = nomen::Nomen::from_db(db);
    let direct = dispatch_direct(&nomen_direct, &fixtures::memory_get(d_tag)).await;
    assert_ok(&direct, "direct get");
    assert_eq!(direct["result"]["topic"], "conformance/socket-put");
    assert_eq!(direct["result"]["content"], "Stored via socket");

    client.close().await;
}

#[tokio::test]
async fn socket_vs_direct_error_equivalence() {
    let (db, _db_tmp) = init_test_db().await.unwrap();
    let nomen_direct = nomen::Nomen::from_db(db.clone());

    let f = fixtures::memory_search_missing_query();
    let direct = dispatch_direct(&nomen_direct, &f).await;

    let (_server, sock_path, _sock_tmp, _handle) = setup_socket_server(db).await;
    let client = nomen_wire::NomenClient::connect(&sock_path).await.expect("connect");
    let sock = dispatch_socket(&client, &f).await;

    assert_err(&direct, "direct");
    assert_err(&sock, "socket");
    assert_envelope_eq(&direct, &sock, "direct", "socket");

    client.close().await;
}

#[tokio::test]
async fn socket_unknown_action_equivalence() {
    let (db, _db_tmp) = init_test_db().await.unwrap();
    let nomen_direct = nomen::Nomen::from_db(db.clone());

    let f = fixtures::unknown_action();
    let direct = dispatch_direct(&nomen_direct, &f).await;

    let (_server, sock_path, _sock_tmp, _handle) = setup_socket_server(db).await;
    let client = nomen_wire::NomenClient::connect(&sock_path).await.expect("connect");
    let sock = dispatch_socket(&client, &f).await;

    assert_err(&direct, "direct");
    assert_err(&sock, "socket");
    assert_envelope_eq(&direct, &sock, "direct", "socket");

    client.close().await;
}

// ════════════════════════════════════════════════════════════════════
// End-to-end HTTP smoke test (real TCP listener)
// ════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn e2e_http_smoke_test() {
    let (nomen, _tmp) = test_nomen().await.unwrap();
    store_test_memory(&nomen, "conformance/e2e-http", "E2E HTTP smoke test memory").await;

    let router = build_test_router(nomen);

    // Bind to a random available port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Spawn the HTTP server
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // Give the server a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();
    let base = format!("http://{addr}");

    // 1. Health check
    let health_resp = client
        .get(format!("{base}/memory/api/health"))
        .send()
        .await
        .expect("health request");
    assert_eq!(health_resp.status(), 200);
    let health: Value = health_resp.json().await.unwrap();
    assert_eq!(health["status"], "ok");

    // 2. Dispatch: memory.list
    let list_resp = client
        .post(format!("{base}/memory/api/dispatch"))
        .json(&json!({ "action": "memory.list", "params": {} }))
        .send()
        .await
        .expect("list request");
    assert_eq!(list_resp.status(), 200);
    let list: Value = list_resp.json().await.unwrap();
    assert_ok(&list, "e2e list");
    assert_eq!(list["meta"]["version"], "v2");
    assert!(list["result"]["count"].as_u64().unwrap() >= 1, "should have at least 1 memory");

    // 3. Dispatch: memory.put
    let put_resp = client
        .post(format!("{base}/memory/api/dispatch"))
        .json(&json!({
            "action": "memory.put",
            "params": {
                "topic": "conformance/e2e-put",
                "content": "Created via e2e HTTP test",
                "visibility": "public"
            }
        }))
        .send()
        .await
        .expect("put request");
    assert_eq!(put_resp.status(), 200);
    let put: Value = put_resp.json().await.unwrap();
    assert_ok(&put, "e2e put");
    let d_tag = put["result"]["d_tag"].as_str().unwrap();

    // 4. Dispatch: memory.get (roundtrip verification)
    let get_resp = client
        .post(format!("{base}/memory/api/dispatch"))
        .json(&json!({ "action": "memory.get", "params": { "d_tag": d_tag } }))
        .send()
        .await
        .expect("get request");
    assert_eq!(get_resp.status(), 200);
    let get: Value = get_resp.json().await.unwrap();
    assert_ok(&get, "e2e get");
    assert_eq!(get["result"]["topic"], "conformance/e2e-put");

    // 5. Dispatch: error case (missing query)
    let err_resp = client
        .post(format!("{base}/memory/api/dispatch"))
        .json(&json!({ "action": "memory.search", "params": {} }))
        .send()
        .await
        .expect("error request");
    assert_eq!(err_resp.status(), 200);
    let err: Value = err_resp.json().await.unwrap();
    assert_err(&err, "e2e error");
    assert_eq!(err["error"]["code"], "invalid_params");

    // 6. Dispatch: unknown action
    let unk_resp = client
        .post(format!("{base}/memory/api/dispatch"))
        .json(&json!({ "action": "bogus.action", "params": {} }))
        .send()
        .await
        .expect("unknown action request");
    assert_eq!(unk_resp.status(), 200);
    let unk: Value = unk_resp.json().await.unwrap();
    assert_err(&unk, "e2e unknown");
    assert_eq!(unk["error"]["code"], "unknown_action");

    server_handle.abort();
}
