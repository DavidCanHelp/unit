#!/usr/bin/env bash
# drill.sh — Dockerized multihost wedge drill for unit's Phase 3 guarantees
# (v0.35.0, commit 1a489af): every wait in a recruit tree is terminally
# bounded; abandonment is fail-closed and self-reports up the tree; declined
# parts become UNPLACED slots recruited when capacity appears; supervision
# runs even on an empty live-peer view; no worker is ever torn down.
#
# Four scenarios, each on a fresh stack:
#   S1 wedged holder while capacity exists   -> bounded resolution
#   S2 wedged ONLY peer (empty view)         -> ABANDONED, responsive, no kill
#   S3 over-ceiling mid defers, caps out     -> failure self-reports upstream
#   S4 declined part (UNPLACED) + capacity   -> placed and collected
#
# The wedge is `docker pause` (cgroup freezer — whole-container SIGSTOP).
# DRILL_NETEM=1 adds tc-netem WAN realism per container (delay/loss/reorder).
# UNIT_RECRUIT_TIMEOUT_SECS compresses the 60s recruit window; the 15s peer
# eviction window is fixed. Exit is nonzero on any failed check.
set -u

cd "$(dirname "$0")"
COMPOSE="docker compose -f compose.drill.yml"
LOGS="drill-logs"
NETEM="${DRILL_NETEM:-0}"
TO="${UNIT_RECRUIT_TIMEOUT_SECS:-2}"
PASS=0; FAIL=0; TOTAL=0
mkdir -p "$LOGS"

check() { # check <name> <exit-status>
    TOTAL=$((TOTAL + 1))
    if [ "$2" -eq 0 ]; then echo "  PASS  $1"; PASS=$((PASS + 1));
    else echo "  FAIL  $1"; FAIL=$((FAIL + 1)); fi
}

inject() { # inject <svc> <forth line>
    $COMPOSE exec -T "$1" inject "$2"
}

node_id() { # node_id <name> — 16-hex mesh id from the boot banner
    grep -oE 'Mesh node [0-9a-f]{16}' "$LOGS/$1.log" | head -1 | awk '{print $3}'
}

poll() { # poll <secs> <file> <grep-ERE> [svc-to-nudge-with-RECRUITS]
    local deadline=$(( $(date +%s) + $1 ))
    while [ "$(date +%s)" -lt "$deadline" ]; do
        grep -qE "$3" "$2" 2>/dev/null && return 0
        [ -n "${4:-}" ] && inject "$4" 'RECRUITS' >/dev/null 2>&1
        sleep 2
    done
    grep -qE "$3" "$2" 2>/dev/null
}

up() { # up <svc...> — fresh stack with current TO. The logs dir is
    # NEVER deleted (a bind-mounted dir that is rm'd goes stale on Docker
    # Desktop); each entrypoint truncates its own file at boot instead.
    down
    mkdir -p "$LOGS"
    UNIT_RECRUIT_TIMEOUT_SECS="$TO" DRILL_NETEM="$NETEM" $COMPOSE up -d "$@" >/dev/null 2>&1
}

down() {
    $COMPOSE unpause mid >/dev/null 2>&1 || true
    $COMPOSE --profile leaves down -t 2 >/dev/null 2>&1
}

snap_logs() { # snap_logs <scenario> — keep logs for post-mortem/CI artifacts
    rm -rf "drill-logs-$1"; cp -r "$LOGS" "drill-logs-$1" 2>/dev/null || true
}
trap down EXIT

wait_discovery() { # wait_discovery <watcher> <target> — watcher sees target id
    local tid=""
    for _ in $(seq 1 20); do tid=$(node_id "$2"); [ -n "$tid" ] && break; sleep 1; done
    [ -n "$tid" ] || return 1
    for _ in $(seq 1 20); do
        inject "$1" 'MESH-STATUS' >/dev/null 2>&1
        grep -q "$tid" "$LOGS/$1.log" && { echo "$tid"; return 0; }
        sleep 1
    done
    return 1
}

