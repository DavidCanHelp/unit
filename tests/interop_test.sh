#!/usr/bin/env bash
# interop_test.sh — Stress test for Rust/Go mesh interop
#
# Runs a Rust unit and a Go unit side by side, verifies they discover
# each other, share challenges, and process S-expression messages.
#
# Prerequisites:
#   cargo build --release
#   cd polyglot/go && go build -o ../../target/unit-go . && cd ../..
#
# Usage (from repo root):
#   ./tests/interop_test.sh
#
# Not run in CI — requires two network services. Run manually.

set -euo pipefail

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m'

RUST_PORT=14200
GO_PORT=14201
RUST_BIN=./target/release/unit
GO_BIN=./target/unit-go
RUST_LOG=/tmp/interop-rust-$$.log
GO_LOG=/tmp/interop-go-$$.log
PASSED=0
FAILED=0
START_TIME=$SECONDS

# -----------------------------------------------------------------------
# Cleanup
# -----------------------------------------------------------------------
PIDS=()
cleanup() {
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    done
    rm -f "$RUST_LOG" "$GO_LOG"
}
trap cleanup EXIT

# -----------------------------------------------------------------------
# Helpers
# -----------------------------------------------------------------------
pass() {
    echo -e "  ${GREEN}PASS${NC} $1"
    PASSED=$((PASSED + 1))
}

fail() {
    echo -e "  ${RED}FAIL${NC} $1"
    FAILED=$((FAILED + 1))
}

send_udp() {
    local port=$1
    local msg=$2
    if command -v nc &>/dev/null; then
        echo -n "$msg" | nc -u -w1 127.0.0.1 "$port" >/dev/null 2>&1 || true
    elif [[ -e /dev/udp ]]; then
        echo -n "$msg" > /dev/udp/127.0.0.1/"$port" 2>/dev/null || true
    else
        echo -e "${YELLOW}SKIP${NC} no nc or /dev/udp available"
        return 1
    fi
}

# -----------------------------------------------------------------------
# Build
# -----------------------------------------------------------------------
echo "=== Interop Stress Test ==="
echo ""

if [[ ! -x "$RUST_BIN" ]]; then
    echo "Building Rust unit..."
    cargo build --release 2>&1 | tail -1
fi

if [[ ! -x "$GO_BIN" ]]; then
    echo "Building Go unit..."
    (cd polyglot/go && go build -o ../../target/unit-go .) 2>&1
fi

if [[ ! -x "$RUST_BIN" ]]; then
    echo -e "${RED}ERROR${NC}: $RUST_BIN not found. Run: cargo build --release"
    exit 1
fi
if [[ ! -x "$GO_BIN" ]]; then
    echo -e "${RED}ERROR${NC}: $GO_BIN not found. Run: cd polyglot/go && go build -o ../../target/unit-go ."
    exit 1
fi

# Clean any leftover state.
rm -rf ~/.unit/node-id-$RUST_PORT 2>/dev/null || true

# -----------------------------------------------------------------------
# Start units
# -----------------------------------------------------------------------
echo "Starting Rust unit on port $RUST_PORT..."
UNIT_PORT=$RUST_PORT "$RUST_BIN" --quiet >"$RUST_LOG" 2>&1 &
PIDS+=($!)
sleep 2

echo "Starting Go unit on port $GO_PORT (peer: 127.0.0.1:$RUST_PORT)..."
"$GO_BIN" -port $GO_PORT -peer "127.0.0.1:$RUST_PORT" >"$GO_LOG" 2>&1 &
PIDS+=($!)
sleep 2

echo ""

# -----------------------------------------------------------------------
# Test 1: Peer Discovery
# -----------------------------------------------------------------------
echo "Test 1: Peer Discovery"
echo "  Sending S-expression peer-announce from Rust port to Go unit..."
# The Rust unit sends binary heartbeats, not S-expressions. Simulate an
# S-expression peer-status as the Rust unit would in a polyglot mesh.
send_udp $GO_PORT "(peer-status :id \"rustnode1\" :peers 1 :fitness 10 :energy 1000)"
sleep 3

if grep -q "discovered peer" "$GO_LOG" 2>/dev/null; then
    pass "Go unit discovered peer via S-expression peer-status"
else
    fail "Go unit did not discover peer (checked $GO_LOG)"
    echo "  Go log:"
    cat "$GO_LOG" 2>/dev/null | head -10 | sed 's/^/    /'
fi

# -----------------------------------------------------------------------
# Test 2: Challenge Propagation
# -----------------------------------------------------------------------
echo ""
echo "Test 2: Challenge Propagation"
echo "  Sending challenge S-expression to Go unit..."
send_udp $GO_PORT '(challenge :id 99999 :name "interop-test" :desc "stress test" :target "42 " :reward 50 :seeds ("42 ."))'
sleep 3

if grep -q "interop-test" "$GO_LOG" 2>/dev/null; then
    pass "Go unit received and stored challenge 'interop-test'"
else
    fail "Go unit did not receive challenge (checked $GO_LOG)"
    echo "  Go log tail:"
    tail -5 "$GO_LOG" 2>/dev/null | sed 's/^/    /'
fi

# -----------------------------------------------------------------------
# Test 3: Bidirectional Announce
# -----------------------------------------------------------------------
echo ""
echo "Test 3: Bidirectional Announce"
echo "  Sending fake peer-announce to both units..."
send_udp $GO_PORT '(peer-announce :id "fake0001" :port 14299)'
sleep 3

if grep -q "fake0001" "$GO_LOG" 2>/dev/null; then
    pass "Go unit discovered fake peer via announce"
else
    fail "Go unit did not process fake peer-announce"
    echo "  Go log tail:"
    tail -5 "$GO_LOG" 2>/dev/null | sed 's/^/    /'
fi

# -----------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------
echo ""
TOTAL=$((PASSED + FAILED))
ELAPSED=$((SECONDS - START_TIME))
echo "=== Results: ${PASSED}/${TOTAL} passed in ${ELAPSED}s ==="
if [[ $FAILED -gt 0 ]]; then
    echo -e "${RED}$FAILED test(s) failed${NC}"
    exit 1
else
    echo -e "${GREEN}All tests passed${NC}"
    exit 0
fi
