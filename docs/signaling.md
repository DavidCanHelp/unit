<!--
SECTION OUTLINE (proposed before drafting):
  1. Motivation
  2. Mechanism — two layers (direct inbox + environmental field)
  3. Forth surface (EMIT!, LISTEN, INBOX?, MARK!, SENSE)
  4. Selection pressure: mate-finding first
  5. Selection pressure: resource-location, deferred
  6. The honesty question
  7. Connection to the demo
  8. Open questions
  9. Summary — what v0.27 ships if implemented as written
-->

# Inter-unit signaling

**Status:** design doc for v0.27. No implementation in this PR.
**Scope:** add a two-layer signaling substrate that evolved Forth code
can reach. Wire mate selection through the substrate. Leave honesty
empirical, not enforced.

## 1. Motivation

unit can already replicate, mutate, mate, specialize, evolve solutions to
challenges, and gossip S-expressions over UDP. What it cannot do is *say
something to another unit because it chose to*. `SHARE-ALL` ships
dictionary entries; `SEND` and `SEXP-SEND"` deliver mesh messages. None
of these are a signal in the ALife sense — none are a cheap, evolvable
gesture that another unit can choose how to interpret. Sexual
reproduction shipped in v0.26 with mechanical partner selection
(tournament over fitness). Niche construction shipped at the same time
but currently couples only to challenge categories. There is no channel
through which a unit can advertise itself, its niche, or its state to
peers — and therefore no surface where signaling honesty can either
emerge or fail to emerge.

Adding a real signaling layer makes three latent threads load-bearing.
Sexual reproduction gains a *choice* term: mating is no longer "the sim
picked a partner from the peer list" but "this unit listened and these
peers signaled." Niche construction gains a *trace*: a unit can deposit
a marker that other units in the same neighborhood read, so
specialization becomes legible to the colony. The energy/metabolism
system gains its first communicative cost — broadcasting consumes
energy, listening does not — and that asymmetry is what later lets us
ask whether honest signals stabilize. The immune system already turns
unsolved problems into colony-visible challenges; signaling extends the
same idea to colony-visible *units*.

## 2. Mechanism — two layers

Signaling is one mechanism with two timescales: a fast direct channel
and a slow environmental channel. Both live behind the same Forth verbs
where it makes sense, so an evolved program does not have to know which
layer it is touching to participate.

### 2.1 Direct: peer inbox

A direct signal is a value broadcast from one unit to its neighbors and
delivered into a per-recipient inbox. The data shape is intentionally
minimal: a single `Cell` value (a tagged i64, the existing VM word size)
plus the sender's `NodeId`. Anything richer can be encoded as a sequence
of signals or routed through `SEXP-SEND"` instead. Keeping the payload
to one cell preserves the property that signaling is *cheap to evolve*
— a single GP mutation can swap which value is broadcast.

A *neighbor* is any peer the sender already considers reachable for
mesh purposes. Concretely: in the legacy single-VM-per-process mode this
is the live set in `mesh::PeerTable`; in the v0.27 multi-unit-per-process
mode it is the union of in-process siblings (the `MultiUnitHost`'s unit
list) and the host's remote peers. We do *not* introduce a new spatial
graph — the signaling layer rides on the existing peer topology so the
bounded-k gossip path (`--gossip-k N`) governs broadcast reach the same
way it governs every other mesh dispatch.

The inbox is FIFO with a fixed capacity of 64 entries per unit. On
overflow, the oldest entry is dropped (drop-head, not drop-incoming) so
recent signals always survive. This matches the existing pattern for
`mesh.rs`'s pending intent buffers and avoids back-pressure plumbing.
A unit can drain the inbox one entry at a time with `LISTEN` or query
its depth with `INBOX?`.

### 2.2 Environmental: deposit + decay

An environmental signal is a value placed into a shared field that
decays over time. Where direct signals are addressed (sender → neighbors,
arrives this tick), environmental signals are addressed only by *where
and when* — a unit deposits a marker with `MARK!` and any unit that
later runs `SENSE` in the same locale reads the current strength.

Locale, in v0.27, is the unit's *niche bucket*: the dominant
specialization category from `niche.rs`'s `NicheProfile`. This avoids
inventing a new spatial coordinate system the codebase does not have.
Two units that have specialized into "fibonacci" share an environmental
slot; a "sorting" specialist deposits to a different slot. The shared
environment is a `HashMap<NicheCategory, EnvSignal>` owned by the
process (the `MultiUnitHost` in two-tier mode, or a thin per-process
singleton in legacy mode). Cross-process environmental sharing is
explicitly out of scope for v0.27 — the field is per-host. Cross-host
diffusion is an experiment for later.

