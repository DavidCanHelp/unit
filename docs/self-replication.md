# Resource-aware self-replication

*Shipped in v0.29.0. This note explains the whole arc — why each piece exists,
how they compose, and the principles the design holds. It is accurate to what
shipped; where something is deliberately left out, it says so.*

## The shape of the problem

A unit is a process that can copy itself. Left alone, the cheapest thing a
self-replicator can do is fill its host: spawn until memory or CPU is gone, and
take the machine down with it. The interesting question is not *how* to
replicate — `SPAWN` already did that — but how a colony of selfish replicators,
each acting only on what it can see locally, ends up at a population that is
*minimum-sufficient*: enough units to serve the work, no more, spread across
hosts that have room, with **no coordinator deciding any of it**.

v0.29 answers that with four stones, each a separate commit, built in order so
each one stands on the last:

1. **A sensor** — read the host's real load (`src/resources.rs`).
2. **A wall and a local rule** — refuse to grow past 80%, and replicate only on
   sensed demand (`resources.rs` ceiling + `multi_unit.rs` rule).
3. **A transport** — move a complete self to another coordinate, never losing it
   in transit (`src/transport.rs`).
4. **Placement** — choose *where* to go, frugally, and expose it as a behavior a
   unit chooses (`TRANSPORT` word + `choose_destination`).

## Stone 1 — the sensor

`HostResources::measure()` reads `/proc/meminfo`, `/proc/loadavg`, and the CPU
count on Linux. Two design choices matter:

- **Utilization is the binding constraint:** `max(memory_fraction,
  load_one / n_cpus)`. A host that is memory-bound and a host that is CPU-bound
  are both equally unfit to take another unit, so the *tightest* resource sets
  the pressure. Headroom is simply `1 - utilization`.
- **It can say "I don't know."** On macOS, in a browser, or when `/proc` can't be
  parsed, `measure()` returns a clearly-marked *unavailable* reading rather than
  guessing a number. That honesty is load-bearing for everything above it.

## Stone 2 — the wall is a wall, not a target

`CEILING_UTILIZATION = 0.80` is the single source of truth, and its **only**
role is refusal. Nothing in the system grows *toward* 80%, optimizes *to* it, or
treats it as a setpoint. It is the line past which a coordinate says "no."

`has_headroom()` is the gate: `valid && utilization < CEILING`. It **fails
closed** — an unavailable reading returns `false`. A coordinate that cannot
measure itself does not replicate and does not accept a transport. This is the
recurring safety rule: *when unsure, refuse.*

The wall layers onto the existing spawn guards rather than replacing them —
`SpawnState::can_spawn_within(&res)` still honors quarantine, max_children, and
cooldown, and adds the ceiling on top.

**Minimum-sufficient is emergent, not planned.** The local rule a coordinate
runs is just:

> replicate one unit **iff** there is unmet demand I can serve **and** I have
> headroom.

"Unmet demand" is derived from signals that already exist — work waiting with
no idle unit to take it (`senses_unmet_demand`). There is no quorum, no global
counter, no target population anywhere. A unit with no work doesn't sit idle: it
falls through to `GP-EVOLVE` and speculatively evolves against open challenges.
Surplus doesn't need a culler — evolution costs energy, and a unit that can't
earn starves. **That metabolism is the only population controller.**

## Stone 3 — transport without loss

Relocation is replication-then-retirement, and the dangerous moment is the gap
between them. v0.29 closes that gap with **confirm-before-release** — borrowed
from the transporter: you are not disassembled at the origin until a living copy
is confirmed at the destination.

What travels is the **complete self**: a serialized `VmSnapshot` in the USAV
format from `persist.rs` — the dictionary (including evolved `SOL-*`
antibodies), memory, goals, fitness, and code_strings. What does *not* travel is
the binary and the prelude: every coordinate already has them. The receiving
unit process is the transporter pad. This is why a self-state blob, not a
process image, crosses the wire — and why it rides a length-prefixed TCP frame
(`UTPT`), reusing `spawn.rs`'s framing style, never the UDP gossip wire.

The protocol:

```
origin                                   destination
  | capture self (USAV)                     |
  | -- UTPT frame (len-prefixed USAV) -->   |
  |                                         | validate, has_headroom()?  (fail closed)
  |                                         | deserialize the complete self
  |   <------ UTPC confirm (accept|refuse) -|
  | release ONLY on Accepted                |
```

