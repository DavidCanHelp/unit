// multi_unit.rs — single-process multi-unit host + mesh bridge
//
// A direct port of the WASM browser demo's BrowserMesh model (web/unit.js):
// many `VM` instances live in one OS process, share an address space, and
// communicate by direct method calls — no fork, no UDP, no peer table.
//
// `MultiUnitHost` (lower half of this file) is the strictly-intra-process
// runtime. `MultiUnitNode` (upper half of the section after the host) is
// the bridge: it owns a `MultiUnitHost` and a `MeshNode`, advertising the
// host's unit count to other processes via the existing bounded-k gossip
// mesh. Addressing stays explicit: in-process siblings are reached via the
// host's `share_word`/`teach_from`; remote processes are reached via the
// mesh. The bridge does not create a unified address space.
//
// Mirrors the WASM model deliberately:
//   - `spawn` (web/unit.js:76) → `MultiUnitHost::spawn`
//   - `_pickWorker` (web/unit.js:167) → `pick_worker`
//   - `executeGoal` (web/unit.js:179) → `execute_goal`
//   - `shareWord`  (web/unit.js:196) → `share_word`
//   - `teachFrom`  (web/unit.js:204) → `teach_from`

use crate::vm::VM;

/// Per-unit state. `vm` is the Forth VM. `busy` and `tasks_completed`
/// match the BrowserUnit fields used by the worker picker. `user_words`
/// tracks definition source strings as they were supplied (mirrors
/// `BrowserUnit.userWords`, web/unit.js:42–43, 234) — Forth's `SEE`
/// returns decompiled internal form (e.g. `LIT(3)`), not re-evaluable
/// source, so the host has to track originals explicitly.
pub struct UnitSlot {
    pub vm: VM,
    pub busy: bool,
    pub tasks_completed: u64,
    pub user_words: Vec<String>,
    /// Consecutive ticks this unit has spent pinned at the energy hard
    /// floor (see [`EnergyState::at_hard_floor`](crate::energy::EnergyState::at_hard_floor)).
    /// Reaching [`STARVED_TICKS_TO_DIE`] is death. Ordinary GP debt never
    /// pins the floor (GP's gate pauses above it), so only unsustainable
    /// lifestyles — e.g. a runaway `LIVE` — accumulate here.
    pub starved_ticks: u32,
}

/// Consecutive at-hard-floor ticks before a unit dies. At the node's 1s
/// tick cadence this is a ~30s grace window in which task rewards, mesh
/// work, or a keeper's FEED can still rescue the unit.
pub const STARVED_TICKS_TO_DIE: u32 = 30;

/// Starvation-fuse ticks burned per tick under ACUTE famine (see
/// [`FAMINE_ACUTE_OVERSHOOT`](crate::energy::FAMINE_ACUTE_OVERSHOOT)):
/// death in 5 pinned ticks instead of 30, because the alternative at
/// util ≥ 96% is the kernel OOM-killing every resident at once.
pub const FAMINE_ACUTE_FUSE: u32 = 6;

/// Utilization below which a host REBOUNDS: population regrows into
/// measured headroom after famine or emigration has thinned it. Kept a
/// full 10 points under the 80% famine ceiling so births and deaths
/// never churn at a shared boundary — the [70%, 80%) band is stable in
/// both directions. Measured util (not unit count) is deliberately the
/// signal: whatever heap fragmentation makes of freed memory, births
/// stop when the measurement says the room is spent.
pub const REBOUND_UTILIZATION: f64 = 0.70;
/// Ticks between births during a rebound: refill is deliberate, not a
/// spawn flood — the newborns' own memory demand must land in measure()
/// before the next birth decision reads it.
pub const REBOUND_INTERVAL_TICKS: u64 = 5;
/// Energy a parent endows its child at birth, on top of the SPAWN_COST
/// the reproduction itself burns. Only a unit still holding
/// [`REBOUND_PARENT_MIN`] after both can breed: reproduction is a
/// surplus behavior, and the endowment keeps the newborn out of famine's
/// immediate reach without minting energy from nothing.
pub const BIRTH_ENDOWMENT: i64 = 500;
/// Post-birth energy a parent must retain to qualify for breeding.
pub const REBOUND_PARENT_MIN: i64 = 300;

/// Result of dispatching one goal.
pub struct GoalResult {
    pub unit_index: usize,
    pub output: String,
}

/// Host owning N VMs in one process. Goal dispatch is synchronous,
/// matching the JS event-loop model: while one VM evals, the others wait.
pub struct MultiUnitHost {
    pub units: Vec<UnitSlot>,
    cap: usize,
    /// Lifetime spawn counter (never decremented). Seeds each new unit's
    /// rng stream uniquely — slot indexes get reused after deaths, and two
    /// units sharing a stream draw identical famine luck in lockstep.
    /// pub(crate): the transport landing path adopts units directly and
    /// must draw from the same counter.
    pub(crate) spawned_total: u64,
    /// Backlog of goals that arrived with no idle unit to take them — the
    /// minimal honest demand signal. The synchronous dispatch path
    /// (`execute_goal`) serves each goal immediately, so without this counter
    /// the host has no record of work it *couldn't* place. It is the "work
    /// waiting" half of [`senses_unmet_demand`](Self::senses_unmet_demand);
    /// the "no idle unit" half is read from the `busy` flags directly.
    pub pending_goals: usize,
    /// Per-host environmental signal field — the second signaling layer.
    /// MARK! deposits into it (via outbox routing); SENSE reads from it
    /// (via per-VM env_view caches refreshed between evals). Native-only
    /// in v0.28; the wasm32 demo runs without one.
    #[cfg(not(target_arch = "wasm32"))]
    pub env_field: crate::signaling::EnvironmentalField,
}

impl MultiUnitHost {
    pub fn new(cap: usize) -> Self {
        MultiUnitHost {
            units: Vec::new(),
            cap,
            spawned_total: 0,
            pending_goals: 0,
            #[cfg(not(target_arch = "wasm32"))]
            env_field: crate::signaling::EnvironmentalField::new(),
        }
    }

    /// Default cap of 100 — well above the WASM demo's 7 but still bounded
    /// so users can't accidentally allocate gigabytes of VMs.
    pub fn with_default_cap() -> Self {
        Self::new(100)
    }

    /// Colony immune-knowledge stats for the chronicle line:
    /// `(distinct SOL-* names, total SOL-* copies)` across all hosted units.
    /// Kinds measure what the colony KNOWS; copies measure how widely the
    /// knowledge has spread (share/death-cry/inheritance). The divergence of
    /// the two over a long soak is one honest adaptation-vs-churn signal.
    pub fn sol_stats(&self) -> (usize, usize) {
        let mut kinds = std::collections::HashSet::new();
        let mut copies = 0usize;
        for slot in &self.units {
            for e in &slot.vm.dictionary[slot.vm.kernel_word_count..] {
                if !e.hidden && e.name.starts_with("SOL-") {
                    copies += 1;
                    kinds.insert(e.name.clone());
                }
            }
        }
        (kinds.len(), copies)
    }

