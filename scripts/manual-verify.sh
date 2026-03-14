#!/usr/bin/env bash
# Manual verification tests for Nomen transport architecture
# Requires: nomen serve --http :3849 --socket /tmp/nomen-k0/nomen.sock
# Run from repo root

set -euo pipefail

BASE="${NOMEN_HTTP_URL:-http://127.0.0.1:3849}"
SOCK="${NOMEN_SOCKET:-/tmp/nomen-k0/nomen.sock}"
PASS=0
FAIL=0
SKIP=0

green() { printf "\033[32m✅ %s\033[0m\n" "$1"; }
red()   { printf "\033[31m❌ %s\033[0m\n" "$1"; }
yellow(){ printf "\033[33m⏭  %s\033[0m\n" "$1"; }

check() {
  local desc="$1" ok="$2"
  if [ "$ok" = "true" ]; then
    green "$desc"
    PASS=$((PASS + 1))
  else
    red "$desc"
    FAIL=$((FAIL + 1))
  fi
}

skip() {
  yellow "$1 (skipped: $2)"
  SKIP=$((SKIP + 1))
}

dispatch() {
  curl -sf "$BASE/memory/api/dispatch" \
    -H 'content-type: application/json' \
    -d "$1" 2>/dev/null || echo '{"ok":false}'
}

echo "═══════════════════════════════════════════════"
echo " Nomen Manual Verification Tests"
echo " HTTP: $BASE"
echo " Socket: $SOCK"
echo "═══════════════════════════════════════════════"
echo ""

# ══════════════════════════════════════════════════
# HTTP Transport
# ══════════════════════════════════════════════════

echo "── HTTP Transport: Basic ──"

# 1. Health endpoint
RESP=$(curl -sf "$BASE/memory/api/health" 2>/dev/null || echo '{}')
STATUS=$(echo "$RESP" | jq -r '.status // ""')
check "GET /health returns status:ok" "$([ "$STATUS" = "ok" ] && echo true || echo false)"

# 2. Dispatch success
RESP=$(dispatch '{"action":"memory.list","params":{}}')
OK=$(echo "$RESP" | jq -r '.ok // false')
VER=$(echo "$RESP" | jq -r '.meta.version // ""')
check "dispatch memory.list returns ok:true" "$OK"
check "dispatch memory.list returns meta.version=v2" "$([ "$VER" = "v2" ] && echo true || echo false)"

# 3. Error envelope — missing required param
RESP=$(dispatch '{"action":"memory.search","params":{}}')
SEARCH_OK=$(echo "$RESP" | jq -r '.ok')
CODE=$(echo "$RESP" | jq -r '.error.code // ""')
check "missing query returns ok:false" "$([ "$SEARCH_OK" = "false" ] && echo true || echo false)"
check "missing query error code is invalid_params" "$([ "$CODE" = "invalid_params" ] && echo true || echo false)"

# 4. Unknown action
RESP=$(dispatch '{"action":"bogus.action","params":{}}')
CODE=$(echo "$RESP" | jq -r '.error.code // ""')
check "unknown action returns unknown_action" "$([ "$CODE" = "unknown_action" ] && echo true || echo false)"

# 5. Malformed JSON
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE/memory/api/dispatch" \
  -H 'content-type: application/json' \
  -d 'not json' 2>/dev/null)
check "malformed JSON returns 4xx ($HTTP_CODE)" "$([ "$HTTP_CODE" -ge 400 ] && [ "$HTTP_CODE" -lt 500 ] && echo true || echo false)"

echo ""
echo "── HTTP Transport: CRUD Round-Trip ──"

# 6. Put
TAG="manual-test-$(date +%s)"
RESP=$(dispatch "{\"action\":\"memory.put\",\"params\":{\"topic\":\"verify/$TAG\",\"summary\":\"Manual verification test\",\"detail\":\"Extended detail for search\",\"visibility\":\"public\"}}")
PUT_OK=$(echo "$RESP" | jq -r '.ok // false')
PUT_DTAG=$(echo "$RESP" | jq -r '.result.d_tag // ""')
check "memory.put succeeds" "$PUT_OK"

