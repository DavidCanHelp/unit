#!/usr/bin/env bash
# season.sh — carrying capacity under a MOVING budget.
#
# Every soak so far ran against a fixed memory budget; nature's defining
# pressure is that capacity changes. This harness boots one comfortable
# node (300 units, 512 MiB), then moves memory.max LIVE through a
# gradual drought to 192 MiB, a winter hold, and a spring back to
# 512 MiB — and asserts the population tracks the habitat:
#
#   DROUGHT  famine sheds units as the budget falls (deaths > 0), the
#            node keeps ticking, and the kernel never fires. Each step
#            shrinks toward (last RSS + guard), never below what is
#            physically resident — a drought faster than the organism
#            can shed is just an execution, not a test.
#   WINTER   population stabilizes at the small budget's capacity.
#   SPRING   abundance-funded rebound births regrow the population into
#            the restored budget (births > winter's, units rising).
#
# One node, no peers: with migration off the table, conservation is pure
# units == 300 − deaths + births, checked against the chronicle.
# Assertions read (node-status …) sexp lines only, per drill doctrine.
#
#   bash docker/season.sh          # full cycle, ~25 min
set -u
cd "$(dirname "$0")"
COMPOSE="docker compose -f compose.drill.yml --profile season"
LOG="drill-logs/season.log"
PASS=0; FAIL=0
check() {
    if [ "$2" -eq 0 ]; then PASS=$((PASS+1)); echo "  PASS  $1"
    else FAIL=$((FAIL+1)); echo "  FAIL  $1"; fi
}
# Last chronicle line's field (node-status is the stable sexp surface).
nsf() { grep '(node-status ' "$LOG" | tail -1 | grep -oE ":$1 [0-9]+" | head -1 | awk '{print $2}'; }
oomk() { $COMPOSE exec -T season cat /sys/fs/cgroup/memory.events 2>/dev/null | awk '/oom_kill/{print $2}'; }
ticking() { # ticks advanced over 20s?
    local a b; a=$(nsf tick); sleep 20; b=$(nsf tick)
    [ -n "$a" ] && [ -n "$b" ] && [ "$b" -gt "$a" ]
}

echo "=== SEASON: boot (512 MiB, 300 units) ==="
mkdir -p drill-logs; : > "$LOG"
SEASON_MEM=512m $COMPOSE up -d season >/dev/null 2>&1
for i in $(seq 1 60); do
    [ "$(nsf units)" = "300" ] && break
    sleep 5
done
[ "$(nsf units)" = "300" ]; check "boot: 300 units chronicled" $?
# Let RSS climb toward saturation so the drought bites real occupancy.
echo "    (letting memory demand materialize, 6 min)"
sleep 360

echo "=== SEASON: drought (512 → 192 MiB, stepped) ==="
LIMIT_KB=$((512 * 1024))
TARGET_KB=$((192 * 1024))
STEPS=0
while [ "$LIMIT_KB" -gt "$TARGET_KB" ] && [ "$STEPS" -lt 12 ]; do
    STEPS=$((STEPS+1))
    # Next limit: 32 MiB down, but never below current RSS + 24 MiB guard
    # (memswap == mem, so a limit under residency is instant execution).
    RSS_KB=$($COMPOSE exec -T season cat /sys/fs/cgroup/memory.current 2>/dev/null | awk '{print int($1/1024)}')
    RSS_KB=${RSS_KB:-200000}
    NEXT_KB=$((LIMIT_KB - 32 * 1024))
    FLOOR_KB=$((RSS_KB + 24 * 1024))
    [ "$NEXT_KB" -lt "$FLOOR_KB" ] && NEXT_KB=$FLOOR_KB
    [ "$NEXT_KB" -lt "$TARGET_KB" ] && NEXT_KB=$TARGET_KB
    if [ "$NEXT_KB" -ge "$LIMIT_KB" ]; then
        echo "    (holding at $((LIMIT_KB/1024)) MiB — waiting for shed below rss=$((RSS_KB/1024)) MiB)"
        sleep 90
        continue
    fi
    LIMIT_KB=$NEXT_KB
    echo "    (step $STEPS: budget → $((LIMIT_KB/1024)) MiB, rss=$((RSS_KB/1024)) MiB, units=$(nsf units))"
    docker update --memory "${LIMIT_KB}k" --memory-swap "${LIMIT_KB}k" docker-season-1 >/dev/null
    sleep 90
done
[ "$LIMIT_KB" -le "$TARGET_KB" ]; check "drought: budget walked down to 192 MiB in $STEPS steps" $?
DROUGHT_DEATHS=$(nsf deaths); DROUGHT_DEATHS=${DROUGHT_DEATHS:-0}
[ "$DROUGHT_DEATHS" -gt 0 ]; check "drought: famine shed units (deaths=$DROUGHT_DEATHS)" $?
[ "$(oomk)" = "0" ]; check "drought: the kernel never fired (oom_kill 0)" $?
ticking; check "drought: node kept ticking throughout" $?

echo "=== SEASON: winter hold (192 MiB, 5 min) ==="
sleep 300
WINTER_UNITS=$(nsf units); WINTER_DEATHS=$(nsf deaths); WINTER_BIRTHS=$(nsf births)
WINTER_UNITS=${WINTER_UNITS:-0}; WINTER_DEATHS=${WINTER_DEATHS:-0}; WINTER_BIRTHS=${WINTER_BIRTHS:-0}
# 192 MiB feeds ~242 units at saturated cost; famine's fuse pipeline
# overshoots below that, never above it.
[ "$WINTER_UNITS" -lt 250 ] && [ "$WINTER_UNITS" -gt 50 ]
check "winter: population at small-budget capacity (units=$WINTER_UNITS)" $?
[ "$(oomk)" = "0" ]; check "winter: still zero oom_kill" $?
[ "$WINTER_UNITS" -eq $((300 - WINTER_DEATHS + WINTER_BIRTHS)) ]
check "winter: conservation exact (units == 300 − $WINTER_DEATHS + $WINTER_BIRTHS)" $?

echo "=== SEASON: spring (192 → 512 MiB, 10 min) ==="
docker update --memory 512m --memory-swap 512m docker-season-1 >/dev/null
sleep 600
SPRING_UNITS=$(nsf units); SPRING_BIRTHS=$(nsf births); SPRING_DEATHS=$(nsf deaths)
SPRING_UNITS=${SPRING_UNITS:-0}; SPRING_BIRTHS=${SPRING_BIRTHS:-0}; SPRING_DEATHS=${SPRING_DEATHS:-0}
[ "$SPRING_BIRTHS" -gt "$WINTER_BIRTHS" ]
check "spring: rebound births resumed ($WINTER_BIRTHS → $SPRING_BIRTHS)" $?
[ "$SPRING_UNITS" -gt "$WINTER_UNITS" ]
check "spring: population regrowing into restored budget ($WINTER_UNITS → $SPRING_UNITS)" $?
[ "$(oomk)" = "0" ]; check "spring: zero oom_kill across the whole cycle" $?
[ "$SPRING_UNITS" -eq $((300 - SPRING_DEATHS + SPRING_BIRTHS)) ]
check "season: conservation exact across the full cycle (units == 300 − $SPRING_DEATHS + $SPRING_BIRTHS)" $?

echo ""
echo "(season-verdict :boot 300 :winter $WINTER_UNITS :spring $SPRING_UNITS :deaths $SPRING_DEATHS :births $SPRING_BIRTHS :oom $(oomk) :passed $PASS :failed $FAIL)"
$COMPOSE down -t 3 >/dev/null 2>&1
[ "$FAIL" -eq 0 ]