    pub fn len(&self) -> usize {
        self.units.len()
    }
    pub fn cap(&self) -> usize {
        self.cap
    }
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }
    pub fn is_full(&self) -> bool {
        self.units.len() >= self.cap
    }

    /// Spawn one fresh unit (loads the prelude). Returns its index, or `None`
    /// if at cap.
    pub fn spawn(&mut self) -> Option<usize> {
        if self.is_full() {
            return None;
        }
        let mut vm = VM::new();
        // Suppress banner + prelude output during boot the way wasm_entry does
        // (src/wasm_entry.rs:35–38): capture into output_buffer, then drop.
        vm.silent = true;
        vm.output_buffer = Some(String::new());
        vm.load_prelude();
        vm.output_buffer = None;
        vm.silent = false;
        let idx = self.units.len();
        // Stamp a per-unit synthesized id so SAY! signals carry distinct
        // sender attribution between siblings. The 0xC0FE prefix marks
        // these as host-synthesized rather than mesh-issued.
        vm.node_id_cache = Some([0xC0, 0xFE, 0, 0, 0, 0, 0, idx as u8]);
        // Distinct per-unit rng streams. VM::new seeds every rng with 0, so
        // sibling units would draw identical randomness in lockstep — which
        // turns per-unit famine luck into no variance at all (and synchronizes
        // any other per-unit draw made at the same cadence). Seeded from the
        // lifetime spawn counter, not the slot index: indexes are reused
        // after deaths.
        self.spawned_total += 1;
        vm.rng =
            crate::features::mutation::SimpleRng::new(0x9e37_79b9_7f4a_7c15 ^ self.spawned_total);
        self.units.push(UnitSlot {
            vm,
            busy: false,
            tasks_completed: 0,
            user_words: Vec::new(),
            starved_ticks: 0,
        });
        Some(idx)
    }

    /// Drain unit[idx]'s outbox and route each signal:
    ///   - Direct: deliver to every sibling's inbox (sender does not
    ///     self-receive).
    ///   - Environmental: deposit into the per-host `EnvironmentalField`
    ///     keyed by the signal's niche.
    ///
    /// Returns the count of cross-unit deliveries (Direct signal × sibling
    /// count); Environmental deposits are not counted in this number.
    /// Callers invoke after eval to propagate SAY! / MARK! emissions.
    pub fn route_signals_from(&mut self, idx: usize) -> usize {
        if idx >= self.units.len() {
            return 0;
        }
        let outgoing: Vec<crate::signaling::Signal> =
            std::mem::take(&mut self.units[idx].vm.outbox);
        if outgoing.is_empty() {
            return 0;
        }
        let mut delivered = 0;
        for signal in &outgoing {
            match &signal.kind {
                crate::signaling::SignalKind::Direct => {
                    for (j, slot) in self.units.iter_mut().enumerate() {
                        if j == idx {
                            continue;
                        }
                        slot.vm.inbox.push(signal.clone());
                        delivered += 1;
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                crate::signaling::SignalKind::Environmental { niche } => {
                    self.env_field.deposit(niche.clone(), signal.value as f64);
                }
                #[cfg(target_arch = "wasm32")]
                crate::signaling::SignalKind::Environmental { .. } => {
                    // No-op on wasm32 — MARK! shim never produces these
                    // signals, but defend against future code paths.
                }
                crate::signaling::SignalKind::EnergyGift => {
                    // Route the gift to the neediest sibling. The donor
                    // already spent value + friction at GIVE time, so the
                    // transfer conserves: donor − (n+1), recipient + n,
                    // friction dissipated. An undeliverable gift (no
                    // sibling) returns to the donor — friction still lost.
                    let poorest = self
                        .units
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != idx)
                        .min_by_key(|(_, s)| s.vm.energy.energy)
                        .map(|(j, _)| j);
                    match poorest {
                        Some(j) => {
                            self.units[j].vm.energy.earn(signal.value, "gift-received");
                            delivered += 1;
                        }
                        None => {
                            self.units[idx].vm.energy.earn(signal.value, "gift-returned");
                        }
                    }
                }
            }
        }
        delivered
    }

    /// Refresh unit[idx]'s `env_view` cache from the per-host environmental
    /// field, keyed by its dominant niche. Called between evals so SENSE
    /// returns a current value. Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn refresh_env_view(&mut self, idx: usize) {
        if idx >= self.units.len() {
            return;
        }
        let niche = crate::niche::dominant_niche(&self.units[idx].vm.niche_profile)
            .map(|(k, _)| k)
            .unwrap_or_else(|| "general".to_string());
        let v = self.env_field.sense(&niche);
        self.units[idx].vm.env_view = v;
    }

    /// Apply one decay step to the environmental field. Native-only; the
    /// wasm32 demo has no field to age.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn env_decay_tick(&mut self) {
        self.env_field.decay_tick();
    }

    /// Define a word on one specific unit and record the source string in
    /// that unit's `user_words` so it can later be taught to siblings.
    /// `definition` should look like `": NAME ... ;"`.
    pub fn define_on(&mut self, idx: usize, definition: &str) -> bool {
        if idx >= self.units.len() {
            return false;
        }
        self.units[idx].vm.eval(definition);
        self.units[idx].user_words.push(definition.to_string());
        true
    }

    /// Spawn up to `n` units; stops when at cap. Returns count actually spawned.
    pub fn spawn_n(&mut self, n: usize) -> usize {
        let mut spawned = 0;
        for _ in 0..n {
            if self.spawn().is_none() {
                break;
            }
            spawned += 1;
        }
        spawned
    }

    /// Pick the least-busy idle unit by tasks_completed. Skips busy units.
    /// Falls back to unit 0 if every unit is busy. Returns `None` only when
    /// the host is empty. Mirrors `_pickWorker` (web/unit.js:167).
    pub fn pick_worker(&self) -> Option<usize> {
        if self.units.is_empty() {
            return None;
        }
        let mut best: Option<usize> = None;
        let mut best_score: u64 = u64::MAX;
        for (i, slot) in self.units.iter().enumerate() {
            if slot.busy {
                continue;
            }
            if slot.tasks_completed < best_score {
                best_score = slot.tasks_completed;
                best = Some(i);
            }
        }
        best.or(Some(0))
    }

    /// Dispatch one Forth expression to the least-busy unit. Captures the
    /// VM's output. Returns `None` if the host is empty.
    pub fn execute_goal(&mut self, code: &str) -> Option<GoalResult> {
        let i = self.pick_worker()?;
        let slot = &mut self.units[i];
        slot.busy = true;
        let output = slot.vm.eval(code);
        slot.tasks_completed += 1;
        slot.busy = false;
        Some(GoalResult {
            unit_index: i,
            output,
        })
    }

    /// Eval `definition` on every unit (zero-copy `&str` reuse — same address
    /// space, no serialization). Records `definition` in each unit's
    /// `user_words`. Mirrors `shareWord` (web/unit.js:196).
    pub fn share_word(&mut self, definition: &str) {
        for slot in self.units.iter_mut() {
            slot.vm.eval(definition);
            slot.user_words.push(definition.to_string());
        }
    }

    /// Copy named user-defined words from `source_idx` to every other unit.
    /// Looks up each name in `source.user_words` for a matching `: NAME ...`
    /// definition string (last one wins) and re-evaluates it on siblings.
    /// Mirrors `teachFrom` (web/unit.js:204) — but uses the host's tracked
    /// definitions rather than `SEE`, since `SEE`'s output is decompiled
    /// internal form (e.g. `LIT(3)`) and not re-evaluable.
    /// Returns the names actually taught.
    pub fn teach_from(&mut self, source_idx: usize, words: &[&str]) -> Vec<String> {
        let mut taught = Vec::new();
        if source_idx >= self.units.len() {
            return taught;
        }
        // For each requested name, find the most recent matching `: NAME ...`
        // entry in source's user_words.
        let mut to_replay: Vec<(String, String)> = Vec::new();
        for &word in words {
            let needle = format!(": {} ", word);
            let needle_alt = format!(": {}\n", word);
            let def = self.units[source_idx]
                .user_words
                .iter()
                .rev()
                .find(|d| {
                    let t = d.trim_start();
                    t.starts_with(&needle) || t.starts_with(&needle_alt)
                })
                .cloned();
            if let Some(d) = def {
                to_replay.push((word.to_string(), d));
            }
        }
        for (word, def) in to_replay {
            taught.push(word);
            for (i, slot) in self.units.iter_mut().enumerate() {
                if i == source_idx {
                    continue;
                }
                slot.vm.eval(&def);
                slot.user_words.push(def.clone());
            }
        }
        taught
    }

    /// True iff this host senses unmet demand it cannot currently serve:
    /// there is work waiting (`pending_goals > 0`) AND no idle unit to take it
    /// (every unit is busy). Both halves come from existing dispatch state —
    /// `pending_goals` and the per-unit `busy` flags — not from any new global
    /// signal. An empty host senses nothing.
    ///
    /// This is the demand half of the local replication rule: if any unit is
    /// idle, the colony can already serve its load and must not replicate.
    pub fn senses_unmet_demand(&self) -> bool {
        self.pending_goals > 0
            && !self.units.is_empty()
            && self.units.iter().all(|u| u.busy)
    }

    /// The local replication rule every coordinate runs. It replicates one
    /// unit IFF there is unmet demand this host can serve AND the spawn guard
    /// (quarantine / max_children / cooldown) plus the binding-constraint
    /// ceiling both permit it.
    ///
    /// There is no coordinator, no quorum, no global counter, and no target
    /// population: just `demand ∧ headroom`, evaluated from this host's own
    /// state. Returns `Ok(())` when the rule fires; `Err(reason)` otherwise —
    /// `"no unmet demand"` when the colony can already serve its load, or the
    /// guard/ceiling refusal string from
    /// [`SpawnState::can_spawn_within`](crate::spawn::SpawnState::can_spawn_within).
    pub fn replication_decision(
        &self,
        res: &crate::resources::HostResources,
        spawn_state: &crate::spawn::SpawnState,
    ) -> Result<(), String> {
        if !self.senses_unmet_demand() {
            return Err("no unmet demand".into());
        }
        spawn_state.can_spawn_within(res)
    }

    /// The no-work fall-through: a unit with no assigned goal runs its
    /// `LIVE` word — the dictionary-resident life loop (prelude default:
    /// `GP-EVOLVE`, i.e. speculative evolution against open challenges).
    /// The host decides *when* an idle tick happens; the genome decides
    /// *what living is* — so a unit's habits are heritable, shareable, and
    /// mutable like any other word. Execution is metered, so a `LIVE` that
    /// loops forever starves rather than hanging the host; a unit whose
    /// dictionary has lost `LIVE` entirely simply idles (and, unable to
    /// earn, eventually dies). Returns the index of the unit set to work,
    /// or `None` if every unit is already busy.
    ///
    /// Surplus self-resolves through the energy metabolism; this adds no
    /// reclaim or cull logic.
    pub fn evolve_one_unworked(&mut self) -> Option<usize> {
        let idx = self.units.iter().position(|u| !u.busy)?;
        let slot = &mut self.units[idx];
        slot.busy = true;
        if slot.vm.find_word("LIVE").is_some() {
            slot.vm.eval("LIVE");
        }
        slot.busy = false;
        // Route anything LIVE emitted (signals, energy gifts).
        self.route_signals_from(idx);
        Some(idx)
    }
}

// ===========================================================================
// MultiUnitNode — bridge between in-process units and the inter-process mesh
// ===========================================================================
//
// Two-tier deployment:
//   * `host: MultiUnitHost` — N in-process VMs, O(1) communication via
//     direct `eval`. The host is the failure boundary.
//   * `mesh: Option<MeshNode>` — one process-level peer in the mesh, talking
//     UDP gossip to other processes via the existing bounded-k pipeline.
//
// Addressing is explicit. Local sibling reach uses the host directly
// (`share_word`, `teach_from`). Remote reach uses `send_to_process` /
// `drain_and_dispatch` here, which sit on top of `MeshNode::send_sexp` and
// `recv_sexp_messages` unchanged. The mesh peer is the *process*, not the
// unit — peers advertise their unit count via `MeshNode::set_load`.
//
// Crash semantics are fate-shared: when a host process dies, its UDP
// socket closes and its heartbeats stop. Other peers' `evict_peers_older_than`
// (or the network thread's 15s timer) eventually removes the dead peer.
// In-flight work on the dead host is simply gone — no resurrection, no
// per-unit liveness tracking.

use crate::mesh::{self, MeshNode, NodeId};
use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct RemoteProcess {
    pub host_id: NodeId,
    pub host_id_hex: String,
    pub units_hosted: u32,
    /// The peer's advertised resource headroom (`0..=100`), gossiped via the
    /// heartbeat. The input to sufficient-first placement.
    pub advertised_headroom: u8,
    pub addr: SocketAddr,
}

#[derive(Debug, Clone)]
pub struct DispatchedRemoteMsg {
    pub from_host_hex: String,
    pub unit_index: usize,
    pub output: String,
}

/// What one [`MultiUnitNode::tick`] did. Returned so the run loop can log each
/// meaningful event and so tests can assert the tick's behavior without
/// sockets or real time.
/// One unit's death, reported so the run loop can log the obituary and
/// tests can assert mortality without sockets.
pub struct UnitDeath {
    pub fitness: i64,
    pub generation: u32,
    /// Antibodies (SOL-* words) the unit bequeathed.
    pub antibodies: usize,
    /// Local siblings that absorbed at least one of them.
    pub heirs: usize,
}

#[derive(Default)]
pub struct TickReport {
    /// Goals received from the mesh and dispatched to local units this tick.
    pub dispatched: Vec<DispatchedRemoteMsg>,
    /// Number of unworked units that ran their `LIVE` word this tick.
    pub evolved_units: usize,
    /// Units that died of sustained starvation this tick (their antibodies
    /// were bequeathed to siblings and broadcast as a death-cry).
    pub deaths: Vec<UnitDeath>,
    /// Antibody words absorbed this tick from *remote* death-cries.
    pub scavenged_words: usize,
    /// Best fitness across local units after this tick (for the evolve log).
    pub best_fitness: i64,
    /// True if the host was over the ceiling (mislocated) this tick.
    pub mislocated: bool,
    /// Per-unit famine tax applied this tick (0 = host under ceiling).
    pub famine_tax: i64,
    /// A rebound birth happened this tick: (child generation, child energy).
    pub birth: Option<(u32, i64)>,
    /// The placement outcome, present only if the local rule fired.
    pub transport: Option<TickTransport>,
}

/// The placement outcome within a single tick.
pub enum TickTransport {
    /// Mislocated, but no peer advertised sufficient room — the unit stays.
    NoDestination,
    /// A destination was chosen and a transport attempted. `outcome` carries
    /// the confirm-before-release result; the origin slot was retired iff it is
    /// `Ok(Accepted)` (see [`MultiUnitNode::relocate_unit_with`]).
    Attempted {
        target_hex: String,
        target_headroom: u8,
        outcome: Result<crate::transport::ConfirmOutcome, crate::transport::TransportError>,
    },
}

pub struct MultiUnitNode {
    pub host: MultiUnitHost,
    pub mesh: Option<MeshNode>,
    /// Per-node RNG for the placement tie-break, seeded from this node's mesh
    /// identity so different nodes shedding into the same gossiped view pick
    /// different tied-maximum peers (decorrelating concurrent senders).
    rng: crate::features::mutation::SimpleRng,
    /// Antibody words absorbed from remote death-cries during the most
    /// recent `drain_and_dispatch` (read by `tick` into its report).
    pub scavenged_last_drain: usize,
    /// Round-robin cursor for the per-tick LIVE budget (see
    /// [`LIVE_BUDGET_PER_TICK`]).
    live_cursor: usize,
    /// Monotonic tick counter for rate-limited rules (rebound interval).
    ticks_total: u64,
    /// Tick of the most recent rebound birth.
    last_birth_tick: u64,
}

/// How many idle units run their LIVE word per tick. Unbudgeted, every idle
/// unit evolved serially inside one tick: once evolution had real work on
/// every unit (the post-win no-op fix), a 300-unit node's tick cost
/// exploded from ~1s to minutes — starving supervision, transport cadence,
/// and the chronicle itself (soak finding #3: one status line in 700s).
/// Time-slicing is host physics, not policy: the budget bounds tick
/// latency, the rotating cursor guarantees every unit still evolves in
/// turn, and colony-wide evolutionary throughput becomes a measurable
/// budget×tick-rate instead of an accident of population size.
pub const LIVE_BUDGET_PER_TICK: usize = 16;

/// Wall-clock budget for the tick's LIVE pass. The unit-count budget alone
/// still let ticks balloon when every unit had REAL evolutionary work
/// (soak round 4: 16 units × ~3s of post-win GP = 47s ticks — supervision
/// and transport cadence degraded 47×). Time is the resource the tick loop
/// actually spends, so time is what the budget bounds: rotate until the
/// slice is spent (always at least one unit), and evolution throughput
/// becomes whatever fits in the slice — cheap LIVEs run many units per
/// tick, expensive ones few, and the tick stays a tick.
pub const LIVE_TICK_BUDGET_MS: u64 = 250;

/// Harvest a unit's immune memory: its `SOL-*` antibody words (name +
/// re-evaluable decompiled source), the inheritance a dying unit leaves
/// behind. Bounded by [`crate::sexp::DEATH_CRY_MAX_ANTIBODIES`].
pub fn harvest_antibodies(vm: &VM) -> Vec<(String, String)> {
    vm.dictionary[vm.kernel_word_count..]
        .iter()
        .filter(|e| !e.hidden && e.name.starts_with("SOL-"))
        .take(crate::sexp::DEATH_CRY_MAX_ANTIBODIES)
        .map(|e| {
            let source = crate::snapshot::decompile_word(e, &vm.dictionary, &vm.primitive_names);
            (e.name.clone(), source)
        })
        .collect()
}

