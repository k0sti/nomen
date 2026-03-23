//! Tests for the Nomen API v2 layer: types, errors, dispatch, and operations.

use anyhow::Result;
use surrealdb::engine::local::{Db, SurrealKv};
use surrealdb::Surreal;

// ── Test helpers ────────────────────────────────────────────────────

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

// ════════════════════════════════════════════════════════════════════
// 1. Unit tests for api::types
// ════════════════════════════════════════════════════════════════════

mod types_tests {
    use nomen::api::types::*;
    use serde_json::json;

    #[test]
    fn visibility_parse_all_variants() {
        assert_eq!(Visibility::parse("public"), Some(Visibility::Public));
        assert_eq!(Visibility::parse("group"), Some(Visibility::Group));
        assert_eq!(Visibility::parse("circle"), Some(Visibility::Circle));
        assert_eq!(Visibility::parse("personal"), Some(Visibility::Personal));
        assert_eq!(Visibility::parse("internal"), Some(Visibility::Internal));
    }

    #[test]
    fn visibility_parse_legacy_private() {
        assert_eq!(Visibility::parse("private"), Some(Visibility::Personal));
    }

    #[test]
    fn visibility_parse_unknown() {
        assert_eq!(Visibility::parse("unknown"), None);
        assert_eq!(Visibility::parse(""), None);
        assert_eq!(Visibility::parse("PUBLIC"), None);
    }

    #[test]
    fn visibility_as_str_roundtrip() {
        for (s, v) in &[
            ("public", Visibility::Public),
            ("group", Visibility::Group),
            ("circle", Visibility::Circle),
            ("personal", Visibility::Personal),
            ("internal", Visibility::Internal),
        ] {
            assert_eq!(v.as_str(), *s);
            assert_eq!(Visibility::parse(s), Some(v.clone()));
        }
    }

    #[test]
    fn visibility_to_tier() {
        assert_eq!(Visibility::Public.to_tier(""), "public");
        assert_eq!(
            Visibility::Group.to_tier("engineering"),
            "group:engineering"
        );
        assert_eq!(Visibility::Personal.to_tier(""), "personal");
        assert_eq!(Visibility::Internal.to_tier(""), "internal");
        assert_eq!(Visibility::Circle.to_tier("abc123"), "circle:abc123");
    }

    #[test]
    fn retrieval_params_defaults() {
        let defaults = RetrievalParams::default();
        assert!((defaults.vector_weight - 0.7).abs() < f32::EPSILON);
        assert!((defaults.text_weight - 0.3).abs() < f32::EPSILON);
        assert!(!defaults.aggregate);
        assert!(!defaults.graph_expand);
        assert_eq!(defaults.max_hops, 1);
    }

    #[test]
    fn retrieval_params_from_json() {
        let val = json!({
            "vector_weight": 0.5,
            "text_weight": 0.5,
            "aggregate": true,
            "graph_expand": true,
            "max_hops": 3
        });
        let p: RetrievalParams = serde_json::from_value(val).unwrap();
        assert!((p.vector_weight - 0.5).abs() < f32::EPSILON);
        assert!(p.aggregate);
        assert!(p.graph_expand);
        assert_eq!(p.max_hops, 3);
    }

    #[test]
    fn retrieval_params_from_partial_json() {
        let val = json!({"aggregate": true});
        let p: RetrievalParams = serde_json::from_value(val).unwrap();
        // Defaults for missing fields
        assert!((p.vector_weight - 0.7).abs() < f32::EPSILON);
        assert!(p.aggregate);
        assert_eq!(p.max_hops, 1);
    }

