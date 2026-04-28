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
}

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
}

impl MultiUnitHost {
    pub fn new(cap: usize) -> Self {
        MultiUnitHost {
            units: Vec::new(),
            cap,
        }
    }

    /// Default cap of 100 — well above the WASM demo's 7 but still bounded
    /// so users can't accidentally allocate gigabytes of VMs.
    pub fn with_default_cap() -> Self {
        Self::new(100)
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
        self.units.push(UnitSlot {
            vm,
            busy: false,
            tasks_completed: 0,
            user_words: Vec::new(),
        });
        Some(idx)
    }

    /// Drain unit[idx]'s outbox and deliver each Direct signal into every
    /// other unit's inbox. Returns the count of signals delivered.
    /// Environmental signals are routed through `EnvironmentalField`
    /// (added in the next commit) — Direct-only here.
    /// Callers invoke after eval to propagate SAY! emissions.
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
            if !signal.is_direct() {
                continue;
            }
            for (j, slot) in self.units.iter_mut().enumerate() {
                if j == idx {
                    continue;
                }
                slot.vm.inbox.push(signal.clone());
                delivered += 1;
            }
        }
        delivered
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
    pub addr: SocketAddr,
}

#[derive(Debug, Clone)]
pub struct DispatchedRemoteMsg {
    pub from_host_hex: String,
    pub unit_index: usize,
    pub output: String,
}

pub struct MultiUnitNode {
    pub host: MultiUnitHost,
    pub mesh: Option<MeshNode>,
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
        Ok(MultiUnitNode {
            host: MultiUnitHost::new(cap),
            mesh,
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
        // Advertise unit count via the heartbeat `load` field. Trigger one
        // heartbeat now so peers learn quickly.
        if let Some(ref m) = self.mesh {
            m.set_load(self.host.len() as u32);
            m.force_heartbeat();
        }
        count
    }

    /// Snapshot of remote processes seen via the mesh, with their advertised
    /// in-process unit counts (i.e. peer.load). Excludes self.
    pub fn remote_processes(&self) -> Vec<RemoteProcess> {
        let mesh = match self.mesh.as_ref() {
            Some(m) => m,
            None => return Vec::new(),
        };
        let my_id = *mesh.id();
        mesh.peer_unit_counts()
            .into_iter()
            .filter(|(id, _, _)| *id != my_id)
            .map(|(id, load, addr)| RemoteProcess {
                host_id: id,
                host_id_hex: mesh::id_to_hex(&id),
                units_hosted: load,
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

    /// Drain any pending mesh messages. For each `(host-msg :to <us> ...)`
    /// envelope, dispatch the payload to one of our in-process units via
    /// `host.execute_goal` (least-busy picker) and record the result. Other
    /// messages (heartbeats, other s-expressions) are left to be handled by
    /// callers that need them.
    pub fn drain_and_dispatch(&mut self) -> Vec<DispatchedRemoteMsg> {
        let mut events = Vec::new();
        let (raw_msgs, my_hex) = match self.mesh.as_ref() {
            Some(m) => (m.recv_sexp_messages(), mesh::id_to_hex(m.id())),
            None => return events,
        };
        for raw in raw_msgs {
            let parsed = match crate::sexp::try_parse_mesh_msg(&raw) {
                Some(s) => s,
                None => continue,
            };
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
}