Decay is multiplicative per tick: `strength *= 0.95`. Below a floor of
`1` the entry is removed. A `MARK!` of value `v` overwrites the slot
when `v` exceeds the current strength, otherwise it sums and clamps.
This gives both reinforcement (repeated marks accumulate) and
displacement (a stronger novel signal can take over) without any new
parameters beyond the decay rate.

### 2.3 Why two layers, not two systems

Direct signals are fast, targeted, and lossy under bounded-k gossip —
exactly right for "I am here, I want to mate, I have energy." Envir-
onmental signals are slow, locale-addressed, and persistent — exactly
right for "this niche has had recent activity, this region is rich,
this kind of work is being done here." The two share the Forth
interface (`EMIT!` and `MARK!` both consume from the stack; `LISTEN`
and `SENSE` both push to it) and share the energy lever (both
broadcast verbs cost energy, both read verbs are free). The substrate
is one mechanism with two timescales, not two parallel systems with
their own state machines.

## 3. Forth surface

Five new words. Naming follows the existing convention: bang suffixes
(`!`) on words that *write*, question marks on words that *test
without consuming state*. `EMIT!` is deliberately spelled with a bang
to distinguish it from the existing `EMIT` (which writes a single
character to stdout — see `P_EMIT` in `src/vm/mod.rs`).

| Word     | Stack effect             | Energy cost | Rationale |
|----------|--------------------------|-------------|-----------|
| `EMIT!`  | `( v -- )`               | 2           | Broadcast value `v` to every neighbor's inbox. Cost is small but non-zero; this is the load-bearing asymmetry that lets honesty be an empirical question. |
| `LISTEN` | `( -- v -1 \| 0 )`       | 0           | Pop the oldest inbox entry, push value and `-1`; if the inbox is empty, push only `0`. The two-cell `value/flag` shape matches existing Forth idioms (cf. `KEY?`-style words in standard Forth). |
| `INBOX?` | `( -- n )`               | 0           | Push the count of pending inbox entries. Lets evolved code branch on "did anyone signal me" without consuming the queue. |
| `MARK!`  | `( v -- )`               | 3           | Deposit value `v` into the environmental slot for this unit's current niche. Slightly more expensive than `EMIT!` because the effect persists across ticks. |
| `SENSE`  | `( -- v )`               | 0           | Read the current environmental strength for this unit's niche slot, or `0` if empty. |

Costs are charged through the existing `EnergyTracker::spend` path in
`src/energy.rs`; the reasons (`"emit"`, `"mark"`) join the existing
`"spawn"`, `"gp"`, `"send"` set. If a unit cannot afford the cost the
verb is a no-op and pushes nothing additional to the stack — same
failure-as-silence behavior as `SEND` under network failure today.

The five words add exactly five new `P_*` constants and five new
entries in `register_primitives`. No new word categories, no new
documentation taxonomy in `docs/words.md`.

## 4. Selection pressure: mate-finding first

The point of this layer in v0.27 is to make sexual reproduction
*choose* with information instead of *select* by mechanism.

Today, `reproduction::select_mate` (src/reproduction.rs:81) takes a
`&[(NodeId, i64)]` of peer fitness pairs and runs a tournament of three.
Fitness is the only signal. The proposal is to replace the input — not
the algorithm — so the tournament selects over a *signaled* candidate
set rather than the raw peer list:

1. Before reproduction is attempted, every unit that wants to mate
   broadcasts its intent with `EMIT!` of a self-fitness value (or, more
   evolvably, *whatever value its dictionary chooses to broadcast in
   response to a "ready to mate" prelude word*).
2. `select_mate` reads the inbox via a new `gather_mate_signals(...)`
   helper, builds the `(NodeId, signaled_value)` list from inbox
   entries, and runs the existing tournament against *that* list.
3. If the inbox is empty, `select_mate` falls back to today's
   peer-fitness path. No regression for units that don't signal.

The change is small in code (one helper, one input swap) but large in
substrate. A unit that broadcasts a high signal is more likely to be
chosen — but `EMIT!` costs energy, and the broadcast value is *whatever
the unit puts on the stack*, not a verified fitness reading. This is
where deception becomes possible and where the experiment becomes
interesting.

The prelude (`src/prelude.fs`) gains a single new word, e.g.

```forth
: COURT  FITNESS EMIT! ;   \ honest courting; subject to GP mutation
```

…which a unit can override, mutate, or replace via `SMART-MUTATE` like
any other dictionary entry. The word is courtesy, not law.

## 5. Selection pressure: resource-location, deferred

