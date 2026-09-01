# Validation — adversarial harnesses

Documentation claims here are hypotheses; these harnesses are the
executable evidence. Everything below runs real binaries in real
containers over real sockets, asserts against machine-readable
S-expression surfaces (never log prose), and is exercised on every push
by CI's two `wedge-drill` legs (netem off and on).

## The wedge drill

```
just drill                # build the image and run every scenario
DRILL_ONLY=S8 bash docker/drill.sh      # one scenario, for development
DRILL_NETEM=1 bash docker/drill.sh      # WAN realism: 40ms ±10ms, 1% loss, reorder
UNIT_RECRUIT_TIMEOUT_SECS=2 ...         # compress the 60s recruit window
```

| Scenario | What it proves |
|----------|----------------|
| S1 | A wedged recruit holder (docker pause = SIGSTOP) with capacity present resolves boundedly: re-recruit recovery, or fail-closed abandonment — never a hang; late replies are first-write-wins duplicates |
| S2 | The only peer wedged past the 15s eviction window (empty live view): the wait is terminally bounded (`ABANDONED`), the recruiter stays responsive, the worker is never torn down |
| S3 | An over-ceiling middle node defers, caps out, and the failure self-reports **up the tree** through the normal completion path |
| S4 | A part declined at emission lands as an UNPLACED ledger slot and is recruited the moment capacity appears |
| S5 | cgroup-v2-limited containers honestly advertise their own budgets; placement chooses the truthful roomy peer, never the tight one |
| S6 | TRANSPORT confirm-before-release across containers: an origin releases only after its copy landed (`accepted ≤ landed`); under loss the design fails toward duplication, never loss |
| S7 | Split-brain: a real partition (network disconnect, no RST), mutual bounded abandonment on both sides, then a clean heal — re-discovery, a fresh round-trip, no double-settles |
| S8 | Resource ecology under simultaneous pressure: three over-ceiling senders shed toward small receivers; a mid-shed blackhole; every survivor keeps ticking; wall breaches self-correct (the wall is an attractor — static equilibrium does not exist while landed units grow); colony-wide conservation with a traffic-scaled duplication bound |

The wedge semantics differ deliberately: `docker pause` freezes a
process (its sockets queue), `docker network disconnect` blackholes it
(packets vanish, no RST) — they exercise different failure paths, and
both found real bugs.

## The evolution soak

```
bash docker/soak.sh up        # three balanced nodes, 900 units
bash docker/soak.sh report    # anytime — hours later included
bash docker/soak.sh down
```

`SOAK_MEM` sets the per-node memory budget (default `128m`).

The report turns the colony's chronicle into a verdict under stated
evidence rules — `(soak-colony … :verdict
adaptive|partial|churn|casualty)` — plus per-node trajectories and
event-consistent conservation (`units == expected + landed − released`;
documented duplication is not a violation). Adaptation evidence is
antibody *kinds* growth (new challenges solved) together with *peak*
fitness growth; a colony that is climbing always reads low CURRENT
fitness, by construction. A node whose chronicle lags the colony max by
more than 300 ticks is `:stale yes` — dead or wedged, its ledger is
testimony rather than state — and any stale node forces `:verdict
casualty` and `:balanced stale-ledger`, whatever the evolution numbers
say.

### What overnight soaks have established (2026-08-31/09-01)

Three multi-hour runs, 3×300 units each; raw chronicles reproduce these
via `soak.sh report`:

- **128 MiB/node**: all three unit processes were OOM-killed by the
  kernel ~35 min in (`memory.events oom_kill 1` each) while the
  chronicle read `deaths 0` — the energy-mortality model is blind to the
  memory axis. Growth ran ~7 MB/min with no deceleration at that budget.
- **512 MiB/node, 8 h**: memory reached a genuine fixed point —
  ~190–196 MiB per node (≈650 KB per unit at saturation), flat to
  ±0.5 MiB for seven straight hours. Zero deaths, exact conservation,
  steady ~0.96 ticks/s. Evolution was *punctuated*: a first-hour burst
  (597 kinds), three hours of total stasis, then a second burst
  (+217 kinds in hour 5), then stasis again at 814. Peak fitness pinned
  at 980 throughout — the GP search-capacity frontier.
- **192 MiB/node (scarcity — budget ≈ 99% of natural demand)**: the
  colony was overcommitted from boot (900 units against ~720 of true
  carrying capacity), and migration is zero-sum, so the pressure had to
  land somewhere: two nodes shed 157 units onto the third — whose
  admission gate rightly passed them at arrival size, before their
  post-landing growth materialized — and that node was OOM-killed at
  99.9% util with every escape refused. The two survivors then held a
  flat 460-unit equilibrium for 3+ hours. The open gap this names: the
  organism has no way to *shrink* under memory pressure (mortality is
  energy-only), so colony-wide overcommit is resolved by the kernel
  killing a node rather than by units dying.

## The observability surfaces these assert against

- `(node-status …)` — one line per measure cadence from a persistent
  node; see docs/operations.md for the field reference.
- `RECRUITS-SEXP` — one parseable `(recruit-slot …)` per ledger slot.
- `(soak-node …)` / `(soak-colony …)` / `(soak-conservation …)` — the
  soak report itself.

These shapes are API: the prose log lines around them may be reworded
freely, but harnesses and tooling parse only the S-expressions.