impl MultiUnitNode {
    /// Create a new node. If `mesh_port` is `Some(p)`, start a `MeshNode` on
    /// port `p` (use 0 for OS-assigned). `seed_peers` lets this node bootstrap
    /// onto an existing mesh.
    pub fn new(
        cap: usize,
        mesh_port: Option<u16>,
        seed_peers: Vec<SocketAddr>,
    ) -> Result<Self, String> {
        let mesh = match mesh_port {
            Some(p) => Some(MeshNode::start(p, seed_peers)?),
            None => None,
        };
        // Seed the placement RNG from the mesh identity so each node draws an
        // independent tie-break sequence; fall back to a fixed seed off-mesh.
        let rng_seed = mesh
            .as_ref()
            .map(|m| {
                m.id()
                    .iter()
                    .enumerate()
                    .fold(0u64, |acc, (i, b)| acc | ((*b as u64) << (i * 8)))
            })
            .filter(|&s| s != 0)
            .unwrap_or(0x9e37_79b9_7f4a_7c15);
        Ok(MultiUnitNode {
            host: MultiUnitHost::new(cap),
            mesh,
            rng: crate::features::mutation::SimpleRng::new(rng_seed),
            scavenged_last_drain: 0,
            live_cursor: 0,
            ticks_total: 0,
            last_birth_tick: 0,
        })
    }

    /// This host process's mesh node id, or `None` if running without a mesh.
    pub fn host_id(&self) -> Option<NodeId> {
        self.mesh.as_ref().map(|m| *m.id())
    }

    pub fn host_id_hex(&self) -> Option<String> {
        self.host_id().map(|id| mesh::id_to_hex(&id))
    }

    /// UDP port this node's mesh is bound to, or `None` if no mesh.
    pub fn mesh_port(&self) -> Option<u16> {
        self.mesh.as_ref().map(|m| m.local_port())
    }

    /// Number of in-process units (= sibling count + 1 from any unit's view).
    pub fn host_unit_count(&self) -> usize {
        self.host.len()
    }

    /// Spawn `n` in-process units, inject host-aware Forth constants per unit,
    /// and re-advertise the new unit count via the mesh's heartbeat field.
    pub fn spawn_n(&mut self, n: usize) -> usize {
        let before = self.host.len();
        let count = self.host.spawn_n(n);
        let host_hex = self.host_id_hex().unwrap_or_default();
        for i in before..self.host.len() {
            inject_host_constants(&mut self.host.units[i].vm, &host_hex, i);
        }
        // Update each existing unit's SIBLING-COUNT variable (in case more
        // siblings just appeared). New siblings reflect host.len() - 1.
        let siblings = self.host.len().saturating_sub(1) as i64;
        for slot in self.host.units.iter_mut() {
            slot.vm.eval(&format!("{} _SIBLINGS !", siblings));
        }
        // Advertise unit count + current resource headroom via the heartbeat,
        // then trigger one now so peers learn quickly.
        if let Some(ref m) = self.mesh {
            m.set_load(self.host.len() as u32);
            m.set_headroom(crate::resources::HostResources::measure().advertised_headroom_pct());
            m.force_heartbeat();
        }
        count
    }

    /// Re-measure this coordinate's resources and re-advertise its headroom on
    /// the heartbeat. Cheap; callers can invoke on tick so the gossiped view
    /// stays current as load changes. No-op without a mesh.
    pub fn advertise_resources(&self) {
        if let Some(ref m) = self.mesh {
            m.set_headroom(crate::resources::HostResources::measure().advertised_headroom_pct());
        }
    }

    /// Snapshot of remote processes seen via the mesh, with their advertised
    /// in-process unit counts (i.e. peer.load). Excludes self.
    pub fn remote_processes(&self) -> Vec<RemoteProcess> {
        let mesh = match self.mesh.as_ref() {
            Some(m) => m,
            None => return Vec::new(),
        };
        let my_id = *mesh.id();
        mesh.peer_resource_view()
            .into_iter()
            .filter(|(id, _, _, _)| *id != my_id)
            .map(|(id, load, headroom, addr)| RemoteProcess {
                host_id: id,
                host_id_hex: mesh::id_to_hex(&id),
                units_hosted: load,
                advertised_headroom: headroom,
                addr,
            })
            .collect()
    }

    /// Send a payload to a specific remote process by host id. The payload is
    /// wrapped as `(host-msg :to "<hex>" :from "<hex>" :payload "<text>")` and
    /// sent via the existing mesh.send_sexp gossip path. Returns true if the
    /// target was found in the peer table and a packet was put on the wire.
    pub fn send_to_process(&self, target: &NodeId, payload: &str) -> bool {
        let mesh = match self.mesh.as_ref() {
            Some(m) => m,
            None => return false,
        };
        let target_addr = mesh
            .peer_unit_counts()
            .into_iter()
            .find(|(id, _, _)| id == target)
            .map(|(_, _, addr)| addr);
        let addr = match target_addr {
            Some(a) => a,
            None => return false,
        };
        let from_hex = mesh::id_to_hex(mesh.id());
        let to_hex = mesh::id_to_hex(target);
        // Escape double quotes in payload to keep the s-expression parseable.
        let safe = payload.replace('"', "'");
        let sexp = format!(
            "(host-msg :to \"{}\" :from \"{}\" :payload \"{}\")",
            to_hex, from_hex, safe
        );
        mesh.send_sexp_to(addr, &sexp);
        true
    }

    /// True iff THIS coordinate is mislocated: it is over the ceiling — local
    /// `has_headroom()` is false. The honest trigger is local resource
    /// pressure; there is no separate mislocation score. A coordinate with
    /// local headroom is content and never tries to relocate its units.
    pub fn is_mislocated(&self, local: &crate::resources::HostResources) -> bool {
        crate::transport::is_mislocated(local)
    }

    /// Two-tier destination from this node's own gossiped resource view. This
    /// delegates to [`transport::choose_destination`](crate::transport::choose_destination)
    /// — the single source of truth for the rule (abundant → emptiest with a
    /// random tie-break; else first-sufficient) — over candidates built from the
    /// live peer view, then maps the chosen address back to its `RemoteProcess`.
    /// `&mut self` because the tie-break advances the per-node RNG.
    pub fn choose_destination(&mut self) -> Option<RemoteProcess> {
        let remotes = self.remote_processes();
        let candidates: Vec<crate::transport::Candidate> = remotes
            .iter()
            .map(|p| crate::transport::Candidate {
                headroom_pct: p.advertised_headroom,
                addr: p.addr,
            })
            .collect();
        let chosen_addr = crate::transport::choose_destination(&candidates, &mut self.rng)?.addr;
        remotes.into_iter().find(|p| p.addr == chosen_addr)
    }

    /// Relocate unit `idx` to `dest_addr`, performing the actual transport via
    /// the injected `send` closure (the real
    /// [`send_transport`](crate::transport::send_transport) in production, a
    /// stub in tests). It captures the unit's complete self as serialized USAV
    /// bytes and hands them to `send`.
    ///
    /// Confirm-before-release at the placement layer: the origin slot is
    /// retired (removed from the host) ONLY on `Ok(ConfirmOutcome::Accepted)` —
    /// a confirmed live copy on the destination. On any `Err` the slot is
    /// retained, untouched; the unit keeps running exactly as it was. A peer
    /// that lied about its headroom simply refuses at the transport layer, so
    /// `send` returns `Err` and the unit stays — no detection, no blacklist.
    pub fn relocate_unit_with<S>(
        &mut self,
        idx: usize,
        send: S,
    ) -> Result<crate::transport::ConfirmOutcome, crate::transport::TransportError>
    where
        S: FnOnce(&[u8]) -> Result<crate::transport::ConfirmOutcome, crate::transport::TransportError>,
    {
        if idx >= self.host.units.len() {
            return Err(crate::transport::TransportError::Io("no such unit".into()));
        }
        // Capture the complete self (USAV bytes) — the transport payload.
        let snap = self.host.units[idx].vm.make_snapshot();
        let payload = crate::persist::serialize_snapshot(&snap);
        let outcome = send(&payload);
        if crate::transport::should_release(&outcome) {
            // Released: a live copy exists elsewhere. Retire the origin slot.
            self.host.units.remove(idx);
        }
        outcome
    }

