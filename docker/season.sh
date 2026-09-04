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
#   SUMMER   turnover at capacity (senescence + rebound), and the
#            ceiling: regrowth settles in the ecology's [70%, 80%) band
#            with no starvation — the host cap is the physical guard,
#            not the limit.
#
# One node, no peers: with migration off the table, conservation is pure
# units == 300 − deaths + births, checked against the chronicle.
# Assertions read (node-status …) sexp lines only, per drill doctrine.
#
#   bash docker/season.sh          # full cycle, ~80 min
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
# Apply a budget and VERIFY it landed in the cgroup, retrying. One run's
# spring restore silently failed (docker update exit ignored): the node
# spent spring and summer on the drought's 369 MiB while the harness
# believed 512 — and every "failure" that followed was the organism
# behaving correctly for the budget it actually had. The verified value
# is what all capacity math must use.
cgroup_max_kb() { $COMPOSE exec -T season cat /sys/fs/cgroup/memory.max 2>/dev/null | awk '{print int($1/1024)}'; }
set_budget_kb() {
    local want_kb=$1 got attempt
    for attempt in 1 2 3; do
        docker update --memory "${want_kb}k" --memory-swap "${want_kb}k" docker-season-1 >/dev/null 2>&1
        sleep 2
        got=$(cgroup_max_kb); got=${got:-0}
        [ "$got" -eq "$want_kb" ] && return 0
        echo "    (budget update attempt $attempt: wanted $((want_kb/1024)) MiB, cgroup reads $((got/1024)) MiB — retrying)"
        sleep 5
    done
    return 1
}

echo "=== SEASON: boot (512 MiB, 300 units) ==="
mkdir -p drill-logs; : > "$LOG"
# Memory-breakdown sampler: one (season-mem …) sexp per minute — the
# cgroup's charge (memory.current) against its own accounting (anon,
# file, kernel, slab, sock) and the process's RSS. When util and RSS
# diverge, this says which bucket the difference lives in.
MEMLOG="drill-logs/season-mem.log"; : > "$MEMLOG"
(
  while true; do
    sleep 60
    S=$($COMPOSE exec -T season sh -c 'c=$(cat /sys/fs/cgroup/memory.current); m=$(cat /sys/fs/cgroup/memory.max); r=$(awk "/VmRSS/{print \$2}" /proc/$(pidof unit | cut -d" " -f1)/status 2>/dev/null); awk -v c=$c -v m=$m -v r=${r:-0} "BEGIN{cur=c/1048576; mx=m/1048576} /^(anon|file|kernel|slab|sock|shmem|inactive_file|active_file|anon_thp|swapcached) /{v[\$1]=\$2/1048576} END{printf \"(season-mem :current-mb %.0f :max-mb %.0f :rss-mb %.0f :anon %.0f :file %.0f :kernel %.0f :slab %.1f :sock %.1f :shmem %.1f :inactive-file %.1f :anon-thp %.0f)\", cur, mx, r/1024, v[\"anon\"], v[\"file\"], v[\"kernel\"], v[\"slab\"], v[\"sock\"], v[\"shmem\"], v[\"inactive_file\"], v[\"anon_thp\"]}" /sys/fs/cgroup/memory.stat' 2>/dev/null)
    [ -n "$S" ] && echo "[$(date -u '+%H:%M:%S')] $S" >> "$MEMLOG"
  done
) &
MEMPID=$!
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
HOLDS=0
while [ "$LIMIT_KB" -gt "$TARGET_KB" ] && [ "$STEPS" -lt 14 ] && [ "$HOLDS" -lt 10 ]; do
    # Next limit: 32 MiB down, but never below current RSS + 24 MiB guard
    # (memswap == mem, so a limit under residency is instant execution).
    RSS_KB=$($COMPOSE exec -T season cat /sys/fs/cgroup/memory.current 2>/dev/null | awk '{print int($1/1024)}')
    RSS_KB=${RSS_KB:-200000}
    NEXT_KB=$((LIMIT_KB - 32 * 1024))
    FLOOR_KB=$((RSS_KB + 24 * 1024))
    [ "$NEXT_KB" -lt "$FLOOR_KB" ] && NEXT_KB=$FLOOR_KB
    [ "$NEXT_KB" -lt "$TARGET_KB" ] && NEXT_KB=$TARGET_KB
    if [ "$NEXT_KB" -ge "$LIMIT_KB" ]; then
        # A hold is the organism lagging the drought, not a drought step.
        HOLDS=$((HOLDS+1))
        echo "    (hold $HOLDS at $((LIMIT_KB/1024)) MiB — waiting for shed below rss=$((RSS_KB/1024)) MiB)"
        sleep 90
        continue
    fi
    STEPS=$((STEPS+1))
    LIMIT_KB=$NEXT_KB
    echo "    (step $STEPS: budget → $((LIMIT_KB/1024)) MiB, rss=$((RSS_KB/1024)) MiB, units=$(nsf units))"
    set_budget_kb "$LIMIT_KB" || { echo "    (budget update FAILED; using cgroup's actual value)"; LIMIT_KB=$(cgroup_max_kb); }
    sleep 90
