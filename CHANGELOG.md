# Changelog

All notable changes to this project are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/).

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
