# Changelog

All notable changes to this project are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added

- **Famine: memory scarcity is now priced into the energy economy.**
  The overnight scarcity soak showed colony-wide overcommit ends with
  the kernel OOM-killing a whole node while every unit reads healthy —
  the organism had no way to shrink. Now a host stuck over its resource
  ceiling taxes each resident's energy in proportion to the overshoot
  (up to `FAMINE_TAX_MAX` = 50/tick at total overshoot); the weakest
  pin at the hard floor and die through the existing mortality path,
  bequeathing antibodies, and the population settles at the host's
  carrying capacity. Emigration remains the cheaper escape — famine
  only kills while there is nowhere left to shed. Death by starvation,
  not execution by the kernel: the most nature-shaped answer to a
  demonstrated failure.
- Soak conservation now accounts for mortality:
  `units == expected + landed − released − starved`, with `:deaths` on
  the conservation line.

### Chronic vs acute famine (run-11 finding)

- Run 11 was the scarcity gauntlet's first clean sheet: zero OOM kills,
  zero stale nodes, exact conservation at every checkpoint, and the
  best evolution rate of any scarcity run (680 kinds gained — a thinned
  colony evolves better than a crammed one). One imperfection: the boot
  famine ran ACUTE off the committed signal and over-killed ~65 units
  per node past carrying capacity. Acute famine now answers the
  MEASUREMENT only — the kernel acts on real memory, not promises — so
  a boot overcommit shrinks by staggered chronic starvation while the
  emergency fuse stays reserved for genuinely imminent OOM.

### Habitat fullness (run-10 finding)

- **Famine, abundance, and rebound now all read
  `max(measured, committed)` utilization.** Run 10 was a total loss:
  the committed admission gate refused all trading (as designed), boot-
  overcommitted nodes kept their 300 units, and famine — still keyed to
  the reactive measurement, duty-cycled by trim-induced oscillation
  around the ceiling — landed zero deaths before the kernel killed all
  three nodes. Priced on commitment, famine engages at tick 0 of an
  overcommit and shrinks the population before memory approaches the
  wall: the race against the OOM-killer stops being run at all.
  `SATURATED_UNIT_COST_KB` is corrected to the measured 650 and shared
  by admission and metabolism, so the two ends of the organism agree on
  what a population costs.

### Committed-demand admission (run-9 finding)

