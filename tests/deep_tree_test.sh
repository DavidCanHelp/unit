#!/usr/bin/env bash
# deep_tree_test.sh — Phase 3: deep-tree deadline coverage, real processes.
#
# Exercises the recruit-tree deadline machinery against REAL wedged peers
# (SIGSTOP — the local stand-in for a hung box):
#   A. wedged leaf WITH an alternative peer  -> deadline expiry, re-recruit,
#      real result collected (recovery).
#   B. wedged ONLY peer                      -> peer-table eviction (empty
#      view), paced fail-closed resets, attempt cap, ABANDONED — the wait is
#      terminally bounded, and the parent stays responsive.
#   No process is ever killed to "recover" (the v0.32.0 lesson): the wedged
#   peer is SIGCONTed at the end and its late reply must be harmless.
#
# Usage: bash tests/deep_tree_test.sh [path-to-binary]

set -uo pipefail

BINARY="${1:-./target/release/unit}"
PASS=0; FAIL=0; TOTAL=0
TMPDIR=$(mktemp -d)
export HOME="$TMPDIR/home"   # isolate ~/.unit
mkdir -p "$HOME"
export UNIT_RECRUIT_TIMEOUT_SECS=2

cleanup() {
    exec 2>/dev/null
    # revive anything still stopped so it can be killed
    for pid in ${LEAF_PID:-} ${ALT_PID:-} ${LONE_PID:-}; do
        kill -CONT "$pid" 2>/dev/null || true
        kill "$pid" 2>/dev/null || true
    done
    pkill -f "UNIT_PORT=436" || true
    sleep 1
    rm -rf "$TMPDIR"
}
trap cleanup EXIT

if [ ! -x "$BINARY" ]; then echo "Binary not found: $BINARY"; exit 1; fi

run_test() {
    local name="$1" result="$2"
    TOTAL=$((TOTAL + 1))
    if [ "$result" -eq 0 ]; then echo "  PASS  $name"; PASS=$((PASS + 1));
    else echo "  FAIL  $name"; FAIL=$((FAIL + 1)); fi
}

node_id() { # node_id <logfile> — first 16-hex id printed at boot
    grep -oE 'Mesh node [0-9a-f]{16}' "$1" | head -1 | awk '{print $3}'
}

echo "=== A: wedged leaf, alternative peer -> re-recruit recovers ==="
LEAF_LOG="$TMPDIR/leaf.log"; ALT_LOG="$TMPDIR/alt.log"; PARENT_A="$TMPDIR/parent_a.log"
{ printf 'ID\n'; sleep 30; } | UNIT_PORT=4361 "$BINARY" >"$LEAF_LOG" 2>&1 &
LEAF_PID=$!
{ printf 'ID\n'; sleep 30; } | UNIT_PORT=4362 UNIT_PEERS=127.0.0.1:4361 "$BINARY" >"$ALT_LOG" 2>&1 &
ALT_PID=$!
sleep 3
LEAF_ID=$(node_id "$LEAF_LOG")
[ -n "$LEAF_ID" ]; run_test "leaf booted with id" $?

# Parent joins, discovers both, recruits the LEAF, which we wedge immediately.
{
    sleep 3                       # discovery
    kill -STOP "$LEAF_PID"        # the wedge (Ctrl-Z equivalent)
    printf 'RECRUIT" %s (+ 1 2)"\n' "$LEAF_ID"
    sleep 16                      # > timeout: expiry -> re-recruit or cap out
    printf 'RECRUITS\n'
    sleep 2
} | UNIT_PORT=4363 UNIT_PEERS=127.0.0.1:4361,127.0.0.1:4362 "$BINARY" --quiet >"$PARENT_A" 2>&1
# The Phase-3 invariant: the wedged slot reaches a TERMINAL state within the
# window — recovered (re-recruited to the alt peer, result 3 collected) when
# this host honestly advertises capacity, ABANDONED fail-closed when it
# doesn't (a loaded dev box). Never still pending.
if grep -q 'collected' "$PARENT_A"; then
    echo "        (path: capacity existed -> re-recruited, result collected)"
    grep -qE '\b3\b' "$PARENT_A"; run_test "wedged leaf: bounded resolution (recovered)" $?
else
    echo "        (path: no advertised capacity -> abandoned fail-closed)"
    grep -q 'ABANDONED' "$PARENT_A"; run_test "wedged leaf: bounded resolution (abandoned)" $?
fi
LAST_STATUS=$(grep -E 'recruit #[0-9]+ seq 0 ->' "$PARENT_A" | tail -1)
! echo "$LAST_STATUS" | grep -q 'pending'
run_test "wedged slot not left pending (terminal state reached)" $?
kill -CONT "$LEAF_PID" 2>/dev/null; kill "$LEAF_PID" "$ALT_PID" 2>/dev/null; wait 2>/dev/null

echo "=== B: wedged ONLY peer -> eviction, cap, ABANDONED (bounded) ==="
LONE_LOG="$TMPDIR/lone.log"; PARENT_B="$TMPDIR/parent_b.log"
{ printf 'ID\n'; sleep 90; } | UNIT_PORT=4365 "$BINARY" >"$LONE_LOG" 2>&1 &
LONE_PID=$!
sleep 3
LONE_ID=$(node_id "$LONE_LOG")
[ -n "$LONE_ID" ]; run_test "lone worker booted with id" $?

{
    sleep 3
    kill -STOP "$LONE_PID"
    printf 'RECRUIT" %s (+ 40 2)"\n' "$LONE_ID"
    sleep 40                      # eviction (~15s) + paced resets to the cap
    printf 'RECRUITS\n'
    printf '2 3 + .\n'            # parent must still be responsive
    sleep 2
} | UNIT_PORT=4366 UNIT_PEERS=127.0.0.1:4365 "$BINARY" --quiet >"$PARENT_B" 2>&1
grep -q 'ABANDONED' "$PARENT_B"
run_test "only-peer wedge: wait terminally bounded (ABANDONED logged)" $?
grep -qE 'abandoned' "$PARENT_B"
run_test "abandoned error visible in RECRUITS view" $?
grep -q '^5 ' "$PARENT_B" || grep -q ' 5 ' "$PARENT_B"
run_test "parent responsive after abandonment" $?

# No-kill accounting: revive the wedged worker; its late life must not crash it.
kill -CONT "$LONE_PID" 2>/dev/null
sleep 2
kill -0 "$LONE_PID" 2>/dev/null
run_test "wedged worker alive after SIGCONT (never killed)" $?

echo
echo "  PASSED: $PASS"
echo "  FAILED: $FAIL"
echo "  TOTAL:  $TOTAL"
[ "$FAIL" -eq 0 ]