    /// One tick of the persistent run loop, factored out so the loop body is
    /// testable without sockets or sleeps. Returns a [`TickReport`].
    ///
    /// In order: (a) drain and dispatch inbound mesh work; (b) advance each
    /// unit's metabolism one step; (c) every unworked (idle) unit runs one
    /// bounded GP-EVOLVE step — the "a unit with no work evolves" principle;
    /// (d) the local placement rule — if this host is mislocated (over the
    /// ceiling per `local`), pick a unit and attempt a sufficient-first
    /// transport with confirm-before-release.
    ///
    /// `local` is the host resource reading (caller-measured in production,
    /// test-injected in tests). `transport` performs the actual relocation:
    /// given the chosen destination and the serialized self, return the confirm
    /// outcome (the real `send_transport` in production, a stub in tests). It is
    /// invoked at most once, only when mislocated with a sufficient destination.
    /// GP-EVOLVE caps itself at a fixed batch of generations and is
    /// energy-gated, so a tick can't run away and a starving unit pauses.
    pub fn tick<S>(&mut self, local: &crate::resources::HostResources, transport: S) -> TickReport
    where
        S: FnOnce(
            &RemoteProcess,
            &[u8],
        )
            -> Result<crate::transport::ConfirmOutcome, crate::transport::TransportError>,
    {
        // a. inbound mesh work (also absorbs antibodies from remote
        //    death-cries; see `scavenged_last_drain`).
        let dispatched = self.drain_and_dispatch();
        let scavenged_words = self.scavenged_last_drain;

        // b. metabolism: famine, then passive regen + starvation accounting
        //    for every unit.
        //
        //    Famine prices host memory scarcity into the energy economy: a
        //    host stuck over its resource ceiling cannot feed everyone, so
        //    each resident is taxed in proportion to the overshoot. The
        //    weakest pin at the hard floor and die through the ordinary
        //    mortality path below (bequeathing antibodies), and the
        //    population shrinks toward the host's carrying capacity.
        //    Emigration (step d) runs every tick and is the cheaper escape;
        //    famine only kills while the colony has nowhere left to shed —
        //    without it, colony-wide overcommit ends with the kernel
        //    OOM-killing a whole node instead of units starving (observed
        //    in the 2026-08-31 scarcity soak). An invalid measurement
        //    taxes nothing: fail toward life.
        let mut famine_acute = false;
        let famine_tax = if local.valid && !local.has_headroom() {
            let overshoot = ((local.utilization - crate::resources::CEILING_UTILIZATION)
                / (1.0 - crate::resources::CEILING_UTILIZATION))
                .clamp(0.0, 1.0);
            famine_acute = overshoot >= crate::energy::FAMINE_ACUTE_OVERSHOOT;
            let mut tax = 1 + (overshoot * (crate::energy::FAMINE_TAX_MAX - 1) as f64) as i64;
            if famine_acute {
                tax *= crate::energy::FAMINE_ACUTE_MULTIPLIER;
            }
            for slot in self.host.units.iter_mut() {
                // Foraging luck: each unit draws 50–150% of the tax. Without
                // per-unit variance a colony of same-aged units pins at the
                // hard floor in lockstep and mortality arrives as one
                // synchronized avalanche that blows far past carrying
                // capacity (observed: 102 deaths in one second, a node
                // famined from 300 units down to 38 against a ~240
                // capacity). Variance staggers the deaths so the famine can
                // lift between waves.
                let luck = 50 + slot.vm.rng.next_usize(101) as i64;
                slot.vm.energy.famine((tax * luck / 100).max(1));
            }
            tax
        } else {
            // Abundance: the habitat feeds. Below the rebound threshold,
            // residents earn extra regen in proportion to unused headroom —
            // the income side of the energy-tracks-habitat symmetry whose
            // expense side is the famine tax above. This is what funds
            // rebound births: without it, post-famine survivors hover at
            // poverty and the population never regrows.
            if local.valid && local.utilization < REBOUND_UTILIZATION {
                let fraction =
                    ((REBOUND_UTILIZATION - local.utilization) / REBOUND_UTILIZATION).clamp(0.0, 1.0);
                // Floored at 1: any under-threshold habitat feeds at least
                // a little. A pure linear fade rounds to zero just below
                // the threshold, leaving a dead zone where a node is
                // "abundant" but earns nothing — recovery then stalls
                // asymptotically (run 8: a crisis node parked at 67% util
                // managed 2 births in 3 hours).
                let bonus =
                    ((fraction * crate::energy::ABUNDANCE_REGEN_MAX as f64) as i64).max(1);
                for slot in self.host.units.iter_mut() {
                    slot.vm.energy.earn(bonus, "abundance");
                }
            }
            0
        };
        for slot in self.host.units.iter_mut() {
            slot.vm.energy.tick();
        }

        // c. unworked units run their LIVE word — the dictionary-resident
        //    life loop (prelude default: GP-EVOLVE). The host provides the
        //    tick; the genome decides what living is. Metering makes a
        //    runaway LIVE starve instead of hanging the host; a unit
        //    without LIVE idles.
        let mut evolved_units = 0;
        let budget = LIVE_BUDGET_PER_TICK.min(self.host.units.len());
        let live_deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(LIVE_TICK_BUDGET_MS);
        for _ in 0..budget {
            if self.host.units.is_empty() {
                break;
            }
            // Time box: the count budget is the deterministic ceiling, the
            // wall clock the real one. At least one unit always lives.
            if evolved_units > 0 && std::time::Instant::now() >= live_deadline {
                break;
            }
            let i = self.live_cursor % self.host.units.len();
            self.live_cursor = self.live_cursor.wrapping_add(1);
            if !self.host.units[i].busy {
                self.host.units[i].busy = true;
                if self.host.units[i].vm.find_word("LIVE").is_some() {
                    self.host.units[i].vm.eval("LIVE");
                }
                self.host.units[i].busy = false;
                evolved_units += 1;
                // Route anything LIVE emitted — SAY!/MARK! signals and GIVE
                // energy gifts — so evolved social behavior actually flows
                // between siblings every tick.
                self.host.route_signals_from(i);
            }
        }

        // c1b. starvation accounting, AFTER the LIVE phase so it reads the
        //     post-burn balance. A unit pinned at the hard floor is living
        //     beyond its means; consecutive pinned ticks count toward
        //     death, and ordinary GP debt hovers above the floor and
        //     resets nothing. Checking pre-LIVE let abundance income lift
        //     a runaway unit out of the floor zone each tick before its
        //     burn — making unsustainable lifestyles immortal in a rich
        //     habitat. Metabolize, live, then account.
        for slot in self.host.units.iter_mut() {
            if slot.vm.energy.at_hard_floor() {
                // Acute famine burns the fuse FAMINE_ACUTE_FUSE ticks per
                // tick: with the kernel seconds from killing the whole
                // host, individual deaths must land first so their freed
                // memory (trimmed on the death path) relieves the node.
                slot.starved_ticks += if famine_acute { FAMINE_ACUTE_FUSE } else { 1 };
            } else {
                slot.starved_ticks = 0;
            }
        }

        // c2. mortality: a unit at the hard floor for STARVED_TICKS_TO_DIE
        //     consecutive ticks dies. Its final act bequeaths its immune
        //     memory — SOL-* antibodies go to local siblings directly and
        //     to the mesh as a death-cry. The failed life strategy dies
        //     with the unit; the solved-challenge knowledge survives it.
        let mut deaths = Vec::new();
        let mut i = 0;
        while i < self.host.units.len() {
            if self.host.units[i].starved_ticks >= STARVED_TICKS_TO_DIE {
                let slot = self.host.units.remove(i);
                let antibodies = harvest_antibodies(&slot.vm);
                let fitness = slot.vm.fitness.score;
                let generation = slot.vm.spawn_state.generation;
                let mut heirs = 0;
                for s in self.host.units.iter_mut() {
                    if s.vm.absorb_antibodies(&antibodies) > 0 {
                        heirs += 1;
                    }
                }
                if let Some(ref m) = self.mesh {
                    let from = mesh::id_to_hex(m.id());
                    let cry =
                        crate::sexp::msg_death_cry(&from, fitness, generation, &antibodies);
                    m.send_sexp(&cry.to_string());
                }
                deaths.push(UnitDeath {
                    fitness,
                    generation,
                    antibodies: antibodies.len(),
                    heirs,
                });
            } else {
                i += 1;
            }
        }
        // c3. rebound: births into measured headroom. Famine's demographic
        //     other half — after deaths or emigration have thinned a host,
        //     nothing else regrows the population, and post-crisis colonies
        //     stayed at a fraction of carrying capacity (soak run 6: a
        //     ~240-capacity node held 32 units for two hours). The rule is
        //     local and surplus-driven: comfortably under the ceiling, at
        //     most one birth per interval, and only a unit still rich after
        //     paying reproduction's full price may breed. The child gets
        //     the parent's endowment (no energy minted) and inherits its
        //     antibodies — birth passes immune knowledge down the way death
        //     bequeaths it sideways.
        self.ticks_total = self.ticks_total.wrapping_add(1);
        let mut birth = None;
        if local.valid
            && local.utilization < REBOUND_UTILIZATION
            && !self.host.is_full()
            && deaths.is_empty()
            && famine_tax == 0
            && self.ticks_total.wrapping_sub(self.last_birth_tick) >= REBOUND_INTERVAL_TICKS
        {
            let full_price =
                crate::energy::SPAWN_COST + BIRTH_ENDOWMENT + REBOUND_PARENT_MIN;
            let parent = self
                .host
                .units
                .iter()
                .enumerate()
                .filter(|(_, sl)| sl.vm.energy.energy >= full_price)
                .max_by_key(|(_, sl)| sl.vm.energy.energy)
                .map(|(i, _)| i);
            if let Some(pi) = parent {
                let paid = self.host.units[pi]
                    .vm
                    .energy
                    .spend(crate::energy::SPAWN_COST + BIRTH_ENDOWMENT, "rebound-birth");
                if paid {
                    let antibodies = harvest_antibodies(&self.host.units[pi].vm);
                    let child_gen = self.host.units[pi].vm.spawn_state.generation + 1;
                    if let Some(ci) = self.host.spawn() {
                        let child = &mut self.host.units[ci].vm;
                        child.energy.energy = BIRTH_ENDOWMENT;
                        child.spawn_state.generation = child_gen;
                        child.absorb_antibodies(&antibodies);
                        self.last_birth_tick = self.ticks_total;
                        birth = Some((child_gen, BIRTH_ENDOWMENT));
                    }
                }
            }
        }

        // Read the GP engine's own best (evolution.best.fitness), not the
        // mesh fitness ledger (fitness.score) — GP-EVOLVE never writes the
        // latter, which made this report claim "best fitness 0" on live
        // boxes while evolution was visibly progressing.
        let best_fitness = self
            .host
            .units
            .iter()
            .filter_map(|s| s.vm.evolution.as_ref())
            .filter_map(|e| e.best.as_ref())
            .map(|b| b.fitness as i64)
            .max()
            .unwrap_or(0);

        // d. local placement rule: relocate only when over the ceiling AND a
        //    sufficient destination exists. Honesty selected, not policed — a
        //    refused/failed transport leaves the origin unit in place.
        let mislocated = self.is_mislocated(local);
        let transport_outcome = if mislocated && !self.host.units.is_empty() {
            match self.choose_destination() {
                Some(dest) => {
                    let target_hex = dest.host_id_hex.clone();
                    let target_headroom = dest.advertised_headroom;
                    // Move the youngest unit; over successive ticks this sheds
                    // load incrementally rather than evacuating all at once.
                    let idx = self.host.units.len() - 1;
                    let outcome = self.relocate_unit_with(idx, |payload| transport(&dest, payload));
                    Some(TickTransport::Attempted {
                        target_hex,
                        target_headroom,
                        outcome,
                    })
                }
                None => Some(TickTransport::NoDestination),
            }
        } else {
            None
        };

        TickReport {
            dispatched,
            evolved_units,
            deaths,
            scavenged_words,
            best_fitness,
            mislocated,
            famine_tax,
            birth,
            transport: transport_outcome,
        }
    }

    /// Drain any pending mesh messages. For each `(host-msg :to <us> ...)`
    /// envelope, dispatch the payload to one of our in-process units via
    /// `host.execute_goal` (least-busy picker) and record the result. Other
    /// messages (heartbeats, other s-expressions) are left to be handled by
    /// callers that need them.
    pub fn drain_and_dispatch(&mut self) -> Vec<DispatchedRemoteMsg> {
        let mut events = Vec::new();
        self.scavenged_last_drain = 0;
        let (raw_msgs, my_hex) = match self.mesh.as_ref() {
            Some(m) => (m.recv_sexp_messages(), mesh::id_to_hex(m.id())),
            None => return events,
        };
        for raw in raw_msgs {
            let parsed = match crate::sexp::try_parse_mesh_msg(&raw) {
                Some(s) => s,
                None => continue,
            };
            // A peer's dying unit bequeathed its immune memory: absorb the
            // (trust-gated: SOL-* only, bounded) antibodies into every
            // local unit that lacks them. The colony eats its dead.
            if let Some(antibodies) = crate::sexp::read_death_cry(&parsed) {
                for slot in self.host.units.iter_mut() {
                    self.scavenged_last_drain += slot.vm.absorb_antibodies(&antibodies);
                }
                continue;
            }
            if crate::sexp::msg_type(&parsed) != Some("host-msg") {
                continue;
            }
            let to = parsed
                .get_key(":to")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            if to != my_hex {
                continue;
            }
            let from = parsed
                .get_key(":from")
                .and_then(|s| s.as_str())
                .unwrap_or("?")
                .to_string();
            let payload = parsed
                .get_key(":payload")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            if payload.is_empty() {
                continue;
            }
            if let Some(r) = self.host.execute_goal(&payload) {
                // Refresh _SIBLINGS in case spawn happened mid-flight; cheap.
                let siblings = self.host.len().saturating_sub(1) as i64;
                self.host.units[r.unit_index]
                    .vm
                    .eval(&format!("{} _SIBLINGS !", siblings));
                events.push(DispatchedRemoteMsg {
                    from_host_hex: from,
                    unit_index: r.unit_index,
                    output: r.output,
                });
            }
        }
        // Refresh each unit's MESH-PROCESS-COUNT variable from the live table.
        let remotes = self.remote_processes().len() as i64;
        for slot in self.host.units.iter_mut() {
            slot.vm.eval(&format!("{} _REMOTES !", remotes));
        }
        events
    }
}

/// Inject the host-aware constants and variables that a unit's Forth source
/// can read. Mirrors the WASM model's BROWSER-PEERS pattern (web/unit.js:83):
/// constants for stable values, VARIABLEs for ones the host updates.
fn inject_host_constants(vm: &mut crate::vm::VM, host_id_hex: &str, unit_idx: usize) {
    // Constants — set once per unit.
    vm.eval(&format!(": HOST-ID .\" {}\" CR ;", host_id_hex));
    vm.eval(&format!(": UNIT-IDX {} ;", unit_idx));
    // Live values — backed by host-updated variables.
    vm.eval("VARIABLE _SIBLINGS 0 _SIBLINGS !");
    vm.eval(": SIBLING-COUNT _SIBLINGS @ ;");
    vm.eval("VARIABLE _REMOTES 0 _REMOTES !");
    vm.eval(": MESH-PROCESS-COUNT _REMOTES @ ;");
}

#[cfg(test)]
mod bridge_tests {
    use super::*;
    use std::time::Duration;

    /// Helper: spin two MultiUnitNodes pointing at each other on loopback,
    /// force a heartbeat exchange, and return them. Eviction is bypassed
    /// for the tests' wall-time (heartbeats every 2s otherwise).
    fn pair(units_a: usize, units_b: usize) -> (MultiUnitNode, MultiUnitNode) {
        let mut a = MultiUnitNode::new(64, Some(0), vec![]).expect("start a");
        a.spawn_n(units_a);
        let a_addr: SocketAddr = format!("127.0.0.1:{}", a.mesh_port().unwrap())
            .parse()
            .unwrap();
        let mut b = MultiUnitNode::new(64, Some(0), vec![a_addr]).expect("start b");
        b.spawn_n(units_b);
        // Bidirectional heartbeat exchange so each peer table contains the other.
        for _ in 0..3 {
            a.mesh.as_ref().unwrap().force_heartbeat();
            b.mesh.as_ref().unwrap().force_heartbeat();
            std::thread::sleep(Duration::from_millis(20));
        }
        (a, b)
    }