    #[test]
    fn api_response_success_serialization() {
        let resp = nomen::api::ApiResponse::success(json!({"count": 5}));
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["result"]["count"], 5);
        assert!(v.get("error").is_none() || v["error"].is_null());
        assert_eq!(v["meta"]["version"], "v2");
    }

    #[test]
    fn api_response_error_serialization() {
        let err = nomen::api::errors::ApiError::invalid_params("bad input");
        let resp = nomen::api::ApiResponse::error(err);
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v.get("result").is_none() || v["result"].is_null());
        assert_eq!(v["error"]["code"], "invalid_params");
        assert_eq!(v["error"]["message"], "bad input");
        assert_eq!(v["meta"]["version"], "v2");
    }

    #[tokio::test]
    async fn resolve_visibility_scope_canonical() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();
        let params = json!({"visibility": "public"});
        let (vis, scope) = resolve_visibility_scope(&params, &nomen, "default").unwrap();
        assert_eq!(vis, Some(Visibility::Public));
        assert!(scope.is_none());
    }

    #[tokio::test]
    async fn resolve_visibility_scope_group_requires_scope() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();
        let params = json!({"visibility": "group"});
        let result = resolve_visibility_scope(&params, &nomen, "default");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn resolve_visibility_scope_group_with_scope() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();
        let params = json!({"visibility": "group", "scope": "engineering"});
        let (vis, scope) = resolve_visibility_scope(&params, &nomen, "default").unwrap();
        assert_eq!(vis, Some(Visibility::Group));
        assert_eq!(scope, Some("engineering".to_string()));
    }

    #[tokio::test]
    async fn resolve_visibility_scope_legacy_tier() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();
        let params = json!({"tier": "group:engineering"});
        let (vis, scope) = resolve_visibility_scope(&params, &nomen, "default").unwrap();
        assert_eq!(vis, Some(Visibility::Group));
        assert_eq!(scope, Some("engineering".to_string()));
    }

    #[tokio::test]
    async fn resolve_visibility_scope_none() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();
        let params = json!({});
        let (vis, scope) = resolve_visibility_scope(&params, &nomen, "default").unwrap();
        assert!(vis.is_none());
        assert!(scope.is_none());
    }
}

// ════════════════════════════════════════════════════════════════════
// 2. Unit tests for api::errors
// ════════════════════════════════════════════════════════════════════

mod errors_tests {
    use nomen::api::errors::ApiError;

    #[test]
    fn error_codes_match_spec() {
        let cases: Vec<(ApiError, &str)> = vec![
            (ApiError::invalid_params("x"), "invalid_params"),
            (ApiError::invalid_scope("x"), "invalid_scope"),
            (ApiError::not_found("x"), "not_found"),
            (ApiError::unauthorized("x"), "unauthorized"),
            (ApiError::rate_limited("x"), "rate_limited"),
            (ApiError::internal("x"), "internal_error"),
            (ApiError::unknown_action("foo"), "unknown_action"),
        ];
        for (err, expected_code) in cases {
            assert_eq!(err.code(), expected_code, "code mismatch for {}", err);
        }
    }

    #[test]
    fn error_messages_preserved() {
        let err = ApiError::invalid_params("topic is required");
        assert_eq!(err.message(), "topic is required");

        let err = ApiError::unknown_action("foo.bar");
        assert_eq!(err.message(), "Unknown action: foo.bar");
    }

    #[test]
    fn error_display() {
        let err = ApiError::not_found("memory xyz");
        let display = format!("{err}");
        assert!(display.contains("not_found"));
        assert!(display.contains("memory xyz"));
    }

    #[test]
    fn from_anyhow() {
        let anyhow_err = anyhow::anyhow!("db connection failed");
        let api_err = ApiError::from_anyhow(anyhow_err);
        assert_eq!(api_err.code(), "internal_error");
        assert!(api_err.message().contains("db connection failed"));
    }
}

// ════════════════════════════════════════════════════════════════════
// 3. Unit tests for api::dispatch
// ════════════════════════════════════════════════════════════════════

mod dispatch_tests {
    use nomen::api::dispatch::mcp_tool_to_action;

    #[test]
    fn mcp_tool_to_action_all_known() {
        let known = vec![
            ("memory_search", "memory.search"),
            ("memory_put", "memory.put"),
            ("memory_get", "memory.get"),
            ("memory_list", "memory.list"),
            ("memory_delete", "memory.delete"),
            ("message_ingest", "message.ingest"),
            ("message_list", "message.list"),
            ("message_context", "message.context"),
            ("message_send", "message.send"),
            ("memory_consolidate", "memory.consolidate"),
            ("memory_cluster", "memory.cluster"),
            ("memory_sync", "memory.sync"),
            ("memory_embed", "memory.embed"),
            ("memory_prune", "memory.prune"),
            ("entity_list", "entity.list"),
            ("entity_relationships", "entity.relationships"),
            ("group_list", "group.list"),
            ("group_members", "group.members"),
            ("group_create", "group.create"),
            ("group_add_member", "group.add_member"),
            ("group_remove_member", "group.remove_member"),
        ];
        for (tool, expected) in known {
            assert_eq!(
                mcp_tool_to_action(tool),
                Some(expected.to_string()),
                "failed for tool: {tool}"
            );
        }
    }

    #[test]
    fn mcp_tool_to_action_unknown() {
        assert_eq!(mcp_tool_to_action("unknown_tool"), None);
        assert_eq!(mcp_tool_to_action("notacommand"), None);
        assert_eq!(mcp_tool_to_action(""), None);
        assert_eq!(mcp_tool_to_action("memory"), None);
        assert_eq!(mcp_tool_to_action("foo_bar_baz"), None);
    }