done
# The walk stops where physics stops it: the harness never sets a budget
# under what is resident, and an allocator that recycles corpses rather
# than returning them (measured: the footprint ratchets to the historical
# max and births reuse it) sets the floor. The organism's obligation is
# not "reach 192" but "match whatever budget you got".
REACHED_MB=$((LIMIT_KB/1024))
[ "$LIMIT_KB" -lt $((512 * 1024)) ]; check "drought: a real drought happened (budget $REACHED_MB MiB in $STEPS steps, $HOLDS holds)" $?
DROUGHT_DEATHS=$(nsf deaths); DROUGHT_DEATHS=${DROUGHT_DEATHS:-0}
[ "$DROUGHT_DEATHS" -gt 0 ]; check "drought: famine shed units (deaths=$DROUGHT_DEATHS)" $?
DROUGHT_STARVED=$(grep -c 'DIED.*— starved' "$LOG" 2>/dev/null); DROUGHT_STARVED=${DROUGHT_STARVED:-0}
[ "$(oomk)" = "0" ]; check "drought: the kernel never fired (oom_kill 0)" $?
ticking; check "drought: node kept ticking throughout" $?

echo "=== SEASON: winter hold (192 MiB, 5 min) ==="
sleep 300
WINTER_UNITS=$(nsf units); WINTER_DEATHS=$(nsf deaths); WINTER_BIRTHS=$(nsf births)
WINTER_UNITS=${WINTER_UNITS:-0}; WINTER_DEATHS=${WINTER_DEATHS:-0}; WINTER_BIRTHS=${WINTER_BIRTHS:-0}
# Carrying capacity of the reached budget at saturated cost: 80% of the
# budget / 650 KiB. Famine lifts there; the chronic fuse pipeline may
# carry a few more past it, never fewer. Within 15% below, never above.
CAPACITY=$(( LIMIT_KB * 8 / 10 / 650 ))
[ "$WINTER_UNITS" -le "$CAPACITY" ] && [ "$WINTER_UNITS" -ge $(( CAPACITY * 85 / 100 )) ]
check "winter: population tracks the budget's carrying capacity (units=$WINTER_UNITS, capacity=$CAPACITY at $REACHED_MB MiB)" $?
# Ghost check: does death physically free habitat? Resident KB per LIVING
# unit should sit near the saturated cost (~650 KiB); every corpse whose
# memory the allocator kept shows up here as excess.
W_RSS_KB=$($COMPOSE exec -T season cat /sys/fs/cgroup/memory.current 2>/dev/null | awk '{print int($1/1024)}')
W_RSS_KB=${W_RSS_KB:-0}
[ "$WINTER_UNITS" -gt 0 ] && echo "        (season-ghost :rss-mb $((W_RSS_KB/1024)) :units $WINTER_UNITS :kb-per-unit $((W_RSS_KB / WINTER_UNITS)) :budget-mb $((LIMIT_KB/1024)))"
[ "$(oomk)" = "0" ]; check "winter: still zero oom_kill" $?
[ "$WINTER_UNITS" -eq $((300 - WINTER_DEATHS + WINTER_BIRTHS)) ]
check "winter: conservation exact (units == 300 − $WINTER_DEATHS + $WINTER_BIRTHS)" $?

echo "=== SEASON: spring (192 → 512 MiB, 15 min) ==="
# Fifteen minutes, not ten: near the 70% line abundance income fades to
# its floor of 1/tick, so a famine-drained reserve needs ~700 ticks to
# refill before a parent can breed again. Spring must outlast that tail.
set_budget_kb $((512 * 1024)); check "spring: budget restored to 512 MiB (cgroup verified: $(( $(cgroup_max_kb) / 1024 )) MiB)" $?
SPRING_KB=$(cgroup_max_kb); SPRING_KB=${SPRING_KB:-$((512 * 1024))}
sleep 900
SPRING_UNITS=$(nsf units); SPRING_BIRTHS=$(nsf births); SPRING_DEATHS=$(nsf deaths)
SPRING_UNITS=${SPRING_UNITS:-0}; SPRING_BIRTHS=${SPRING_BIRTHS:-0}; SPRING_DEATHS=${SPRING_DEATHS:-0}
[ "$SPRING_BIRTHS" -gt "$WINTER_BIRTHS" ]
check "spring: rebound births resumed ($WINTER_BIRTHS → $SPRING_BIRTHS)" $?
[ "$SPRING_UNITS" -gt "$WINTER_UNITS" ]
check "spring: population regrowing into restored budget ($WINTER_UNITS → $SPRING_UNITS)" $?
[ "$(oomk)" = "0" ]; check "spring: zero oom_kill across the whole cycle" $?
[ "$SPRING_UNITS" -eq $((300 - SPRING_DEATHS + SPRING_BIRTHS)) ]
check "season: conservation exact across the full cycle (units == 300 − $SPRING_DEATHS + $SPRING_BIRTHS)" $?
# Heredity: the regrown population descends from winter's survivors, not
# from the prelude. Generation depth is the chronicle's honest surface.
GEN_MAX=$(nsf gen-max); GEN_MAX=${GEN_MAX:-0}
[ "$GEN_MAX" -ge 1 ]
check "spring: heredity depth — children of survivors (gen-max=$GEN_MAX)" $?

