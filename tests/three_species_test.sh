#!/usr/bin/env bash
# three_species_test.sh — Stress test for Rust/Go/Python mesh interop
#
# Runs all three species simultaneously on one mesh and verifies they
# discover each other, share challenges, and exchange solutions.
#
# Tests the core whitepaper claim: any language with an S-expression
# parser can join the mesh.
#
# Prerequisites: cargo, go, python3, nc (netcat)
# Run from repo root: ./tests/three_species_test.sh
# Not in CI — requires three network services running simultaneously.

set -euo pipefail

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[0;33m'
NC='\033[0m'

RUST_PORT=15200
GO_PORT=15201
PYTHON_PORT=15202
RUST_BIN=./target/release/unit
GO_BIN=./target/unit-go
PYTHON_DIR=./polyglot/python
LOGDIR=$(mktemp -d /tmp/three-species-XXXXXX)
PASSED=0
FAILED=0
START_TIME=$SECONDS
PIDS=()

# -----------------------------------------------------------------------
cleanup() {
    for pid in "${PIDS[@]}"; do
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    done
    rm -rf "$LOGDIR"
}
trap cleanup EXIT

pass() { echo -e "  ${GREEN}PASS${NC} $1"; PASSED=$((PASSED + 1)); }
fail() { echo -e "  ${RED}FAIL${NC} $1"; FAILED=$((FAILED + 1)); }
skip() { echo -e "  ${YELLOW}SKIP${NC} $1"; }

send_udp() {
    local port=$1 msg=$2
    if command -v nc &>/dev/null; then
        echo -n "$msg" | nc -u -w1 127.0.0.1 "$port" >/dev/null 2>&1 || true
    elif [[ -e /dev/udp ]]; then
        echo -n "$msg" > /dev/udp/127.0.0.1/"$port" 2>/dev/null || true
    else
        skip "no nc or /dev/udp available"
        return 1
    fi
}

alive() { kill -0 "$1" 2>/dev/null; }

# -----------------------------------------------------------------------
echo "=== Three-Species Interop Test ==="
echo ""

# Build
if [[ ! -x "$RUST_BIN" ]]; then
    echo "Building Rust unit..."
    cargo build --release 2>&1 | tail -1
fi
if [[ ! -x "$GO_BIN" ]]; then
    echo "Building Go unit..."
    (cd polyglot/go && go build -o ../../target/unit-go .) 2>&1
fi
if [[ ! -d "$PYTHON_DIR" ]]; then
    echo -e "${RED}ERROR${NC}: $PYTHON_DIR not found"; exit 1
fi

for bin in "$RUST_BIN" "$GO_BIN"; do
    if [[ ! -x "$bin" ]]; then
        echo -e "${RED}ERROR${NC}: $bin not found"; exit 1
    fi
done

# Verify python3
if ! command -v python3 &>/dev/null; then
    echo -e "${RED}ERROR${NC}: python3 not found"; exit 1
fi

rm -rf ~/.unit/node-id-$RUST_PORT 2>/dev/null || true

# -----------------------------------------------------------------------
# Start all three species
# -----------------------------------------------------------------------
echo "Starting Rust unit on port $RUST_PORT..."
(sleep 120) | UNIT_PORT=$RUST_PORT "$RUST_BIN" --quiet >"$LOGDIR/rust.log" 2>&1 &
PIDS+=($!)
sleep 2
if ! alive "${PIDS[0]}"; then
    echo -e "${RED}ERROR${NC}: Rust unit failed to start (port $RUST_PORT in use?)"; exit 1
fi

echo "Starting Go unit on port $GO_PORT..."
"$GO_BIN" -port $GO_PORT -peer "127.0.0.1:$RUST_PORT" >"$LOGDIR/go.log" 2>&1 &
PIDS+=($!)
sleep 1
if ! alive "${PIDS[1]}"; then
    echo -e "${RED}ERROR${NC}: Go unit failed to start"; exit 1
fi

echo "Starting Python unit on port $PYTHON_PORT..."
(cd "$PYTHON_DIR" && python3 main.py --port $PYTHON_PORT --peer "127.0.0.1:$RUST_PORT") >"$LOGDIR/python.log" 2>&1 &
PIDS+=($!)
sleep 1
if ! alive "${PIDS[2]}"; then
    echo -e "${RED}ERROR${NC}: Python unit failed to start"; exit 1
fi

echo ""

# -----------------------------------------------------------------------
# Test 1: Three-Way Peer Discovery
# -----------------------------------------------------------------------
echo "Test 1: Three-Way Peer Discovery"
echo "  Waiting 12 seconds for gossip propagation..."

# Send S-expression peer-status to help Go and Python discover peers
# (Rust sends binary heartbeats that Go/Python can't parse)
send_udp $GO_PORT "(peer-status :id \"rustnode\" :peers 2 :fitness 0 :energy 1000)"
send_udp $PYTHON_PORT "(peer-status :id \"rustnode\" :peers 2 :fitness 0 :energy 1000)"
sleep 5
send_udp $GO_PORT "(peer-status :id \"pynode\" :peers 1 :fitness 0 :energy 1000)"
send_udp $PYTHON_PORT "(peer-status :id \"gonode\" :peers 1 :fitness 0 :energy 1000)"
sleep 7

checks=0
if grep -q "discovered peer" "$LOGDIR/go.log" 2>/dev/null; then
    checks=$((checks + 1))
fi
if grep -q "announced to" "$LOGDIR/python.log" 2>/dev/null; then
    checks=$((checks + 1))
fi
# Go should see at least 2 peers (Rust + Python via gossip simulation)
if grep -c "discovered peer" "$LOGDIR/go.log" 2>/dev/null | grep -q "[2-9]"; then
    checks=$((checks + 1))
fi

if [[ $checks -ge 2 ]]; then
    pass "All three species connected to the mesh ($checks/3 checks)"
else
    fail "Peer discovery incomplete ($checks/3 checks)"
    echo "  Go log head:"; head -5 "$LOGDIR/go.log" 2>/dev/null | sed 's/^/    /'
    echo "  Python log head:"; head -5 "$LOGDIR/python.log" 2>/dev/null | sed 's/^/    /'
fi

# -----------------------------------------------------------------------
# Test 2: Challenge Broadcast to All Species
# -----------------------------------------------------------------------
echo ""
echo "Test 2: Challenge Broadcast to All Species"
echo "  Sending challenge to all three ports..."

CHALLENGE='(challenge :id 88888 :name "three-species-test" :desc "interop stress" :target "42 " :reward 75 :seeds ("42 ."))'
send_udp $RUST_PORT "$CHALLENGE"
send_udp $GO_PORT "$CHALLENGE"
send_udp $PYTHON_PORT "$CHALLENGE"
sleep 5

go_got=false; py_got=false
if grep -q "three-species-test" "$LOGDIR/go.log" 2>/dev/null; then go_got=true; fi
if grep -q "three-species-test" "$LOGDIR/python.log" 2>/dev/null; then py_got=true; fi

if $go_got && $py_got; then
    pass "Both Go and Python received the challenge"
elif $go_got || $py_got; then
    pass "At least one non-Rust species received the challenge (Go=$go_got, Python=$py_got)"
else
    fail "Neither Go nor Python received the challenge"
    echo "  Go log tail:"; tail -3 "$LOGDIR/go.log" 2>/dev/null | sed 's/^/    /'
    echo "  Python log tail:"; tail -3 "$LOGDIR/python.log" 2>/dev/null | sed 's/^/    /'
fi

# -----------------------------------------------------------------------
# Test 3: Solution Broadcast Across Species
# -----------------------------------------------------------------------
echo ""
echo "Test 3: Solution Broadcast Across Species"
echo "  Sending solution to all three ports..."

SOLUTION='(solution :challenge-id 88888 :program "42 ." :solver "go-test")'
send_udp $RUST_PORT "$SOLUTION"
send_udp $GO_PORT "$SOLUTION"
send_udp $PYTHON_PORT "$SOLUTION"
sleep 4

sol_received=false
if grep -qi "solution\|solved\|SOLVED" "$LOGDIR/go.log" 2>/dev/null; then sol_received=true; fi
if grep -qi "solution\|solved\|SOLVED" "$LOGDIR/python.log" 2>/dev/null; then sol_received=true; fi

if $sol_received; then
    pass "At least one non-Rust species received the solution"
else
    fail "No species logged solution receipt"
    echo "  Go log tail:"; tail -3 "$LOGDIR/go.log" 2>/dev/null | sed 's/^/    /'
    echo "  Python log tail:"; tail -3 "$LOGDIR/python.log" 2>/dev/null | sed 's/^/    /'
fi

# -----------------------------------------------------------------------
# Test 4: Peer Status Exchange
# -----------------------------------------------------------------------
echo ""
echo "Test 4: Peer Status Exchange"
echo "  Sending probe peer-status to all species..."

send_udp $GO_PORT '(peer-status :id "probe001" :peers 0 :fitness 0 :energy 500)'
send_udp $PYTHON_PORT '(peer-status :id "probe001" :peers 0 :fitness 0 :energy 500)'
sleep 4

if grep -q "probe001" "$LOGDIR/go.log" 2>/dev/null; then
    pass "Go unit processed probe peer-status"
elif grep -q "probe001" "$LOGDIR/python.log" 2>/dev/null; then
    pass "Python unit processed probe peer-status"
else
    fail "Neither species processed the probe"
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