    #[tokio::test]
    async fn dispatch_unknown_action_returns_error() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();
        let resp = nomen::api::dispatch(
            &nomen,
            "default",
            "nonexistent.action",
            &serde_json::json!({}),
        )
        .await;
        assert!(!resp.ok);
        let err = resp.error.unwrap();
        assert_eq!(err.code, "unknown_action");
    }
}

// ════════════════════════════════════════════════════════════════════
// 4. Integration tests for v2 operations via dispatch
// ════════════════════════════════════════════════════════════════════

mod operations_tests {
    use serde_json::json;

    #[tokio::test]
    async fn memory_put_get_roundtrip() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();

        // Put
        let put_resp = nomen::api::dispatch(
            &nomen, "test",
            "memory.put",
            &json!({"topic": "apiv2-test/roundtrip", "summary": "Test roundtrip memory", "detail": "Detailed info"}),
        ).await;
        assert!(put_resp.ok, "put failed: {:?}", put_resp.error);
        let result = put_resp.result.unwrap();
        assert_eq!(result["topic"], "apiv2-test/roundtrip");
        assert!(result["d_tag"].as_str().is_some());

        // Get by topic
        let get_resp = nomen::api::dispatch(
            &nomen,
            "test",
            "memory.get",
            &json!({"topic": "apiv2-test/roundtrip"}),
        )
        .await;
        assert!(get_resp.ok, "get failed: {:?}", get_resp.error);
        let mem = get_resp.result.unwrap();
        assert_eq!(mem["topic"], "apiv2-test/roundtrip");
        assert_eq!(mem["summary"], "Test roundtrip memory");

