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
set -u
cd "$(dirname "$0")"
COMPOSE="docker compose -f compose.drill.yml"
LOGS="drill-logs"

case "${1:-}" in
up)
    mkdir -p "$LOGS"
    : > "$LOGS/s1.log"; : > "$LOGS/s2.log"; : > "$LOGS/s3.log"
    $COMPOSE up -d s1 s2 s3
    echo "soak colony up (3 nodes × 300 units, 128 MiB each)."
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
        }
        last_t[id]=t; last_f[id]=f; last_k[id]=k; last_c[id]=c
        last_u[id]=u; last_o[id]=o; last_n[id]=n; last_d[id]=d; last_ut[id]=ut
        if (f>peak_f[id]) peak_f[id]=f
    }
    END {
        if (length(last_t)==0) { print "(soak-report :error \"no chronicle lines found — is the colony up?\")"; exit 1 }
        ticks=0; units=0; out=0; inn=0; deaths=0; kinds=0; copies=0; fk=0; lk=0; ff=0; lf=0; fc=0; lc=0
        for (id in last_t) {
            span = last_t[id]-first_t[id]; if (span>ticks) ticks=span
            units += last_u[id]; out += last_o[id]; inn += last_n[id]; deaths += last_d[id]
            kinds  = (last_k[id]>kinds ? last_k[id] : kinds)
            copies += last_c[id]
            fk += first_k[id]; lk += last_k[id]; ff += first_f[id]; lf += last_f[id]
            fc += first_c[id]; lc += last_c[id]
            printf "(soak-node :id \"%s\" :ticks %d :units %d :util %d :fit %d :peak-fit %d :sol-kinds %d :sol-copies %d :out %d :in %d :deaths %d)\n", \
                id, last_t[id], last_u[id], last_ut[id], last_f[id], peak_f[id], last_k[id], last_c[id], last_o[id], last_n[id], last_d[id]
        }
        # verdict by the stated evidence rules
        kg = lk - fk; fg = lf - ff; cg = lc - fc
        verdict = "churn"
        if (kg > 0 && fg > 0) verdict = "adaptive"
        else if (kg > 0 || fg > 0) verdict = "partial"
        printf "(soak-colony :ticks %d :units %d :migrations-out %d :migrations-in %d :deaths %d :kinds-gained %d :fit-gained %d :copies-gained %d :verdict %s)\n", \
            ticks, units, out, inn, deaths, kg, fg, cg, verdict
        printf "(soak-conservation :units %d :expected 900 :out %d :in %d :balanced %s)\n", \
            units, out, inn, (units==900 && out==inn ? "yes" : "check")
    }' "$LOGS"/s1.log "$LOGS"/s2.log "$LOGS"/s3.log
    ;;
down)
    $COMPOSE --profile soak down -t 3
    ;;
*)
    echo "usage: soak.sh up|report|down"; exit 2
    ;;
esac