    #[test]
    fn host_id_is_set_and_stable() {
        let mut a = MultiUnitNode::new(8, Some(0), vec![]).unwrap();
        a.spawn_n(2);
        let id1 = a.host_id().unwrap();
        let id2 = a.host_id().unwrap();
        assert_eq!(id1, id2);
        assert_eq!(a.host_id_hex().unwrap().len(), 16);
    }

    #[test]
    fn sibling_count_excludes_self() {
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(4);
        // From any unit's view: 3 siblings.
        let out = a.host.units[0].vm.eval("SIBLING-COUNT .");
        assert!(out.contains('3'), "out: {:?}", out);
    }

    #[test]
    fn remote_processes_excludes_self_and_includes_unit_count() {
        let (mut a, b) = pair(2, 3);
        let _ = a.drain_and_dispatch(); // ignore any stray heartbeat envelopes
        let remotes = a.remote_processes();
        // a's table should contain exactly b (one peer), with units_hosted = 3.
        let b_id = b.host_id().unwrap();
        let entry = remotes
            .iter()
            .find(|r| r.host_id == b_id)
            .expect("b not visible from a");
        assert_eq!(entry.units_hosted, 3, "b advertised wrong unit count");
        assert!(
            !remotes.iter().any(|r| r.host_id == a.host_id().unwrap()),
            "remote_processes must exclude self"
        );
    }

    #[test]
    fn cross_process_message_is_dispatched_to_a_local_unit() {
        let (mut a, mut b) = pair(2, 3);
        let _ = a.drain_and_dispatch();
        let _ = b.drain_and_dispatch();
        let b_id = b.host_id().unwrap();
        // a sends a Forth fragment to b; b should dispatch to one of its units.
        assert!(a.send_to_process(&b_id, "2 3 + ."));
        // Give the OS a moment to deliver the UDP packet.
        std::thread::sleep(Duration::from_millis(50));
        let dispatched = b.drain_and_dispatch();
        assert_eq!(dispatched.len(), 1, "expected 1 dispatched msg, got {:?}", dispatched);
        let ev = &dispatched[0];
        assert!(ev.unit_index < b.host.len());
        assert!(
            ev.output.contains('5'),
            "expected `5` in dispatched output: {:?}",
            ev.output
        );
        // The dispatched unit's tasks_completed should have incremented.
        assert_eq!(b.host.units[ev.unit_index].tasks_completed, 1);
    }

    #[test]
    fn host_crash_evicts_peer_from_remote_table() {
        let (mut a, b) = pair(2, 2);
        let _ = a.drain_and_dispatch();
        // Sanity: a sees b.
        let b_id = b.host_id().unwrap();
        assert!(a.remote_processes().iter().any(|r| r.host_id == b_id));
        // Drop b; its mesh thread shuts down and heartbeats stop.
        drop(b);
        // Wait long enough that b's last_seen is stale by our threshold.
        std::thread::sleep(Duration::from_millis(80));
        // Force a's prune with a 50ms threshold — b's entry is older than that.
        let evicted = a
            .mesh
            .as_ref()
            .unwrap()
            .evict_peers_older_than(Duration::from_millis(50));
        assert!(evicted >= 1, "expected to evict at least 1 stale peer");
        assert!(
            !a.remote_processes().iter().any(|r| r.host_id == b_id),
            "b should be gone from a's remote_processes after eviction"
        );
    }

    #[test]
    fn host_constants_are_per_unit() {
        let mut a = MultiUnitNode::new(8, Some(0), vec![]).unwrap();
        a.spawn_n(3);
        // UNIT-IDX should differ per unit.
        for i in 0..3 {
            let out = a.host.units[i].vm.eval("UNIT-IDX .");
            assert!(
                out.contains(&i.to_string()),
                "unit {} UNIT-IDX out: {:?}",
                i,
                out
            );
        }
    }

    // -----------------------------------------------------------------------
    // Resource-aware placement (PART A gossip + PART B sufficient-first)
    // -----------------------------------------------------------------------