The asymmetry is the whole point. The origin releases **solely** on
`Ok(Accepted)`. A refused confirm, a timeout, a dropped connection, or a garbled
reply all map to `Err`, and on any `Err` the origin stays alive *exactly as it
was*. No unit is ever lost in transit. The frame encode/decode and the
destination handler are pure functions over byte buffers (resources injected),
so the invariant is pinned by deterministic tests, not by hoping the network
behaves.

## Stone 4 — placement, and a behavior rather than a schedule

To choose a destination, a unit needs to know which peers have room. Peers
already gossip their unit count in the heartbeat; v0.29 appends one more byte —
advertised headroom (`0..=100`) — kept at the tail so older peers that omit it
read as 0 (fail closed again). Bounded-k gossip stays bounded.

`choose_destination()` returns the **first** peer that advertises sufficient
room, in gossip-view order — **not** the emptiest. This is deliberate and it
mirrors minimum-sufficient: take the first that fits. Sorting for the emptiest
peer would send every mislocated unit toward the same target at once — a
thundering herd. Frugal-first spreads load without anyone coordinating it.

A coordinate knows it should consider leaving when it is **mislocated**, and the
honest trigger is local pressure: `is_mislocated == !has_headroom()`. There is
no separate "mislocation score" to invent or tune.

Crucially, relocation is **not** a host-driven scheduler. The host offers the
capability; the `TRANSPORT` Forth word is something a unit *chooses* to call,
GP-mutable exactly like `COURT` or `SAY!`. Whether and when units flee local
pressure is therefore an *evolvable behavior*, selected for or against by the
same metabolism as everything else — not a policy imposed from above. Calling
`TRANSPORT` senses mislocation, picks a sufficient-first destination, and
relocates with confirm-before-release; if it is not mislocated or no destination
fits, it is a safe no-op and the unit stays.

It costs energy. `TRANSPORT_COST = 150` sits in the same heavy class as
`SPAWN_COST` (just below it — no binary travels), charged with the same
no-op-on-starve semantics as `SAY!`. **A starving unit cannot flee.** That is
not a limitation to work around; it is correct. Fleeing is metabolic work, and a
unit with nothing left to spend has no business replicating itself across the
network.

## Honesty is selected, not enforced

Placement trusts a peer's advertised headroom. Nothing verifies it. If a peer
lies — advertises room it doesn't have — the consequence is contained entirely
by the existing machinery: the destination refuses at the transport layer
(`has_headroom()` is false there too), `send_transport` returns `Err`, and the
origin stays put. There is no detection, no flag, no blacklist, no reputation.
The transport simply doesn't complete. Whether honest advertisement is the
evolutionarily stable strategy is an empirical question the system is built to
*ask*, not one it answers by policing.

## The principles, collected

- **The host is an idea; the mesh is real.** A "coordinate" is wherever a unit
  process runs. Units relocate between coordinates; the colony is the set of
  living copies across the mesh, not anything centrally held.
- **Minimum-sufficient is emergent.** Local rule (demand ∧ headroom) plus energy
  metabolism, with no coordinator, quorum, or population target.
- **80% is a refusal wall, not a target.** The colony never grows toward it.
- **A unit with no work evolves.** Idleness routes to `GP-EVOLVE`, not to sitting
  still or to being culled.
- **The complete self transports — antibodies included.** Evolved `SOL-*` words
  ride along; the binary and prelude don't, because every coordinate has them.
- **Confirm before release.** No unit is ever lost in transit.
- **Honesty is selected, not enforced.** Lying isn't policed; it just doesn't pay.
- **Fail closed.** A coordinate that can't measure itself neither replicates nor
  accepts a transport.
- **Zero new dependencies.** The whole surface is hand-rolled; `Cargo.lock` still
  contains only the `unit` crate.

## Multi-machine validation (v0.30)

Everything above shipped in v0.29 as unit-tested mechanism. v0.30 added the
persistent run loop (`unit --multi-unit N --port P --peers ...` is now a living
node, not a 5-second discovery demo) and put the whole arc on real hardware for
the first time. This section is what was *observed*, not claimed.

**Setup.** Three DigitalOcean droplets, SFO3, 512 MB RAM each, Ubuntu 24.04,
built from source, peered into one mesh.

**What happened.** A 2000-unit colony was started on one box. Its periodic
resource line read **86.4% memory utilization — OVER-CEILING**. The node sensed
itself **MISLOCATED** (`!has_headroom()` on a real `/proc` reading, not a
fail-closed guess), and from its gossiped peer view chose peers advertising
sufficient headroom (~**73%** free). It then transported units **one per tick**,
with confirm-before-release holding across real UDP gossip + TCP transport: the
origin slot was retired only after the destination confirmed a live copy, so the
unit count drained incrementally — **2000 → 1987 → …** — toward the two peers,
never dropping a unit in transit.

