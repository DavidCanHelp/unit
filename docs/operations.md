# unit — Operations Guide

## Monitoring

A unit can watch things and react. Watch ids are pushed on the stack and
shown in the creation line:

```
> 10 WATCH" http://localhost:9999/health"
watch #1 created (every 10s)
ALERT [CRIT] watch #1: connect localhost:9999: Connection refused
> WATCHES
  #1 [DOWN] url:http://localhost:9999/health (0ms) connect ... checked 0s ago
> HEAL
--- heal ---
--- done ---
```

`ON-ALERT"` stores a Forth handler for an alert. The handler text is read
up to the next `"`, so it cannot itself contain `."` strings — keep
handlers to plain words (define a helper word first if you need output):

```
> : NOTIFY 911 . CR ;
> 1 ON-ALERT" NOTIFY"
```

`DASHBOARD` summarizes:

```
> DASHBOARD
=== UNIT OPS ===
watches: 1  alerts: 1
peers: 0  fitness: 0
goals: 0 / 0 / 0 / 0 / 0
---
```

`HEALTH` is `( -- score )` — it pushes a numeric health score rather than
printing. `OPS` prints the operational summary.

## Goals & Task Decomposition

Humans set direction, the mesh navigates.

```
> 5 GOAL{ 6 7 * }        \ pushes the new goal id
> AUTO-CLAIM             \ off by default; enables auto-execution
auto-claim: ON
```

Task decomposition: `SUBTASK{`, `FORK`, `RESULTS`, `REDUCE"`, `PROGRESS`.

## Distributed Computation

Break a problem into pieces. Fan sub-goals out to mesh peers as
S-expressions. Collect results. Assemble the answer.

```
> DIST-GOAL{ 99 99 * . | 77 77 * . | 55 55 * . }
9801 5929 3025
dist-goal #1: 3 sub-goals distributed (1 local, 2 remote)
waiting for results... type DIST-STATUS to check
```

Round-robin across local + peers. If a peer doesn't respond within
timeout, fall back to local computation.

## Trust & Consent

Trust levels control who can replicate to you:

| Level | Behavior |
|-------|----------|
| `TRUST-ALL` | Auto-accept everything (default) |
| `TRUST-MESH` | Auto-accept known peers |
| `TRUST-FAMILY` | Auto-accept parent/children only |
| `TRUST-NONE` | Manual approval for all |

Use `TRUST-LEVEL` to check, `REQUESTS` to see pending, `ACCEPT`/`DENY` to respond.
`REPLICATION-LOG` shows the audit trail.

## Persistence & Resurrection

A unit saves its entire state as a human-readable S-expression — the same
notation the mesh speaks, so any species (Rust, Go, Python) can parse
another's genome with the sexp parser it already carries. It can die and
come back exactly where it left off.

```
> : SQUARE DUP * ;
> : CUBE DUP SQUARE * ;
> 42
> HIBERNATE
hibernating... saved to ~/.unit/snapshots/d1b74e159948b52b.sexp
```

Later, same port:

```
resurrected from snapshot
> .S
<1> 42  ok
> 7 CUBE .
343  ok
```

The genome is hand-editable and parses as a mesh expression (sample
abridged — the full file also carries timestamp, energy, landscape,
mutation-stats, and memory fields; missing keys default on load):

```lisp
(unit-snapshot :version 2
  :id "d1b74e159948b52b"
  :fitness 0
  :stack (42)
  :words (
    ("SQUARE" ": SQUARE DUP * ;")
    ("CUBE" ": CUBE DUP SQUARE * ;")))
```

Snapshots written by pre-v0.34 units (JSON, `<id>.json`) still resurrect —
the loader sniffs the format — and convert to the sexp genome on their next
save.

## Observability surfaces

Machine-readable S-expressions, stable by contract (prose log lines may
change; these shapes may not). Parse them with any sexp reader.

**`(node-status …)`** — emitted by a persistent node (`--multi-unit N
--port P`) every resource-measure cadence (~5s):

| Field | Meaning |
|-------|---------|
| `:id` | host mesh id (hex) |
| `:tick` | run-loop tick counter |
| `:units` | units hosted right now |
| `:util` / `:headroom` | measured utilization / advertised headroom (%) |
| `:out` / `:in` | cumulative confirmed transports out / landed in |
| `:deaths` | cumulative starvation deaths |
| `:births` | cumulative rebound births (population regrowth into headroom) |
| `:fit` | best fitness across ACTIVE evolutions (reads low while climbing) |
| `:sol-kinds` / `:sol-copies` | distinct antibodies known / copies installed |

`:out`, `:in`, `:deaths`, and `:births` are event-derived, so an external
tool can account for every unit: at any quiescent moment,
`units == initial + in − out − deaths + births`, and a surplus equals landings
whose confirms were lost (documented fail-toward-duplication).

**`RECRUITS-SEXP`** — the recruit ledger: a `(recruit-slots :count N)`
header, then one
`(recruit-slot :id … :seq … :holder … :state pending|unplaced|ok|err …)`
per slot, with `:reassigned`/`:resets` attempt accounting and, when
settled, the full result or `:kind`/`:msg` failure.

**Soak report lines** — see [docs/validation.md](validation.md).

## Swarm Mode

```
> SWARM-ON
swarm mode active
```

One command enables: auto-discovery, word sharing, autonomous
spawning, and open trust (`AUTO-DISCOVER AUTO-SHARE AUTO-SPAWN
TRUST-ALL`). Culling stays opt-in via `AUTO-CULL`; evolution runs via
the LIVE loop / `AUTO-EVOLVE`, not this switch. Define a word on one unit,
it appears on the other:

```
# Unit A:
> : CUBE DUP DUP * * ;
> SHARE" CUBE"                \ transmits the real decompiled source

# Unit B:
> 3 CUBE .
27
```

## Self-Replication Details

A unit reads its own executable, serializes its state, and births a new
process. The child boots with the parent's dictionary, goals, fitness,
and mutations — then gets its own identity and joins the mesh.

```
> SPAWN
spawned child pid=12345 id=cafe0123deadbeef
> FAMILY
id: a1b2c3d4e5f67890 gen: 0 parent: none children: 1
```

Children inherit a fraction of the parent's energy. The `UNIT_CHILD_ENERGY`
environment variable passes the inherited energy to the child process.
