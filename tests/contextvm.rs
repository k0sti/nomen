//! Tests for the CVM (ContextVM) adapter layer.
//!
//! These tests exercise `CvmHandler` directly — no relay or Nostr transport needed.
//! They verify that JSON-RPC requests are correctly routed through the canonical
//! `api::dispatch` layer and that ACL/rate-limiting policies work as expected.

use anyhow::Result;
use contextvm_sdk::{JsonRpcMessage, JsonRpcRequest};
use serde_json::{json, Value};
use surrealdb::engine::local::{Db, SurrealKv};
use surrealdb::Surreal;

use nomen::cvm::CvmHandler;

// ── Test helpers ────────────────────────────────────────────────────

async fn init_test_db() -> Result<(Surreal<Db>, tempfile::TempDir)> {
    let tmp = tempfile::tempdir()?;
    let db = Surreal::new::<SurrealKv>(tmp.path()).await?;
    db.use_ns("nomen_test").use_db("nomen_test").await?;
    db.query(nomen::db::SCHEMA).await?.check()?;
    Ok((db, tmp))
}

async fn test_handler() -> Result<(CvmHandler, tempfile::TempDir)> {
    let (db, tmp) = init_test_db().await?;
    let nomen = nomen::Nomen::from_db(db);
    let handler = CvmHandler::new(Box::new(nomen), vec![], 30);
    Ok((handler, tmp))
}

fn make_request(method: &str, params: Option<Value>) -> JsonRpcMessage {
    JsonRpcMessage::Request(JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: json!(1),
        method: method.to_string(),
        params,
    })
}

fn extract_result(response: &JsonRpcMessage) -> &Value {
    match response {
        JsonRpcMessage::Response(r) => &r.result,
        _ => panic!("Expected Response, got: {:?}", response),
    }
}

// ════════════════════════════════════════════════════════════════════
// 1. Method handling tests
// ════════════════════════════════════════════════════════════════════

mod method_tests {
    use super::*;

    #[tokio::test]
    async fn initialize_returns_server_info() {
        let (handler, _tmp) = test_handler().await.unwrap();
        let req = make_request("initialize", None);
        let resp = handler.handle_message(&req).await;
        let result = extract_result(&resp);

        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "nomen");
        assert!(result["serverInfo"]["version"].is_string());
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn tools_list_returns_tool_definitions() {
        let (handler, _tmp) = test_handler().await.unwrap();
        let req = make_request("tools/list", None);
        let resp = handler.handle_message(&req).await;
        let result = extract_result(&resp);

        // tools/list should return a `tools` array
        let tools = result["tools"]
            .as_array()
            .expect("tools should be an array");
        assert!(!tools.is_empty(), "should have at least one tool");

        // Check that memory_search is present
        let has_memory_search = tools
            .iter()
            .any(|t| t["name"].as_str() == Some("memory_search"));
        assert!(has_memory_search, "should contain memory_search tool");

        // Check that memory_put is present
        let has_memory_put = tools
            .iter()
            .any(|t| t["name"].as_str() == Some("memory_put"));
        assert!(has_memory_put, "should contain memory_put tool");
    }

    #[tokio::test]
    async fn ping_returns_empty_object() {
        let (handler, _tmp) = test_handler().await.unwrap();
        let req = make_request("ping", None);
        let resp = handler.handle_message(&req).await;
        let result = extract_result(&resp);
        assert_eq!(*result, json!({}));
    }

    #[tokio::test]
    async fn tools_call_dispatches_through_api() {
        let (handler, _tmp) = test_handler().await.unwrap();

        // Call memory_list via tools/call
        let req = make_request(
            "tools/call",
            Some(json!({
                "name": "memory_list",
                "arguments": {}
            })),
        );
        let resp = handler.handle_message(&req).await;
        let result = extract_result(&resp);

        // tools/call wraps response in content array
        let content = result["content"].as_array().expect("content array");
        assert!(!content.is_empty());
        assert_eq!(content[0]["type"], "text");

        // Parse the inner text as JSON
        let inner: Value = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
        assert!(inner["ok"].as_bool().unwrap(), "dispatch should succeed");
    }

    #[tokio::test]
    async fn tools_call_unknown_tool_returns_error() {
        let (handler, _tmp) = test_handler().await.unwrap();

        let req = make_request(
            "tools/call",
            Some(json!({
                "name": "nonexistent_tool",
                "arguments": {}
            })),
        );
        let resp = handler.handle_message(&req).await;
        let result = extract_result(&resp);

        let content = result["content"].as_array().expect("content array");
        assert!(result["isError"].as_bool().unwrap_or(false));
        assert!(content[0]["text"]
            .as_str()
            .unwrap()
            .contains("Unknown tool"));
    }

    #[tokio::test]
    async fn direct_action_dispatch_memory_list() {
        let (handler, _tmp) = test_handler().await.unwrap();

        // Direct action dispatch (e.g. "memory.list")
        let req = make_request("memory.list", Some(json!({})));
        let resp = handler.handle_message(&req).await;
        let result = extract_result(&resp);

        // Direct dispatch wraps in ApiResponse envelope
        assert!(result["ok"].as_bool().unwrap(), "dispatch should succeed");
        assert_eq!(result["meta"]["version"], "v2");
    }