    /// Force a few more heartbeats from `b` so `a` re-learns its advertisement.
    fn settle_heartbeats(a: &MultiUnitNode, b: &MultiUnitNode) {
        for _ in 0..3 {
            b.mesh.as_ref().unwrap().force_heartbeat();
            a.mesh.as_ref().unwrap().force_heartbeat();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn gossiped_headroom_surfaces_on_remote_processes() {
        let (mut a, b) = pair(1, 1);
        // b advertises a known headroom; round-trip it through the heartbeat.
        b.mesh.as_ref().unwrap().set_headroom(57);
        settle_heartbeats(&a, &b);
        let _ = a.drain_and_dispatch();
        let b_id = b.host_id().unwrap();
        let entry = a
            .remote_processes()
            .into_iter()
            .find(|r| r.host_id == b_id)
            .expect("b not visible from a");
        assert_eq!(
            entry.advertised_headroom, 57,
            "advertised headroom must round-trip the heartbeat"
        );
    }

    #[test]
    fn node_choose_destination_picks_sufficient_peer() {
        let (mut a, b) = pair(1, 1);
        b.mesh.as_ref().unwrap().set_headroom(60); // > 20 → sufficient
        settle_heartbeats(&a, &b);
        let _ = a.drain_and_dispatch();
        let dest = a.choose_destination().expect("b advertises room");
        assert_eq!(dest.host_id, b.host_id().unwrap());
    }

    #[test]
    fn node_choose_destination_none_when_no_peer_sufficient() {
        let (mut a, b) = pair(1, 1);
        b.mesh.as_ref().unwrap().set_headroom(5); // < 20 → insufficient
        settle_heartbeats(&a, &b);
        let _ = a.drain_and_dispatch();
        assert!(
            a.choose_destination().is_none(),
            "no peer advertises sufficient room"
        );
    }

    #[test]
    fn node_is_mislocated_tracks_local_headroom() {
        let a = MultiUnitNode::new(8, None, vec![]).unwrap();
        let healthy = crate::resources::HostResources::from_parts(1000, 500, 0.0, 4);
        assert!(!a.is_mislocated(&healthy), "with room a unit never flees");
        let pressed = crate::resources::HostResources::from_parts(1000, 50, 0.0, 4);
        assert!(a.is_mislocated(&pressed), "over the ceiling → mislocated");
    }

    #[test]
    fn relocate_retires_origin_only_on_confirmed_copy() {
        use crate::transport::{ConfirmOutcome, TransportError};
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(3);
        assert_eq!(a.host.len(), 3);

        // Transport REFUSED (or any Err) → origin slot retained, untouched.
        let out = a.relocate_unit_with(1, |_payload| Err(TransportError::Refused));
        assert!(out.is_err());
        assert_eq!(
            a.host.len(),
            3,
            "a refused/failed transport must NOT retire the origin"
        );

        // Transport ACCEPTED → confirmed live copy → origin slot retired.
        let out = a.relocate_unit_with(1, |payload| {
            assert!(!payload.is_empty(), "the complete self must be captured");
            Ok(ConfirmOutcome::Accepted)
        });
        assert!(matches!(out, Ok(ConfirmOutcome::Accepted)));
        assert_eq!(
            a.host.len(),
            2,
            "confirmed live copy → origin slot retired (released)"
        );
    }

    // A reading with ample headroom (50% util, under the 80% ceiling) and one
    // over the ceiling (95% util). from_parts is pub(crate).
    fn under_ceiling_reading() -> crate::resources::HostResources {
        crate::resources::HostResources::from_parts(1000, 500, 0.0, 4)
    }
    fn over_ceiling_reading() -> crate::resources::HostResources {
        crate::resources::HostResources::from_parts(1000, 50, 0.0, 4)
    }
    // A transport stub that must never run (asserts the rule didn't fire).
    fn never_transport(
        _d: &RemoteProcess,
        _p: &[u8],
    ) -> Result<crate::transport::ConfirmOutcome, crate::transport::TransportError> {
        panic!("transport must not be attempted");
    }

    #[test]
    fn tick_on_lone_node_is_safe_noop() {
        // No peers, no inbound work, under ceiling: tick drains nothing,
        // retires nothing, attempts no transport, and does not panic.
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(2);
        let report = a.tick(&under_ceiling_reading(), never_transport);
        assert!(report.dispatched.is_empty());
        assert!(!report.mislocated);
        assert!(report.transport.is_none());
        assert_eq!(a.host.len(), 2, "tick must not retire units with no work");
    }

    #[test]
    fn tick_evolves_unworked_units_and_advances_metabolism() {
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(2);
        // Fresh units have no evolution state.
        assert!(a.host.units[0].vm.evolution.is_none());

        let report = a.tick(&under_ceiling_reading(), never_transport);

        // Both idle units took the no-work fall-through into GP-EVOLVE.
        assert_eq!(report.evolved_units, 2, "every unworked unit evolves");
        assert!(
            a.host.units[0].vm.evolution.is_some(),
            "GP-EVOLVE initialized evolution state"
        );
        assert!(a.host.units[1].vm.evolution.is_some());
        // Metabolism advanced: passive regen recorded as earned energy.
        assert!(
            a.host.units[0].vm.energy.total_earned >= 1,
            "metabolism ticked (passive regen)"
        );
    }

    #[test]
    fn tick_report_reads_gp_best_not_mesh_fitness_score() {
        // Regression: the tick report read vm.fitness.score (the mesh
        // fitness ledger, which GP-EVOLVE never writes) and reported
        // "best fitness 0" on live boxes while evolution progressed.
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(1);

        // Seed an UNFINISHED GP run (a finished one is now reaped so the
        // next tick can take the next rung — the post-win no-op fix) with an
        // unbeatable best, so the tick's LIVE pass continues it but cannot
        // change the max the report must surface.
        let mut evo = crate::evolve::EvolutionState::new(crate::evolve::fib10_challenge(), 5000);
        evo.running = true;
        evo.generation = 1;
        let mut best = crate::evolve::Candidate::new("0 1 10 0 DO OVER + SWAP LOOP DROP .");
        best.fitness = 5432.0;
        evo.best = Some(best);
        a.host.units[0].vm.evolution = Some(evo);
        // The mesh ledger stays at 0 — the old code would report 0 here.
        assert_eq!(a.host.units[0].vm.fitness.score, 0);

        let report = a.tick(&under_ceiling_reading(), never_transport);
        assert_eq!(
            report.best_fitness, 5432,
            "report must surface evolution.best.fitness, not fitness.score"
        );
    }

    #[test]
    fn tick_under_ceiling_never_transports() {
        // Even with a sufficient peer visible, a host with local headroom is
        // content and never relocates a unit.
        let (mut a, b) = pair(1, 1);
        b.mesh.as_ref().unwrap().set_headroom(60);
        settle_heartbeats(&a, &b);
        let _ = a.drain_and_dispatch();
        let report = a.tick(&under_ceiling_reading(), never_transport);
        assert!(!report.mislocated);
        assert!(report.transport.is_none());
        assert_eq!(a.host.len(), 1, "content host keeps its unit");
    }

    #[test]
    fn tick_over_ceiling_with_sufficient_peer_transports() {
        use crate::transport::ConfirmOutcome;
        let (mut a, b) = pair(1, 1);
        b.mesh.as_ref().unwrap().set_headroom(60); // > 20 → sufficient
        settle_heartbeats(&a, &b);
        let _ = a.drain_and_dispatch();
        let b_id_hex = b.host_id_hex().unwrap();

        // Over the ceiling + a sufficient peer → transport attempted. The
        // injected stub confirms acceptance, so the origin slot is retired.
        let report = a.tick(&over_ceiling_reading(), |dest, payload| {
            assert_eq!(dest.host_id_hex, b_id_hex, "sufficient-first picked b");
            assert!(!payload.is_empty(), "the complete self was captured");
            Ok(ConfirmOutcome::Accepted)
        });
        assert!(report.mislocated, "over ceiling → mislocated");
        match report.transport {
            Some(TickTransport::Attempted {
                target_hex,
                target_headroom,
                outcome,
            }) => {
                assert_eq!(target_hex, b.host_id_hex().unwrap());
                assert_eq!(target_headroom, 60);
                assert!(matches!(outcome, Ok(ConfirmOutcome::Accepted)));
            }
            other => panic!("expected Attempted, got {:?}", other.is_some()),
        }
        assert_eq!(a.host.len(), 0, "confirmed live copy → origin retired");
    }

    #[test]
    fn tick_over_ceiling_no_sufficient_peer_stays_put() {
        // Over the ceiling, but the only peer is itself tight → no destination,
        // the unit stays. Honesty selected: we don't force a bad placement.
        let (mut a, b) = pair(1, 1);
        b.mesh.as_ref().unwrap().set_headroom(5); // insufficient
        settle_heartbeats(&a, &b);
        let _ = a.drain_and_dispatch();
        let report = a.tick(&over_ceiling_reading(), never_transport);
        assert!(report.mislocated);
        assert!(matches!(report.transport, Some(TickTransport::NoDestination)));
        assert_eq!(a.host.len(), 1, "no sufficient peer → unit stays");
    }

    // --- mortality (needs the tick harness helpers above) ---

    #[test]
    fn sustained_floor_starvation_is_death_and_bequeaths_antibodies() {
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(2);
        // Unit 0: has immune memory, and a lifestyle it cannot afford.
        // Energy is the post-burn pinned state (-500): the tick's regen
        // lifts it to -499, still inside the at-floor zone — exactly the
        // steady state a runaway LIVE produces every tick.
        a.host.units[0].vm.eval(": SOL-TESTFACT 99 ;");
        a.host.units[0].vm.eval(": LIVE BEGIN 0 UNTIL ;");
        a.host.units[0].vm.energy.energy = -500;
        a.host.units[0].starved_ticks = STARVED_TICKS_TO_DIE - 1;

        let report = a.tick(&under_ceiling_reading(), never_transport);

        assert_eq!(report.deaths.len(), 1, "the starved unit died");
        assert_eq!(a.host.len(), 1, "the dead unit was removed");
        assert_eq!(report.deaths[0].antibodies, 1, "bequeathed SOL-TESTFACT");
        assert_eq!(report.deaths[0].heirs, 1, "the sibling inherited");
        // The survivor can actually run the inherited word.
        let out_top = {
            let vm = &mut a.host.units[0].vm;
            vm.eval("SOL-TESTFACT");
            vm.stack.last().copied()
        };
        assert_eq!(out_top, Some(99), "inherited antibody executes");
    }

    #[test]
    fn ordinary_gp_debt_is_not_death() {
        // GP's own energy gate hovers debt above the hard floor; that must
        // never read as at-floor, so default colonies do not self-extinguish.
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(1);
        a.host.units[0].vm.energy.energy = -495; // the GP hover zone
        let _ = a.tick(&under_ceiling_reading(), never_transport);
        assert_eq!(a.host.len(), 1, "unit alive");
        // Note: at -495 the unit is NOT at_hard_floor (floor zone is <= -498),
        // so starved_ticks must not have advanced from the accounting pass.
        assert_eq!(
            a.host.units[0].starved_ticks, 0,
            "GP debt does not accumulate toward death"
        );
    }

    #[test]
    fn famine_taxes_only_over_ceiling_hosts() {
        // Idle LIVE words make the energy accounting exact: no GP costs.
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(2);
        for slot in a.host.units.iter_mut() {
            slot.vm.eval(": LIVE ;");
            slot.vm.energy.energy = 1000;
        }
        // Under the ceiling: passive regen plus the abundance bonus
        // (50% util → +2); no famine.
        let _ = a.tick(&under_ceiling_reading(), never_transport);
        assert_eq!(a.host.units[0].vm.energy.energy, 1003, "no famine under ceiling");
        // Over the ceiling at 95% util: overshoot 0.75 of the post-ceiling
        // range, base tax 1 + 0.75×49 = 37, drawn per unit at 50–150%
        // foraging luck (18..=55), minus the regen the tick pays back.
        let _ = a.tick(&over_ceiling_reading(), never_transport);
        let taxed = 1003 + 1 - a.host.units[0].vm.energy.energy;
        assert!(
            (18..=55).contains(&taxed),
            "famine taxes in proportion to overshoot with per-unit luck (got {})",
            taxed
        );
        // Sibling units must NOT be taxed in lockstep — identical rng
        // streams would resynchronize starvation into avalanche mortality.
        let taxed1 = 1003 + 1 - a.host.units[1].vm.energy.energy;
        assert_ne!(taxed, taxed1, "per-unit foraging luck diverges between siblings");
    }

    #[test]
    fn famine_starves_the_weakest_toward_carrying_capacity() {
        // A host stuck over ceiling with nowhere to shed (no peers) must
        // shrink through the ordinary mortality path: its weakest resident
        // pins at the hard floor and dies, while its richest — the better
        // earner — survives on metabolic surplus. This is the organism-level
        // answer to the scarcity soak, where overcommit ended with the
        // kernel OOM-killing a whole node instead of units starving.
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(2);
        for slot in a.host.units.iter_mut() {
            slot.vm.eval(": LIVE ;");
        }
        a.host.units[0].vm.energy.energy = 5000; // rich
        a.host.units[1].vm.energy.energy = -400; // already scraping bottom
        let mut first_death_tick = None;
        for t in 0..100 {
            let report = a.tick(&over_ceiling_reading(), never_transport);
            if !report.deaths.is_empty() {
                first_death_tick = Some(t);
                break;
            }
        }
        assert!(first_death_tick.is_some(), "famine must produce a death");
        assert_eq!(a.host.len(), 1, "population shrank by exactly one");
        assert!(
            a.host.units[0].vm.energy.energy > 0,
            "the survivor is the rich unit — famine selects on surplus"
        );
    }

    #[test]
    fn acute_famine_wins_the_race_the_kernel_would_win() {
        // At util 97% (overshoot 0.85 ≥ FAMINE_ACUTE_OVERSHOOT) famine is
        // acute: tax ×4 and the starvation fuse burns 6/tick. A healthy
        // 1000-energy unit must die within ~25 ticks — gradual famine's
        // ~60–90 needed ticks is exactly how three soak nodes lost to the
        // OOM-killer.
        let acute = crate::resources::HostResources::from_parts(1000, 30, 0.0, 4);
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(1);
        a.host.units[0].vm.eval(": LIVE ;");
        a.host.units[0].vm.energy.energy = 1000;
        let mut died_at = None;
        for t in 0..40 {
            let report = a.tick(&acute, never_transport);
            if !report.deaths.is_empty() {
                died_at = Some(t);
                break;
            }
        }
        assert!(
            died_at.is_some_and(|t| t <= 25),
            "acute famine must kill within ~25 ticks (got {:?})",
            died_at
        );
        // Ordinary famine (95% util, below the acute threshold) must NOT
        // move this fast: same setup survives those same 25 ticks.
        let mut b = MultiUnitNode::new(8, None, vec![]).unwrap();
        b.spawn_n(1);
        b.host.units[0].vm.eval(": LIVE ;");
        b.host.units[0].vm.energy.energy = 1000;
        for _ in 0..25 {
            let report = b.tick(&over_ceiling_reading(), never_transport);
            assert!(report.deaths.is_empty(), "ordinary famine stays gradual");
        }
    }

    #[test]
    fn rebound_births_into_measured_headroom() {
        // Under 70% util with a rich resident: exactly one birth per
        // interval. The child carries the parent's endowment (no energy
        // minted) and inherits its antibodies — birth passes immune
        // knowledge down the way death bequeaths it sideways.
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(2);
        for slot in a.host.units.iter_mut() {
            slot.vm.eval(": LIVE ;");
        }
        a.host.units[0].vm.eval(": SOL-HERITAGE 7 ;");
        a.host.units[0].vm.energy.energy = 5000; // the qualifying parent
        a.host.units[1].vm.energy.energy = 100; // too poor to breed

        // Tick 1..=REBOUND_INTERVAL_TICKS-1: interval not yet elapsed.
        for _ in 0..(REBOUND_INTERVAL_TICKS - 1) {
            let r = a.tick(&under_ceiling_reading(), never_transport);
            assert!(r.birth.is_none(), "no birth before the interval elapses");
        }
        let r = a.tick(&under_ceiling_reading(), never_transport);
        assert!(r.birth.is_some(), "interval elapsed + headroom + surplus → birth");
        assert_eq!(a.host.len(), 3, "population grew by one");
        let child = &mut a.host.units[2].vm;
        // Born after this tick's regen pass: exactly the endowment.
        assert_eq!(child.energy.energy, BIRTH_ENDOWMENT);
        assert_eq!(child.spawn_state.generation, 1, "child is next generation");
        child.eval("SOL-HERITAGE");
        assert_eq!(child.stack.last().copied(), Some(7), "child inherited the antibody");
        // Parent paid the full price from its own reserves.
        assert!(
            a.host.units[0].vm.energy.energy
                < 5000 - crate::energy::SPAWN_COST - BIRTH_ENDOWMENT
                + REBOUND_INTERVAL_TICKS as i64 + 2,
            "parent paid SPAWN_COST + BIRTH_ENDOWMENT"
        );
    }

    #[test]
    fn rebound_never_fires_over_threshold_or_without_surplus() {
        // 75% util (over REBOUND_UTILIZATION, under the famine ceiling):
        // the stable band — no births, no famine.
        let banded = crate::resources::HostResources::from_parts(1000, 250, 0.0, 4);
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(1);
        a.host.units[0].vm.eval(": LIVE ;");
        a.host.units[0].vm.energy.energy = 5000;
        for _ in 0..(REBOUND_INTERVAL_TICKS * 3) {
            let r = a.tick(&banded, never_transport);
            assert!(r.birth.is_none(), "the [70%,80%) band must be still");
            assert_eq!(r.famine_tax, 0);
        }
        // Ample headroom but no unit can afford reproduction: no birth.
        let mut b = MultiUnitNode::new(8, None, vec![]).unwrap();
        b.spawn_n(1);
        b.host.units[0].vm.eval(": LIVE ;");
        b.host.units[0].vm.energy.energy = 500; // below the full price
        for _ in 0..(REBOUND_INTERVAL_TICKS * 3) {
            let r = b.tick(&under_ceiling_reading(), never_transport);
            assert!(r.birth.is_none(), "reproduction is a surplus behavior");
        }
        assert_eq!(b.host.len(), 1);
    }

    #[test]
    fn abundance_feeds_in_proportion_to_headroom() {
        // 50% util: fraction (0.70−0.50)/0.70 ≈ 0.286 → bonus 2, plus the
        // ordinary +1 regen. The income side of energy-tracks-habitat.
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(1);
        a.host.units[0].vm.eval(": LIVE ;");
        a.host.units[0].vm.energy.energy = 100;
        let _ = a.tick(&under_ceiling_reading(), never_transport);
        assert_eq!(a.host.units[0].vm.energy.energy, 103, "abundance bonus + regen");
        // In the [70%, 80%) band there is no habitat income and no famine:
        // just the bare +1 regen. The band is metabolically neutral.
        let banded = crate::resources::HostResources::from_parts(1000, 250, 0.0, 4);
        let mut b = MultiUnitNode::new(8, None, vec![]).unwrap();
        b.spawn_n(1);
        b.host.units[0].vm.eval(": LIVE ;");
        b.host.units[0].vm.energy.energy = 2000;
        let _ = b.tick(&banded, never_transport);
        assert_eq!(b.host.units[0].vm.energy.energy, 2001, "band is income-neutral");
        // Just under the threshold (69% util) the linear bonus computes to
        // zero but is floored at 1 — no dead zone where an "abundant" host
        // pays nothing and recovery stalls asymptotically.
        let near = crate::resources::HostResources::from_parts(1000, 310, 0.0, 4);
        let mut c = MultiUnitNode::new(8, None, vec![]).unwrap();
        c.spawn_n(1);
        c.host.units[0].vm.eval(": LIVE ;");
        c.host.units[0].vm.energy.energy = 100;
        let _ = c.tick(&near, never_transport);
        assert_eq!(c.host.units[0].vm.energy.energy, 102, "floor of 1 under threshold");
    }

    #[test]
    fn abundance_funds_post_crisis_rebound() {
        // The run-7 finding: famine survivors are paupers, breeding costs
        // ~1000 energy, and with no income the population never regrows
        // (3 births in 3 hours at 39% util). With abundance income the
        // same paupers must refatten and breed within a bounded horizon.
        let empty = crate::resources::HostResources::from_parts(1000, 610, 0.0, 4); // 39% util
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(2);
        for slot in a.host.units.iter_mut() {
            slot.vm.eval(": LIVE ;");
            slot.vm.energy.energy = 10; // post-famine poverty
        }
        let mut births = 0;
        for _ in 0..400 {
            let r = a.tick(&empty, never_transport);
            if r.birth.is_some() {
                births += 1;
            }
        }
        assert!(births >= 1, "abundance income must fund at least one birth");
        assert!(a.host.len() > 2, "population regrows under abundance");
    }

    #[test]
    fn evolved_altruism_flows_through_the_tick() {
        // A lineage that evolved generosity into LIVE: each tick it gives 10
        // to the neediest sibling. The tick must route the gift.
        let mut a = MultiUnitNode::new(8, None, vec![]).unwrap();
        a.spawn_n(2);
        a.host.units[0].vm.eval(": LIVE 10 GIVE ;");
        a.host.units[0].vm.energy.energy = 1000;
        a.host.units[1].vm.eval(": LIVE ;"); // idle sibling
        a.host.units[1].vm.energy.energy = 0;

        let _ = a.tick(&under_ceiling_reading(), never_transport);

        // Sibling got the gift (+10), passive regen (+1), and the 50%-util
        // abundance bonus (+2).
        assert_eq!(a.host.units[1].vm.energy.energy, 13, "gift arrived via the tick");
        // Donor: -11 for the gift+friction, +1 regen, +2 abundance.
        assert_eq!(a.host.units[0].vm.energy.energy, 1000 - 11 + 1 + 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_respects_cap() {
        let mut h = MultiUnitHost::new(3);
        assert_eq!(h.spawn(), Some(0));
        assert_eq!(h.spawn(), Some(1));
        assert_eq!(h.spawn(), Some(2));
        assert!(h.is_full());
        assert_eq!(h.spawn(), None);
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn spawn_n_returns_actual_count() {
        let mut h = MultiUnitHost::new(5);
        assert_eq!(h.spawn_n(3), 3);
        assert_eq!(h.spawn_n(10), 2); // only 2 slots remain
        assert_eq!(h.len(), 5);
    }

    #[test]
    fn pick_worker_picks_least_busy() {
        let mut h = MultiUnitHost::new(5);
        h.spawn_n(3);
        h.units[0].tasks_completed = 5;
        h.units[1].tasks_completed = 1;
        h.units[2].tasks_completed = 3;
        assert_eq!(h.pick_worker(), Some(1));
    }

    #[test]
    fn pick_worker_skips_busy() {
        let mut h = MultiUnitHost::new(5);
        h.spawn_n(3);
        h.units[0].busy = true;
        h.units[1].busy = true;
        h.units[2].tasks_completed = 7;
        assert_eq!(h.pick_worker(), Some(2));
    }

    #[test]
    fn pick_worker_falls_back_to_zero_when_all_busy() {
        let mut h = MultiUnitHost::new(3);
        h.spawn_n(2);
        h.units[0].busy = true;
        h.units[1].busy = true;
        assert_eq!(h.pick_worker(), Some(0));
    }

    #[test]
    fn pick_worker_returns_none_when_empty() {
        let h = MultiUnitHost::new(3);
        assert_eq!(h.pick_worker(), None);
    }

    #[test]
    fn execute_goal_runs_and_increments_tasks() {
        let mut h = MultiUnitHost::new(3);
        h.spawn_n(2);
        let r = h.execute_goal("2 3 + .").unwrap();
        assert!(r.output.contains('5'), "output: {:?}", r.output);
        assert_eq!(h.units[r.unit_index].tasks_completed, 1);
        assert!(!h.units[r.unit_index].busy);
    }

    #[test]
    fn execute_goal_round_robins_across_idle_units() {
        let mut h = MultiUnitHost::new(3);
        h.spawn_n(3);
        // First three goals should hit three different units (all start at 0
        // tasks_completed; pick_worker returns the first encountered min).
        let r0 = h.execute_goal("1 .").unwrap();
        let r1 = h.execute_goal("2 .").unwrap();
        let r2 = h.execute_goal("3 .").unwrap();
        let mut hits = vec![r0.unit_index, r1.unit_index, r2.unit_index];
        hits.sort();
        assert_eq!(hits, vec![0, 1, 2], "expected one goal per unit");
    }

    #[test]
    fn share_word_makes_word_available_on_every_unit() {
        let mut h = MultiUnitHost::new(5);
        h.spawn_n(3);
        h.share_word(": DOUBLE 2 * ;");
        for i in 0..3 {
            let out = h.units[i].vm.eval("21 DOUBLE .");
            assert!(out.contains("42"), "unit {} output: {:?}", i, out);
        }
    }

    #[test]
    fn teach_from_copies_definition_to_others() {
        let mut h = MultiUnitHost::new(5);
        h.spawn_n(3);
        // Define a word only on unit 0 (use define_on to record source string).
        assert!(h.define_on(0, ": TRIPLE 3 * ;"));
        // Sanity: unit 1 doesn't know it yet.
        let probe = h.units[1].vm.eval("7 TRIPLE .");
        assert!(
            probe.contains("unknown"),
            "unit 1 already knows TRIPLE: {:?}",
            probe
        );
        // Teach from unit 0.
        let taught = h.teach_from(0, &["TRIPLE"]);
        assert_eq!(taught, vec!["TRIPLE".to_string()]);
        // Units 1 and 2 now know TRIPLE.
        for i in 1..3 {
            let out = h.units[i].vm.eval("7 TRIPLE .");
            assert!(out.contains("21"), "unit {} output: {:?}", i, out);
        }
    }

    #[test]
    fn define_on_records_user_word() {
        let mut h = MultiUnitHost::new(3);
        h.spawn_n(1);
        assert!(h.define_on(0, ": HELLO 99 ;"));
        assert_eq!(h.units[0].user_words, vec![": HELLO 99 ;".to_string()]);
        let out = h.units[0].vm.eval("HELLO .");
        assert!(out.contains("99"), "out: {:?}", out);
    }

    #[test]
    fn share_word_records_user_word_on_every_unit() {
        let mut h = MultiUnitHost::new(3);
        h.spawn_n(2);
        h.share_word(": GREET 42 ;");
        for slot in &h.units {
            assert_eq!(slot.user_words, vec![": GREET 42 ;".to_string()]);
        }
    }

    #[test]
    fn teach_from_skips_unknown_words() {
        let mut h = MultiUnitHost::new(3);
        h.spawn_n(2);
        // No unit defines NOPE; teach_from should return empty.
        let taught = h.teach_from(0, &["NOPE-NOT-A-WORD"]);
        assert!(taught.is_empty(), "got: {:?}", taught);
    }

    // -----------------------------------------------------------------------
    // Signaling host integration (v0.28)
    // -----------------------------------------------------------------------

    #[test]
    fn say_then_route_lands_in_sibling_inboxes() {
        let mut h = MultiUnitHost::new(3);
        h.spawn_n(3);
        // Unit 0 says "42".
        h.units[0].vm.eval("42 SAY!");
        assert_eq!(h.units[0].vm.outbox.len(), 1);
        let delivered = h.route_signals_from(0);
        assert_eq!(delivered, 2, "should reach both siblings, not self");
        assert_eq!(h.units[0].vm.inbox.len(), 0, "sender does not self-receive");
        assert_eq!(h.units[1].vm.inbox.len(), 1);
        assert_eq!(h.units[2].vm.inbox.len(), 1);
        assert_eq!(h.units[1].vm.inbox.iter().next().unwrap().value, 42);
    }

    #[test]
    fn route_clears_outbox_after_delivery() {
        let mut h = MultiUnitHost::new(2);
        h.spawn_n(2);
        h.units[0].vm.eval("7 SAY!");
        h.route_signals_from(0);
        assert!(h.units[0].vm.outbox.is_empty());
    }

    #[test]
    fn listen_drains_signals_in_order() {
        let mut h = MultiUnitHost::new(2);
        h.spawn_n(2);
        h.units[0].vm.eval("100 SAY!");
        h.route_signals_from(0);
        h.units[0].vm.eval("200 SAY!");
        h.route_signals_from(0);
        // Unit 1 has two signals; LISTEN twice returns oldest first.
        h.units[1].vm.eval("LISTEN");
        let after_first: Vec<i64> = h.units[1].vm.stack.clone();
        assert_eq!(after_first, vec![100, -1]);
        h.units[1].vm.stack.clear();
        h.units[1].vm.eval("LISTEN");
        assert_eq!(h.units[1].vm.stack, vec![200, -1]);
    }

    #[test]
    fn route_from_invalid_idx_is_zero() {
        let mut h = MultiUnitHost::new(2);
        h.spawn_n(2);
        assert_eq!(h.route_signals_from(99), 0);
    }

    #[test]
    fn route_with_empty_outbox_delivers_nothing() {
        let mut h = MultiUnitHost::new(2);
        h.spawn_n(2);
        assert_eq!(h.route_signals_from(0), 0);
        assert!(h.units[1].vm.inbox.is_empty());
    }

    #[test]
    fn spawn_assigns_distinct_node_ids() {
        let mut h = MultiUnitHost::new(3);
        h.spawn_n(3);
        let id0 = h.units[0].vm.node_id_cache.unwrap();
        let id1 = h.units[1].vm.node_id_cache.unwrap();
        let id2 = h.units[2].vm.node_id_cache.unwrap();
        assert_ne!(id0, id1);
        assert_ne!(id1, id2);
        // Sender attribution is preserved through routing.
        h.units[0].vm.eval("5 SAY!");
        h.route_signals_from(0);
        let received = h.units[1].vm.inbox.iter().next().unwrap();
        assert_eq!(received.sender, id0);
    }

    // -----------------------------------------------------------------------
    // Environmental signaling host integration (v0.28, native-only)
    // -----------------------------------------------------------------------

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn mark_then_route_deposits_in_env_field() {
        let mut h = MultiUnitHost::new(2);
        h.spawn_n(2);
        h.units[0]
            .vm
            .niche_profile
            .specializations
            .insert("fibonacci".to_string(), 0.9);
        h.units[0].vm.eval("100 MARK!");
        h.route_signals_from(0);
        assert_eq!(h.env_field.sense("fibonacci"), 100);
        assert_eq!(h.env_field.sense("general"), 0);
        // Direct delivery count for env signals is zero.
        assert_eq!(h.units[1].vm.inbox.len(), 0);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn refresh_env_view_populates_sense() {
        let mut h = MultiUnitHost::new(2);
        h.spawn_n(2);
        h.env_field.deposit("fibonacci".to_string(), 200.0);
        h.units[1]
            .vm
            .niche_profile
            .specializations
            .insert("fibonacci".to_string(), 0.9);
        h.refresh_env_view(1);
        h.units[1].vm.eval("SENSE");
        assert_eq!(h.units[1].vm.stack, vec![200]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn env_decay_tick_ages_field() {
        let mut h = MultiUnitHost::new(1);
        h.spawn_n(1);
        h.env_field.deposit("fib".to_string(), 100.0);
        for _ in 0..5 {
            h.env_decay_tick();
        }
        // 100 * 0.95^5 ≈ 77
        let v = h.env_field.sense("fib");
        assert!((76..=78).contains(&v), "got {}", v);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn mixed_say_and_mark_route_correctly() {
        let mut h = MultiUnitHost::new(2);
        h.spawn_n(2);
        h.units[0]
            .vm
            .niche_profile
            .specializations
            .insert("sorting".to_string(), 0.9);
        // One SAY! and one MARK! from the same unit, same eval cycle.
        h.units[0].vm.eval("7 SAY! 50 MARK!");
        h.route_signals_from(0);
        // Direct went to sibling.
        assert_eq!(h.units[1].vm.inbox.len(), 1);
        assert_eq!(h.units[1].vm.inbox.iter().next().unwrap().value, 7);
        // Environmental went to the field.
        assert_eq!(h.env_field.sense("sorting"), 50);
    }

    // -----------------------------------------------------------------------
    // Emergent local replication rule (resource-aware self-replication)
    // -----------------------------------------------------------------------

    #[test]
    fn senses_unmet_demand_false_when_idle_unit_exists() {
        let mut h = MultiUnitHost::new(3);
        h.spawn_n(3);
        h.pending_goals = 1;
        h.units[0].busy = true;
        h.units[1].busy = true;
        // Unit 2 is idle — the colony can serve the waiting goal itself.
        assert!(!h.senses_unmet_demand());
    }

    #[test]
    fn senses_unmet_demand_false_when_no_work_waiting() {
        let mut h = MultiUnitHost::new(3);
        h.spawn_n(2);
        h.units[0].busy = true;
        h.units[1].busy = true;
        // All busy, but nothing is waiting — no demand.
        assert_eq!(h.pending_goals, 0);
        assert!(!h.senses_unmet_demand());
    }

    #[test]
    fn senses_unmet_demand_true_only_when_all_busy_with_waiting_work() {
        let mut h = MultiUnitHost::new(3);
        h.spawn_n(2);
        h.units[0].busy = true;
        h.units[1].busy = true;
        h.pending_goals = 1;
        assert!(h.senses_unmet_demand());
    }

    #[test]
    fn senses_unmet_demand_false_on_empty_host() {
        let mut h = MultiUnitHost::new(3);
        h.pending_goals = 5; // demand recorded, but no units to be busy
        assert!(!h.senses_unmet_demand());
    }

    #[test]
    fn replication_decision_fires_only_on_demand_and_headroom() {
        use crate::resources::HostResources;
        use crate::spawn::SpawnState;

        let mut h = MultiUnitHost::new(3);
        h.spawn_n(2);
        let healthy = HostResources::from_parts(1000, 500, 0.0, 4); // 50% < ceiling
        let spawn = SpawnState::new();

        // No demand yet → rule does not fire, regardless of headroom.
        assert_eq!(
            h.replication_decision(&healthy, &spawn).unwrap_err(),
            "no unmet demand"
        );

        // Now: all busy with work waiting AND headroom → the rule fires.
        h.units[0].busy = true;
        h.units[1].busy = true;
        h.pending_goals = 1;
        assert!(h.replication_decision(&healthy, &spawn).is_ok());

        // Same demand, but host over the ceiling → refuse (never a target).
        let over = HostResources::from_parts(1000, 50, 0.0, 4); // 95% used
        let err = h.replication_decision(&over, &spawn).unwrap_err();
        assert!(err.contains("ceiling"), "expected ceiling refusal: {err}");

        // Demand + headroom but a pre-existing guard set → still refuse.
        let mut quarantined = SpawnState::new();
        quarantined.quarantine = true;
        let err = h.replication_decision(&healthy, &quarantined).unwrap_err();
        assert!(err.contains("quarantine"), "expected quarantine: {err}");
    }

    #[test]
    fn no_work_path_invokes_evolve() {
        let mut h = MultiUnitHost::new(3);
        h.spawn_n(2);
        // A fresh unit has no evolution state.
        assert!(h.units[0].vm.evolution.is_none());
        // The no-work fall-through routes an idle unit into GP-EVOLVE...
        let idx = h.evolve_one_unworked().expect("an idle unit exists");
        // ...which initializes evolution state — proof the path ran evolve.
        assert!(h.units[idx].vm.evolution.is_some());
        assert!(!h.units[idx].busy, "unit released after evolving");
    }

    #[test]
    fn no_work_path_returns_none_when_all_busy() {
        let mut h = MultiUnitHost::new(2);
        h.spawn_n(2);
        h.units[0].busy = true;
        h.units[1].busy = true;
        // Nothing idle to put to work.
        assert_eq!(h.evolve_one_unworked(), None);
    }

    // -------------------------------------------------------------------
    // LIVE (the dictionary-resident life loop) and mortality
    // -------------------------------------------------------------------

    #[test]
    fn redefined_live_word_drives_the_idle_tick() {
        // The host calls LIVE; the genome defines it. Redefining LIVE
        // changes what an idle tick does — no host change required.
        let mut h = MultiUnitHost::new(4);
        h.spawn();
        h.units[0].vm.eval(": LIVE 7 ;");
        h.evolve_one_unworked();
        assert_eq!(
            h.units[0].vm.stack.last().copied(),
            Some(7),
            "idle tick ran the redefined LIVE, not the default"
        );
        assert!(
            h.units[0].vm.evolution.is_none(),
            "redefined LIVE no longer runs GP-EVOLVE"
        );
    }

    #[test]
    fn suicidal_live_starves_within_the_tick_not_hangs() {
        // A unit that evolves LIVE into an infinite loop must cost itself,
        // not the host: the metabolic meter halts it inside the tick.
        let mut h = MultiUnitHost::new(4);
        h.spawn();
        h.units[0].vm.eval(": LIVE BEGIN 0 UNTIL ;");
        h.units[0].vm.energy.energy = -400; // little left to burn
        h.evolve_one_unworked(); // must return promptly
        let vm = &h.units[0].vm;
        assert!(vm.halted, "runaway LIVE was halted by starvation");
        assert!(vm.energy.at_hard_floor(), "burned to the hard floor");
    }



    #[test]
    fn death_cry_roundtrip_applies_the_trust_gate() {
        let antibodies = vec![
            ("SOL-FIB10".to_string(), ": SOL-FIB10 55 ;".to_string()),
            ("LIVE".to_string(), ": LIVE 0 ;".to_string()), // hostile
            ("SOL-EVIL".to_string(), "x".repeat(3000)),     // oversized
            ("SOL-QUOTE".to_string(), ": SOL-QUOTE .\" hi\\\"there\" ;".to_string()),
        ];
        let cry = crate::sexp::msg_death_cry("cafe", 42, 3, &antibodies);
        let parsed = crate::sexp::parse(&cry.to_string()).expect("cry parses");
        let read = crate::sexp::read_death_cry(&parsed).expect("is a death-cry");
        let names: Vec<&str> = read.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"SOL-FIB10"));
        assert!(names.contains(&"SOL-QUOTE"), "escaped content survives");
        assert!(!names.contains(&"LIVE"), "behavioral words are gated out");
        assert!(!names.contains(&"SOL-EVIL"), "oversized sources are gated out");
    }

    #[test]
    fn forged_death_cry_cannot_redefine_behavior() {
        // Feed a hostile cry through the single-VM chatter path: it must
        // install missing SOL-* words and nothing else.
        let mut vm = VM::new();
        vm.silent = true;
        vm.load_prelude();
        vm.silent = false;
        let live_defs_before = vm
            .dictionary
            .iter()
            .filter(|e| e.name == "LIVE")
            .count();
        let cry = crate::sexp::msg_death_cry(
            "attacker",
            0,
            0,
            &[
                ("LIVE".to_string(), ": LIVE 666 ;".to_string()),
                ("SOL-GIFT".to_string(), ": SOL-GIFT 7 ;".to_string()),
            ],
        );
        vm.process_chatter_msg(&cry.to_string());
        let live_defs_after = vm
            .dictionary
            .iter()
            .filter(|e| e.name == "LIVE")
            .count();
        assert_eq!(live_defs_before, live_defs_after, "LIVE untouched");
        assert!(vm.find_word("SOL-GIFT").is_some(), "gift absorbed");
        vm.eval("SOL-GIFT");
        assert_eq!(vm.stack.last().copied(), Some(7));
    }

    #[test]
    fn absorb_never_overwrites_an_existing_antibody() {
        let mut vm = VM::new();
        vm.silent = true;
        vm.load_prelude();
        vm.silent = false;
        vm.eval(": SOL-MINE 1 ;");
        let n = vm.absorb_antibodies(&[("SOL-MINE".to_string(), ": SOL-MINE 2 ;".to_string())]);
        assert_eq!(n, 0, "existing antibody not overwritten");
        vm.eval("SOL-MINE");
        assert_eq!(vm.stack.last().copied(), Some(1), "original definition kept");
    }

    // -------------------------------------------------------------------
    // The energy economy: GIVE — flows, not faucets
    // -------------------------------------------------------------------

    #[test]
    fn give_flows_to_the_poorest_sibling_and_conserves() {
        let mut h = MultiUnitHost::new(4);
        h.spawn();
        h.spawn();
        h.spawn();
        h.units[0].vm.energy.energy = 1000; // donor
        h.units[1].vm.energy.energy = 400;
        h.units[2].vm.energy.energy = 100; // poorest
        let total_before: i64 = h.units.iter().map(|u| u.vm.energy.energy).sum();

        h.units[0].vm.eval("50 GIVE");
        h.route_signals_from(0);

        assert_eq!(h.units[0].vm.energy.energy, 1000 - 51, "donor paid gift + friction");
        assert_eq!(h.units[1].vm.energy.energy, 400, "middle sibling untouched");
        assert_eq!(h.units[2].vm.energy.energy, 150, "poorest received the gift");
        let total_after: i64 = h.units.iter().map(|u| u.vm.energy.energy).sum();
        assert_eq!(total_after, total_before - 1, "exactly the friction dissipated");
    }

    #[test]
    fn give_with_no_sibling_returns_minus_friction() {
        let mut h = MultiUnitHost::new(4);
        h.spawn();
        h.units[0].vm.energy.energy = 1000;
        h.units[0].vm.eval("50 GIVE");
        h.route_signals_from(0);
        assert_eq!(
            h.units[0].vm.energy.energy,
            999,
            "undeliverable gift returned; friction lost"
        );
    }

    #[test]
    fn give_is_clamped_and_refused_when_unaffordable() {
        let mut h = MultiUnitHost::new(4);
        h.spawn();
        h.spawn();
        // Clamp: a 9999 gift becomes GIVE_MAX.
        h.units[0].vm.energy.energy = 2000;
        h.units[1].vm.energy.energy = 0;
        h.units[0].vm.eval("9999 GIVE");
        h.route_signals_from(0);
        assert_eq!(h.units[1].vm.energy.energy, crate::energy::GIVE_MAX);
        // Refusal: a unit near the floor cannot give at all.
        h.units[0].vm.energy.energy = -490;
        let before = h.units[1].vm.energy.energy;
        h.units[0].vm.eval("50 GIVE");
        h.route_signals_from(0);
        assert_eq!(h.units[1].vm.energy.energy, before, "no gift from the destitute");
        assert_eq!(h.units[0].vm.energy.energy, -490, "and nothing spent");
    }
}

#[cfg(test)]
mod sol_stats_tests {
    use super::*;

    #[test]
    fn test_sol_stats_kinds_vs_copies() {
        let mut h = MultiUnitHost::new(4);
        h.spawn();
        h.spawn();
        h.spawn();
        assert_eq!(h.sol_stats(), (0, 0), "fresh colony knows nothing");
        // Two units learn the same antibody, one learns a second kind.
        h.units[0].vm.eval(": SOL-FIB10 55 ;");
        h.units[1].vm.eval(": SOL-FIB10 55 ;");
        h.units[1].vm.eval(": SOL-SUM10 45 ;");
        let (kinds, copies) = h.sol_stats();
        assert_eq!(kinds, 2, "two distinct antibodies known");
        assert_eq!(copies, 3, "three installed copies across the colony");
    }
}

#[cfg(test)]
mod live_budget_tests {
    use super::*;

    #[test]
    fn test_live_budget_bounds_per_tick_and_rotates() {
        // Tick latency must not scale with population: at most
        // LIVE_BUDGET_PER_TICK units run LIVE per tick, and the cursor
        // rotates so every unit gets its turn across consecutive ticks.
        let n = LIVE_BUDGET_PER_TICK * 2;
        let mut node = MultiUnitNode::new(n * 2, None, vec![]).unwrap();
        node.spawn_n(n);
        // Mark each unit so we can see who lived: LIVE pushes its index.
        for (i, slot) in node.host.units.iter_mut().enumerate() {
            slot.vm.eval(&format!(": LIVE {} ;", i));
        }
        let r1 = node.tick(&crate::resources::HostResources::unavailable(), |_, _| {
            Err(crate::transport::TransportError::Refused)
        });
        assert_eq!(r1.evolved_units, LIVE_BUDGET_PER_TICK, "budget per tick");
        let r2 = node.tick(&crate::resources::HostResources::unavailable(), |_, _| {
            Err(crate::transport::TransportError::Refused)
        });
        assert_eq!(r2.evolved_units, LIVE_BUDGET_PER_TICK);
        // Rotation: after two ticks, EVERY unit lived exactly once (its
        // index sits on its stack).
        for (i, slot) in node.host.units.iter().enumerate() {
            assert_eq!(
                slot.vm.stack_top(),
                Some(i as i64),
                "unit {i} must have lived exactly once across the rotation"
            );
        }
    }
}