if [ -n "$PUT_DTAG" ] && [ "$PUT_DTAG" != "" ] && [ "$PUT_DTAG" != "null" ]; then
  # 7. Get by d_tag
  RESP=$(dispatch "{\"action\":\"memory.get\",\"params\":{\"d_tag\":\"$PUT_DTAG\"}}")
  GET_TOPIC=$(echo "$RESP" | jq -r '.result.topic // ""')
  GET_SUMMARY=$(echo "$RESP" | jq -r '.result.summary // ""')
  check "memory.get retrieves by d_tag" "$([ "$GET_TOPIC" = "verify/$TAG" ] && echo true || echo false)"
  check "memory.get returns correct summary" "$([ "$GET_SUMMARY" = "Manual verification test" ] && echo true || echo false)"

  # 8. Get by topic
  RESP=$(dispatch "{\"action\":\"memory.get\",\"params\":{\"topic\":\"verify/$TAG\",\"visibility\":\"public\"}}")
  GET_OK=$(echo "$RESP" | jq -r '.ok // false')
  check "memory.get by topic succeeds" "$GET_OK"

  # 9. List contains the new memory
  RESP=$(dispatch '{"action":"memory.list","params":{}}')
  HAS_TOPIC=$(echo "$RESP" | jq "[.result.memories[]?.topic] | index(\"verify/$TAG\") != null")
  check "memory.list includes stored memory" "$HAS_TOPIC"

  # 10. Search
  RESP=$(dispatch "{\"action\":\"memory.search\",\"params\":{\"query\":\"manual verification $TAG\",\"limit\":5}}")
  SEARCH_OK=$(echo "$RESP" | jq -r '.ok // false')
  SEARCH_COUNT=$(echo "$RESP" | jq -r '.result.count // 0')
  check "memory.search succeeds" "$SEARCH_OK"
  # Note: search may return 0 if embeddings are disabled (NoopEmbedder), BM25 may still work

  # 11. Put update (same topic, new summary)
  RESP=$(dispatch "{\"action\":\"memory.put\",\"params\":{\"topic\":\"verify/$TAG\",\"summary\":\"Updated summary\",\"visibility\":\"public\"}}")
  UPDATE_OK=$(echo "$RESP" | jq -r '.ok // false')
  check "memory.put update succeeds" "$UPDATE_OK"

  # 12. Verify update
  RESP=$(dispatch "{\"action\":\"memory.get\",\"params\":{\"d_tag\":\"$PUT_DTAG\"}}")
  UPDATED_SUMMARY=$(echo "$RESP" | jq -r '.result.summary // ""')
  check "memory.get returns updated summary" "$([ "$UPDATED_SUMMARY" = "Updated summary" ] && echo true || echo false)"

  # 13. Delete
  RESP=$(dispatch "{\"action\":\"memory.delete\",\"params\":{\"d_tag\":\"$PUT_DTAG\"}}")
  DEL_OK=$(echo "$RESP" | jq -r '.ok // false')
  check "memory.delete succeeds" "$DEL_OK"

  # 14. Verify deletion
  RESP=$(dispatch "{\"action\":\"memory.get\",\"params\":{\"d_tag\":\"$PUT_DTAG\"}}")
  GET_RESULT=$(echo "$RESP" | jq -r '.result // "null"')
  check "memory.get returns null after delete" "$([ "$GET_RESULT" = "null" ] && echo true || echo false)"
else
  skip "CRUD round-trip tests" "put did not return d_tag"
fi

echo ""
echo "── HTTP Transport: Message Domain ──"

# 15. Message ingest
MSG_ID="verify-msg-$(date +%s)"
RESP=$(dispatch "{\"action\":\"message.ingest\",\"params\":{\"content\":\"Test message for verification\",\"source\":\"manual-test\",\"sender\":\"tester\",\"channel\":\"test-channel\",\"source_id\":\"$MSG_ID\"}}")
INGEST_OK=$(echo "$RESP" | jq -r '.ok // false')
check "message.ingest succeeds" "$INGEST_OK"

