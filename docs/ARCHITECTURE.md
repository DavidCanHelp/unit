# unit: two-tier architecture

This document describes the deployment architecture of `unit` after commit
`616b8b0`. The original single-VM-per-process model is still available and is
still the default; the two-tier model is opt-in via `--multi-unit N`. This
document is for future contributors, ALife researchers reading the repo, and
the author's future self.

## The original model and why it didn't scale

Until recently, every `unit` was an OS process. A native deployment of N units
meant N forked binaries, each with its own Forth VM, its own state directory
under `~/.unit/<id>/`, and its own UDP socket joined to the mesh. The mesh
peer table stored one entry per process, and since each process was a unit,
the peer table held one entry per unit. Discovery happened via heartbeat
gossip, and `mesh::send_sexp` (`src/mesh.rs`) broadcast every payload to every
known peer.

This model is operationally clean — fork-level isolation, independent crash
domains, easy to reason about — but it does not survive scale. Three walls
showed up in the bench scaffolding (`--bench`, `--bench-two-tier`):

1. **Chatter dispatch was O(N²).** Per `mesh::send_sexp`, every unit sending
   one message per tick produced N×(N-1) recipient deliveries. The bench at
   N=10 000 measured 99.99 million dispatches per tick, projecting to roughly
   45 seconds of pure dispatch work per tick — a per-tick budget exceeded by
   the chatter alone, with no time left for actual computation.

2. **Peer-table memory was O(N²) aggregate.** Each process held a peer table
   of every other process; entries are 80 bytes plus HashMap overhead, so at
   N=1 000 000 each process needed ~100 MB just for the peer table, times N
   processes, comes out to roughly 100 TB of aggregate peer-table state.
   Even if you spread that across machines, it is not a workable footprint.

3. **Process RSS was 5–10 MB per forked unit minimum.** The Rust runtime,
   libc, the VM struct, and the network thread together baseline at several
   megabytes per process before the unit does anything. A million units in
   the fork-per-unit model means tens of terabytes of RSS, plus a million
   PIDs, plus a million UDP sockets, plus the kernel scheduler load of a
   million processes. The OS will not host that on any machine you can buy.

The fork-per-unit shape was the bottleneck; no amount of optimizing
`send_sexp` or shrinking the VM struct gets you to a million units in that
model.

## The two-tier model

Two changes, taken together, move the architecture into a different regime.

**Inside a process, units are multiplexed.** `multi_unit::MultiUnitHost`
(`src/multi_unit.rs`) holds a `Vec<UnitSlot>` where each slot owns one full
Forth `VM`. New units are added with `spawn`/`spawn_n`; goal dispatch picks
the least-busy idle VM via `pick_worker`, mirroring the WASM browser demo's
`_pickWorker` (`web/unit.js:167`). Sibling-to-sibling communication is direct
method calls: `share_word(definition)` evaluates a definition string on every
sibling, and `teach_from(source_idx, words)` copies named user-defined words
from one sibling into the rest by replaying their source strings (the host
tracks them in `UnitSlot.user_words` because Forth's `SEE` returns
decompiled internal form, not re-evaluable source). The host is the failure
boundary; per-unit memory is ~140–180 kB measured, roughly 30× smaller than
fork-per-unit.

**Across processes, the mesh handles bounded-k gossip.** `mesh::MeshNode`
(`src/mesh.rs`) is unchanged in role — it is still the network thread that
binds a UDP socket, exchanges heartbeats, runs peer discovery, and routes
s-expression envelopes — but two changes made it scalable. `PeerTable`
(`src/mesh.rs:202`) stores peers in a `Vec<PeerInfo>` plus a
`HashMap<NodeId, usize>` index, supporting `sample_k_addrs(k, &mut rng)` in
true O(k) by rejection-sampling random indices into the Vec. And
`gossip_fanout` (now in `MeshState`) controls both `send_sexp` (`mesh.rs:1418`)
and `send_heartbeat` (`mesh.rs:1776`); when set to `Some(k)`, both paths
sample k random peers per call instead of broadcasting to all. Information
propagates epidemically in O(log_k M) ticks. Default k is 8, configurable
via `--gossip-k K`. The all-to-all path remains available for A/B comparison.

**`multi_unit::MultiUnitNode` is the bridge.** It owns one `MultiUnitHost`
and one `MeshNode`. The process is the mesh peer; the host's unit count is
advertised via the heartbeat's `load` field
(`MeshNode::set_load`, `mesh.rs:699`). Other processes see this in their
peer tables, and `remote_processes()` exposes the snapshot to local code.
Cross-process messages use `send_to_process(target, payload)`, which wraps
the payload as `(host-msg :to "<hex>" :from "<hex>" :payload "...")` and
sends it via the targeted (non-broadcast) `send_sexp_to`. The receiving host
calls `drain_and_dispatch()`, which filters envelopes addressed to itself
and routes each payload to one of its in-process units via the same
`pick_worker` semantics used for local goals.