The natural follow-on, once mate-finding has been observed in the wild
for a while, is to couple environmental signals to the energy system.
Sketch: a unit that completes a high-reward challenge runs `MARK!` with
a value proportional to the reward; kin units running `SENSE` in the
same niche read elevated strength and gain a small fitness bonus when
they pursue work in that category. This connects niche construction to
collective foraging without inventing new state — the niche map already
exists, the energy system already tracks rewards, the two just become
mutually visible. **Not in v0.27.** Flagged here so the design above
does not foreclose it.

## 6. The honesty question

Honesty is not enforced by construction.

`EMIT!` puts whatever the sender's stack holds onto the wire. There is
no signature, no verification, no honest-broker. The only discipline on
deception is metabolic: broadcasting costs energy, and a unit that
broadcasts inflated mate-signals while running short on energy cannot
sustain it. Whether that cost is sufficient to stabilize honest
signaling — or whether deception drifts in, oscillates, or dominates —
is the empirical question this design exists to ask.

The research payoff is the experiment, not the answer. A signaling
substrate where honesty is wired in by construction is a substrate that
has answered the interesting question before the simulation runs. We
ship the substrate, run colonies, and report what the colonies do.
That is the ALIFE 2027 paper hook.

## 7. Connection to the demo

The current "Hello?" / "Anyone there?" / "Spawn" sequence in
`web/index.html` (`loneChatterTick`, lines ~700–730) is presentation:
the bubbles are emitted by JS on a timer and the spawn is a JS call to
`doSpawn()`. After this design ships, the same visitor experience
becomes the *visible surface of a real layer*.

The smallest change: when a unit's `EMIT!` fires in the WASM mesh, the
JS shim renders the broadcast value as a chatter bubble — using the
existing `setBubble` and `addChatter` paths. If the value is small and
positive, render it as a number. If the unit's dictionary contains a
convention for stringy values (e.g., a packed ASCII cell, or a
sentinel that maps to a phrase), render that. The lone unit's
"Hello?" becomes the bubble for an `EMIT!` that the lone unit is
actually executing because its prelude says so when alone. The
"Spawn" bubble becomes the bubble for the `EMIT!` that fires
immediately before reproduction. JS no longer fakes the conversation;
JS *renders* it.

The visitor sees the same thing. The substrate underneath is real.

## 8. Open questions

- **Inbox capacity.** 64 is a guess sized to "more than gossip-k for any
  reasonable k, less than memory-pressure on 1000-unit hosts." Should
  this scale with peer count, or stay flat?
- **Environmental decay rate.** 0.95/tick is a starting value chosen to
  give roughly 14-tick half-life. Should it be tunable per niche
  category? Per environment?
- **Signal type.** v0.27 ships single-cell signals. A two-cell variant
  (`value`, `tag`) would let signals be typed (`mate-ready`,
  `food-here`, `danger`) without parsing. Worth the API surface?
- **Cross-process environmental field.** Per-host today. Diffusing
  environmental signals across hosts via gossip is the obvious next
  step but adds protocol surface — should it wait for v0.28?
- **GP visibility.** Should `EMIT!` and `MARK!` be in the GP
  primitive set from day one (so evolution can discover them
  unbidden), or gated to user-defined words only at first?
- **Signal cost as a tuning knob vs. a research variable.** If we
  publish "honesty stabilizes at cost=2," we want to be sure cost=2
  was chosen *before* observing the result. Pre-register the value or
  vary it?

These are not blockers. They are choices the implementation pass — or
its code review — should make explicit.

## 9. Summary

If this design is implemented as written, v0.27 ships:

- Five new Forth words (`EMIT!`, `LISTEN`, `INBOX?`, `MARK!`, `SENSE`)
  costing 2, 0, 0, 3, 0 energy respectively, registered in
  `src/vm/mod.rs` and implemented in `src/vm/primitives.rs` (or a new
  `src/vm/signaling.rs` if the implementer prefers).
- A per-unit FIFO inbox of 64 single-cell signals, drop-head on
  overflow, riding the existing `mesh::PeerTable` for neighbor
  resolution.
- A per-host environmental field keyed by niche category, with
  multiplicative decay at `0.95/tick` and a sum-or-displace `MARK!`
  rule.
- A swap of `reproduction::select_mate`'s input from raw peer fitness
  to inbox-gathered signals, with peer-fitness fallback when the inbox
  is empty. One new helper, one prelude word (`COURT`).
- A WASM/JS shim change in `web/index.html` so that real `EMIT!`
  events from the WASM mesh render through the existing bubble +
  chatter pipeline. The lone-unit "Hello?" sequence becomes the
  visible surface of `EMIT!` calls the lone unit is actually executing.
- Zero new dependencies. No protocol-level changes. No simulation
  behavior changes outside the new verbs and the one-line input swap
  in mate selection — every existing test continues to pass.

The point of v0.27 is not the words. The point is the experiment they
make possible.