        // Cleanup
        nomen::api::dispatch(
            &nomen,
            "test",
            "memory.delete",
            &json!({"topic": "apiv2-test/roundtrip"}),
        )
        .await;
    }

    #[tokio::test]
    async fn memory_put_search() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();

        nomen::api::dispatch(
            &nomen, "test",
            "memory.put",
            &json!({"topic": "apiv2-test/search-target", "summary": "Unique xylophone melody searching"}),
        ).await;

        let search_resp = nomen::api::dispatch(
            &nomen,
            "test",
            "memory.search",
            &json!({"query": "xylophone melody"}),
        )
        .await;
        assert!(search_resp.ok, "search failed: {:?}", search_resp.error);
        let result = search_resp.result.unwrap();
        let results = result["results"].as_array().unwrap();
        assert!(!results.is_empty(), "search should find the stored memory");
        assert!(results
            .iter()
            .any(|r| r["topic"] == "apiv2-test/search-target"));

        // Cleanup
        nomen::api::dispatch(
            &nomen,
            "test",
            "memory.delete",
            &json!({"topic": "apiv2-test/search-target"}),
        )
        .await;
    }

    #[tokio::test]
    async fn memory_put_list() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();

        nomen::api::dispatch(
            &nomen,
            "test",
            "memory.put",
            &json!({"topic": "apiv2-test/list-item", "summary": "A listable memory"}),
        )
        .await;

        let list_resp = nomen::api::dispatch(&nomen, "test", "memory.list", &json!({})).await;
        assert!(list_resp.ok, "list failed: {:?}", list_resp.error);
        let result = list_resp.result.unwrap();
        let memories = result["memories"].as_array().unwrap();
        assert!(memories
            .iter()
            .any(|m| m["topic"] == "apiv2-test/list-item"));

        // Cleanup
        nomen::api::dispatch(
            &nomen,
            "test",
            "memory.delete",
            &json!({"topic": "apiv2-test/list-item"}),
        )
        .await;
    }

    #[tokio::test]
    async fn memory_put_delete() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();

        nomen::api::dispatch(
            &nomen,
            "test",
            "memory.put",
            &json!({"topic": "apiv2-test/delete-me", "summary": "Will be deleted"}),
        )
        .await;

        // Verify it exists
        let get_resp = nomen::api::dispatch(
            &nomen,
            "test",
            "memory.get",
            &json!({"topic": "apiv2-test/delete-me"}),
        )
        .await;
        assert!(get_resp.ok);
        assert!(!get_resp.result.as_ref().unwrap().is_null());

        // Delete
        let del_resp = nomen::api::dispatch(
            &nomen,
            "test",
            "memory.delete",
            &json!({"topic": "apiv2-test/delete-me"}),
        )
        .await;
        assert!(del_resp.ok, "delete failed: {:?}", del_resp.error);
        assert_eq!(del_resp.result.unwrap()["deleted"], true);

        // Verify gone
        let get_resp2 = nomen::api::dispatch(
            &nomen,
            "test",
            "memory.get",
            &json!({"topic": "apiv2-test/delete-me"}),
        )
        .await;
        assert!(get_resp2.ok);
        assert!(get_resp2.result.unwrap().is_null());
    }

    #[tokio::test]
    async fn message_ingest_list() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();

        let ingest_resp = nomen::api::dispatch(
            &nomen,
            "test",
            "message.ingest",
            &json!({
                "content": "Hello from API v2 test",
                "source": "test-apiv2",
                "sender": "test-user",
                "channel": "test-channel"
            }),
        )
        .await;
        assert!(ingest_resp.ok, "ingest failed: {:?}", ingest_resp.error);
        assert!(ingest_resp.result.unwrap()["id"].as_str().is_some());

        let list_resp = nomen::api::dispatch(
            &nomen,
            "test",
            "message.list",
            &json!({"source": "test-apiv2"}),
        )
        .await;
        assert!(list_resp.ok, "message.list failed: {:?}", list_resp.error);
        let result = list_resp.result.unwrap();
        let messages = result["messages"].as_array().unwrap();
        assert!(messages
            .iter()
            .any(|m| m["content"] == "Hello from API v2 test"));
    }

    #[tokio::test]
    async fn group_lifecycle() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();
        let group_id = "apiv2-test-group";

        // Create
        let create_resp = nomen::api::dispatch(
            &nomen,
            "test",
            "group.create",
            &json!({"id": group_id, "name": "Test Group", "members": ["npub1test"]}),
        )
        .await;
        assert!(
            create_resp.ok,
            "group.create failed: {:?}",
            create_resp.error
        );

        // List
        let list_resp = nomen::api::dispatch(&nomen, "test", "group.list", &json!({})).await;
        assert!(list_resp.ok);
        let groups = list_resp.result.unwrap()["groups"]
            .as_array()
            .unwrap()
            .clone();
        assert!(groups.iter().any(|g| g["id"] == group_id));

        // Members
        let members_resp =
            nomen::api::dispatch(&nomen, "test", "group.members", &json!({"id": group_id})).await;
        assert!(members_resp.ok);
        let members = members_resp.result.unwrap();
        assert_eq!(members["count"], 1);

        // Add member
        let add_resp = nomen::api::dispatch(
            &nomen,
            "test",
            "group.add_member",
            &json!({"id": group_id, "npub": "npub1new"}),
        )
        .await;
        assert!(add_resp.ok, "group.add_member failed: {:?}", add_resp.error);

        // Verify added
        let members_resp2 =
            nomen::api::dispatch(&nomen, "test", "group.members", &json!({"id": group_id})).await;
        assert_eq!(members_resp2.result.unwrap()["count"], 2);

        // Remove member
        let remove_resp = nomen::api::dispatch(
            &nomen,
            "test",
            "group.remove_member",
            &json!({"id": group_id, "npub": "npub1new"}),
        )
        .await;
        assert!(
            remove_resp.ok,
            "group.remove_member failed: {:?}",
            remove_resp.error
        );

        // Verify removed
        let members_resp3 =
            nomen::api::dispatch(&nomen, "test", "group.members", &json!({"id": group_id})).await;
        assert_eq!(members_resp3.result.unwrap()["count"], 1);
    }

    #[tokio::test]
    async fn entity_list_empty() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();

        let resp = nomen::api::dispatch(&nomen, "test", "entity.list", &json!({})).await;
        assert!(resp.ok, "entity.list failed: {:?}", resp.error);
        let result = resp.result.unwrap();
        assert_eq!(result["count"], 0);
        assert!(result["entities"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unknown_action_error_response() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();

        let resp = nomen::api::dispatch(&nomen, "test", "bogus.action", &json!({})).await;
        assert!(!resp.ok);
        let err = resp.error.unwrap();
        assert_eq!(err.code, "unknown_action");
        assert!(err.message.contains("bogus.action"));
    }

    #[tokio::test]
    async fn memory_put_without_topic_returns_invalid_params() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();

        let resp = nomen::api::dispatch(
            &nomen,
            "test",
            "memory.put",
            &json!({"summary": "no topic provided"}),
        )
        .await;
        assert!(!resp.ok);
        assert_eq!(resp.error.unwrap().code, "invalid_params");
    }

    #[tokio::test]
    async fn memory_search_without_query_returns_invalid_params() {
        let (nomen, _tmp) = super::test_nomen().await.unwrap();

        let resp = nomen::api::dispatch(&nomen, "test", "memory.search", &json!({})).await;
        assert!(!resp.ok);
        assert_eq!(resp.error.unwrap().code, "invalid_params");
    }
}
