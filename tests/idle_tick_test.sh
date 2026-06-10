#!/usr/bin/env bash
# idle_tick_test.sh — pin that the REPL's periodic duties run WITHOUT stdin.
#
# Hardware finding (2026-06-09, Phase 1 wedge test): recruit evaluation and
# the supervision passes were input-gated — an idle worker held recruited
# work pending indefinitely, an idle issuer's death pass sat 21s overdue,
# and a single Enter keypress released both. The REPL now ticks on a timer
# (REPL_TICK) while idle; these tests run real processes whose stdin is
# held open but silent, so any regression to input-gating hangs the pins.
#
# Pin 1 (all platforms): an idle worker that receives a recruit evaluates
#   and replies, and an idle issuer collects the reply — no input on the
#   worker ever, none on the issuer during the wait window.
# Pin 2 (Linux only): an idle issuer detects a dead holder and re-recruits
#   within PEER_TIMEOUT + tick slack, with the replacement worker (also
#   idle) completing the work. Linux-only because re-recruit placement
#   needs real advertised headroom and HostResources fails closed (0) on
#   non-Linux hosts.
#
# Usage: ./tests/idle_tick_test.sh [path-to-binary]

set -euo pipefail

BINARY="${1:-./target/release/unit}"
if [ ! -x "$BINARY" ]; then
    echo "Binary not found: $BINARY (run: cargo build --release)"
    exit 1
fi
BINARY="$(cd "$(dirname "$BINARY")" && pwd)/$(basename "$BINARY")"

DIR=$(mktemp -d)
PIDS=""
cleanup() {
    for p in $PIDS; do
        kill -9 "$p" 2>/dev/null || true
        wait "$p" 2>/dev/null || true # reap quietly (no job-control noise)
    done
    rm -rf "$DIR"
}
trap cleanup EXIT

cd "$DIR"
PORT_BASE=$((20000 + ($$ % 20000)))
PASS=0
FAIL=0

node_id_from_log() {
    sed -n 's/.*Mesh node \([0-9a-f]\{16\}\).*/\1/p' "$1" | head -1
}

# ---------------------------------------------------------------------------
# Pin 1: idle worker evaluates a recruit; idle issuer collects the reply.
# ---------------------------------------------------------------------------
echo "--- pin 1: idle recruit evaluation + idle result collection ---"

W_PORT=$PORT_BASE
I_PORT=$((PORT_BASE + 1))
mkfifo w_in i_in
mkdir -p whome ihome

# Worker: stdin held open, NEVER written to. Everything it does after boot
# must come from its idle tick.
HOME="$DIR/whome" UNIT_PORT=$W_PORT "$BINARY" < w_in > worker.log 2>&1 &
PIDS="$PIDS $!"
exec 8> w_in

HOME="$DIR/ihome" UNIT_PORT=$I_PORT UNIT_PEERS=127.0.0.1:$W_PORT \
    "$BINARY" < i_in > issuer.log 2>&1 &
ISSUER_PID=$!
PIDS="$PIDS $ISSUER_PID"
exec 9> i_in

sleep 4 # boot + heartbeat exchange so the issuer knows the worker's real id
WID=$(node_id_from_log worker.log)
if [ -z "$WID" ]; then
    echo "  FAIL  pin1 (no worker id in log)"
    cat worker.log
    exit 1
fi

printf 'RECRUIT" %s (+ 2 2)"\n' "$WID" >&9
# Silent window: the worker must evaluate on its idle tick and the issuer
# must collect the reply on its own idle tick — no input anywhere.
sleep 4
printf 'RECRUITS\n' >&9
sleep 1
printf 'BYE\n' >&9
exec 9>&-
wait "$ISSUER_PID" 2>/dev/null || true

if grep -qF 'ok value=[4]' issuer.log; then
    echo "  PASS  idle-recruit-eval-and-collect"
    PASS=$((PASS + 1))
else
    echo "  FAIL  idle-recruit-eval-and-collect"
    echo "  --- issuer.log ---"
    cat issuer.log
    echo "  --- worker.log ---"
    cat worker.log
    FAIL=$((FAIL + 1))
fi
exec 8>&-

# ---------------------------------------------------------------------------
# Pin 2 (Linux only): idle issuer re-recruits off a dead holder within
# PEER_TIMEOUT (15s) + tick slack, no REPL input during the window.
# ---------------------------------------------------------------------------
if [ "$(uname)" = "Linux" ]; then
    echo "--- pin 2: idle death-pass re-recruit ---"

    A_PORT=$((PORT_BASE + 2))
    B_PORT=$((PORT_BASE + 3))
    C_PORT=$((PORT_BASE + 4))
    mkfifo a_in b_in c_in
    mkdir -p ahome bhome chome

    HOME="$DIR/bhome" UNIT_PORT=$B_PORT "$BINARY" < b_in > b.log 2>&1 &
    PIDS="$PIDS $!"
    exec 11> b_in
    HOME="$DIR/chome" UNIT_PORT=$C_PORT "$BINARY" < c_in > c.log 2>&1 &
    C_PID=$!
    PIDS="$PIDS $C_PID"
    exec 12> c_in
    HOME="$DIR/ahome" UNIT_PORT=$A_PORT \
        UNIT_PEERS=127.0.0.1:$B_PORT,127.0.0.1:$C_PORT \
        "$BINARY" < a_in > a.log 2>&1 &
    A_PID=$!
    PIDS="$PIDS $A_PID"
    exec 13> a_in

    sleep 4
    CID=$(node_id_from_log c.log)
    if [ -z "$CID" ]; then
        echo "  FAIL  pin2 (no victim id in log)"
        exit 1
    fi

    # Kill the victim BEFORE recruiting to it. (Killing after doesn't work
    # anymore: an idle holder now evaluates within one REPL_TICK, so trivial
    # work settles the slot before any kill can land — which is the very fix
    # under test.) C stays in A's peer view until the PEER_TIMEOUT prune, so
    # the recruit below opens a slot held by a corpse.
    kill -9 "$C_PID" 2>/dev/null || true
    wait "$C_PID" 2>/dev/null || true
    sleep 1

    printf 'RECRUIT" %s (+ 3 4)"\n' "$CID" >&13
    # From here the issuer gets NO input: gossip expiry (PEER_TIMEOUT 15s)
    # then the death pass must fire on an idle tick, re-recruit to B, and B
    # (also idle) must evaluate and reply.
    sleep 20
    printf 'RECRUITS\n' >&13
    sleep 1
    printf 'BYE\n' >&13
    exec 13>&-
    wait "$A_PID" 2>/dev/null || true

    if grep -qF '(re-recruited 1x)' a.log && grep -qF 'ok value=[7]' a.log; then
        echo "  PASS  idle-death-pass-re-recruit"
        PASS=$((PASS + 1))
    else
        echo "  FAIL  idle-death-pass-re-recruit"
        echo "  --- a.log ---"
        cat a.log
        FAIL=$((FAIL + 1))
    fi
    exec 11>&-
    exec 12>&- 2>/dev/null || true
else
    echo "--- pin 2 skipped: needs Linux (HostResources fails closed elsewhere) ---"
fi

# ---------------------------------------------------------------------------
echo ""
echo "idle-tick: PASSED $PASS, FAILED $FAIL"
[ "$FAIL" -eq 0 ]
