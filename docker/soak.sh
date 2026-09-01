#!/usr/bin/env bash
# soak.sh — long-lived evolution colony for unit.
#
# The project's core thesis — that colony life produces ADAPTATION rather
# than churn — has never had executable evidence beyond seconds-long runs.
# This harness runs a balanced three-node colony for hours and turns its
# machine-readable (node-status ...) chronicle into an honest verdict.
#
#   bash docker/soak.sh up          # start the colony (detached)
#   bash docker/soak.sh report      # analyze the chronicle so far (anytime)
#   bash docker/soak.sh down        # stop and clean up
#
# Evidence rules the report applies (stated, not vibes):
#   ADAPTIVE  — :sol-kinds grows over the run (new challenges solved; the
#               landscape is generating and the colony is climbing it) and
#               :fit rises with it.
#   SPREADING — :sol-copies grows faster than kinds (knowledge propagating).
#   CHURN     — GP keeps ticking but kinds/fit are flat for the whole run.
# Deaths, migrations, and conservation are reported alongside; a mortality
# or transport count of zero is itself a finding about current dynamics.
#
# Stale-node rule (learned from the 2026-08-31 scarcity run, where one
# node was OOM-killed and its frozen ledger kept the colony report
# reading "deaths 0 / balanced yes"): a node whose last chronicle tick
# lags the colony's max by more than 300 ticks (~5 min) is reported
# :stale yes — dead or stalled, its ledger is testimony, not state. Any
# stale node forces :verdict casualty and taints the conservation check.
set -u
cd "$(dirname "$0")"
COMPOSE="docker compose -f compose.drill.yml"
LOGS="drill-logs"

case "${1:-}" in
up)
    mkdir -p "$LOGS"
    : > "$LOGS/s1.log"; : > "$LOGS/s2.log"; : > "$LOGS/s3.log"
    $COMPOSE up -d s1 s2 s3
    echo "soak colony up (3 nodes × 300 units, ${SOAK_MEM:-128m} each)."
    echo "let it live for hours, then: bash docker/soak.sh report"
    ;;
report)
    awk '
    function num(s) { return s + 0 }
    /\(node-status / {
        # extract fields from the flat sexp line
        id=""; for (i=1;i<=NF;i++) {
            if ($i==":id")        { gsub(/"/,"",$(i+1)); id=$(i+1) }
            if ($i==":tick")      t=num($(i+1))
            if ($i==":units")     u=num($(i+1))
            if ($i==":util")      ut=num($(i+1))
            if ($i==":out")       o=num($(i+1))
            if ($i==":in")        n=num($(i+1))
            if ($i==":deaths")    d=num($(i+1))
            if ($i==":fit")       f=num($(i+1))
            if ($i==":sol-kinds") k=num($(i+1))
            if ($i==":sol-copies") c=num($(i+1))
        }
        if (id=="") next
        if (!(id in first_t)) {
            first_t[id]=t; first_f[id]=f; first_k[id]=k; first_c[id]=c; first_u[id]=u
            first_pf[id]=f
        }
        last_t[id]=t; last_f[id]=f; last_k[id]=k; last_c[id]=c
        last_u[id]=u; last_o[id]=o; last_n[id]=n; last_d[id]=d; last_ut[id]=ut
        if (f>peak_f[id]) peak_f[id]=f
    }
    END {
        if (length(last_t)==0) { print "(soak-report :error \"no chronicle lines found — is the colony up?\")"; exit 1 }
        # A dead or wedged node stops writing its chronicle; its last line
        # then testifies to a state that no longer exists. Compare every
        # node against the colony max tick to catch that lie.
        colmax=0; for (id in last_t) if (last_t[id]>colmax) colmax=last_t[id]
        ticks=0; units=0; out=0; inn=0; deaths=0; kinds=0; copies=0; fk=0; lk=0; fpk=0; lpk=0; fc=0; lc=0
        stale=0; stale_units=0
        for (id in last_t) {
            span = last_t[id]-first_t[id]; if (span>ticks) ticks=span
            is_stale[id] = (colmax - last_t[id] > 300)
            if (is_stale[id]) { stale++; stale_units += last_u[id] }
            units += last_u[id]; out += last_o[id]; inn += last_n[id]; deaths += last_d[id]
            kinds  = (last_k[id]>kinds ? last_k[id] : kinds)
            copies += last_c[id]
            fk += first_k[id]; lk += last_k[id]
            # fit gain measured on the PEAK, not the active frontier: a
            # healthy colony is always grinding something unsolved, so its
            # CURRENT fitness reads low precisely when it is climbing.
            fpk += first_pf[id]; lpk += peak_f[id]
            fc += first_c[id]; lc += last_c[id]
            printf "(soak-node :id \"%s\" :ticks %d :units %d :util %d :fit %d :peak-fit %d :sol-kinds %d :sol-copies %d :out %d :in %d :deaths %d :stale %s)\n", \
                id, last_t[id], last_u[id], last_ut[id], last_f[id], peak_f[id], last_k[id], last_c[id], last_o[id], last_n[id], last_d[id], (is_stale[id] ? "yes" : "no")
        }
        # verdict by the stated evidence rules; a stale node overrides them
        # all — evolution gains mean nothing next to a dead third of the
        # colony, and "adaptive" over a casualty would be flattery.
        kg = lk - fk; fg = lpk - fpk; cg = lc - fc
        verdict = "churn"
        if (kg > 0 && fg > 0) verdict = "adaptive"
        else if (kg > 0 || fg > 0) verdict = "partial"
        if (stale > 0) verdict = "casualty"
        printf "(soak-colony :ticks %d :units %d :migrations-out %d :migrations-in %d :deaths %d :stale-nodes %d :stale-units %d :kinds-gained %d :fit-gained %d :copies-gained %d :verdict %s)\n", \
            ticks, units, out, inn, deaths, stale, stale_units, kg, fg, cg, verdict
        # Event-consistent accounting: units == expected + (landed - released)
        # (a landing whose confirm was lost keeps both copies — documented
        # fail-toward-duplication; naive equality would flag it wrongly).
        # A stale ledger contributes units that may no longer exist, so the
        # equation can only be trusted when every node is live.
        printf "(soak-conservation :units %d :expected 900 :out %d :in %d :balanced %s)\n", \
            units, out, inn, (stale > 0 ? "stale-ledger" : (units == 900 + inn - out ? "yes" : "VIOLATION"))
    }' "$LOGS"/s1.log "$LOGS"/s2.log "$LOGS"/s3.log
    ;;
down)
    $COMPOSE --profile soak down -t 3
    ;;
*)
    echo "usage: soak.sh up|report|down"; exit 2
    ;;
esac