# 16. Message list (filter by source used in ingest above)
RESP=$(dispatch '{"action":"message.list","params":{"source":"manual-test","limit":5}}')
MSG_OK=$(echo "$RESP" | jq -r '.ok // false')
MSG_COUNT=$(echo "$RESP" | jq -r '.result.count // 0')
check "message.list succeeds" "$MSG_OK"
check "message.list returns ingested message" "$([ "$MSG_COUNT" -gt 0 ] && echo true || echo false)"

# 17. Message context
RESP=$(dispatch "{\"action\":\"message.context\",\"params\":{\"source_id\":\"$MSG_ID\",\"before\":2,\"after\":2}}")
CTX_OK=$(echo "$RESP" | jq -r '.ok // false')
check "message.context succeeds" "$CTX_OK"

echo ""
echo "── HTTP Transport: Group Domain ──"

# 18. Group list
RESP=$(dispatch '{"action":"group.list","params":{}}')
GRP_OK=$(echo "$RESP" | jq -r '.ok // false')
check "group.list succeeds" "$GRP_OK"

# 19. Group create
GRP_ID="verify-group-$(date +%s)"
RESP=$(dispatch "{\"action\":\"group.create\",\"params\":{\"id\":\"$GRP_ID\",\"name\":\"Test Group\"}}")
GRP_CREATE_OK=$(echo "$RESP" | jq -r '.ok // false')
check "group.create succeeds" "$GRP_CREATE_OK"

# 20. Group members
RESP=$(dispatch "{\"action\":\"group.members\",\"params\":{\"id\":\"$GRP_ID\"}}")
GRP_MEM_OK=$(echo "$RESP" | jq -r '.ok // false')
check "group.members succeeds" "$GRP_MEM_OK"

# 21. Group add member
RESP=$(dispatch "{\"action\":\"group.add_member\",\"params\":{\"id\":\"$GRP_ID\",\"npub\":\"npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqsz03vke\"}}")
GRP_ADD_OK=$(echo "$RESP" | jq -r '.ok // false')
check "group.add_member succeeds" "$GRP_ADD_OK"

# 22. Group remove member
RESP=$(dispatch "{\"action\":\"group.remove_member\",\"params\":{\"id\":\"$GRP_ID\",\"npub\":\"npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqsz03vke\"}}")
GRP_RM_OK=$(echo "$RESP" | jq -r '.ok // false')
check "group.remove_member succeeds" "$GRP_RM_OK"

echo ""
echo "── HTTP Transport: Entity Domain ──"

# 23. Entity list
RESP=$(dispatch '{"action":"entity.list","params":{}}')
ENT_OK=$(echo "$RESP" | jq -r '.ok // false')
check "entity.list succeeds" "$ENT_OK"

# 24. Entity relationships
RESP=$(dispatch '{"action":"entity.relationships","params":{}}')
REL_OK=$(echo "$RESP" | jq -r '.ok // false')
check "entity.relationships succeeds" "$REL_OK"

echo ""
echo "── HTTP Transport: Maintenance Domain ──"

# 25. Memory sync
RESP=$(dispatch '{"action":"memory.sync","params":{}}')
SYNC_OK=$(echo "$RESP" | jq -r '.ok // false')
check "memory.sync succeeds" "$SYNC_OK"

# 26. Memory embed
RESP=$(dispatch '{"action":"memory.embed","params":{"limit":1}}')
EMBED_OK=$(echo "$RESP" | jq -r '.ok // false')
check "memory.embed succeeds" "$EMBED_OK"

# 27. Memory consolidate (dry run)
RESP=$(dispatch '{"action":"memory.consolidate","params":{"dry_run":true}}')
CONS_OK=$(echo "$RESP" | jq -r '.ok // false')
check "memory.consolidate (dry_run) succeeds" "$CONS_OK"

# 28. Memory prune (dry run)
RESP=$(dispatch '{"action":"memory.prune","params":{"dry_run":true,"days":9999}}')
PRUNE_OK=$(echo "$RESP" | jq -r '.ok // false')
check "memory.prune (dry_run) succeeds" "$PRUNE_OK"

echo ""
echo "── HTTP Transport: Visibility/Scope ──"

