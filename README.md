# unit

**A self-replicating software nanobot** — a minimal Forth interpreter that is also a networked mesh agent.

![unit demo](unit-demo.gif)

**[Try the live demo](https://davidcanhelp.github.io/unit/)** | **Install**: `cargo install unit`

[![CI](https://github.com/DavidCanHelp/unit/actions/workflows/ci.yml/badge.svg)](https://github.com/DavidCanHelp/unit/actions/workflows/ci.yml)

## Install

```
cargo install unit
```

## What Happens

```
$ unit
unit v0.10.2 — seed online
Mesh node a1b2c3d4e5f67890 gen=0 peers=0 fitness=0
auto-claim: ON
> 2 3 + .
5 ok
> : SQUARE DUP * ;
 ok
> 7 SQUARE .
49 ok
```

## The Idea

A unit is the smallest self-replicating piece of software. It boots from
a handful of kernel primitives, builds its own language, networks with
peers over UDP gossip, packages its own binary, and spawns copies of
itself. It monitors services, heals failures, mutates its own code, and
evolves toward higher fitness. Zero external dependencies. The language
builds itself. The agent *is* the language.

## The Four Concerns

| Concern | Mechanism |
|---------|-----------|
| **Execute** | Forth VM — stacks, dictionary, inner interpreter |
| **Communicate** | UDP gossip mesh with consensus-based replication |
| **Replicate** | Reads own binary, packages state, spawns child processes |
| **Mutate** | Rewrites word definitions, fitness-driven evolution |

## Swarm Mode

```sh
# Terminal 1
UNIT_PORT=4201 unit
> SWARM-ON
swarm mode active
```

```sh
# Terminal 2 — discovers Terminal 1 automatically
UNIT_PORT=4202 unit
> PEERS .
1
```

Define a word on one unit. It appears on the other:

```
# Terminal 1:
> : CUBE DUP DUP * * ;
> SHARE" CUBE"

# Terminal 2:
> 3 CUBE .
27
```

Submit work on one. The other executes it:

```
# Terminal 1 (auto-claim off):
> AUTO-CLAIM
> 5 GOAL{ 6 7 * }
goal #101 created [exec]: 6 7 *

# Terminal 2 (auto-claim on) picks it up:
[auto] claimed task #102 (goal #101): 6 7 *
[auto] stack: 42
[auto] task #102 done
```

Too much work? The mesh spawns a child. Underperforming units cull
themselves. One command: `SWARM-ON`.

## Goals — Human Guidance

Humans set direction, the mesh navigates.

```
> 5 GOAL{ 6 7 * }
goal #101 created [exec]: 6 7 *
[auto] stack: 42

> 5 GOAL{ 1000 10 SPLIT DO I LOOP }
goal #103 created [split 10×100]: DO I LOOP

> DASHBOARD
╔══════════════════════════════════════╗
║         UNIT OPS DASHBOARD           ║
╚══════════════════════════════════════╝
─── watches ───
  #1 [UP  ] ▁▂▃▂▁▂▃▂ 45 myapp:8080/health
─── alerts ───
  all clear
─── mesh ───
  peers: 2  fitness: 45
```

## Self-Replication

A unit reads its own executable, serializes its state, and births a new
process. The child boots with the parent's dictionary, goals, fitness,
and mutations — then gets its own identity and joins the mesh.

```
> SPAWN
spawned child pid=12345 id=cafe0123deadbeef
> FAMILY
id: a1b2c3d4e5f67890 gen: 0 parent: none children: 1
> PACKAGE-SIZE .
644440
```

Trust levels control who can replicate to you:

| Level | Behavior |
|-------|----------|
| `TRUST-ALL` | Auto-accept everything (default) |
| `TRUST-MESH` | Auto-accept known peers |
| `TRUST-FAMILY` | Auto-accept parent/children only |
| `TRUST-NONE` | Manual approval for all |

## Monitoring & Ops

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

Service goes down. Alert fires. Handler runs. Mesh fixes it. Next check
auto-resolves.

## Architecture

```
src/
├── vm/              the seed — standalone Forth interpreter
│   ├── mod.rs       VM struct, constants, interpreter, dispatch
│   ├── primitives.rs  stack, arithmetic, memory, I/O
│   ├── compiler.rs  definitions, control flow, prelude
│   └── tests.rs     82 unit tests
├── types.rs         Cell, Entry, Instruction
├── mesh.rs          UDP gossip, consensus, discovery, word sharing
├── goals.rs         goal registry, task decomposition
├── spawn.rs         self-replication, UREP package format
├── persist.rs       state serialization, snapshots
├── features/
│   ├── io_words.rs  file, HTTP, shell
│   ├── mutation.rs  self-mutation engine
│   ├── fitness.rs   fitness tracking, evolution
│   ├── monitor.rs   watches, alerts, dashboard
│   └── ws_bridge.rs WebSocket bridge for browsers
└── main.rs          feature wiring, REPL, entry point
```

198 tests. Zero dependencies. ~10,000 lines of Rust + Forth.

## All the Words

### Stack

| Word | Effect | | Word | Effect |
|------|--------|-|------|--------|
| `DUP` | `( a -- a a )` | | `2DUP` | `( a b -- a b a b )` |
| `DROP` | `( a -- )` | | `2DROP` | `( a b -- )` |
| `SWAP` | `( a b -- b a )` | | `NIP` | `( a b -- b )` |
| `OVER` | `( a b -- a b a )` | | `TUCK` | `( a b -- b a b )` |
| `ROT` | `( a b c -- b c a )` | | `.S` | print stack |

### Arithmetic & Logic

| Word | Effect | | Word | Effect |
|------|--------|-|------|--------|
| `+` `-` `*` `/` `MOD` | arithmetic | | `=` `<` `>` | comparison |
| `AND` `OR` `NOT` | bitwise logic | | `ABS` `NEGATE` `MIN` `MAX` | prelude |
| `1+` `1-` `2*` `2/` | shortcuts | | `0=` `0<` `<>` | predicates |

### I/O

| Word | Description |
|------|-------------|
| `.` `.S` `EMIT` `CR` `SPACE` `SPACES` `TYPE` | output |
| `KEY` | read one character |
| `."` | print string literal |
| `FILE-READ"` `FILE-WRITE"` `FILE-EXISTS"` `FILE-LIST"` `FILE-DELETE"` | filesystem |
| `HTTP-GET"` `HTTP-POST"` | raw HTTP/1.1 |
| `SHELL"` `ENV"` `TIMESTAMP` `SLEEP` | system |
| `IO-LOG` `SANDBOX-ON` `SANDBOX-OFF` `SHELL-ENABLE` | security |

### Control Flow

| Word | Description |
|------|-------------|
| `IF` `ELSE` `THEN` | conditional |
| `DO` `LOOP` `I` `J` | counted loop |
| `BEGIN` `UNTIL` `WHILE` `REPEAT` | indefinite loop |
| `:` `;` `RECURSE` | word definitions |
| `VARIABLE` `CONSTANT` `CREATE` `DOES>` | data words |
| `WORDS` `SEE` | introspection |
| `EVAL"` | evaluate a string of Forth |

### Mesh & Gossip

| Word | Description |
|------|-------------|
| `PEERS` `MESH-STATUS` `ID` | mesh info |
| `SEND` `RECV` | raw messaging |
| `REPLICATE` `PROPOSE` | consensus replication |
| `DISCOVER` `AUTO-DISCOVER` | LAN discovery |
| `SHARE"` `SHARE-ALL` `AUTO-SHARE` `SHARED-WORDS` | word sharing |
| `SWARM-ON` `SWARM-OFF` `SWARM-STATUS` `SWARM` | swarm mode |

### Goals & Tasks

| Word | Description |
|------|-------------|
| `GOAL"` | `( priority -- id )` description-only goal |
| `GOAL{` `}` | `( priority -- id )` executable Forth goal |
| `GOALS` `TASKS` `REPORT` `CLAIM` `COMPLETE` | lifecycle |
| `CANCEL` `STEER` `RESULT` `GOAL-RESULT` | management |
| `SPLIT` `FORK` `SUBTASK{` `RESULTS` `REDUCE"` `PROGRESS` | decomposition |
| `AUTO-CLAIM` `TIMEOUT` | execution control |

### Monitoring

| Word | Description |
|------|-------------|
| `WATCH"` `WATCH-FILE"` `WATCH-PROC"` | create watches |
| `WATCHES` `UNWATCH` `WATCH-LOG` `UPTIME` | manage watches |
| `ON-ALERT"` `ALERTS` `ACK` `ALERT-HISTORY` `HEAL` | alerting |
| `DASHBOARD` `HEALTH` `OPS` | overview |
| `EVERY` `SCHEDULE` `UNSCHED` | scheduling |

### Fitness & Evolution

| Word | Description |
|------|-------------|
| `FITNESS` `LEADERBOARD` `RATE` | scoring |
| `MUTATE` `MUTATE-WORD"` `UNDO-MUTATE` `MUTATIONS` | mutation |
| `EVOLVE` `AUTO-EVOLVE` `BENCHMARK"` | evolution |

### Spawn & Replication

| Word | Description |
|------|-------------|
| `SPAWN` `SPAWN-N` | local replication |
| `PACKAGE` `PACKAGE-SIZE` | build UREP package |
| `REPLICATE-TO"` | remote replication |
| `CHILDREN` `FAMILY` `GENERATION` `KILL-CHILD` | lineage |
| `ACCEPT-REPLICATE` `DENY-REPLICATE` `QUARANTINE` `MAX-CHILDREN` | safety |

### Trust & Consent

| Word | Description |
|------|-------------|
| `TRUST-ALL` `TRUST-MESH` `TRUST-FAMILY` `TRUST-NONE` | trust levels |
| `TRUST-LEVEL` `REQUESTS` `ACCEPT` `DENY` `DENY-ALL` | consent flow |
| `REPLICATION-LOG` | audit trail |
| `SECURE-SWARM` `LOCKDOWN` | presets |

### Persistence

| Word | Description |
|------|-------------|
| `SAVE` `LOAD-STATE` `RESET` | state management |
| `SNAPSHOT` `SNAPSHOTS` `RESTORE` | versioned backups |
| `AUTO-SAVE` `REIDENTIFY` | automation |

## Binary Sizes

| Target | Size |
|--------|------|
| Native (Linux/macOS) | ~700KB |
| WASM (browser) | ~230KB |
| UREP replication package | ~650KB |

## License

MIT — see [LICENSE](LICENSE).
