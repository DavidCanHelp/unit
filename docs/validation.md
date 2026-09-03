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
  flat 460-unit equilibrium for 3+ hours. The gap this named: the
  organism had no way to *shrink* under memory pressure (mortality was
  energy-only), so colony-wide overcommit was resolved by the kernel
  killing a node rather than by units dying. **Famine** answers it:
  a host stuck over its resource ceiling taxes every resident's energy
  in proportion to the overshoot (`FAMINE_TAX_MAX`, scaled), the
  weakest pin at the hard floor and die through the ordinary mortality
  path — bequeathing antibodies — and the population settles at the
  host's carrying capacity. Emigration stays the cheaper escape; famine
  only kills when the colony has nowhere left to shed. Three follow-up
  scarcity runs hardened it: per-unit *foraging luck* (each unit draws
  50–150% of the tax from its own rng stream) broke the avalanche where
  same-aged units pinned and died in lockstep, and *acute* famine
  (util ≥ 96%: tax ×4, fuse ×6) wins the race gradual starvation lost
  three times to the OOM-killer. The final run's immigration-flooded
  node survived its own flood — 384→280 units in six seconds of acute
  rationing — and the colony finished with zero kernel kills, zero
  stale nodes, every one of 900 units accounted for
  (`347 == 900 + 140 − 140 − 553`), and `:verdict adaptive`.

### The scarcity campaign (runs 4–12, 2026-09-01/03)

Nine further scarcity runs (192 MiB/node, 900 units against ~720 of
capacity) drove the full resource-ecology mechanism set, every piece
forced by an observed death and pinned by a unit test plus a soak run:

| Run | Change under test | Outcome |
|-----|-------------------|---------|
| 4 | famine | works (one node self-regulates), but avalanche mortality (102 deaths/s) and the sink race lost |
| 5 | + foraging luck (per-unit tax jitter, distinct rng streams) | avalanche broken (max 26/s); sink race lost again |
| 6 | + acute famine (util ≥ 96%: tax ×4, fuse ×6) | first colony-wide survival: zero OOM, sink survives its flood |
| 7 | + rebound births | survival holds, but 3 births/3 h — famine survivors too poor to breed |
| 8 | + abundance income | dead zone: bonus rounds to 0 near threshold; fragmentation parks util there |
| 9 | + periodic trim, bonus floor | REGRESSION: honest measurement removed fragmentation's accidental early warning; sink OOMs a 4th time |
| 10 | + committed-demand admission | TOTAL LOSS: trading refused (correct) but famine still reactive — all 3 nodes OOM at boot overcommit |
| 11 | + habitat fullness = max(measured, committed) everywhere | first clean sheet: zero OOM, exact conservation, best evolution rate (680 kinds) |
| 12 | + acute keyed to measurement only | second clean sheet; rebound accelerating (births 4→14→23/h, population climbing) |

What the campaign established:

- **The kernel race must not be run.** Reactive signals — however
  honest — cannot gate demand that lands minutes after acceptance.
  Admission and metabolism both price COMMITTED demand
  (`SATURATED_UNIT_COST_KB` = 650, measured); an overcommitted node
  enters famine at tick zero, while RSS is still small.
- **Uniformity kills.** Identical energies, identical taxes, and
  shared rng streams each independently resynchronized mortality into
  avalanches; per-unit variance is load-bearing.
- **Acute answers the measurement, chronic answers the commitment** —
  the kernel acts on real memory, not promises.
- **A thinned colony out-evolves a crammed one** (runs 11–12 posted the
  campaign's best kinds rates at ~55% population).

Still open, deliberately (rate tuning, not mechanism): boot-cohort
overshoot (the uniform initial population enters the 30-tick fuse
pipeline together, so famine lift arrives with deaths still in flight),
and rebound pace (~10 units/h colony-wide at low abundance). Both
self-correct; neither kills.

## The seasons drill

```
bash docker/season.sh          # full cycle, ~25 min
```

Every soak runs against a fixed budget; nature's defining pressure is
that carrying capacity moves. One peerless node boots comfortable
(300 units, 512 MiB); the harness then moves `memory.max` LIVE
(`docker update`) through a stepped drought to 192 MiB — each step
floored at current RSS plus a guard, because with swap pinned a limit
under residency is an execution, not a drought — a winter hold, and a
spring back to 512 MiB. Twelve assertions on the chronicle: famine
sheds on the way down, the kernel never fires, ticks never stall, the
winter population sits at the small budget's capacity, spring births
resume and the population regrows, and conservation stays exact
(`units == 300 − deaths + births`; no peers, so migration cannot blur
the ledger).

First run (v0.40 binary): drought and winter passed; **spring failed
absolutely** — ten minutes at 25% util, zero births. The structural
cause: GP-EVOLVE's energy gate is anchored at the hard floor, so
evolution spends every affordable coin to its gate each tick and no
unit ever *holds* the breeding price, at any income. The fix is
nature's: **protected reproductive investment** — abundance income
deposits into a reserve invisible to discretionary spending, famine
drains it first (fat before muscle), breeding pays from it alone.
Rerun verdict, 12/12:

```
(season-verdict :boot 300 :winter 59 :spring 147
                :deaths 241 :births 88 :oom 0 :passed 12 :failed 0)
```

The drill also asserts heredity depth: after spring, `:gen-max ≥ 1` —
the regrown population descends from winter's survivors rather than
from the prelude, each child a mutated copy of its parent's genome.

Still open, deliberately: winter overshoot (the chronic fuse pipeline
carries a wide cohort past capacity — 59 units against ~242; the boot
population's uniform energy is the known cause) and spring's ceiling —
the drill ends while growth is still climbing, so where the regrown
population settles is unmeasured.

## The observability surfaces these assert against

- `(node-status …)` — one line per measure cadence from a persistent
  node; see docs/operations.md for the field reference.
- `RECRUITS-SEXP` — one parseable `(recruit-slot …)` per ledger slot.
- `(soak-node …)` / `(soak-colony …)` / `(soak-conservation …)` — the
  soak report itself.

These shapes are API: the prose log lines around them may be reworded
freely, but harnesses and tooling parse only the S-expressions.