The numbers from the bench scaffolding:

- Bounded-k chatter dropped projected per-tick cost at N=10 000 units from
  about 45 s (all-to-all) to about 36 ms (gossip k=8) — roughly 1 240×.
- Bounded-k heartbeat dropped per-process steady-state bandwidth from
  O(M) — measured at 786 msg/s, 70.7 kB/s at M=200 all-to-all — to O(k),
  measured at 64 msg/s, 5.75 kB/s at the same M=200 with k=8. That is a
  12× per-process reduction at M=200, and the ratio grows linearly with M.
- Cross-process `send_to_process` p50/p95 latency stayed flat at roughly
  270 µs across every (M, N) configuration tested through 200×50 = 10 000
  aggregate units, including under the bandwidth-flattened gossip mode.
  Targeted sends do not use gossip, so changing gossip mode does not move
  this number, which the data confirms.
- Aggregate peer-table memory at M=100 is 1.16 MB across all processes,
  not the TB-class projection that the unit-as-peer model would have
  required at the same aggregate population.

## The four design choices

These are deliberate; reversing any of them gives back something the
two-tier model is buying.

**Explicit local versus remote primitives.** Forth's principle is to expose
the machine. The cost of an in-process `vm.eval(definition)` is on the order
of microseconds; the cost of a UDP packet plus parse plus dispatch is
hundreds of microseconds at minimum, and on a real network is bounded by
the speed of light, not by your CPU. These are different kinds of operations
and the language should make that obvious. Local sibling reach uses
`MultiUnitHost::share_word`/`teach_from` and the per-unit Forth shims that
go with them; remote reach uses `MultiUnitNode::send_to_process` and the
existing mesh send words. There is no unified address space, no
location-transparent send. A unit author has to know whether it is talking
to a sibling (cheap, synchronous, same address space) or a remote peer
(over the wire, asynchronous, may drop). Hiding this would be friendlier
in the short term and dishonest in the long term.

**Units know their host process.** Each unit has, as injected Forth words
at spawn time (`multi_unit::inject_host_constants`):

- `HOST-ID` — the mesh node hex of the process this unit lives in.
- `UNIT-IDX` — this unit's index within its host.
- `SIBLING-COUNT` — read from a `_SIBLINGS` VARIABLE the host updates.
- `MESH-PROCESS-COUNT` — read from a `_REMOTES` VARIABLE the host updates.

A unit can ask "what process am I in?" and "who are my siblings?" as
distinct questions from "who is on the mesh?". A goal scheduler written
inside Forth could use this to prefer local dispatch over remote dispatch
when latency matters. None of these queries are free of locality
information; they are *about* locality.

**Fate-shared crash semantics.** If a host process dies, every unit inside
it dies with it. The mesh layer detects the dropout via the existing
heartbeat-timeout machinery (`mesh.rs:HEARTBEAT_INTERVAL` and `PEER_TIMEOUT`,
plus the explicit `evict_peers_older_than` accessor used for tests) and
removes the dead peer from every other process's peer table. The units that
were inside the dead host are simply gone. There is no resurrection, no
per-unit liveness tracking across processes, no in-flight work
redistribution, no migration. The host is the failure boundary, full stop.

This is a tradeoff. It rules out "the unit kept running on a different
machine after its parent died" — a feature the original fork-per-unit model
also did not provide, but which a more ambitious orchestration layer might.
The simplicity is the payoff: there are no protocols for partial failure
across hosts, no quorum checkpointing, no stuck-state cleanup. A host crash
is one event the rest of the mesh notices once and otherwise ignores. It
is also true to the biological metaphor — when a cell dies, the molecules
inside it do not migrate to a sibling cell.

**The process is the mesh peer, not the unit.** The peer table holds
processes; each process advertises its unit count via the heartbeat's
`load` field. Aggregate population is the sum of `peer.load` across the
mesh plus this host's own count. This is what cuts the peer-table memory
problem from O(N²) to O(M²), where M is the process count and N is the
aggregate unit count. At M=100 hosting N=10 000 aggregate units, the peer
table is 99 entries per process, ~11 kB per process, ~1.16 MB across the
whole mesh. Treating each unit as a separate peer would have produced the
TB-class number from the original model; it is the same data at a
different granularity, and the granularity is the point.

A consequence: anything the mesh used to know about individual units — for
example, per-unit fitness or per-unit load — has to be either aggregated
at the host level (sum, max, etc.) or discovered out-of-band by sending an
explicit query to the relevant process. The peer-status envelope already
broadcasts host-level fitness and capacity (`mesh::msg_peer_status`); it
does not break down per unit, and adding per-unit detail there would put
us right back into the O(N) state-per-broadcast regime.