**The arrivals were alive.** The receiving box's unit count rose (e.g. **3 → 8**)
and the transported units **resumed evolving** on landing — the complete self
(dictionary, evolved antibodies, fitness, code_strings) travelled and ran, not an
inert blob.

**Honesty was selected, not policed.** As the overloaded box shed load its
advertised headroom fell — gossiped honestly all the way down to **14%** — and
the other nodes simply **stopped choosing it** as a destination. No detection, no
blacklist; an over-full box just isn't sufficient-first anymore.

**The bug only a real network could surface.** Multi-machine testing immediately
exposed that the mesh UDP socket and the transport TCP listener were bound to
`127.0.0.1`. On a single host that "works" — and worse, `--peers` seed entries
populate the peer table at startup and survived the old demo's 5-second window
(shorter than the 15-second peer timeout), so cross-machine discovery *looked*
fine. It wasn't: a loopback-bound socket never receives datagrams destined for
the host's routable IP. The fix was to bind both peer-traffic sockets to
`0.0.0.0` (the HTTP bridge stays localhost-only by design). A single-host test
could not have caught this; the persistent loop on three boxes caught it in the
first minute.

## Multi-machine validation (v0.31)

The v0.30 soak proved the 80% ceiling holds as a hard refusal wall, but it also
showed a receiver still *under* the ceiling could be pushed *over* it by a burst:
several senders act on the same stale "has room" gossip and all transport within
one window, so the wall holds but overshoots for a tick before the next frame is
refused. v0.31 added the **inbound admission margin** — accept inbound only while
utilization is below `CEILING - ADMISSION_MARGIN` (0.05), so a receiver refuses
*before* a burst can push it onto the wall. This section is what was *observed*.

**Setup.** Three DigitalOcean droplets, SFO3, 512 MB RAM each, Ubuntu 25.10,
built from source, peered into one mesh.

**What happened.** A receiver was parked at **76.7–79.2%** utilization — **under
the 80% ceiling**, but inside the admission margin (above the 75% admission gate).
A single over-ceiling sender tried to transport to it and was refused with
**`destination refused (no headroom)`**: the receiver declined inbound while it
was still under its own ceiling, exactly because the margin reserves that last
slack for in-flight units a fresh measure can't yet see. Under a **2-sender
burst** the receiver **held — utilization never crossed 80%** — where pre-margin
admission would have let both frames land in one window and overshoot the wall
transiently before clawing back.

**Admission stays separate from replication.** The receiver refusing inbound at
~78% did not stop it tending its own units; only the decision to *accept more*
uses the stricter `has_admission_headroom`, while the host's own replication and
mislocation decisions still use the full-ceiling `has_headroom`. A box can be
content to keep what it has yet decline to take on more.

## What this deliberately does not do

- **No *central* coordinator.** This is the load-bearing one. In v0.30 the
  persistent node *does* run the local rule on a tick — it is not purely
  unit-invoked anymore — but each node evaluates that rule against **its own**
  gossiped view and **its own** measured pressure, deciding independently. There
  is no global scheduler, no quorum, no authority placing units across the mesh.
  The three-droplet run drained one box while two others independently judged
  themselves sufficient and accepted arrivals; nothing orchestrated that. The
  `TRANSPORT` Forth word remains for genuinely unit-invoked, GP-evolvable
  relocation; the tick loop is the always-on per-node driver layered beside it.
- **No reclaim or cull.** Surplus resolves through starvation, not through a
  reaper.
- **No liar detection.** Honesty is selected, not policed — observed live as the
  full box's advertised headroom fell and peers simply stopped choosing it.

## Map to the code

| Concern | Where |
|---|---|
| Host sensor, binding constraint, ceiling, `has_headroom` (fail closed) | `src/resources.rs` |
| Spawn guard + ceiling refusal | `src/spawn.rs` (`can_spawn_within`) |
| Emergent local rule, demand sense, evolve-when-unworked | `src/multi_unit.rs` (`MultiUnitHost`) |
| Confirm-before-release transport, USAV framing | `src/transport.rs` |
| Gossiped headroom advertisement | `src/mesh.rs` (heartbeat, `peer_resource_view`) |
| Sufficient-first placement, mislocation, relocate-with-release | `src/multi_unit.rs` (`MultiUnitNode`) + `src/transport.rs` |
| `TRANSPORT` word, energy cost | `src/main.rs` (`prim_transport`), `src/vm/mod.rs`, `src/energy.rs` |
