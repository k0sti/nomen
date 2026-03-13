#!/usr/bin/env bash
# CVM smoke test — verifies round-trip communication with a running Nomen CVM server.
#
# Prerequisites:
#   - A running Nomen server with --context-vm enabled
#   - NOMEN_SERVER_PUBKEY: hex pubkey or npub of the server
#   - NOMEN_NSEC: client nsec (must be on the server's allowlist, or allowlist empty)
#   - NOMEN_RELAY: relay URL (default: wss://zooid.atlantislabs.space)
#
# Usage:
#   NOMEN_SERVER_PUBKEY=<pubkey> NOMEN_NSEC=<nsec> ./scripts/cvm-smoke-test.sh

set -euo pipefail

if [ -z "${NOMEN_SERVER_PUBKEY:-}" ]; then
    echo "Error: NOMEN_SERVER_PUBKEY is required (hex or npub)"
    echo "Usage: NOMEN_SERVER_PUBKEY=<pubkey> NOMEN_NSEC=<nsec> $0"
    exit 1
fi

if [ -z "${NOMEN_NSEC:-}" ]; then
    echo "Error: NOMEN_NSEC is required"
    exit 1
fi

RELAY="${NOMEN_RELAY:-wss://zooid.atlantislabs.space}"
TIMEOUT="${NOMEN_TIMEOUT:-30}"
ENCRYPTION="${NOMEN_ENCRYPTION:-optional}"

echo "Running CVM smoke test..."
echo "  Server: ${NOMEN_SERVER_PUBKEY:0:16}..."
echo "  Relay:  $RELAY"
echo "  Timeout: ${TIMEOUT}s"
echo "  Encryption: $ENCRYPTION"
echo ""

exec cargo run --example cvm_smoke_test -- \
    --server-pubkey "$NOMEN_SERVER_PUBKEY" \
    --relay "$RELAY" \
    --nsec "$NOMEN_NSEC" \
    --timeout "$TIMEOUT" \
    --encryption "$ENCRYPTION"
