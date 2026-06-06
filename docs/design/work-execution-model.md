# unit: Work Execution Model — Design Decision Record

*Recorded 2026-06-06. This captures decisions about what `unit` is *for* and how work flows through it. It is a north-star record, not an implementation spec — it states the destination, not the next commit.*

## What a unit is

A unit is the smallest self-contained code that can self-replicate, network, and perform functions. On native, a unit is a real OS process: its own network identity, its own colony, able to fork a full copy of itself. The in-process `--multi-unit` colony (many units sharing one runtime) is retained as a scale/stress *testing* tool, honestly labeled — it is not the canonical form of a unit.

## What `unit` is for (the decision)

`unit` is a zero-dependency, decentralized, self-organizing distributed computation fabric. The unit of work is a program. Scheduling, load-balancing, fault-tolerance, and scaling are *emergent from each node's local resource decisions and gossip* — there is no central coordinator, scheduler, master, or control plane. This is the contribution: most distributed compute has a coordinator; `unit` has none.

The purpose is extrinsic: a unit is a substrate for doing work given from outside, not solely an ALife system that maintains itself. The self-replication, placement, and mesh machinery are the infrastructure that makes externally-supplied work resilient and distributable.

## Self-replication boundary (safety + honesty)

- **Within a host:** a unit self-replicates for real — local process fork via the already-present binary (`std::env::current_exe()` + `std::process::Command`, zero-dep). Fully autonomous, fully safe because the binary is already local. Recursive (copies can replicate) and governed by the resource ceiling so it self-limits.
- **Across hosts:** adding a host is a *human* act — copy the `unit` binary over and run it; it then does everything a `cargo install unit` unit would. The mesh NEVER ships binaries. A unit cannot acquire new ground on its own.
- **State across the mesh:** unit state (pattern) transports freely between running units — that's data, governed by existing trust/admission logic.

This boundary is deliberate: the dangerous capability was never "fork a process," it was "autonomously place my binary on a machine I chose." That one combination is made structurally impossible. Nothing scientifically interesting is lost; the worm hazard is removed by construction. (Consequence: no on-the-wire binary-hash-verification or code-trust tiers are needed, because code never crosses the wire.)

## The layering

- **Fabric** — units, mesh, self-replication, resource-aware placement. Speaks Forth internally.
- **Protocol** — s-expressions over the wire: the stable, polyglot, homoiconic contract for "here is work" / "here is a result." Trivial to parse on both ends, code-and-data same shape (fits a program-shipping system and Forth's character), decouples the controller from each unit's accumulated Forth vocabulary.
- **Controller** — any client in any language that speaks the s-expr protocol; submits jobs, receives results. This is the ingress/egress edge that makes the fabric usable.
- **REPL** — a human dropping into one unit to watch and poke. The microscope, NOT the controller. (The native TUI idea — chatter above, REPL below, zero-dep via std + one termios ioctl + ANSI scroll region, raw-mode restore guard built first — wraps this.)

## Work execution model (the core decision)

Every unit has the same uniform capability — no coordinator role exists:

1. A unit receives an s-expression instruction. The source is indistinguishable and irrelevant: it may come from the controller or from a peer that recruited it — *same interface*.
2. It works on what it can. **If the problem exceeds its own hands, it locally recruits peers with headroom** — using the existing placement logic — handing them sub-instructions in the same s-expr protocol. Recruitment is a local decision about the unit's own work, exactly as placement and replication are local decisions about its own resources.
3. Distribution is therefore recursive and emergent: a recruit whose share is still too big recruits further. The work fans out organically; no one planned it. It self-limits against the resource ceiling because a unit recruits only if it can find a peer with headroom.
4. **Results flow back up the recruitment tree** — the distribution structure IS the return routing. Each unit aggregates its recruits' results with its own and returns upward, until the answer reaches the original asker. Egress falls out of ingress structure for free.

## Fault model: let it crash (Erlang's lead)

Workers are dumb and crashable; resilience lives in the supervision relationship, not in worker error-handling. **The recruiter is the supervisor of its recruits** — it already knows what it asked for and is already waiting.

- A recruit's correctness is binary: it either returns a result or it is dead (silent). No partial/limping states to handle.
- Death detection reuses the existing mesh gossip/heartbeat timeout (the same `PEER_TIMEOUT` prune already in the codebase). On a recruit's death, the recruiter re-recruits that sub-problem to another peer with headroom.
- Supervision nests along the recruitment tree: if a recruiter itself dies, whoever recruited *it* re-recruits its whole subtree, recursively, up to the controller. An Erlang supervision tree that emerged from the recruitment structure rather than being designed separately.
- **Minimum required state:** a recruiting unit holds the sub-instruction it handed out until it gets a result back, so it can re-recruit if the recruit vanishes. It holds the *what*, never the *how-far*. This is the same shape as confirm-before-release in the transport layer (don't discard work until landing is confirmed) — applied to computation.

## Build order (smallest first; each a reviewable milestone)

1. **s-expr ingress on a single unit** — receive an s-expression instruction, evaluate in the Forth VM, return an s-expr result. No distribution, no mesh. The seam everything hangs off.
2. **Recruitment primitive** — a unit hands a sub-instruction to one peer and gets a result back.
3. **Let-it-crash supervision** — re-recruit on peer timeout.

## Principles held throughout

Zero dependencies. No central coordinator (every behavior is a local decision from local pressure + gossip). Honesty selected, not policed. Confirm before release. Fail closed. The resource ceiling is a refusal wall, not a target — and it governs replication and work-recruitment alike, not just placement.

## Explicitly rejected during design (do not revisit without reason)

- **Thousands of small units as the goal** — replaced by depth over breadth; the size/count tradeoff is a continuous knob the workload turns, not a fixed target.
- **Binary self-replication across the wire** — worm hazard; the binary boundary (local-only forking) is the containment.
- **Each-unit-is-each-host / triad-per-host** — considered and dropped; units are processes, many can share a host, replication is local and ceiling-governed.
- **A per-job coordinator role** — replaced by uniform recruit-when-needed, which needs no special unit.
- **Shared ephemeral-unit pool across co-located units** — would require coordination/shared state, violating self-containment; each unit owns its own colony.