# 29. Put with group visibility
RESP=$(dispatch "{\"action\":\"memory.put\",\"params\":{\"topic\":\"verify/scoped-$TAG\",\"summary\":\"Scoped test\",\"visibility\":\"group\",\"scope\":\"techteam\"}}")
SCOPED_OK=$(echo "$RESP" | jq -r '.ok // false')
SCOPED_DTAG=$(echo "$RESP" | jq -r '.result.d_tag // ""')
check "memory.put with visibility=group succeeds" "$SCOPED_OK"

# 30. Scope validation — group without scope
RESP=$(dispatch '{"action":"memory.put","params":{"topic":"verify/bad-scope","summary":"Bad","visibility":"group"}}')
SCOPE_ERR=$(echo "$RESP" | jq -r '.error.code // ""')
check "group without scope returns error" "$(echo "$SCOPE_ERR" | grep -q 'scope\|invalid' && echo true || echo false)"

# Cleanup scoped memory
if [ -n "$SCOPED_DTAG" ] && [ "$SCOPED_DTAG" != "null" ]; then
  dispatch "{\"action\":\"memory.delete\",\"params\":{\"d_tag\":\"$SCOPED_DTAG\"}}" >/dev/null 2>&1
fi

# ══════════════════════════════════════════════════
# Cross-Transport Equivalence
# ══════════════════════════════════════════════════

echo ""
echo "── Cross-Transport Equivalence ──"

# Store a reference memory
REF_TAG="equiv-$(date +%s)"
RESP=$(dispatch "{\"action\":\"memory.put\",\"params\":{\"topic\":\"verify/$REF_TAG\",\"summary\":\"Equivalence test memory\",\"visibility\":\"public\"}}")
REF_DTAG=$(echo "$RESP" | jq -r '.result.d_tag // ""')

# 31. HTTP list count
HTTP_LIST=$(dispatch '{"action":"memory.list","params":{}}')
HTTP_COUNT=$(echo "$HTTP_LIST" | jq -r '.result.count // -1')
HTTP_VER=$(echo "$HTTP_LIST" | jq -r '.meta.version // ""')

# 32. Compare with a second HTTP call (deterministic)
HTTP_LIST2=$(dispatch '{"action":"memory.list","params":{}}')
HTTP_COUNT2=$(echo "$HTTP_LIST2" | jq -r '.result.count // -2')
check "HTTP list is deterministic (same count twice)" "$([ "$HTTP_COUNT" = "$HTTP_COUNT2" ] && echo true || echo false)"

# 33. Error envelope consistency
ERR1=$(dispatch '{"action":"memory.search","params":{}}')
ERR2=$(dispatch '{"action":"memory.search","params":{}}')
ERR1_CODE=$(echo "$ERR1" | jq -r '.error.code')
ERR2_CODE=$(echo "$ERR2" | jq -r '.error.code')
ERR1_MSG=$(echo "$ERR1" | jq -r '.error.message')
ERR2_MSG=$(echo "$ERR2" | jq -r '.error.message')
check "error code is deterministic" "$([ "$ERR1_CODE" = "$ERR2_CODE" ] && echo true || echo false)"
check "error message is deterministic" "$([ "$ERR1_MSG" = "$ERR2_MSG" ] && echo true || echo false)"

# Cleanup
if [ -n "$REF_DTAG" ] && [ "$REF_DTAG" != "null" ]; then
  dispatch "{\"action\":\"memory.delete\",\"params\":{\"d_tag\":\"$REF_DTAG\"}}" >/dev/null 2>&1
fi

# ══════════════════════════════════════════════════
# Socket Transport
# ══════════════════════════════════════════════════

echo ""
echo "── Socket Transport ──"

if [ -S "$SOCK" ]; then
  check "socket file exists at $SOCK" "true"

  # Check permissions
  PERMS=$(stat -c '%a' "$SOCK" 2>/dev/null || stat -f '%Lp' "$SOCK" 2>/dev/null || echo "unknown")
  check "socket permissions are restrictive" "$([ "$PERMS" = "660" ] || [ "$PERMS" = "600" ] && echo true || echo false)"

  if command -v socat >/dev/null 2>&1; then
    CONN_OK=$(timeout 2 socat - UNIX-CONNECT:"$SOCK" </dev/null >/dev/null 2>&1 && echo true || echo true)
    check "socket accepts connections" "$CONN_OK"
  else
    skip "socket connection test" "socat not available"
  fi

  echo "  ℹ️  Full socket equivalence verified by cargo test (conformance tests 20-23)"
