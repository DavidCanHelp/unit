# unit — Operations Guide

## Monitoring & Alerting

```
> 10 WATCH" http://myapp:8080/health"
watch #1 created (every 10s)

> 1 ON-ALERT" ." service down!" CR"
alert handler set for watch #1

> HEAL
--- heal cycle ---
  running handler for alert #2
  service down!
--- heal done ---
```

Use `WATCHES` to list active watches, `UNWATCH` to remove them, `WATCH-LOG` for history.
`DASHBOARD` gives an overview. `HEALTH` and `OPS` summarize system status.

## Goals & Task Decomposition

Humans set direction, the mesh navigates.

```
> 5 GOAL{ 6 7 * }
goal #101 created [exec]: 6 7 *
[auto] stack: 42

> DASHBOARD
--- dashboard ---
watches: 0  alerts: 0  peers: 1  fitness: 30
```

Task decomposition: `SUBTASK{`, `FORK`, `RESULTS`, `REDUCE"`, `PROGRESS`.

## Distributed Computation

Break a problem into pieces. Fan sub-goals out to mesh peers as
S-expressions. Collect results. Assemble the answer.

```
> DIST-GOAL{ 99 99 * . | 77 77 * . | 55 55 * . }
9801 5929 3025
(distributed 3 sub-goals, 1 local, 2 remote)
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

A unit saves its entire state as human-readable JSON. It can die and
come back exactly where it left off.

```
> : SQUARE DUP * ;
> : CUBE DUP SQUARE * ;
> 42
> HIBERNATE
hibernating... saved to ~/.unit/snapshots/d1b74e159948b52b.json
```

Later, same port:

```
resurrected from snapshot
> .S
<1> 42  ok
> 7 CUBE .
343  ok
```

The JSON is hand-editable:

```json
{
  "node_id": "d1b74e159948b52b",
  "fitness": 0,
  "stack": [42],
  "words": {
    "SQUARE": ": SQUARE DUP * ;",
    "CUBE": ": CUBE DUP SQUARE * ;"
  }
}
```

## Swarm Mode

```
> SWARM-ON
swarm mode active
```

One command enables: auto-discovery, word sharing, autonomous
spawn/cull, fitness-driven evolution. Define a word on one unit,
it appears on the other:

```
# Unit A:
> : CUBE DUP DUP * * ;
> SHARE" CUBE"

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