# Wedge bound: eviction window + RECRUIT_TIMEOUT × (MAX_SLOT_ATTEMPTS + 1) + margin.
BOUND=$(( 15 + TO * 6 + 30 ))

echo "=== S1: wedged holder, capacity present -> bounded resolution (netem=$NETEM, timeout=${TO}s) ==="
up root mid leaf1
MID_ID=$(wait_discovery root mid); check "S1 discovery: root sees mid" $?
$COMPOSE pause mid >/dev/null 2>&1
inject root "RECRUIT\" $MID_ID (parallel (+ 1 1) (+ 2 2))\""
poll "$BOUND" "$LOGS/root.log" 'ABANDONED|ok value=' root
check "S1 wedged slot reaches a terminal state within ${BOUND}s" $?
if grep -q 'ok value=' "$LOGS/root.log"; then
    echo "        (path: re-recruited to leaf, result collected)"
else
    echo "        (path: no capacity accepted -> abandoned fail-closed)"
fi
SNAP1=$(grep -E 'recruit #1 seq 0 (->|from)' "$LOGS/root.log" | tail -1)
! echo "$SNAP1" | grep -q 'pending'; check "S1 slot not left pending" $?
inject root '111 222 + .'
poll 10 "$LOGS/root.log" '333'; check "S1 root responsive after resolution" $?
$COMPOSE unpause mid >/dev/null 2>&1
sleep $(( TO + 3 ))   # let the thawed mid flush its late reply
$COMPOSE exec -T mid pgrep -x unit >/dev/null 2>&1
check "S1 wedged worker alive after unpause (never torn down)" $?
inject root 'RECRUITS'; sleep 2
SNAP2=$(grep -E 'recruit #1 seq 0 (->|from)' "$LOGS/root.log" | tail -1)
[ "$(echo "$SNAP1" | sed 's/.*seq 0 //')" = "$(echo "$SNAP2" | sed 's/.*seq 0 //')" ]
check "S1 late reply is a dropped duplicate (slot state unchanged)" $?
snap_logs S1
down

echo "=== S2: wedged ONLY peer -> eviction, empty view, ABANDONED ==="
up root mid
MID_ID=$(wait_discovery root mid); check "S2 discovery: root sees mid" $?
$COMPOSE pause mid >/dev/null 2>&1
inject root "RECRUIT\" $MID_ID (+ 40 2)\""
poll "$BOUND" "$LOGS/root.log" 'ABANDONED'
check "S2 empty-view wait terminally bounded (ABANDONED)" $?
inject root 'RECRUITS'
poll 10 "$LOGS/root.log" 'ERR \[abandoned\]'; check "S2 abandoned error visible in RECRUITS" $?
inject root '111 222 + .'
poll 10 "$LOGS/root.log" '333'; check "S2 root responsive after abandonment" $?
$COMPOSE unpause mid >/dev/null 2>&1
sleep $(( TO + 3 ))
$COMPOSE exec -T mid pgrep -x unit >/dev/null 2>&1
check "S2 wedged worker alive after unpause" $?
N_ABANDON=$(grep -c "ABANDONED" "$LOGS/root.log" 2>/dev/null || echo 0)
[ "$N_ABANDON" -eq 1 ]; check "S2 exactly one abandonment (no double-settle)" $?
snap_logs S2
down

echo "=== S3: over-ceiling mid defers, caps out -> failure self-reports upstream ==="
up root mid
MID_ID=$(wait_discovery root mid); check "S3 discovery: root sees mid" $?
# Push mid over the 80% ceiling with its own load generator (host-wide
# /proc in containers, so sized from live MemAvailable, freed afterwards).
ALLOC_MB=$($COMPOSE exec -T mid sh -c \
  "awk '/MemTotal/{mt=\$2}/MemAvailable/{ma=\$2}/SwapTotal/{st=\$2}/SwapFree/{sf=\$2}END{used=(mt-ma)+(st-sf); tgt=0.88*(mt+st); m=int((tgt-used)/1024); cap=int(ma/1024)-512; if(m>cap)m=cap; if(m<128)m=0; print m}' /proc/meminfo")
