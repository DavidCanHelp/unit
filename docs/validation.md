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
bash docker/soak.sh up        # three balanced 128 MiB nodes, 900 units
bash docker/soak.sh report    # anytime — hours later included
bash docker/soak.sh down
```

The report turns the colony's chronicle into a verdict under stated
evidence rules — `(soak-colony … :verdict adaptive|partial|churn)` —
plus per-node trajectories and event-consistent conservation
(`units == expected + landed − released`; documented duplication is not
a violation). Adaptation evidence is antibody *kinds* growth (new
challenges solved) together with *peak* fitness growth; a colony that is
climbing always reads low CURRENT fitness, by construction.

## The observability surfaces these assert against

- `(node-status …)` — one line per measure cadence from a persistent
  node; see docs/operations.md for the field reference.
- `RECRUITS-SEXP` — one parseable `(recruit-slot …)` per ledger slot.
- `(soak-node …)` / `(soak-colony …)` / `(soak-conservation …)` — the
  soak report itself.

These shapes are API: the prose log lines around them may be reworded
freely, but harnesses and tooling parse only the S-expressions.