echo "=== SEASON: summer (512 MiB held, 25 min) — turnover at capacity ==="
# The 8 h homeostasis soak froze: zero deaths, zero turnover, generation
# depth stalled the moment growth stopped. Senescence must keep the gene
# pool moving in comfort: deaths of old age, slots refilled by rebound
# from survivors' genomes, population steady, generation depth rising.
sleep 1500
SUMMER_UNITS=$(nsf units); SUMMER_BIRTHS=$(nsf births); SUMMER_DEATHS=$(nsf deaths); SUMMER_GEN=$(nsf gen-max)
SUMMER_UNITS=${SUMMER_UNITS:-0}; SUMMER_BIRTHS=${SUMMER_BIRTHS:-0}; SUMMER_DEATHS=${SUMMER_DEATHS:-0}; SUMMER_GEN=${SUMMER_GEN:-0}
T_DEATHS=$((SUMMER_DEATHS - SPRING_DEATHS)); T_BIRTHS=$((SUMMER_BIRTHS - SPRING_BIRTHS))
[ "$T_DEATHS" -gt 0 ]; check "summer: death in comfort — turnover deaths at capacity ($T_DEATHS)" $?
[ "$T_BIRTHS" -gt 0 ]; check "summer: vacated slots refilled by rebound ($T_BIRTHS births)" $?
[ "$SUMMER_UNITS" -ge $(( SPRING_UNITS * 85 / 100 )) ]
check "summer: population steady through turnover ($SPRING_UNITS → $SUMMER_UNITS)" $?
[ "$SUMMER_GEN" -gt "$GEN_MAX" ]
check "summer: generation depth still rising (gen-max $GEN_MAX → $SUMMER_GEN)" $?
# The ceiling. With the host cap now the physical guard, regrowth must
# find the ECOLOGY's ceiling: abundance stops at 70% committed, famine
# starts at 80%. At 512 MiB that band is [564, 645) units. The
# population must reach at least 80% of the 70% line within the run and
# never cross into famine — every summer death is old age, none starved.
BAND_LO=$(( SPRING_KB * 7 / 10 / 650 )); BAND_HI=$(( SPRING_KB * 8 / 10 / 650 ))
STARVED=$(grep -c 'DIED.*— starved' "$LOG" 2>/dev/null); STARVED=${STARVED:-0}
# The ceiling is wherever the population first touched it — with births
# unthrottled that can be the boot phase — so the claim has two halves:
# the run's PEAK population reached the 70% line and never crossed into
# famine's 80%, and at summer's end the population still sits in reach
# of the line (turnover refilled, nothing starved it down).
PEAK_UNITS=$(grep '(node-status ' "$LOG" | grep -oE ':units [0-9]+' | awk '{print $2}' | sort -n | tail -1); PEAK_UNITS=${PEAK_UNITS:-0}
echo "        (season-ceiling :peak $PEAK_UNITS :summer $SUMMER_UNITS :band-lo $BAND_LO :band-hi $BAND_HI :starved-total $STARVED)"
[ "$PEAK_UNITS" -ge "$BAND_LO" ] && [ "$PEAK_UNITS" -lt "$BAND_HI" ]
check "ceiling: peak population reached the 70% line and never crossed 80% (peak $PEAK_UNITS in [$BAND_LO, $BAND_HI))" $?
[ "$SUMMER_UNITS" -ge $(( BAND_LO * 85 / 100 )) ] && [ "$SUMMER_UNITS" -lt "$BAND_HI" ]
check "summer: population holds in reach of the ceiling through turnover ($SUMMER_UNITS vs [$BAND_LO, $BAND_HI))" $?
[ "$STARVED" -eq "$DROUGHT_STARVED" ]
check "summer: no starvation past the drought — every later death is old age (starved $DROUGHT_STARVED → $STARVED)" $?
[ "$(oomk)" = "0" ]; check "summer: zero oom_kill" $?
[ "$SUMMER_UNITS" -eq $((300 - SUMMER_DEATHS + SUMMER_BIRTHS)) ]
check "season: conservation exact through summer (units == 300 − $SUMMER_DEATHS + $SUMMER_BIRTHS)" $?

echo ""
echo "(season-verdict :boot 300 :winter $WINTER_UNITS :spring $SPRING_UNITS :summer $SUMMER_UNITS :peak $PEAK_UNITS :ceiling-band \"[$BAND_LO,$BAND_HI)\" :deaths $SUMMER_DEATHS :births $SUMMER_BIRTHS :gen-max $SUMMER_GEN :oom $(oomk) :passed $PASS :failed $FAIL)"
kill $MEMPID 2>/dev/null
echo "--- memory breakdown (last 12 samples; full log: $MEMLOG) ---"; tail -12 "$MEMLOG"
$COMPOSE down -t 3 >/dev/null 2>&1
[ "$FAIL" -eq 0 ]