[ "${ALLOC_MB:-0}" -gt 0 ]; check "S3 over-ceiling alloc sized (${ALLOC_MB:-0} MiB)" $?
inject mid 'ALLOC-ENABLE'
inject mid "$ALLOC_MB ALLOC-MB ."
poll 60 "$LOGS/mid.log" "$ALLOC_MB +ok"
check "S3 ballast resident on mid" $?
inject root "RECRUIT\" $MID_ID (parallel (+ 1 1) (+ 2 2))\""
poll "$BOUND" "$LOGS/mid.log" 'unplaced, no candidate with headroom'
check "S3 declined parts land as UNPLACED slots (supervised)" $?
poll "$BOUND" "$LOGS/mid.log" 'ABANDONED'
check "S3 mid abandons at the attempt cap" $?
poll "$BOUND" "$LOGS/root.log" 'ERR \[nested\]' root
check "S3 failure self-reported up: root settled, not pending" $?
SNAP3=$(grep -E "recruit #1 seq 0 (->|from)" "$LOGS/root.log" | tail -1)
! echo "$SNAP3" | grep -q 'pending'; check "S3 root slot settled by the self-report" $?
inject mid 'RECLAIM-MB .'
snap_logs S3
down

echo "=== S4: UNPLACED part -> capacity appears -> recruited and collected ==="
TO_SAVE="$TO"; TO=4; BOUND=$(( 15 + TO * 6 + 30 ))
up root mid
wait_discovery mid root >/dev/null; check "S4 discovery: mid sees root" $?
ALLOC_MB=$($COMPOSE exec -T mid sh -c \
  "awk '/MemTotal/{mt=\$2}/MemAvailable/{ma=\$2}/SwapTotal/{st=\$2}/SwapFree/{sf=\$2}END{used=(mt-ma)+(st-sf); tgt=0.88*(mt+st); m=int((tgt-used)/1024); cap=int(ma/1024)-512; if(m>cap)m=cap; if(m<128)m=0; print m}' /proc/meminfo")
[ "${ALLOC_MB:-0}" -gt 0 ]; check "S4 over-ceiling alloc sized (${ALLOC_MB:-0} MiB)" $?
inject mid 'ALLOC-ENABLE'
inject mid "$ALLOC_MB ALLOC-MB ."
poll 60 "$LOGS/mid.log" "$ALLOC_MB +ok"
check "S4 ballast resident on mid" $?
inject mid 'PARALLEL" (parallel (+ 1 1) (+ 2 2))"'
poll "$BOUND" "$LOGS/mid.log" 'unplaced, no candidate with headroom'
check "S4 part declined at emission -> UNPLACED slot" $?
# Capacity appears: free the pressure AND start a worker.
inject mid 'RECLAIM-MB .'
UNIT_RECRUIT_TIMEOUT_SECS="$TO" DRILL_NETEM="$NETEM" $COMPOSE up -d leaf1 >/dev/null 2>&1
poll "$BOUND" "$LOGS/mid.log" 'parallel #1 complete'
check "S4 unplaced slot recruited to new capacity, job completes" $?
inject mid 'RECRUITS'
poll 10 "$LOGS/mid.log" 're-recruited [0-9]+x'
check "S4 placement pass reassigned the unplaced slots (re-recruited Nx)" $?
snap_logs S4
TO="$TO_SAVE"
down

echo
echo "  PASSED: $PASS"
echo "  FAILED: $FAIL"
echo "  TOTAL:  $TOTAL"
[ "$FAIL" -eq 0 ]