    #[tokio::test]
    async fn direct_action_dispatch_unknown_action() {
        let (handler, _tmp) = test_handler().await.unwrap();

        let req = make_request("nonexistent.action", Some(json!({})));
        let resp = handler.handle_message(&req).await;
        let result = extract_result(&resp);

        // Unknown actions should return error envelope
        assert!(!result["ok"].as_bool().unwrap());
        assert_eq!(result["error"]["code"], "unknown_action");
    }

    #[tokio::test]
    async fn notification_returns_empty() {
        let (handler, _tmp) = test_handler().await.unwrap();

        let msg = JsonRpcMessage::Notification(contextvm_sdk::JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "notifications/initialized".to_string(),
            params: None,
        });
        let resp = handler.handle_message(&msg).await;
        let result = extract_result(&resp);
        assert_eq!(*result, json!({}));
    }
}

// ════════════════════════════════════════════════════════════════════
// 2. Policy tests
// ════════════════════════════════════════════════════════════════════

mod policy_tests {
    use super::*;

    #[tokio::test]
    async fn acl_empty_allowlist_allows_all() {
        let (db, _tmp) = init_test_db().await.unwrap();
        let nomen = nomen::Nomen::from_db(db);
        let handler = CvmHandler::new(Box::new(nomen), vec![], 30);

        assert!(handler.check_acl("any_pubkey_hex"));
        assert!(handler.check_acl("another_pubkey"));
    }

    #[tokio::test]
    async fn acl_allowlist_rejects_unauthorized() {
        let (db, _tmp) = init_test_db().await.unwrap();
        let nomen = nomen::Nomen::from_db(db);
        let handler = CvmHandler::new(Box::new(nomen), vec!["allowed_pubkey_1".to_string()], 30);

        assert!(handler.check_acl("allowed_pubkey_1"));
        assert!(!handler.check_acl("unauthorized_pubkey"));
    }

    #[tokio::test]
    async fn rate_limiter_allows_within_limit() {
        let (db, _tmp) = init_test_db().await.unwrap();
        let nomen = nomen::Nomen::from_db(db);
        let handler = CvmHandler::new(Box::new(nomen), vec![], 5);

        // 5 requests should succeed
        for _ in 0..5 {
            assert!(handler.check_rate_limit("test_client"));
        }
        // 6th should fail
        assert!(!handler.check_rate_limit("test_client"));
    }

    #[tokio::test]
    async fn rate_limiter_separate_per_client() {
        let (db, _tmp) = init_test_db().await.unwrap();
        let nomen = nomen::Nomen::from_db(db);
        let handler = CvmHandler::new(Box::new(nomen), vec![], 2);

        assert!(handler.check_rate_limit("client_a"));
        assert!(handler.check_rate_limit("client_a"));
        assert!(!handler.check_rate_limit("client_a")); // exhausted

        // client_b should still have quota
        assert!(handler.check_rate_limit("client_b"));
    }
}

// ════════════════════════════════════════════════════════════════════
// 3. Response envelope tests
// ════════════════════════════════════════════════════════════════════

mod response_tests {
    use super::*;

    #[tokio::test]
    async fn success_response_has_correct_shape() {
        let (handler, _tmp) = test_handler().await.unwrap();
        let req = make_request("ping", None);
        let resp = handler.handle_message(&req).await;

        match resp {
            JsonRpcMessage::Response(r) => {
                assert_eq!(r.jsonrpc, "2.0");
                assert_eq!(r.id, json!(1));
            }
            other => panic!("Expected Response, got: {:?}", other),
        }
    }

    #[test]
    fn error_response_has_correct_shape() {
        let resp = nomen::cvm::make_error_response(json!(42), -32600, "Invalid request");

        match resp {
            JsonRpcMessage::ErrorResponse(r) => {
                assert_eq!(r.jsonrpc, "2.0");
                assert_eq!(r.id, json!(42));
                assert_eq!(r.error.code, -32600);
                assert_eq!(r.error.message, "Invalid request");
            }
            other => panic!("Expected ErrorResponse, got: {:?}", other),
        }
    }

    #[test]
    fn extract_request_id_from_request() {
        let msg = make_request("test", None);
        let id = nomen::cvm::extract_request_id(&msg);
        assert_eq!(id, json!(1));
    }

    #[test]
    fn extract_request_id_from_non_request() {
        let msg = JsonRpcMessage::Notification(contextvm_sdk::JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "test".to_string(),
            params: None,
        });
        let id = nomen::cvm::extract_request_id(&msg);
        assert!(id.is_null());
    }

    #[tokio::test]
    async fn tools_call_memory_search_empty_db() {
        let (handler, _tmp) = test_handler().await.unwrap();

        let req = make_request(
            "tools/call",
            Some(json!({
                "name": "memory_search",
                "arguments": {
                    "query": "test query"
                }
            })),
        );
        let resp = handler.handle_message(&req).await;
        let result = extract_result(&resp);

        let content = result["content"].as_array().expect("content array");
        let inner: Value = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
        assert!(
            inner["ok"].as_bool().unwrap(),
            "search on empty db should succeed"
        );
    }
}
