#!/usr/bin/env bash
# Container entrypoint for the wedge drill.
#
# - Optionally applies netem (delay/jitter/loss/reorder) on eth0 when
#   DRILL_NETEM=1 — requires NET_ADMIN (granted in compose.drill.yml).
# - Starts the unit REPL fed from a FIFO so the drill can inject Forth
#   commands at any point in the choreography:
#       docker compose exec -T <svc> inject 'RECRUITS'
# - Tees all output to /logs/$DRILL_NAME.log (bind-mounted) so the
#   assertion script can grep each node's exact emitted lines.
set -u

NAME="${DRILL_NAME:-node}"
mkdir -p /logs
: > "/logs/$NAME.log"

if [ "${DRILL_NETEM:-0}" = "1" ]; then
    # ~40ms ±10ms delay, 1% loss, slight reorder — WAN-ish realism.
    tc qdisc add dev eth0 root netem delay 40ms 10ms loss 1% reorder 5% \
        && echo "[drill] netem applied on eth0" \
        || echo "[drill] netem FAILED (missing NET_ADMIN?)"
fi

mkfifo /tmp/in
# `inject` helper for docker compose exec.
printf '#!/bin/sh\nprintf "%%s\\n" "$*" > /tmp/in\n' > /usr/local/bin/inject
chmod +x /usr/local/bin/inject

echo "[drill] $NAME starting: port=${UNIT_PORT:-4200} peers=${UNIT_PEERS:-<none>}"
# tail -f keeps the FIFO (and the REPL's stdin) open for the whole drill.
# DRILL_ARGS lets a service run in node mode (e.g. --multi-unit 900);
# word-splitting is intended. REPL services leave it empty.
# shellcheck disable=SC2086
tail -f /tmp/in | unit ${DRILL_ARGS:-} 2>&1 | tee -a "/logs/$NAME.log"