## What stayed unchanged

The shape of the project is the same. unit is still a small composable
agent, still a Forth interpreter, still has zero external Cargo
dependencies (the xorshift64 PRNG used by gossip sampling is written
inline in `mesh.rs:reservoir_sample` and `PeerTable::sample_k_addrs`
rather than pulling `rand`).

The original mesh code paths still work for single-VM-per-process
deployments. Running `unit` with no flags gives you the same REPL, the
same single VM, the same peer-table semantics it always had. `--multi-unit
N` is opt-in. `--gossip-k K` is opt-in. When neither is set, the program
behaves byte-for-byte the way it did before this work, including the
all-to-all heartbeat broadcast.

The WASM browser demo is unaffected. The two-tier work is gated by
`#[cfg(not(target_arch = "wasm32"))]` where it touches `Instant` or
threads, and the multi-unit host is conceptually a port of the
`BrowserMesh` model (`web/unit.js`) into native Rust, not a replacement
for it. The browser demo continues to be the small-N visualization of the
same idea.

The native protocol on the wire is unchanged. Heartbeats, peer-status
envelopes, s-expression payloads, replication packets, discovery beacons
— all the same byte formats. A process running this version interoperates
with a process running the prior version on the wire; bounded-k just
sends the same packets to fewer recipients per call.

## What changed and is worth being honest about

Process-per-unit isolation has been softened to host-per-unit isolation
in the multi-unit deployment mode. Two units inside one `MultiUnitHost`
share an OS process, an address space, and a Rust heap. They each have
their own `VM` struct, but a sufficiently aggressive bug — buffer
overrun, unsafe deserialization, allocator corruption, runaway memory
allocation — in one unit's Forth code can in principle take down the
host and therefore every sibling in it. This was structurally impossible
under fork-per-unit, where the kernel was the isolator.

The mitigation is that the older deployment mode is still available.
Single VM per process, fork to spawn, mesh peer per unit — that
configuration has not been removed and is what you get when you do not
pass `--multi-unit`. If isolation matters more than scale, that is the
right mode. The two-tier mode is for the regime where the choice is
"more units in the same machine" or "no units, because the kernel ran
out of PIDs" — at that point host-level isolation is a worthwhile trade.

Two further softer notes:

- The Forth VM is single-threaded. Inside a multi-unit host, while one
  unit is in `vm.eval`, every sibling waits. There is no scheduler
  fairness yet; this matches the WASM model and is on the roadmap.
- Cross-process latency on a real network will be substantially higher
  than the 270 µs measured on loopback. Order of magnitude estimate:
  hundreds of microseconds on the same datacenter, single-digit
  milliseconds across regions. The bench numbers in this document are
  loopback numbers and should be read accordingly.

## Out of scope, saved for future work

The two-tier work deliberately stops at "the WASM model's strengths,
native, with bounded-k gossip between hosts." Several pieces that fit
naturally into this architecture are not built yet:

- **Scheduler fairness for in-process units.** Today every host evals
  serially and one slow unit blocks its siblings. A cooperative
  scheduler with per-unit time slicing, or true async eval with a
  yieldable VM, would address this. Either is a substantial project.
- **Async eval.** The Forth VM has no yield points. Adding them — for
  network I/O, for sibling messaging, for time slicing — is a VM-level
  change with implications for every Forth word that already exists.
- **Persistence integration with the multi-unit path.** `persist.rs`
  saves and restores a single VM; it does not know about
  `MultiUnitHost`. A natural extension would be host-level snapshots
  that capture every sibling VM as a unit and restore them as a unit on
  next boot.
- **Work redistribution on host crash.** Currently a host crash loses
  every unit inside. A scheduler layer above could, before declaring a
  host dead, drain in-flight work and migrate it elsewhere — but doing
  this correctly is the hard part of distributed systems and is not
  attempted here.
- **Multi-machine bench.** Every measurement in this document was taken
  with all `MultiUnitNode` instances on one host talking over loopback.
  The cross-process latency, gossip bandwidth, and convergence times
  will be different on a real network with real packet loss and real
  scheduling jitter. A multi-machine bench is the next sanity check
  before any production claim about the model.
- **Bounded peer tables with LRU eviction.** At M=10 000 each process's
  peer table would be roughly 1.2 MB. At M much larger than that, the
  per-process peer-table memory becomes a wall by itself. The fix is a
  bounded peer table with LRU or random-sample eviction, accepting that
  any one process knows only a subset of the mesh and relying on
  transitive gossip for routing. This is a real piece of work and is
  not done.
