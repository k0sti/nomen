#!/usr/bin/env bash
# Migrate d-tags to v0.3: 
# 1. internal/{pubkey}/{topic} → private/{topic}
# 2. personal/{pubkey}/{topic} where topic was originally internal → private/{topic}
#
# Uses nomen HTTP API at localhost:3849

set -euo pipefail

API="http://127.0.0.1:3849/memory/api/dispatch"
PUBKEY="d29fe7c1af179eac10767f57ac021f520b44a8ded1fd37b1d1f79c9e545f96d7"
DRY_RUN=false

if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=true
    echo "=== DRY RUN ==="
fi

# Known topics that should be private (were internal before migration)
# These are agent-only memories: rules, tools, projects, identity, etc.
PRIVATE_PREFIXES="rules/ tools/ projects/ project/ clarity/ k0/ lessons/ architecture/ workflow/ observations/ events/ room/"

MEMORIES=$(curl -s -X POST "$API" \
    -H 'Content-Type: application/json' \
    -d '{"action":"memory.list","params":{"limit":10000}}')

MIGRATED=0
SKIPPED=0

echo "$MEMORIES" | jq -r '.result.memories[].d_tag' | sort | while read -r old_dtag; do
    new_dtag=""
    
    # Case 1: internal/{pubkey}/{topic} → private/{topic}
    if [[ "$old_dtag" == internal/${PUBKEY}/* ]]; then
        topic="${old_dtag#internal/${PUBKEY}/}"
        new_dtag="private/${topic}"
    
    # Case 2: personal/{pubkey}/{topic} that should be private
    # Match known private-tier topic prefixes
    elif [[ "$old_dtag" == personal/${PUBKEY}/* ]]; then
        topic="${old_dtag#personal/${PUBKEY}/}"
        is_private=false
        for prefix in $PRIVATE_PREFIXES; do
            if [[ "$topic" == ${prefix}* ]]; then
                is_private=true
                break
            fi
        done
        # Also match context and other known single-word private topics
        case "$topic" in
            context|internal/*) is_private=true ;;
        esac
        
        if [[ "$is_private" == "true" ]]; then
            new_dtag="private/${topic}"
        fi
    fi
    
    if [[ -z "$new_dtag" ]]; then
        continue
    fi
    
    echo "MIGRATE: $old_dtag → $new_dtag"
    
    if [[ "$DRY_RUN" == "true" ]]; then
        continue
    fi
    
    # Get content
    CONTENT=$(echo "$MEMORIES" | jq -r --arg dtag "$old_dtag" \
        '.result.memories[] | select(.d_tag == $dtag) | .content // .detail // ""')
    
    if [[ -z "$CONTENT" ]]; then
        echo "  WARN: empty content, skipping"
        continue
    fi
    
    # Store with new d-tag
    STORE_RESULT=$(curl -s -X POST "$API" \
        -H 'Content-Type: application/json' \
        -d "$(jq -n \
            --arg topic "${new_dtag#private/}" \
            --arg content "$CONTENT" \
            '{action: "memory.put", params: {topic: $topic, content: $content, visibility: "private"}}')")
    
    STORE_OK=$(echo "$STORE_RESULT" | jq -r '.ok // false')
    if [[ "$STORE_OK" != "true" ]]; then
        echo "  ERROR storing: $(echo "$STORE_RESULT" | jq -r '.error.message // "unknown"')"
        continue
    fi
    
    echo "  Stored: $(echo "$STORE_RESULT" | jq -r '.result.d_tag // "?"')"
    
    # Delete old
    DELETE_RESULT=$(curl -s -X POST "$API" \
        -H 'Content-Type: application/json' \
        -d "$(jq -n --arg dtag "$old_dtag" '{action: "memory.delete", params: {d_tag: $dtag}}')")
    
    if [[ "$(echo "$DELETE_RESULT" | jq -r '.ok')" == "true" ]]; then
        echo "  Deleted old: $old_dtag"
    else
        echo "  WARN: delete may have failed"
    fi
    
    sleep 0.1
done

echo ""
echo "=== Done ==="
[[ "$DRY_RUN" == "true" ]] && echo "Re-run without --dry-run to apply."