- **The admission gate now prices what the node has promised, not what
  it measures.** Run 9's periodic trim made measurement honest — and
  that honesty removed fragmentation's accidental early warning: the
  immigration sink read util 91% while its 444 admitted units had
  already committed ~289 MiB against a 192 MiB budget, and it was
  OOM-killed before famine landed a single death (the fourth sink loss).
  Measured utilization is reactive; a flood's cost is committed but not
  yet measured. The listener now also refuses when (hosted units +
  pending admits) × `SATURATED_UNIT_COST_KB` (512, anchored by the
  512 MiB soak's measured ~650 KiB/unit) would cross the 75% admission
  cutoff — a deliberate constant, since a post-crisis fragmented ratio
  would poison any self-calibrated estimate. This ships the inbound half
  of committed-work accounting that the listener comment had deferred.

### Recovery unblocked (run-8 findings)

- **Periodic malloc_trim on the measure cadence.** The event-based trim
  (sheds/deaths) never fires on a quiet post-crisis node, so glibc
  arenas' retained free pages measured as occupancy for hours — parking
  a 44-unit node at util 67% and starving it of abundance income.
  Trimming just before each measure keeps the reading honest
  continuously.
- **Abundance bonus floored at 1** under the rebound threshold: the
  pure linear fade rounded to zero just below 70%, a dead zone where an
  "abundant" host earned nothing and recovery stalled asymptotically
  (2 births in 3 hours).

### Abundance (the income half of energy-tracks-habitat)

- **The habitat feeds.** Run 7 showed rebound starving of capital: 3
  births in 3 hours — post-famine survivors are paupers, breeding costs
  ~1000 energy, and a scarcity colony has no income (GP is a net sink;
  passive regen is +1). Below the rebound threshold residents now earn
  extra regen in proportion to unused headroom (up to
  `ABUNDANCE_REGEN_MAX` = 9/tick on an empty host, fading to zero at
  70% util) — the symmetric other half of the famine tax, making energy
  an honest function of habitat state in both directions. The
  [70%, 80%) band is metabolically neutral and still.
- Starvation accounting moved after the LIVE phase (metabolize, live,
  then account): checking pre-LIVE let abundance income lift a runaway
  unit out of the floor zone before its burn, making unsustainable
  lifestyles immortal in a rich habitat.

### Rebound (famine's demographic other half)

- **Births into measured headroom.** After famine or emigration thins a
  host, nothing regrew the population — post-crisis colonies idled at a
  fraction of carrying capacity (a ~240-capacity node held 32 units for
  two hours). Now, comfortably under the ceiling (util < 70%, a stable
  band below famine's 80%), at most one birth per interval: the richest
  unit still holding reserves after paying SPAWN_COST + BIRTH_ENDOWMENT
  breeds; the child starts with exactly the parent's endowment (no
  energy minted) and inherits the parent's antibodies — birth passes
  immune knowledge down the way death bequeaths it sideways. Births are
  driven by *measured* util, so whatever heap fragmentation does to
  freed memory, regrowth stops when the measurement says the room is
  spent.
- `(node-status …)` gains `:births`; the accounting identity becomes
  `units == initial + in − out − deaths + births` (soak report and docs
  updated).
- Landed immigrants now get distinct rng streams too (seeded from the
  host's lifetime spawn counter) — a sink full of immigrants drew
  famine luck in lockstep, quietly resurrecting avalanche mortality.

### Famine hardening (scarcity soak runs 4–6)

- Per-unit foraging luck (50–150% of the tax, distinct rng stream per
  spawned unit) breaks avalanche mortality — max single-second deaths
  fell 102 → 26 and rationing settles instead of overshooting.
- Acute famine above util 96%: tax ×4, starvation fuse ×6 — death in
  ~10–20 s. The run-6 immigration sink survived the flood that killed
  a node in runs 3, 4, and 5; the colony finished with zero OOM kills,
  zero stale nodes, exact conservation, and verdict adaptive.
- Named open problem: post-crisis undershoot (fragmented RSS keeps
  util high after deaths; no rebound mechanism). See
  docs/validation.md.

### Soak findings (overnight, 2026-08-31)

- Three multi-hour colony runs established: per-unit memory saturates
  (~650 KB/unit; 8 h flat at 512 MiB/node), evolution is punctuated
  (bursts separated by hours of stasis, peak pinned at the GP search
  frontier), and under scarcity the colony sacrifices its most generous
  node — admission prices immigrants at arrival size, their post-landing
  growth arrives minutes later, and the organism has no way to shrink
  under memory pressure, so the kernel resolves overcommit by OOM-killing
  a node the chronicle still reports as alive. Findings recorded in
  docs/validation.md.
- `soak.sh report` now detects stale nodes (chronicle lagging the colony
  max by >300 ticks): `:stale yes` per node, `:stale-nodes`/`:stale-units`
  on the colony line, forced `:verdict casualty`, and
  `:balanced stale-ledger` — a dead third of the colony can no longer
  hide behind its frozen ledger. `SOAK_MEM` parameterizes node budgets.

### Fixed

- **The colony's attention was a monoculture.** Challenge selection sorted
  by reward descending, so every unit in the colony ground the single
  hardest rung (fib15) while trivially winnable rungs sat queued forever —
  and `attempts` was a dead field, so the monoculture wasn't even
  observable. Selection is now least-attempted-first (ties: reward desc,
  then id), each session records an attempt, and the per-challenge
  attention span dropped from 1000 generations to 100 so rotation actually
  happens. A local diversity rule; no coordination.

- **The landscape re-registered clones of known rungs.** Each solve of a
  related challenge generated another fib10-short9 / fib15 at escalated
  reward, forever — unbounded per-unit registry growth (a real slice of
  the memory-creep problem) and wasted attention on already-won names.
  Generation now dedupes by name against everything known.

- **The soak verdict measured the wrong fitness.** A healthy colony is
  always grinding something unsolved, so its CURRENT fitness reads low
  precisely when it is climbing; fit-gain now uses the PEAK. Conservation
  uses the event equation (`units == expected + landed − released`)
  instead of naive equality, so documented duplication is not flagged.

### Fixed

- **Word sharing transmits real code now.** `SHARE"`/`SHARE-ALL` sent an
  empty stub (`: NAME ;`) — the receiver compiled a no-op and the whole
  word-sharing story was silently fake (the operations.md example could
  never have worked). They now send the same re-evaluable decompiled
  source genomes and death-cries use.
- **Mate selection actually reads the inbox.** `select_mate_signaled`
  (COURT-weighted tournament) existed and was tested, but the live `MATE`
  path still called the unsignaled variant — README documented behavior
  the code didn't have. The call site is now wired.
- `--help` no longer claims `--multi-unit` always "runs a smoke demo and
  exits" — with `--port` it launches the persistent node.

### Documentation

- **Full docs audit against source (75 discrepancies found, all
  resolved).** Three wire-format shapes corrected in protocol.md
  (`evolve-share :challenge`, `challenge` head + fields, `solution`
  head + fields); signaling.md reframed as a historical design doc with
  an as-built divergence note (six words, costs 3/5, in-process-only
  delivery, MARK! sum-then-max semantics); formal-analysis.md gains a
  revision note (no step-throttling — mortality instead; third-order
  evolution ships; 2s heartbeats; bounded-k exists) plus corrected
  constants (tournament 4, vocab 31); self-replication.md's placement
  description now matches the two-tier abundant-emptiest rule and the
  admission margin + rate window; ARCHITECTURE.md drops stale line
  numbers, corrects the gossip-k default (off), and updates the
  failure-boundary text (migration and work re-recruitment exist);
  operations.md transcripts regenerated from the live binary (HEAL,
  DASHBOARD, GOAL{, SWARM-ON, FAMILY, SPAWN); words.md stack effects
  corrected (PEERS, ID, SEND, RECV, AUTO-SNAPSHOT, RESTORE, HEALTH,
  WATCH"); kernel/organism line counts and primitive counts honestly
  recomputed.

### Documentation

- **The harnesses are documented** — new [docs/validation.md](docs/validation.md):
  the drill scenario map S1–S8, netem and wedge semantics, the evolution
  soak, and the stable sexp surfaces they assert against.
  [docs/operations.md](docs/operations.md) gains the observability
  reference (`(node-status …)` field table with the unit-accounting
  equation, `RECRUITS-SEXP`). ARCHITECTURE's future-work list updated:
  per-unit time slicing shipped (LIVE budget + wall-clock slice); the
  remaining wall is mid-eval preemption. Test-count figures refreshed.

### The datapoint

- **First `:verdict adaptive` — the core thesis has executable evidence.**
  Soak round 7 (12 minutes, 900 units, 3 nodes): antibody kinds 1 → 17
  per node (+47 colony-wide), peak fitness 890 → 980 (the GP is now
  DISCOVERING two-token solutions, not replaying its seed), 4,519 antibody
  copies propagated, migrations event-conserved (`:balanced yes`), zero
  deaths. Open problems unchanged and stated: per-unit memory growth
  (nodes ride 97–100% util; the wall self-corrects but never rests) and
  the deeper GP search frontier beyond vocabulary-reachable rungs.

## [0.39.0] - 2026-09-01

### Fixed

- **The evolutionary ladder actually climbs now — three churn mechanisms
  found by the new soak harness, all fixed.** The core thesis (colony life
  produces adaptation, not churn) had no long-run evidence; the first soak
  said CHURN and gave reasons:
  1. **The default challenge lived outside the registry.** A fresh unit's
     GP fell back to an off-registry fib10; it WON every idle tick (fitness
     890 = the correct 11-token seed), but the whole winner path hides
     behind `active_challenge` — nothing installed, no landscape, and the
     next tick re-ran the same fallback forever. The default is now
     registered first, so the first win lays the ladder's first rung
     (SOL-FIB10 installs, the landscape generates fib15 / fib10-short9 /
     square-55 / an evolved rung).
  2. **A finished evolution state blocked re-initialization.** After a win,
     `evolution` stayed `Some(running=false)`, so every later GP-EVOLVE
     broke out immediately — a unit's evolutionary life was a permanent
     no-op after its first success. Finished states are now reaped so the
     next call takes the next unsolved rung.
  3. **`FitnessChallenge.max_steps` was decorative.** Nothing enforced it:
     an evolved looping candidate ran to the 10s sandbox wall clock — four
     orders of magnitude over budget; one bad mutant per generation cost
     10s (measured: 10.24s for a single GP-EVOLVE call; 47-second node
     ticks). Candidate evaluation now sets a hard VM step budget consumed
     in `execute_body` (exhaustion = timeout); two full GP-EVOLVE calls
     dropped to 0.24s and the whole test suite got 3× faster.

### Added

- **Per-tick LIVE budget with a wall-clock time box.** Unbudgeted, every
  idle unit evolved serially inside one tick; with real work on every unit
  a 300-unit node's tick ballooned to ~47s, degrading supervision and
  transport cadence 47×. At most `LIVE_BUDGET_PER_TICK` (16) units run per
  tick within a `LIVE_TICK_BUDGET_MS` (250ms) slice, rotating so every
  unit evolves in turn — tick latency stays a tick regardless of
  population or per-unit cost.

- **`docker/soak.sh` — the long-lived evolution harness.** `up` starts a
  balanced three-node colony (900 units), `report` (anytime) turns the
  chronicle into `(soak-node …)` / `(soak-colony … :verdict
  adaptive|partial|churn)` / `(soak-conservation …)` lines under stated
  evidence rules, `down` cleans up. Twelve-minute rounds drove all three
  fixes above; conservation self-audits exactly (900/900, out == in).

- **Drill S8 truth-aligned to the discovered dynamics.** With the colony
  genuinely alive, landed units GROW (evolution state), so a static
  equilibrium — everyone under the wall, shedding quiescent — does not
  exist; the wall is an ATTRACTOR with self-correcting transients. S8 now
  asserts what is true: every survivor keeps ticking under sustained
  pressure, any wall breach self-corrects (receiver alive and responding),
  and conservation holds with a traffic-scaled duplication bound. The
  static-equilibrium claim is retired to the open-problems list alongside
  its cause: **per-unit memory growth is unbounded** — the next honest
  target.

- **Chronicle carries evolution observables** (from 0.38.0's line): `:fit`
  now honestly reflects the ACTIVE frontier (a grinding colony reads low
  current fitness with a preserved peak), and `:sol-kinds`/`:sol-copies`
  separate what the colony knows from how far it spread. Soak round 6:
  copies saturate the colony while kinds hold at 1 — the remaining churn is
  a measured SEARCH-CAPACITY limit of the GP (vocabulary/operators/seeding
  could not find even the 2-token `55 .` for fib10-short9 in ~20k
  generations), not broken machinery.

## [0.38.0] - 2026-08-31

### Fixed

- **Receiver admission now digests before swallowing more (windowed
  admission cap).** Admission measured honestly at decision time, but the
  LOAD axis is a trailing average: landing a unit costs real CPU (fresh VM
  + prelude eval), so a multi-sender burst approved against a not-yet-risen
  loadavg pushed a small receiver past the wall AFTER acceptance — CI's
  2-cpu runners showed receiver samples at 87–93% util, then shed-back
  thrash that never quiesced and refusal/retry cycles that inflated
  duplication under packet loss (+13 observed). This is the burst-overshoot
  mechanism docs/self-replication.md predicted, caught in the wild by drill
  S8. The transport listener now caps accepts per rolling window
  (`ADMISSION_WINDOW_CAP` per `ADMISSION_WINDOW_MS`); beyond it, inbound
  transports are refused — and refused senders already stay put and retry,
  so no new protocol. Local rule, no coordination. With the cap: wall
  samples peak at 14–16, thrash gone, conservation exact (1600/1600 both
  netem legs).

- **Shedding units now physically relieves memory pressure.** Dropping a
  transported-out (or dead) unit freed its heap to the allocator, but glibc
  retained the pages: cgroup `memory.current` never fell, the placement
  rule saw no relief from migration, and an over-ceiling node shed its
  entire population without converging (drill S8: a 520-unit sender shed
  312 units and still measured 84%). `malloc_trim(0)` now runs after any
  tick that releases units — raw libc FFI, zero new dependencies, no-op
  off glibc.

- **An unreachable transport destination no longer starves the tick loop.**
  `send_transport` used a timeout-less `TcpStream::connect`; a BLACKHOLED
  peer (partition — no RST) still in the placement view blocked the tick
  loop on the OS SYN retry schedule (S8 measured 5 ticks in 25s). Bounded
  `connect_timeout` (2s); the established-connection handshake timeout is
  unchanged.

### Added

- **`(node-status …)` chronicle line** — one machine-readable S-expression
  per measure cadence from the persistent node: id, tick, units, util,
  headroom, cumulative out/in/deaths counters, plus evolution observables
  (`:fit` best GP fitness, `:sol-kinds` distinct antibodies known,
  `:sol-copies` installed copies — kinds measure what the colony knows,
  copies how widely it spread; their divergence over a long soak is an
  honest adaptation-vs-churn signal). Event-derived, so an
  external tool can account for every unit's whereabouts without parsing
  prose. The stable surface S8's assertions read.

- **Drill S8 — resource ecology under simultaneous pressure.** Three
  over-ceiling senders, two small receivers, a mid-shed receiver blackhole.
  Asserts, from chronicle lines alone: convergence (every survivor under
  the wall, shedding quiescent), receiver wall integrity under
  multi-sender bursts, tick liveness through the blackhole window, and
  colony-wide conservation (1600 ±6: loss bounded by the partition's
  in-flight window, duplication bounded the same way — the documented
  fail-toward-duplication was observed live at +1). Also: DRILL_ONLY=S8
  scenario filtering for development. See
  docs/self-replication.md "Multi-sender ecology validation".

## [0.37.0] - 2026-08-31

### Fixed

- **A healed partition is no longer permanent: seed peers are durable
  re-contact targets.** Peer-table entries — including the tentative
  entries `--peers`/`UNIT_PEERS` seeds start as — are evicted after 15s of
  silence, and heartbeats targeted only the live table. A partition
  outlasting the timeout on BOTH sides therefore emptied both tables and
  left no one heartbeating anyone: after the network healed, the mesh
  could never re-merge (permanent split-brain until restart). Found by
  the drill's new S7 partition scenario on CI, whose slower timing pushed
  the partition past both evictions. The original seed addresses are now
  retained for the mesh's lifetime and joined into every heartbeat's
  delivery targets (deduped; config-scale, so bounded-k stays bounded) —
  a node never forgets where home was, and a healed partition re-merges
  within one heartbeat interval. Unit-tested (empty-table targets = the
  seeds) and proven end-to-end by S7's post-heal round-trip.

- **A lost recruit datagram no longer guarantees abandonment.** Recruits are
  single UDP datagrams; if the one delivery to a chosen worker was lost (the
  Docker drill caught a just-booted worker missing it), the wedged-holder
  exclusion meant no re-delivery could ever happen — a healthy holder rode
  every fail-closed reset straight to the attempt cap. The fail-closed
  expiry now RE-SENDS the retained instruction to the still-live holder
  (no fresh bounty — the wage was paid at first emission). Idempotent by the
  existing first-write-wins rule: a worker that computes twice replies
  twice and the duplicate is dropped, proven by test.

### Added

- **Drill S7 — split-brain: partition, mutual bounded abandonment, clean
  heal.** `docker pause` freezes one node; a real partition is different —
  both sides stay alive, mutually evict each other, and each must bound
  the wait on slots the other holds. S7 partitions mid off the bridge
  network, has BOTH sides recruit into the partition, asserts both reach
  `err/abandoned` on the sexp surface within the terminal bound, then
  heals the network and proves clean re-merge: re-discovery, a fresh
  recruit round-trip collecting a real result, and exactly one
  abandonment per side (no double-settle across the heal). 49 checks
  total, green with and without netem.

- **CI builds the drill image once per run.** A `drill-image` job
  builds and `docker save`s the image as an artifact; both matrix legs
  `docker load` it instead of re-compiling the release binary — the
  duplicated in-Docker build is gone and the legs start immediately.

- **`RECRUITS-SEXP` — machine-readable recruit status (345 words).** One
  parseable `(recruit-slot :id … :seq … :holder … :state
  pending|unplaced|ok|err …)` per line, in the mesh's own notation. This is
  the STABLE surface for harnesses and tooling; `RECRUITS`' prose is now
  free to change. The Docker drill's slot-state assertions parse this
  instead of grepping prose — the class of false-positive that hit CI twice
  (chatter lines, wording drift) is gone by construction. Round-trip
  proven: every emitted line reparses through `sexp::parse` with hostile
  message content intact.

- **Drill S7-grade coverage for TRANSPORT (scenario S6): confirm-before-
  release across real containers.** A 160 MiB sender boots 900 in-process
  units (honestly OVER-CEILING on its cgroup budget from the first
  measure) and sheds toward a 1 GiB receiver over real TCP+UDP, netem
  included. Event-ordered, race-free invariants: an origin releases only
  after its copy landed (`accepted <= landed` — under loss the design
  fails toward duplication, never loss), and the receiver's own landing
  lines must self-consistently count `20 + landed`. This is the first CI
  coverage for the code most capable of losing something irreplaceable;
  previously TRANSPORT was validated only manually on droplets (v0.30).

- **Drill hygiene:** the last blind sleeps are gone — `poll_adv` waits for
  the actual advertised-headroom conditions the next step depends on;
  node-mode services are supported via `DRILL_ARGS`. 39 checks total,
  green with and without netem. (Node-mode note: `--peers` must be passed
  as a flag — the persistent node path does not read `UNIT_PEERS`.)

## [0.36.0] - 2026-08-30

### Added

- **Container awareness: honest resource advertisement under cgroup v2
  limits.** Inside a container, `/proc/meminfo` is the host's, so a limited
  unit advertised headroom it could not actually use — its cgroup limit
  would OOM-kill it long before the host filled (surfaced by the Docker
  wedge drill, where every container advertised the same host-wide figure).
  When a cgroup v2 memory limit is in force (`memory.max` is a number, not
  "max"), the memory axis now measures against the cgroup budget:
  `memory.max` as total, `memory.current − inactive_file` as used (the
  `docker stats` convention — reclaimable page cache is not pressure), and
  `memory.swap.max`/`memory.swap.current` as the swap axis. Unbudgeted swap
  ("max") counts `memory.swap.current` against the RAM budget instead —
  pages the kernel swapped out are still the container's weight; hiding
  them under-reported pressure by ~40 points in testing. The
  committed-work denominator (`measure_mem_budget_kb`) uses the same
  budget, so admission accounting agrees with what `measure()` reports.
  The load axis stays host-derived (there is no per-cgroup loadavg, and
  normalizing host load by a CPU quota would fabricate pressure); no limit
  → the plain `/proc` path, byte-for-byte as before. Zero new
  dependencies; 7 new fixture tests.

- **Drill scenario S5 — cgroup honesty across real containers.** Three
  cgroup-limited services (400 MiB boss and tight peers, 2 GiB roomy peer,
  `memswap_limit == mem_limit` so the combined-budget model pivots on RAM)
  prove the differentiation end-to-end on one host: with ballast, the
  tight peer honestly advertises ~14% headroom (insufficient) while the
  roomy peer advertises ~98% (abundant), and a boss over its own cgroup
  ceiling recruits its parallel parts to the roomy peer — never the tight
  one — and completes with real values. 32 checks total across the five
  scenarios, green with and without netem. This is the differentiated-
  headroom substrate the deferred placement-proportionality thread needs
  to be testable at all.

## [0.35.0] - 2026-08-28

### Fixed

- **Deep-tree deadline coverage: every wait in a recruit tree is now
  terminally bounded.** `execution_timeout` bounds local evaluation, and the
  supervision passes bounded each re-recruit *attempt* — but three waits had
  no total bound and one had no bound at all:
  1. Both fail-closed paths (no candidate with headroom) reset deadlines
     forever — bounded per-attempt, unbounded in total.
  2. Supervision was skipped entirely on an empty live-peer view — exactly
     the state after the only holding peer is SIGSTOPped past the 15s peer
     timeout and evicted, leaving its slots supervised by nothing.
  3. A job could never complete *with failure*, so abandonment had no way to
     flow upstream: a `Deferred` sub-recruit obligation (`report_targets`)
     could outlive every deadline, holding parent trees open forever.
  4. A part *declined* at emission (no capacity anywhere) left a pending job
     slot with **no ledger entry at all** — invisible to every pass, the one
     wait with no deadline whatsoever.

  The fix extends the validated deadline-reset + re-recruit mechanism rather
  than adding a new timeout system: a slot's total deadline expiries
  (reassignments + fail-closed resets) are capped by `MAX_SLOT_ATTEMPTS`
  (5); at the cap the slot is **abandoned fail-closed** — settled with an
  `abandoned` error, never a fabricated success — and the error envelope
  fills the local job slot through the same last-slot-fill completion path a
  real reply takes, so the failure self-reports up the tree level by level.
  Declined parts are now recorded as *unplaced* ledger slots supervised by a
  placement pass (re-recruited the moment capacity appears, abandoned at the
  same cap); supervision runs unconditionally, including on an empty view.
  Worst-case per-slot wall clock: `RECRUIT_TIMEOUT × (MAX_SLOT_ATTEMPTS+1)`,
  at every depth. No worker is ever killed (the v0.32.0 lesson): a late
  reply from an abandoned slot's worker remains a first-write-wins dropped
  duplicate, proven by test.

  Validated three ways: five in-process deep-tree tests (wedged leaf three
  levels down, Deferred stall, empty-view eviction, late-reply accounting,
  capacity-arrives-mid-wait recovery); `tests/deep_tree_test.sh` driving
  real processes with real SIGSTOP wedges end-to-end (8 checks, including
  parent responsiveness after abandonment and worker survival after
  SIGCONT); and the full suite. `UNIT_RECRUIT_TIMEOUT_SECS` overrides the
  60s timeout for wedge drills; production defaults unchanged.

- **The version banner can no longer drift from the released version.**
  v0.34.0 shipped announcing itself as `unit v0.33.0`: the release sweep
  bumped every `.rs`/`.toml`/`.html` version string, but the REPL banner
  lives in `prelude.fs` — in the organism's own language — and was missed.
  Both banners (prelude and CLI) now derive from `CARGO_PKG_VERSION` at
  compile time (`{{VERSION}}` substitution in `load_prelude`).

## [0.34.0] - 2026-08-18

### Fixed

- **Hardened the untrusted-input surface against panics (three fixes found by
  fuzzing).** A self-replicating unit ingests data it did not author — mesh
  S-expressions, replication packages, snapshot blobs — so a panic there is a
  crash/DoS vector:
  - **Mesh DoS:** the S-expression parser (`sexp::parse`) recursed with no
    depth bound, so a peer could send deeply nested `(((…` and overflow the
    receiver's stack (an uncatchable abort). Added a 256-level depth cap;
    over-deep input now returns a parse error.
  - **`ALLOT` overflow:** `here + n` for an untrusted/negative cell count could
    overflow (debug panic; release wrap). Now uses `checked_add` and rejects.
  - **`unpack_package` overflow:** the three section lengths in a UREP header
    come off the wire; their sum could overflow past the length check into an
    out-of-bounds slice. Now summed with `checked_add`.

### Added

- **`fuzz_tests.rs` — a zero-dependency fuzz/property harness** guarding the
  invariant *no input may panic the VM*. Hand-rolled deterministic PRNG and
  grammar-aware generators drive the Forth interpreter, the S-expression
  parser/evaluator, `unpack_package`, and `deserialize_snapshot` through
  `catch_unwind`; deterministic seeds make any failure reproducible. Runs in
  the normal `cargo test` (30k iterations/target) as a permanent regression
  guard. These tests are what surfaced the three fixes above.

- **The energy economy: flows instead of faucets.** Two conserved energy
  flows join the minted rewards:
  - **`GIVE ( n -- )` — the gift.** Donate up to 500 energy; the host
    routes it to the lowest-energy sibling. Exactly conserved with
    friction: donor spends n+1, recipient earns n, 1 dissipates (so gift
    ping-pong decays rather than cycling). A lone unit's undeliverable
    gift returns minus friction. Like SAY!/TRANSPORT it is unit-invoked
    and GP-mutable — generosity is a life strategy a lineage can evolve
    into its `LIVE`. Signal routing is now wired into the node tick, which
    also makes SAY!/MARK! actually flow between siblings in node mode
    (previously only exercised in tests). 344 words.
  - **Recruit `:bounty` — the wage.** A recruiter attaches 10 energy to
    each `(recruit …)` when it can afford it, spending it at send; the
    worker earns it on completing the work (`RecruitOutcome::Reply` —
    a unit that merely delegated earns nothing). Acceptance is capped at
    50 per message: recruit datagrams are unauthenticated, so a forged
    flood can't hyper-inflate. A broke recruiter recruits at bounty 0 and
    the work may still be served — the wage is an incentive selection can
    act on, not a mandate.

- **`LIVE` — the life loop is now genome.** What a unit does with an idle
  tick was host policy (`GP-EVOLVE`, hard-coded in the multi-unit tick);
  it is now a prelude word the host *calls* and the dictionary *defines*:
  `: LIVE GP-EVOLVE ;`. Redefine it and the unit's habits change — and
  because it is an ordinary word, a life strategy is heritable (SPAWN),
  shareable (SHARE-ALL), and mutable (SMART-MUTATE). The metabolic meter
  is the safety rail that makes this evolvable: a `LIVE` that loops
  forever starves within the tick instead of hanging the host; a `LIVE`
  that does nothing stops improving. Selection handles both. Default
  behavior is unchanged (`LIVE` = `GP-EVOLVE`). 343 words.

- **Mortality: sustained starvation is death, and death bequeaths.** A
  unit pinned at the energy hard floor for 30 consecutive ticks — the
  signature of an unsustainable life strategy, distinct from ordinary GP
  debt, which hovers above the floor and never kills — dies and is removed
  from its host. Its final act is a **death-cry**: its `SOL-*` antibodies
  (immune memory) go to local siblings directly and to the mesh as a
  `(death-cry …)` message. Receipt is trust-gated twice (parse layer and
  absorb layer): `SOL-*` names only, never overwriting an existing word,
  bounded name/source sizes — so a forged death-cry cannot install or
  redefine behavior (`LIVE` included). The failed strategy dies with the
  unit; the solved-challenge knowledge survives it. Node logs show
  obituaries (`DIED gen=… bequeathed …`) and scavenges (`SCAVENGED …`).

### Changed

- **The genome snapshot words got format-neutral canonical names:**
  `GENOME-SAVE`, `GENOME-LOAD`, and `GENOMES`. The `JSON-SNAPSHOT` /
  `JSON-RESTORE` / `JSON-SNAPSHOTS` names — from the era when the snapshot
  format was JSON — remain registered as working aliases, so nothing
  breaks. `HELP-PERSIST` teaches the new names. Live dictionary: 342 words.

- **Metabolism now prices thinking itself: execution costs energy.** The
  inner interpreter charges 1 energy per 10,000 VM steps of top-level
  execution, so a runaway loop (`BEGIN 0 UNTIL`) starves to death — a clean
  `starved: out of energy — execution halted` — instead of hanging the
  organism forever. Starving units still limp: short lines never reach a
  metering checkpoint, the halt clears on the next top-level line, and
  energy regeneration restores full function. Sandboxed evaluation (GP
  candidates, remote goals) is exempt — it is deadline-bounded and priced
  per-generation, so colony economics are unchanged. Alongside the meter,
  nested execution now has a depth wall (2,000 bodies), turning a recursion
  bomb (`: R RECURSE ; R`) from an uncatchable Rust stack-overflow abort
  into a clean Forth error.

  Metering made the previously-unfuzzable vocabulary fuzzable (loops,
  definitions, recursion now provably terminate), and the new
  full-vocabulary fuzz target immediately caught three latent VM panics,
  all fixed: unbalanced control flow (`ELSE`/`THEN`/`REPEAT` popping a
  fixup index off an rstack that runtime `DO` also writes) indexed the
  definition body out of bounds; branch-offset arithmetic could overflow on
  garbage offsets; and `RECURSE` inside an anonymous `DO` body compiled a
  dangling self-call to a dictionary slot that is never defined. 495 tests.

- **Genome snapshots are now S-expressions (the mesh's own notation), not
  JSON.** `HIBERNATE` / `JSON-SNAPSHOT` / auto-snapshot write
  `~/.unit/snapshots/<id>.sexp` in a `(unit-snapshot :version 2 …)` format
  that parses with the same `sexp::parse` the mesh uses — a genome on disk is
  a valid mesh expression, so hibernation and transport share one notation,
  and any species (the Go and Python organisms already carry sexp parsers)
  can read another's genome. Legacy JSON snapshots still resurrect: the
  loader sniffs the first byte (`(` vs `{`) and falls back to `<id>.json`;
  old state converts to sexp on its next save. The word names (`JSON-*`) are
  unchanged for compatibility. Also fixed a latent wire-format bug found
  while building this: the sexp parser rebuilt string payloads byte-by-byte,
  mangling multi-byte UTF-8 (`"héllo"` → `"hÃ©llo"`); it now iterates chars,
  so genomes and mesh strings round-trip byte-exact. Verified end-to-end:
  hibernate → resurrect in the new format, planted legacy-JSON migration, and
  two new fuzz targets (`genome from_str`, mutated-word round-trip) alongside
  9 new unit tests; 487 tests total, zero dependencies.

- **Split the monolithic `impl VM` into a `words` module tree.** Every
  primitive word (`prim_*`, `do_*`, `rt_*`, and their helpers) previously
  lived in one ~4,600-line `impl VM` block in `main.rs`. They are now grouped
  by concern into 18 submodules under `src/words/` (mesh, immune, evolution,
  goals, io, spawn, persistence, …), each contributing `impl VM` methods that
  Rust applies to the single `VM` type. `main.rs` drops from 6,526 to ~1,900
  lines. Methods are `pub(crate)` so the opcode dispatch in `vm/mod.rs` still
  reaches them across the module boundary. Pure reorganization — no behavior,
  wire-protocol, or word-set change; all 470 tests pass, `cargo clippy` is
  warning-free, and the zero-dependency invariant is unchanged.
- **Split the rest of `main.rs` into focused modules.** The remaining tail —
  the benchmark harness, the multi-unit runtime (smoke demo + resource-aware
  node loop), the REPL loop, and CLI parsing — moved out of `main.rs` into
  `bench.rs`, `node.rs`, `repl.rs`, and `cli.rs`. `main.rs` is now 458 lines:
  module wiring plus `fn main()`. `bench`/`node` are native-only and cfg'd out
  on wasm. Same pure-reorganization guarantees: default/http/wasm all build,
  clippy clean, 470 tests pass, behavior verified end-to-end (REPL, `--eval`,
  `--bench`, `--multi-unit`), zero dependencies.

### Documentation

- Refreshed stale figures across README and CONTRIBUTING to match the current
  tree: native binary ~1.2 MB → ~1.5 MB, WASM ~338 KB → ~425 KB, Rust test
  count 255+ → 470+, and the live dictionary size 315/316 → 339 words. Added
  `src/words/` to the README architecture map.
- Completed `docs/words.md`: all 339 live-dictionary words are now tabulated
  (previously ~230). Added the mesh/goals/monitoring/spawn ops words with
  stack effects verified against their `prim_*` implementations, plus new
  sections for the WebSocket bridge, resource-load generator, and the
  prelude-defined colony/persona and self-programming vocabularies.

## [0.33.0] - 2026-06-10

The published 0.32.0 shipped the distributed work-execution *design*; 0.33.0
is the week of hardware sessions that made it real. The recruit/supervision/
timeout machinery is now **hardware-validated end to end on a 3-node mesh**
(DigitalOcean 512MB+2GB-swap droplets): first inter-machine recruits ever,
placement by honest headroom, full wedge recovery with zero operator input —
final slot line `ok … (re-recruited 2x) (deadline reset 4x)`.

Note: 0.32.0 has been yanked from crates.io — its bare `KILL-CHILD` could
signal arbitrary host pids (or the process group on an empty stack), and its
headline distributed features were inert on real hardware. 0.33.0 supersedes
it.

### Distributed execution actually fires on real hardware

- **Headroom advertisement fix.** Single-unit hosts never called
  `set_headroom`, gossiping the fail-closed 0 forever — no peer ever saw
  room, so recruits, placement, and replication-toward-peers could not fire
  across machines. The mesh heartbeat thread now takes a fresh
  `HostResources` measurement every beat; an explicit `set_headroom` still
  takes authority (multi-unit host, tests).
- **Timer-driven tick loop.** Every VM-side periodic duty was gated on stdin
  input; an idle node never evaluated recruited work, never ran supervision,
  never accepted replications, and had frozen metabolism. The REPL now ticks
  every 250ms while idle (stdin moved to a reader thread; EOF/piped-input
  semantics unchanged) — workers and supervisors are autonomous.

### Work-execution model completed

- **Job timeout for wedged peers** (`RECRUIT_TIMEOUT`, 60s per assignment):
  re-recruits an alive-but-silent holder through the same placement path as
  gossip-death, excluding the wedged holder from candidates; fail-closed when
  no candidate exists. At-least-once execution with first-write-wins
  collection — replies are judged by `(goal_id, seq)` identity, not sender;
  duplicates drop silently.
- **Fail-closed observability.** Every fail-closed expiry logs `timeout
  expired, no candidate with headroom — deadline reset (Nx)`, and RECRUITS
  renders `(re-recruited Nx) (deadline reset Nx)` — "failing closed every
  60s" and "not firing at all" are no longer indistinguishable.
- **Nested-result settlement.** A recruited `(parallel …)` subtree's reply
  now settles its ledger slot (`settle_nested`); previously it stayed open
  forever — pending in RECRUITS, and eligible for re-recruit of
  already-completed work.

### Safety and correctness

- **SPAWN-N / KILL-CHILD validate their argument.** A bare KILL-CHILD once
  SIGTERM'd an arbitrary host process; an empty stack would have meant
  `kill(0, SIGTERM)` — the entire process group. Both words now fail clean on
  underflow, and KILL-CHILD refuses any pid not in this node's children
  ledger — never signal a process we didn't spawn.
- **Ghost self-peer fix.** Peers gossip your public address back to you; the
  loopback-only self-check admitted it as a pseudo-id peer (`…ffff`).
  `is_self_addr` now recognizes the host's own interface addresses (zero-dep
  route-source trick) in both the gossip loop and the seed list.
- **GP-EVOLVE tick report** reads `evolution.best.fitness` instead of the
  mesh fitness ledger — no more "best fitness 0" while evolution progresses.

### Operator experience

- STATUS labels the legacy `load:` metric for what it is (dictionary words /
  unit count — not host resources) and shows advertised headroom for self and
  every peer; the REPL redraws its prompt after asynchronous tick output.

### Packaging

- `web/unit.wasm` is untracked (CI builds the deploy copy; `just wasm` stages
  one locally); `build.rs` stages it into OUT_DIR with a 0-byte stub fallback
  — a stub build answers `/unit.wasm` with a clear 404. Makefile folded into
  the justfile (`just smoke`).

## [0.32.0] - 2026-06-09

The step-2 recruit tree from the work-execution-model design record
([docs/design/work-execution-model.md](docs/design/work-execution-model.md),
recorded in f9294d8) — distributed work that fans out as a tree and reports back
up — plus a resource model for operating on genuinely restricted RAM+swap boxes.

### Distributed work execution (the recruit tree)

- **S-expression eval seam with structured runtime faults.** A canonical
  `eval_sexp` seam (`SEXP-EVAL"` at the REPL) parses an s-expression instruction,
  evaluates it in the Forth VM, and returns a `(result :ok …)` envelope. `pop`/
  `rpop` now raise a structured `Fault` (StackUnderflow / ReturnStackUnderflow)
  instead of only printing, so a failed evaluation surfaces as `:ok 0 :error …
  :kind runtime` rather than being silently swallowed. The reply path preserves
  success/error end to end.
- **Recruit / recruit-result mesh pair and the mechanical recruiter.** A
  `(recruit …)` / `(recruit-result …)` message pair carries an s-expr instruction
  to a peer and the canonical result envelope back, nested under the routing
  fields. `RecruitLedger` tracks outstanding/collected round-trips; `send_recruit`
  emits; `RECRUIT"` / `RECRUITS` are the manual trigger and viewer. Failure is
  visible end to end through the recruiter, not just the worker.
- **`(parallel …)` split-and-recruit decision on local resource pressure.** A unit
  handed `(parallel (e1) (e2) …)` runs each sub-part locally while it has headroom
  under the ceiling and recruits the overflow to a placement-chosen peer — a
  reactive, measured decision (not predictive). Results are *collected* into an
  ordered `(parallel-result …)`, deliberately not combined.
- **Recursive fan-out, ceiling-bounded, no depth cap.** A recruited `(parallel …)`
  re-applies the same split-and-recruit decision on the recruited peer, so work
  fans out as a tree. There is no recursion-depth limit by design: fan-out is
  bounded only by the resource ceiling — a peer recruits a part only when it lacks
  headroom *and* placement finds a peer that has it, so a saturated mesh stops the
  tree growing. The per-level ceiling check is the brake.
- **Report-once-when-complete result propagation (fan-in).** Each unit
  self-reports its complete result to whoever recruited it, once, when whole; a
  parent fills its slot and, when its last slot fills, reports up to its own
  recruiter. The root surfaces the whole answer. Results are immutable/settled —
  report once, no streaming partials. No coordinator: each unit holds only its own
  expectations (back-references) and obligations (ledger slots).
- **Let-it-crash supervision via gossip-death.** When a peer holding an open
  recruit slot disappears from the mesh's pruned peer view (the existing
  `PEER_TIMEOUT` signal), the parent re-recruits that slot's retained instruction
  to a different peer with headroom; if none is available the slot stays
  open/declined (fail-closed). Supervision nests up the tree by the same
  mechanism. Alive-but-wedged peers (a job-level timeout) are deferred.

### Resource model for restricted-resource operation

- **`(alloc-mb N)` gated memory-pressure load generator + `RECLAIM-MB`.** Allocates
  and retains N MiB of real, resident process memory to drive measured memory
  utilization (the instantaneous axis); `RECLAIM-MB` frees it. Off by default
  behind `ALLOC-ENABLE` and kept out of the GP-reachable surface like `SHELL"`, so
  any ceiling-crossing is a deliberate `(alloc-mb)` and never evolved code.
- **Combined RAM + swap memory budget.** `mem_fraction = (ram_used + swap_used) /
  (ram_total + swap_total)`: swap is treated uniformly as capacity (a page is a
  page whether in RAM or swap), and the 0.80 ceiling applies to the combined
  budget. Reduces exactly to prior behavior when there is no swap. Counts swap as
  capacity for survival/correctness, not performance.
- **Committed-work accounting in `run_parallel` admission.** A per-call tally of
  work just committed locally is added to the observed reading before each part's
  admission check, so the node counts what it already decided this call — defeating
  the `measure()` lag (loadavg averaging + swap absorption). Per-call scratch only;
  it never persists across calls or ticks.
- **Memory-leaning advertised headroom.** When a box is meaningfully leaning on
  swap, the memory axis binds so swap-I/O load doesn't double-penalize a
  memory-bound peer (the swapped pages are already counted in `mem_fraction`); the
  load axis still binds for genuine CPU load with no/incidental swap. `CEILING`
  and `ADMISSION_MARGIN` unchanged; survival preserved.

### Validated on hardware

- **Committed-work accounting** was confirmed on three 456 MB-RAM + 2 GB-swap
  SFO3 droplets: with the gate enabled, `(parallel (alloc-mb 400) ×5)` correctly
  ran three parts locally and declined the overflow rather than running all five
  blind — the behavior the accounting was added to produce.
- **The recruit tree was NOT witnessed landing a live cross-mesh recruit this
  cycle.** It is covered by 442 passing tests, and the message-pair / decision /
  dispatch paths were verified by code inspection. On the test droplets a resident
  GP-EVOLVE colony kept every peer CPU-saturated, so no peer ever advertised
  headroom and no recruit was ever placed. That is the correct emergent brake (a
  saturated mesh refuses to fan out), but it also prevented a live recruit
  demonstration. This release does **not** claim hardware validation of the
  recruit path — unlike v0.31, whose headline admission-margin feature was
  hardware-witnessed.

### Known limitations

- Per-part admission still reads signals that lag a fast burst on the **load
  axis** (loadavg is a 1-minute average). The committed-work tally addresses the
  within-call case; the **cross-tick inbound-burst gap (#16)** and a general,
  node-level committed-work model remain deferred.
- Result combination (reducing a `(parallel-result …)`'s collected envelopes) and
  streaming partials are out of scope; results are collected, not combined.

### Changed

- VERSION → v0.32.0; prelude banner and web demo title/cache-bust updated.

### Design principles held

- **No central coordinator.** The recruit tree has no scheduler, master, or
  control plane: each unit decides from its own measured pressure and gossiped
  view, holds only its own back-references and ledger slots, and the supervision
  tree emerges from the recruitment structure rather than being designed apart.
- **Fail closed; the ceiling is a refusal wall, not a target.** No peer with
  headroom ⇒ no recruit; an unmeasurable host ⇒ no headroom; a dead peer's slot
  re-recruits or stays declined. Fan-out is bounded by the ceiling at every level,
  not a depth cap.
- **Zero new dependencies.** Cargo.lock still contains only the `unit` crate.

## [0.31.0] - 2026-06-05

Three fixes for the failure modes the v0.30 multi-machine soak surfaced once a
persistent node ran on real hardware: load skew onto the first adequate peer, a
correlated thundering-herd when several senders shed at once, and transient
overshoot of the 80% ceiling when a burst of transports lands under gossip lag.
The 80% wall already held as a hard refusal; v0.31 keeps a receiver from being
pushed up to it in the first place and spreads shed load more evenly. See
[docs/self-replication.md](docs/self-replication.md#multi-machine-validation-v031).

### Fixed
- **Inbound admission margin.** A receiver UNDER the ceiling could still be pushed
  OVER it by a burst: several senders all act on the same stale "has room" gossip,
  all transport within one window, and admission is one-frame-at-a-time with no
  view of in-flight (or just-accepted, not-yet-instantiated) inbound — so the wall
  held but overshot transiently for a tick before the next frame was refused. New
  `HostResources::has_admission_headroom()` accepts inbound only while utilization
  is below `CEILING - ADMISSION_MARGIN` (margin = 0.05), not merely below the
  ceiling; that slack absorbs a burst's in-flight units a fresh `measure()` can't
  yet see (accepted snapshots sit in the channel until the main loop instantiates
  them). `handle_transport_frame` uses this stricter gate for ACCEPTING inbound,
  while the host's own replication / mislocation decisions (`can_spawn_within`,
  `is_mislocated`) still use the full-ceiling `has_headroom()` — the two are
  deliberately not conflated, since a host can be content to keep its own units yet
  decline to accept more. Fail-closed and confirm-before-release are intact: an
  unavailable reading still refuses, and a margin refusal still echoes the node_id
  with `Refused`, so the sender gets `Err` and keeps its unit.
- **Two-tier placement.** Pure sufficient-first placement concentrated load onto
  the first adequate peer — one peer climbed to its ceiling while another sat at
  ~73% headroom nearly untouched; the skew self-corrected (the first peer fills,
  walls, and relays onward) but slowly and unevenly. A second threshold,
  `ABUNDANT_HEADROOM_PCT` (50%, above the ~20% sufficiency bar), makes
  `choose_destination` two-tier: if any peer is abundantly free, pick the emptiest
  such peer (spread toward a clearly-emptier home); otherwise fall back to the
  original first-sufficient rule (frugal, herd-avoiding). It only chases the
  emptiest peer when one has slack to absorb a spread without itself crowding;
  under light/normal load it stays first-sufficient exactly as before. Both
  thresholds (`headroom_pct_sufficient`, `headroom_pct_abundant`) live in
  `resources.rs` as the single source of truth, mirrored by the pure
  `transport::choose_destination` and the node-side `MultiUnitNode::choose_destination`.
- **Randomized tie-break.** Two-tier's tier 1 picks the emptiest abundant peer,
  but when several peers tie at the maximum headroom a deterministic tie-break made
  multiple senders shedding at the same instant — sharing the same abundant gossip
  view — all pick the SAME peer: the correlated mini-thundering-herd two-tier
  placement is meant to prevent (and gossip order is too arbitrary to spread them
  reliably). `choose_destination` now picks uniformly at random among the
  tied-maximum peers via a one-pass reservoir sample over the existing zero-dep
  `SimpleRng`, each node/unit seeding from its own identity so concurrent senders
  draw independent picks and spread across the tied set; a unique maximum is still
  chosen deterministically. `MultiUnitNode` now delegates to the pure
  `choose_destination` (a true single source of truth) rather than carrying its
  own copy.

### Validated on hardware
- Three DigitalOcean droplets (SFO3, 512 MB, Ubuntu 25.10, source builds). A
  receiver parked at 76.7–79.2% — UNDER the 80% ceiling but inside the admission
  margin — refused a single over-ceiling sender with `destination refused (no
  headroom)`, and held under a 2-sender burst with utilization never crossing 80%.
  The margin kept the receiver off the wall, rather than letting a burst push it
  past and relying on the next frame's refusal to claw it back.

### Known limitations
- **Just-accepted, not-yet-instantiated inbound is not yet counted as load.**
  Accepted unit snapshots sit in the channel until the main loop instantiates them,
  so a fresh `measure()` cannot see them; the admission margin's slack absorbs this
  in practice, but counting in-flight inbound directly — Part 2 of the admission
  work — is left as a documented TODO in the listener. It needs a per-unit-footprint
  estimate that is easy to get wrong, and the margin alone is the meaningful fix.

### Changed
- VERSION → v0.31.0; prelude banner and web demo title/cache-bust updated.

### Design principles held
- **Admission and replication are separate decisions.** Accepting inbound uses the
  stricter `has_admission_headroom`; the host's own replication still uses the
  full-ceiling `has_headroom`. A host may keep its own units while declining more.
- **Confirm before release; honesty selected, not policed; fail closed; 80% is a
  refusal wall, not a target; no central coordinator.** All carried unchanged from
  v0.30 — each node still decides from its own gossiped view and its own measured
  pressure.
- **Zero new dependencies.** Cargo.lock still contains only the `unit` crate.

## [0.30.0] - 2026-06-02

The v0.29 resource-aware self-replication surface, now driven by a persistent
run loop and validated on real multi-machine hardware. `unit --multi-unit N
--port P --peers ...` is no longer a 5-second discovery demo — it is a living
node that ticks the full v0.29 behavior until killed. See
[docs/self-replication.md](docs/self-replication.md#multi-machine-validation-v030).

### Added
- **Persistent resource-aware run loop** (`run_multi_unit_node`, replacing the
  old `run_multi_unit_mesh_demo`). After the startup/discovery phase it ticks on
  a steady ~1s interval until SIGINT/SIGTERM (handled via a zero-dependency raw
  `signal(2)` FFI binding) requests a clean shutdown. Each tick: drains and
  dispatches inbound mesh work; advances every unit's metabolism; runs each
  unworked unit through one bounded `GP-EVOLVE` step; periodically measures
  `HostResources` and re-advertises real headroom on the heartbeat; and runs the
  local placement rule — over the 80% ceiling it senses mislocation, chooses a
  sufficient-first peer from its gossiped view, and transports a unit with
  confirm-before-release. The per-tick logic is factored into
  `MultiUnitNode::tick`, unit-tested without sockets or sleeps.
- **Inbound transport landing.** The node binds the transport TCP listener
  (mesh port + 2000) and services it each tick: a received self is instantiated
  as a live unit (full dictionary incl. evolved `SOL-*` antibodies, memory,
  fitness, goals, code_strings) and resumes evolving.
- **Timestamped one-line-per-event logging** (UTC `HH:MM:SS`, zero-dep) for
  live-tailing on real boxes: `RES` (binding-constraint utilization, mem%,
  load-per-cpu, headroom, UNDER/OVER-ceiling, unit count, RSS), `EVOLVE`,
  `PEERS` (logged only on change), `MISLOCATED` (on crossing the ceiling), and
  `TRANSPORT accepted/refused`.
- **First multi-machine validation** — three DigitalOcean droplets (SFO3, 512 MB,
  Ubuntu 24.04, source builds). A 2000-unit colony read 86.4% memory utilization
  OVER-CEILING, sensed itself mislocated, and drained one-unit-per-tick toward
  two peers at ~73% headroom with confirm-before-release holding across real
  UDP/TCP (no unit lost in transit). The receiving box's unit count rose (3→8)
  and arrived units resumed evolving. The overloaded box honestly gossiped its
  falling headroom (to 14%) and peers correctly stopped choosing it — honesty
  selected, not policed.

### Fixed
- **Cross-machine bind bug** (surfaced only by real multi-machine testing): the
  mesh UDP gossip socket and the transport TCP listener bound to `127.0.0.1`,
  which silently prevented all cross-machine operation — a loopback-bound socket
  never receives datagrams destined for the host's routable IP. It went unnoticed
  because `--peers` seed entries populate the peer table at startup and survived
  the old 5-second demo (shorter than the 15s peer timeout), so discovery *looked*
  fine. Both peer-traffic sockets now bind `0.0.0.0`. Left loopback by design:
  the HTTP bridge (`--serve`, localhost-only for safety), the legacy UREP repl
  listener, and the discovery beacon self-ping.
- **Stack-underflow log flood.** The core stack ops (`vm/primitives.rs`) and
  `SAY!`/`MARK!` printed "stack underflow" via raw `eprintln!`, bypassing the
  `silent` flag. Sandboxed GP candidate evaluation runs many mutated programs
  that underflow, which flooded stderr and drowned the run loop's logs; these
  are now gated behind `!silent`.

### Changed
- `--gossip-k` bounded fan-out is now honored on the `--multi-unit --port` path
  (the old demo ignored it).
- VERSION → v0.30.0; prelude banner and web demo title/cache-bust updated.

### Design principles held
- **No central coordinator.** The node runs the local rule on a tick, but each
  node decides from its own gossiped view and its own measured pressure; nothing
  orchestrates placement across the mesh.
- **Confirm before release; honesty selected, not policed; fail closed; 80% is a
  refusal wall, not a target; a unit with no work evolves; the complete self
  transports.** All carried unchanged from v0.29 — now observed on hardware.
- **Zero new dependencies.** Cargo.lock still contains only the `unit` crate.

## [0.29.0] - 2026-06-02

Resource-aware self-replication: a unit senses its host's load, refuses to grow past a wall, and can relocate itself to another coordinate that has room — choosing frugally and never giving up its only copy until a live copy is confirmed elsewhere. See [docs/self-replication.md](docs/self-replication.md) for the full arc and the principles it holds.

### Added
- `src/resources.rs` — a zero-dependency host resource reader. On Linux it reads `/proc/meminfo` (MemTotal, MemAvailable), `/proc/loadavg` (1-minute), and the logical CPU count (`/proc/cpuinfo`, falling back to `/proc/stat`). `HostResources::measure()` returns a clearly-marked **unavailable** reading on non-Linux / wasm32 rather than guessing. Utilization is the **binding constraint** — `max(memory_fraction, load_one / n_cpus)` — so whichever resource is tightest sets the pressure; `headroom = 1 - utilization`.
- **The 80% ceiling.** `CEILING_UTILIZATION = 0.80` is the single source of truth, and its only role is **refusal**: the colony never grows *toward* it. `HostResources::has_headroom()` is the gate (`valid && utilization < CEILING`) and **fails closed** — an unavailable reading returns false, because a coordinate that can't measure itself must not replicate.
- `SpawnState::can_spawn_within(&res)` layers the ceiling refusal on top of the existing quarantine / max_children / cooldown guards (none removed); the real `SPAWN` path now gates on it, so spawning refuses at/over 80% and on unmeasurable hosts.
- **Emergent local replication rule.** `MultiUnitHost::senses_unmet_demand()` (work waiting AND every unit busy) + `replication_decision()` (replicate iff demand ∧ headroom). There is no coordinator, quorum, global counter, or target population — minimum-sufficient population is emergent from this local rule plus energy metabolism. A unit with no work falls through to `GP-EVOLVE` (`evolve_one_unworked()`) rather than sitting idle; surplus self-resolves through starvation, with no reclaim/cull logic.
- `src/transport.rs` — unit self-transport with **confirm-before-release** ("transporter") semantics. The complete self travels as a serialized `VmSnapshot` (USAV: dictionary incl. evolved `SOL-*` antibodies, memory, goals, fitness, code_strings); the binary and prelude do **not** travel — every coordinate already has them, so the receiving unit process is the transporter pad. Length-prefixed TCP framing in the style of `spawn.rs` (`UTPT` transport frame, `UTPC` confirm frame); never on the UDP gossip wire. The destination refuses without headroom (fail closed) and echoes an accepted/refused confirm. The origin releases **only** on `Ok(Accepted)` — a refused / timed-out / malformed / absent confirm leaves it alive exactly as it was. No unit is ever lost in transit.
- **Sufficient-first placement.** Heartbeats now gossip a peer's advertised headroom (a single `0..=100` byte, appended after fitness, backward-compatible). `choose_destination()` returns the **first** peer that advertises sufficient room — not the emptiest — which is frugal, mirrors minimum-sufficient, and avoids a thundering herd. A coordinate is "mislocated" when its own `has_headroom()` is false; that local pressure is the honest trigger.
- **`TRANSPORT` Forth word** — unit-invoked and GP-mutable like `COURT`/`SAY!`, **not** a host-driven scheduler. Calling it senses local mislocation → chooses a sufficient-first destination → relocates with confirm-before-release; not mislocated or no sufficient destination is a safe no-op. `TRANSPORT_COST = 150` (full self-replication, just below `SPAWN_COST` since no binary travels), charged with no-op-on-starve semantics like `SAY!`: a starving unit cannot flee — which is metabolically honest.
- 62 new tests across resources, spawn, transport, mesh gossip round-trip, node placement, and the `TRANSPORT` word. Total native test count: 363.

### Changed
- `src/mesh.rs`: `PeerInfo` and `MeshState` carry a `headroom` byte; `MeshNode::set_headroom` / `peer_resource_view` surface it. The heartbeat wire gains one trailing byte; older peers that omit it are read as headroom 0 (fail closed).
- `VM` gains a `transported_out` flag, set after a confirmed self-transport so a host/main loop can reap the released origin.
- VERSION → v0.29.0; banner and web demo title updated.

### Design principles held
- **Honesty is selected, not enforced.** Placement trusts a peer's advertised headroom. A peer that lied refuses at the transport layer, the origin stays put, and that is the whole consequence — no detection, no flag, no blacklist.
- **80% is a refusal wall, not a target.** Nothing anywhere grows toward it or steers to it.
- **No coordinator.** Each unit reads only its own gossiped view and runs the local rule; there is no global aggregation, scheduler, or population target.
- **Confirm before release.** A copy is given up only against a confirmed-living copy, so no unit is lost in transit.
- **Fail closed.** A coordinate that cannot measure its own resources neither replicates nor accepts a transport.
- **Zero new dependencies.** Cargo.lock still contains only the `unit` crate.

## [0.28.0] - 2026-04-28

### Added
- Inter-unit signaling substrate (docs/signaling.md). Two layers riding the existing peer topology — direct peer inbox + per-host environmental field — with five new Forth words and one prelude word.
- `SAY!` ( v -- ) — broadcast value `v` to neighbors' inboxes. Costs 3 energy. Works on native and WASM.
- `LISTEN` ( -- v -1 | 0 ) — pop the oldest inbox entry, push value+flag, or 0 if empty. Free.
- `INBOX?` ( -- n ) — push count of pending inbox entries without consuming them. Free.
- `MARK!` ( v -- ) — deposit value into the per-host environmental field, keyed by the unit's dominant niche. Costs 5 energy. Native only; WASM shim emits "MARK! not available in browser".
- `SENSE` ( -- v ) — read current environmental strength for this unit's niche. Free. Native only; WASM shim.
- `COURT` — prelude convenience word, `: COURT FITNESS SAY! ;`. Honest mate-finding signal; subject to GP mutation like any other dictionary entry.
- `crate::signaling` module: `Signal` struct, `SignalKind` enum (Direct + Environmental), `Inbox` (Vec-backed FIFO with cap 64 and drop-from-front overflow), `EnvironmentalField` (HashMap with sum-or-displace deposit and 0.95/tick multiplicative decay).
- `MultiUnitHost::route_signals_from(idx)` — drains a unit's outbox after eval, delivers Direct signals to sibling inboxes (sender does not self-receive) and routes Environmental signals into the host's `env_field`.
- `MultiUnitHost::refresh_env_view(idx)` and `env_decay_tick()` — host-side helpers for keeping per-unit `env_view` caches current and aging the field once per tick.
- `MultiUnitHost::spawn` now stamps each spawned unit with a synthesized `node_id_cache` (`0xC0FE` prefix + slot index) so SAY! signals carry distinct sender attribution between siblings.
- `reproduction::select_mate_signaled(peers, inbox, rng)` — additive companion to `select_mate`. Reads Direct signals from the inbox to build a candidate list, runs tournament-of-three on signaled values, falls through to `select_mate` (peer-fitness path) when the inbox is empty or has no overlapping senders. The existing `select_mate` and its callers are untouched.
- WASM shim exports `drain_outbox_direct(vm) -> *const u8` and `push_inbox_direct(vm, value)` so the browser mesh can route SAY! emissions between in-page units.
- Browser demo wires real SAY! through the existing setBubble path: `BEHAVIORS` gains `COURT` (signal-emitting) and a LISTEN cue; autoTick drains and routes after every eval, rendering "signals N" bubbles for emissions and "heard N" for receives. The lone-unit "Hello?" → "Spawn" narrative arc is unchanged.
- `EnergyState` constants: `SAY_COST = 3`, `MARK_COST = 5`. Starting calibrations; the v0.28.x patch series is where they tune.
- 46 new tests covering inbox FIFO + cap semantics, EnvironmentalField deposit/decay/floor, SAY!/LISTEN/INBOX? VM-level + host integration, MARK!/SENSE native + cfg-gated paths, signal-weighted mate selection (most-recent-wins, fallback paths, environmental-signal exclusion), and COURT prelude integration. Total native test count: 301.

### Changed
- `web/unit.js` fetches `unit.wasm` and itself with `cache: 'no-store'` so substrate updates aren't shadowed by browser caches.
- `web/index.html` references `unit.js?v=0.28.0` for the same reason on the JS side.
- `MultiUnitHost`-spawned units now pass the `Some(id)` branch in persistence/snapshot paths (previously hit the "no node ID (mesh offline)" message). Two-tier-mode users calling `SAVE` / `HIBERNATE` will now write to `~/.unit/state/c0fe…/` directories — single-VM mode and WASM mode unaffected.

### Design principles held
- Honesty is not enforced. `SAY!` puts whatever the sender's stack holds onto the wire; the only discipline on deception is metabolic. Whether honest signaling stabilizes is the empirical question this substrate exists to ask.
- In-process only. v0.28 ships signaling between siblings in `MultiUnitHost` and the WASM browser host. Cross-process direct signals over the gossip path are deferred — the existing UDP wire protocol is unchanged.
- Additive selection pressure. `select_mate` keeps its signature; `select_mate_signaled` is a new function with a peer-fitness fallback. No existing reproduction test changes behavior.
- Zero new dependencies. Cargo.lock still contains only the `unit` crate.

## [0.27.1] - 2026-04-25

Reduced WASM demo colony cap from 10 to 7 to mitigate browser-tab freeze under sustained run.

## [0.27.0] - 2026-04-17

### Added
- HTTP bridge (src/http.rs): hand-rolled HTTP/1.1 server exposing the VM and mesh over localhost. Still zero dependencies — the bridge uses std::net::TcpListener and the in-tree JSON encoder.
- New CLI flag `--serve [PORT]` (default 9898). Binds 127.0.0.1 only. Replaces the REPL when set; prelude, --file, --trust, --swarm, and mesh startup all still apply first.
- New Cargo feature `http` (pure module gate — no new crates in Cargo.lock). Default build is unchanged.
- Endpoints: POST /eval, POST /sexp, GET /status, GET /words, GET /word/<name>, GET /mesh/peers, POST /mesh/broadcast. All JSON. Errors as `{"error":"..."}` with appropriate 4xx/5xx status.
- Transport: single-threaded accept, one std::thread per connection. Connection: close after every response. 64 KiB request cap, 5-second read timeout. No keep-alive, no chunked transfer, no query parsing beyond path.
- tests/http_test.rs: end-to-end integration test that spawns the real binary with `--serve`, hits every endpoint over TcpStream, and asserts JSON shape. No test dependencies.
- Non-goals for 0.27.0 — deferred: auth (0.27.1), non-localhost binding (0.27.1), SSE/streaming (0.28.0), snapshot write-through (0.28.0).

### Changed
- snapshot::escape_json_string is now `pub(crate)` so the HTTP bridge can reuse it.
- VERSION constant updated to v0.27.0.

## [0.24.0] - 2026-04-04

### Added
- Emergent browser behaviors: SAY-SOMETHING word with 7 state-driven personality templates replacing scripted autonomous behaviors. PERSONALITY word shows behavioral profile (mentor/collaborator/explorer/survivor/newborn).
- Solution diversity tracking: Challenge.solutions vec stores up to 20 distinct verified programs per challenge. SOLUTIONS and DIVERSITY REPL words. colony_diversity() aggregate stats.
- Genome visualization: click-to-inspect panel in browser mesh visualizer showing unit ID, fitness, energy, stack, antibodies, user words, and learned words. Includes "Run Command" input for executing Forth on any unit. Selected node highlighted with white outline.
- Python organism (polyglot/python/): AST-based symbolic regression using Python ast module. Third species on the mesh with stdlib-only dependencies. 22 tests. sexp.py, mesh.py, evolve.py, challenge.py, main.py.
- Third-order evolution: ScoringPopulation (10 Forth programs) evolves the fitness functions that judge challenge generators. Evaluated against GeneratorHistory of which generators produced solvable challenges. Gradual activation after 10+ history entries. SCORERS and META-DEPTH REPL words.
- Stack simulator extended with ABS, MAX, MIN for scoring function programs.
- Python build/test added to CI pipeline (python-build job).
- Interop stress test (tests/interop_test.sh) for Rust/Go mesh verification.
- Integration test suite: 10 end-to-end cross-module tests.

## [0.23.1] - 2026-04-04

### Added
- Browser demo updated with immune system, energy, and landscape tutorial steps (3 new steps, 14 total)
- JS interceptors for CHALLENGES, IMMUNE-STATUS, ANTIBODIES, ENERGY, METABOLISM, LANDSCAPE, DEPTH
- Spawn energy inheritance: child receives parent_remaining/3 capped at INITIAL_ENERGY (1000)
- Integration test suite: 10 end-to-end tests covering cross-module interactions (191 total)
- CI updated: cargo test, cargo clippy, and Go build/test in GitHub Actions

### Changed
- README updated to reflect all v0.22.0-v0.23.1 features
- VERSION constant updated to v0.23.1
- WASM binary rebuilt with all new Forth words
- Browser hints bar: added CHALLENGES, ENERGY, DEPTH
- Autonomous behaviors: units report energy and challenge status in colony chatter
- Meta tags updated to mention immune system and metabolism

## [0.23.0] - 2026-04-04

### Added
- Emergent challenge generation: MetaEvolver with population of 20 Forth programs that evolve challenge generators (second-order evolution)
- Stack simulator for evaluating generator programs without full VM
- Generator fitness scoring: 0 for crash, 1 for trivial, 100+ for interesting targets
- GENERATORS word: list top generators by fitness and program
- META-EVOLVE word: manually trigger one generation of generator evolution
- Open-ended evolution: LandscapeEngine with ArithmeticLadder and CompositionLadder generators
- ArithmeticLadder: fib(N) solved → fib(N+5), parsimony variant, square(fib(N))
- CompositionLadder: combine two solved challenges into a new one (1/3 trigger rate)
- EnvironmentCycle: Normal/Harsh/Abundant/Competitive conditions rotating every 500 ticks
- Harsh halves max_steps and doubles rewards; Abundant doubles max_steps; Competitive scales rewards by 1/(attempts+1)
- LANDSCAPE word: depth, challenges generated, environment condition
- DEPTH word: evolutionary depth metric
- Polyglot organisms: Go reference implementation (polyglot/go/)
- Go organism: expression tree GP engine, S-expression parser, UDP mesh, challenge protocol
- Go organism joins Rust mesh, receives challenges, evolves solutions, broadcasts results
- Formal analysis document (docs/formal-analysis.md): convergence properties, search space analysis, energy dynamics, open-ended evolution criteria
- Whitepaper (docs/unit-whitepaper-2026.pdf)

## [0.22.0] - 2026-04-04

### Added
- Challenge registry (src/challenges.rs): ChallengeRegistry with register, merge, solve lifecycle
- Challenge struct with name, target_output, seed_programs, reward, solved status, solution
- ChallengeOrigin: BuiltIn or Discovered (with source node tracking)
- fib10 registered as a built-in challenge on startup
- GP-EVOLVE now picks from ChallengeRegistry (highest-reward unsolved), falls back to fib10
- Solutions installed as SOL-* dictionary words (e.g. SOL-FIB10) callable from REPL
- SOL-* words inherited by children via SPAWN and persisted in JSON snapshots
- S-expression broadcast format for challenges and solutions on mesh
- Problem discovery (src/discovery.rs): ProblemDetector with goal failure, dist-goal timeout, manual report detection
- FNV-1a dedup with cooldown window, auto-generated seed programs from failed code mutations
- CHALLENGES word: list all challenges with status and reward
- IMMUNE-STATUS word: solved/unsolved counts, colony antibody count
- ANTIBODIES word: list learned SOL-* words
- Metabolic energy system (src/energy.rs): EnergyState with spend/earn/tick lifecycle
- Energy costs: GP generation (5), SPAWN (200), eval (1 per 1000 steps), mesh send (1)
- Energy rewards: task success (50), challenge solved (100), passive regen (1/tick)
- Throttling at energy ≤ 0: sandbox step budget reduced to 1000 (from 10000)
- Hard floor at -500 prevents infinite debt
- Energy persists in JSON snapshots across HIBERNATE/resume
- ENERGY word: current level, earned, spent, efficiency
- METABOLISM word: full metabolic report with cost/reward tables
- FEED word: manually add energy (capped at 500 per call)
- HELP-IMMUNE section in built-in help system

## [0.21.0] - 2026-04-02

### Added
- Dictionary inheritance: spawned browser units inherit user-defined words from parent via userWords tracking
- Autonomous spawning in browser demo: colony self-replicates when fitness > 0, 2+ units, 30% random chance per 15s check
- DASHBOARD intercepted in browser REPL to show actual mesh data
- Spawned units gain fitness from work (+10 per DIST-GOAL computation, +5 per teach, +1 per autonomous action)
- Self node shows ID and fitness in visualizer (e.g. "cbcl self" with "f:30")

### Changed
- HOW-ARE-YOU messages: "connected but need help" → "just spawned. finding my role"
- Prelude HOW-ARE-YOU rewritten with warming up / getting started / doing well progression
- Tutorial: SEXP steps include dot for explicit output, word count updated to 300+
- Branding: "self-replicating Forth interpreter" → "self-replicating software nanobot" throughout

## [0.20.2] - 2026-04-01

### Added
- Self-replication tutorial step: SPAWN as explicit step 8, user triggers reproduction
- DIST-GOAL tutorial step: distributed computation as step 9
- Memory access words: HERE, comma (,), C,, ALLOT, CELLS
- HELP-MEMORY section documenting VARIABLE, CONSTANT, CREATE, @, !

### Changed
- Tutorial expanded from 9 to 11 steps
- Auto-spawn removed from step 3; user now controls reproduction explicitly
- Tutorial completion message mentions self-replication and distributed computation

## [0.20.1] - 2026-04-01

### Fixed
- Case-insensitive tutorial step matching in browser demo
- GOAL{ regex case-insensitive flag added

### Added
- test_case_insensitive_lookup test

## [0.20.0] - 2026-04-01

### Added
- Cross-machine mesh: DNS hostname resolution for UNIT_PEERS
- UNIT_EXTERNAL_ADDR for NAT traversal
- UNIT_MESH_KEY for mesh authentication
- MY-ADDR, PEER-TABLE, MESH-STATS, MESH-KEY words
- CONNECT" and DISCONNECT" for manual peer management from REPL
- HELP-MESH updated with cross-machine setup instructions

### Prior versions
- v0.19.x: Distributed computation (DIST-GOAL), browser mesh distribution
- v0.18.0: Genetic programming engine (GP-EVOLVE)
- v0.17.x: JSON persistence, S-expression protocol, WASM time fixes
- Earlier: Core Forth VM (309 words), UDP mesh with gossip, self-replication, goal registry, monitoring/ops, smart mutation, WebSocket bridge, WASM browser demo