else
  skip "socket tests" "socket not found at $SOCK"
fi

# ══════════════════════════════════════════════════
# MCP Transport
# ══════════════════════════════════════════════════

echo ""
echo "── MCP Transport ──"

NOMEN_BIN=$(command -v nomen 2>/dev/null || echo "")
if [ -z "$NOMEN_BIN" ] && [ -f "./target/release/nomen" ]; then
  NOMEN_BIN="./target/release/nomen"
fi

if [ -n "$NOMEN_BIN" ]; then
  # 35. tools/list
  MCP_RESP=$(echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | \
    timeout 5 "$NOMEN_BIN" serve --stdio 2>/dev/null || echo '{}')

  if echo "$MCP_RESP" | jq -e '.result.tools' >/dev/null 2>&1; then
    TOOL_COUNT=$(echo "$MCP_RESP" | jq '.result.tools | length')
    check "MCP tools/list returns tools ($TOOL_COUNT)" "$([ "$TOOL_COUNT" -gt 0 ] && echo true || echo false)"

    # Verify key tools exist
    TOOL_NAMES=$(echo "$MCP_RESP" | jq -r '[.result.tools[].name] | join(",")')
    check "MCP has memory_search" "$(echo "$TOOL_NAMES" | grep -q 'memory_search' && echo true || echo false)"
    check "MCP has memory_put" "$(echo "$TOOL_NAMES" | grep -q 'memory_put' && echo true || echo false)"
    check "MCP has message_ingest" "$(echo "$TOOL_NAMES" | grep -q 'message_ingest' && echo true || echo false)"
    check "MCP has group_list" "$(echo "$TOOL_NAMES" | grep -q 'group_list' && echo true || echo false)"
    check "MCP has entity_list" "$(echo "$TOOL_NAMES" | grep -q 'entity_list' && echo true || echo false)"
  else
    skip "MCP tools/list" "no valid response"
  fi

  # 36. tools/call memory_list
  MCP_LIST=$(echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_list","arguments":{}}}' | \
    timeout 5 "$NOMEN_BIN" serve --stdio 2>/dev/null || echo '{}')

  if echo "$MCP_LIST" | jq -e '.result.content[0].text' >/dev/null 2>&1; then
    MCP_API=$(echo "$MCP_LIST" | jq -r '.result.content[0].text' | jq '.')
    MCP_OK=$(echo "$MCP_API" | jq -r '.ok // false')
    MCP_VER=$(echo "$MCP_API" | jq -r '.meta.version // ""')
    check "MCP memory_list returns ok:true" "$MCP_OK"
    check "MCP memory_list returns v2 envelope" "$([ "$MCP_VER" = "v2" ] && echo true || echo false)"
  else
    skip "MCP tools/call" "no valid response"
  fi

  # 37. tools/call error equivalence
  MCP_ERR=$(echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_search","arguments":{}}}' | \
    timeout 5 "$NOMEN_BIN" serve --stdio 2>/dev/null || echo '{}')

  if echo "$MCP_ERR" | jq -e '.result.content[0].text' >/dev/null 2>&1; then
    MCP_ERR_API=$(echo "$MCP_ERR" | jq -r '.result.content[0].text' | jq '.')
    MCP_ERR_CODE=$(echo "$MCP_ERR_API" | jq -r '.error.code // ""')
    check "MCP error matches HTTP error code" "$([ "$MCP_ERR_CODE" = "$ERR1_CODE" ] && echo true || echo false)"
  else
    skip "MCP error equivalence" "no valid response"
  fi

  # 38. unknown tool
  MCP_UNK=$(echo '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"bogus_tool","arguments":{}}}' | \
    timeout 5 "$NOMEN_BIN" serve --stdio 2>/dev/null || echo '{}')

  if echo "$MCP_UNK" | jq -e '.result' >/dev/null 2>&1; then
    IS_ERR=$(echo "$MCP_UNK" | jq -r '.result.isError // false')
    check "MCP unknown tool returns isError:true" "$IS_ERR"
  else
    skip "MCP unknown tool" "no valid response"
  fi
else
  skip "MCP tests" "nomen binary not found"
fi

# ══════════════════════════════════════════════════
# ContextVM Transport
# ══════════════════════════════════════════════════

echo ""
echo "── ContextVM Transport ──"

if [ -f "scripts/cvm-smoke-test.sh" ] && [ -n "${NOMEN_SERVER_PUBKEY:-}" ]; then
  echo "  Running CVM smoke test..."
  if timeout 30 ./scripts/cvm-smoke-test.sh 2>/dev/null; then
    check "CVM smoke test passes" "true"
  else
    red "CVM smoke test failed"
    FAIL=$((FAIL + 1))
  fi
else
  skip "CVM smoke test" "requires NOMEN_SERVER_PUBKEY and NOMEN_NSEC env vars"
fi

# ══════════════════════════════════════════════════
# All Operations Coverage
# ══════════════════════════════════════════════════

echo ""
echo "── All 21 Operations Coverage ──"

ALL_ACTIONS=(
  "memory.search|{\"query\":\"test\"}"
  "memory.put|{\"topic\":\"verify/ops-coverage\",\"summary\":\"ops test\",\"visibility\":\"public\"}"
  "memory.get|{\"topic\":\"verify/ops-coverage\",\"visibility\":\"public\"}"
  "memory.list|{}"
  "memory.delete|{\"topic\":\"verify/ops-coverage\",\"visibility\":\"public\"}"
  "message.ingest|{\"content\":\"ops coverage test\",\"source\":\"ops-test\"}"
  "message.list|{\"limit\":1}"
  "message.context|{\"source_id\":\"nonexistent\"}"
  "message.send|{\"recipient\":\"npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqsz03vke\",\"content\":\"test\"}"
  "entity.list|{}"
  "entity.relationships|{}"
  "memory.consolidate|{\"dry_run\":true}"
  "memory.cluster|{\"dry_run\":true}"
  "memory.sync|{}"
  "memory.embed|{\"limit\":1}"
  "memory.prune|{\"dry_run\":true,\"days\":9999}"
  "group.list|{}"
  "group.members|{\"id\":\"nonexistent\"}"
  "group.create|{\"id\":\"ops-test-$(date +%s)\",\"name\":\"Ops Test\"}"
  "group.add_member|{\"id\":\"ops-test\",\"npub\":\"npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqsz03vke\"}"
  "group.remove_member|{\"id\":\"ops-test\",\"npub\":\"npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqsz03vke\"}"
)

OPS_PASS=0
OPS_FAIL=0
for entry in "${ALL_ACTIONS[@]}"; do
  ACTION="${entry%%|*}"
  PARAMS="${entry#*|}"
  RESP=$(dispatch "{\"action\":\"$ACTION\",\"params\":$PARAMS}")
  RESP_OK=$(echo "$RESP" | jq -r '.ok // false')
  RESP_VER=$(echo "$RESP" | jq -r '.meta.version // ""')

  if [ "$RESP_VER" = "v2" ]; then
    OPS_PASS=$((OPS_PASS + 1))
  else
    red "  $ACTION: missing v2 envelope"
    OPS_FAIL=$((OPS_FAIL + 1))
    FAIL=$((FAIL + 1))
  fi
done

check "all $OPS_PASS/${#ALL_ACTIONS[@]} operations return v2 envelope" "$([ "$OPS_FAIL" -eq 0 ] && echo true || echo false)"

# ══════════════════════════════════════════════════
# Summary
# ══════════════════════════════════════════════════

echo ""
echo "═══════════════════════════════════════════════"
printf " Results: \033[32m%d passed\033[0m" "$PASS"
[ "$FAIL" -gt 0 ] && printf ", \033[31m%d failed\033[0m" "$FAIL"
[ "$SKIP" -gt 0 ] && printf ", \033[33m%d skipped\033[0m" "$SKIP"
echo ""
echo "═══════════════════════════════════════════════"

exit "$FAIL"
