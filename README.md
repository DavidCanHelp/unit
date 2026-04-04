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
unit v0.23.0 -- seed online
Mesh node a1b2c3d4e5f67890 gen=0 peers=0 fitness=0
> 2 3 + .
5  ok
> : SQUARE DUP * ;
 ok
> 7 SQUARE .
49  ok
> SPAWN
spawned child pid=12345 id=cafe0123deadbeef
> SEXP" (* 6 7)" .
42  ok
```

## The Idea

A unit is the smallest self-replicating piece of software. It boots from
kernel primitives, builds its own language, networks with peers over UDP
gossip, packages its own binary, and spawns copies of itself. It monitors
services, evolves programs through genetic programming, distributes
computation across a mesh, persists its brain as human-readable JSON,
and connects across machines over the internet.

It discovers problems it can't solve, broadcasts them as fitness
challenges, evolves solutions, and installs them as new words the
colony inherits. Every operation costs metabolic energy. Solved
challenges generate harder ones — open-ended evolution with no ceiling.

Forth is the brain. S-expressions are the voice. The mesh is the body.
Zero external dependencies. ~30,000 lines of Rust + Forth + Go.

## The Five Concerns

| Concern | Mechanism |
|---------|-----------|
| **Execute** | Forth VM — stacks, dictionary, inner interpreter |
| **Communicate** | S-expression mesh protocol over UDP gossip |
| **Replicate** | Reads own binary, packages state, spawns child processes |
| **Mutate** | Genetic programming — 50 candidates, tournament selection, 5 mutation operators |
| **Persist** | JSON snapshots — hibernate, resurrect, automatic resurrection on startup |

## S-Expressions

Forth is the execution model. S-expressions are the wire format. Any
future nanobot implementation in any language can parse the mesh messages.

```
> SEXP" (+ 10 32)" .
42  ok
> SEXP" (* 6 7)" .
42  ok
> SEXP-SEND" (event :type ping :data hello)"
sexp sent
```

Mesh messages are self-describing:

```
(peer-status :id "aaa" :peers 2 :fitness 10 :load 190 :capacity 100)
(sub-goal :id 1 :seq 0 :from "aaa" :expr "99 99 *")
(evolve-share :gen 100 :fitness 890 :program "0 1 10 0 DO OVER + SWAP LOOP DROP .")
```

## Genetic Programming

50 programs mutate and compete. The default challenge: find the shortest
program that computes the 10th Fibonacci number (55).

```
> GP-EVOLVE
[gen 0] best: 890 | pop: 50 | "0 1 10 0 DO OVER + SWAP LOOP DROP ." (11 tokens)
[gen 0] WINNER: "0 1 10 0 DO OVER + SWAP LOOP DROP ." (fitness=890, 11 tokens)
```

Tournament selection, crossover, 5 token-level mutation operators (swap,
insert, delete, replace, double). Each candidate evaluated in a sandboxed
VM with step limit. On a mesh, best programs migrate between units every
100 generations.

## Immune System

When a unit can't solve a problem — a failed goal, a timed-out
distributed sub-goal, a manual report — it registers the failure as a
fitness challenge. The challenge broadcasts to the mesh. Every unit in
the colony evolves solutions in parallel. The first solution that passes
verification is installed as a dictionary word (SOL-*) that children
inherit via SPAWN.

```
> GP-EVOLVE
[gen 0] WINNER: "0 1 10 0 DO OVER + SWAP LOOP DROP ." (fitness=890)
[immune] learned word: SOL-FIB10
[landscape] depth 55: generated 3 new challenges from 'fib10'

> CHALLENGES
--- 4 challenges ---
  #11271 fib10 [SOLVED] reward=100
  #11272 fib10-short9 [unsolved] reward=120
  #11273 fib15 [unsolved] reward=150
  #11274 square-55 [unsolved] reward=80

> SOL-FIB10 .
55  ok

> IMMUNE-STATUS
challenges: 4 (1 solved, 3 unsolved)
colony antibodies: 1
  words: SOL-FIB10
```

## Metabolic Energy

Every operation costs energy. Units that run out are throttled — they
still function but at reduced capacity.

```
> ENERGY
energy: 1097/5000 (earned: 102, spent: 5, efficiency: 20.40)

> METABOLISM
--- costs ---
  spawn: 200
  gp generation: 5
  eval per 1000 steps: 1
  mesh send: 1
--- rewards ---
  task success: 50
  challenge solved: 100
  passive regen: 1/tick
```

Energy persists across HIBERNATE/resume. Children inherit a fraction
of the parent's energy — spawning is a real metabolic investment.

## Open-Ended Evolution

Solved challenges generate harder ones. The colony climbs an infinite
ladder of increasing difficulty.

```
> DEPTH
evolutionary depth: 55

> LANDSCAPE
--- landscape ---
depth: 55
challenges generated: 3
environment: normal
```

ArithmeticLadder: fib(10) → fib(15) → fib(20) → ... with parsimony
pressure (fewer tokens = higher reward). CompositionLadder: combine
two solved challenges into a new one. Environment cycles through
Normal / Harsh / Abundant / Competitive every 500 ticks, varying
selection pressure.

## Distributed Computation

Break a problem into pieces. Fan sub-goals out to mesh peers as
S-expressions. Collect results. Assemble the answer.

```
> DIST-GOAL{ 99 99 * . | 77 77 * . | 55 55 * . }
9801 5929 3025
(distributed 3 sub-goals, 1 local, 2 remote)
```

Round-robin across local + peers. If a peer doesn't respond within
timeout, fall back to local computation. The distributing unit also
participates — it doesn't just delegate.

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

## Cross-Machine Mesh

Two machines, same mesh:

```sh
# Machine A
UNIT_PORT=4201 unit

# Machine B (discovers A, gossip finds the rest)
UNIT_PORT=4201 UNIT_PEERS=<A-ip>:4201 unit
```

DNS hostnames work: `UNIT_PEERS=myhost.example.com:4201`

NAT traversal: `UNIT_EXTERNAL_ADDR=203.0.113.5:4201`

Authentication: `UNIT_MESH_KEY=mysecret` on all machines.

Manual connect from the REPL:

```
> CONNECT" 192.168.1.10:4201"
connected to 192.168.1.10:4201
> PEER-TABLE
--- peer table ---
  cafe0123deadbeef @ 192.168.1.10:4201 fitness=45 seen=1s ago
```

Gossip self-assembles: A tells B about C, the mesh grows.

## Polyglot Organisms

The S-expression protocol is language-independent. A Go organism joins
the same mesh, evolving arithmetic expression trees instead of Forth.

```sh
# Terminal 1: Rust unit
UNIT_PORT=4200 unit

# Terminal 2: Go organism
cd polyglot/go && go run . -peer 127.0.0.1:4200
```

The Go organism appears in the Rust unit's `PEERS` list, receives
challenges, evolves solutions using expression trees, and broadcasts
results. Different language, different mutation strategy, same protocol.

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

## Goals

Humans set direction, the mesh navigates.

```
> 5 GOAL{ 6 7 * }
goal #101 created [exec]: 6 7 *
[auto] stack: 42

> DASHBOARD
--- dashboard ---
watches: 0  alerts: 0  peers: 1  fitness: 30
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

## Architecture

```
src/
├── vm/               # Forth virtual machine
│   ├── mod.rs        # VM struct, interpreter, dispatch (~200 primitives)
│   ├── primitives.rs # stack, arithmetic, memory, I/O
│   ├── compiler.rs   # definitions, control flow, prelude loader
│   └── tests.rs      # VM tests
├── types.rs          # Cell (i64), Entry, Instruction enum
├── mesh.rs           # UDP gossip, peer discovery, word sharing, cross-machine
├── sexp.rs           # S-expression parser, serializer, Forth translator
├── evolve.rs         # Genetic programming engine
├── challenges.rs     # Challenge registry, immune system
├── discovery.rs      # Problem detection from failures
├── energy.rs         # Metabolic energy system
├── landscape.rs      # Dynamic fitness landscape, environment cycles
├── distgoal.rs       # Distributed goal splitting and collection
├── goals.rs          # Goal registry, task decomposition
├── snapshot.rs       # JSON snapshots, persistence, resurrection
├── spawn.rs          # Self-replication, UREP package format
├── persist.rs        # Binary state serialization
├── platform.rs       # Platform detection (native vs WASM)
├── wasm_entry.rs     # WASM C FFI bindings
├── prelude.fs        # Forth prelude (~600 lines)
├── features/
│   ├── io_words.rs   # file, HTTP, shell, env
│   ├── mutation.rs   # self-mutation engine, smart mutation
│   ├── fitness.rs    # fitness tracking, leaderboard
│   ├── monitor.rs    # watches, alerts, dashboard, scheduler
│   └── ws_bridge.rs  # WebSocket bridge (raw RFC 6455)
└── main.rs           # feature wiring, REPL, CLI, entry point

polyglot/go/          # Go organism (expression trees, goroutines)
├── main.go           # entry point, gossip loop, periodic evolution
├── sexp/             # S-expression parser
├── mesh/             # UDP mesh networking
├── evolve/           # GP engine with expression trees
└── challenge/        # challenge/solution protocol

docs/
├── unit-whitepaper-2026.pdf
└── formal-analysis.md
```

173+ tests. Zero dependencies. ~30,000 lines of Rust + Forth + Go.

## All the Words

309 words. Organized by category:

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
| `AND` `OR` `NOT` | bitwise logic | | `ABS` `NEGATE` `MIN` `MAX` | math |
| `1+` `1-` `2*` `2/` | shortcuts | | `0=` `0<` `<>` `TRUE` `FALSE` | predicates |

### Memory

| Word | Description |
|------|-------------|
| `@` `!` | fetch / store |
| `HERE` `,` `C,` `ALLOT` `CELLS` | data space allocation |
| `VARIABLE` `CONSTANT` `CREATE` | data words |

### I/O

| Word | Description |
|------|-------------|
| `.` `.S` `EMIT` `CR` `SPACE` `SPACES` `TYPE` | output |
| `KEY` `."` | input / string literal |
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
| `WORDS` `SEE` `EVAL"` | introspection |

### S-Expressions

| Word | Description |
|------|-------------|
| `SEXP"` | parse S-expression, translate to Forth, execute |
| `SEXP-SEND"` | broadcast S-expression to mesh peers |
| `SEXP-RECV` | drain inbound S-expression messages |

### Mesh & Gossip

| Word | Description |
|------|-------------|
| `PEERS` `MESH-STATUS` `ID` `MY-ADDR` | mesh info |
| `PEER-TABLE` `MESH-STATS` `MESH-KEY` | cross-machine |
| `CONNECT"` `DISCONNECT"` | manual peer management |
| `SEND` `RECV` | raw messaging |
| `DISCOVER` `AUTO-DISCOVER` | LAN discovery |
| `SHARE"` `SHARE-ALL` `AUTO-SHARE` `SHARED-WORDS` | word sharing |
| `SWARM-ON` `SWARM-OFF` `SWARM-STATUS` | swarm mode |

### Distributed Computation

| Word | Description |
|------|-------------|
| `DIST-GOAL{` | distribute pipe-separated expressions across peers |
| `DIST-STATUS` | show active distributed goals |
| `DIST-CANCEL` | cancel all distributed goals |

### Genetic Programming

| Word | Description |
|------|-------------|
| `GP-EVOLVE` | run 10 generations (call repeatedly to continue) |
| `GP-STATUS` `GP-BEST` | inspect evolution state |
| `GP-STOP` `GP-RESET` | control evolution |

### Immune System & Energy

| Word | Description |
|------|-------------|
| `CHALLENGES` | list all challenges with status and reward |
| `IMMUNE-STATUS` | summary: solved, unsolved, antibody count |
| `ANTIBODIES` | list learned SOL-* words |
| `ENERGY` | current energy level and efficiency |
| `METABOLISM` | full metabolic report with cost/reward table |
| `FEED` | `( n -- )` manually add energy (capped at 500) |
| `LANDSCAPE` | landscape status: depth, environment |
| `DEPTH` | evolutionary depth metric |

### Goals & Tasks

| Word | Description |
|------|-------------|
| `GOAL"` | `( priority -- id )` description-only goal |
| `GOAL{` `}` | `( priority -- id )` executable Forth goal |
| `GOALS` `TASKS` `REPORT` `CLAIM` `COMPLETE` | lifecycle |
| `SUBTASK{` `FORK` `RESULTS` `REDUCE"` `PROGRESS` | decomposition |
| `AUTO-CLAIM` `TIMEOUT` | execution control |

### Monitoring

| Word | Description |
|------|-------------|
| `WATCH"` `WATCH-FILE"` `WATCH-PROC"` | create watches |
| `WATCHES` `UNWATCH` `WATCH-LOG` `UPTIME` | manage watches |
| `ON-ALERT"` `ALERTS` `ACK` `ALERT-HISTORY` `HEAL` | alerting |
| `DASHBOARD` `HEALTH` `OPS` | overview |
| `EVERY` `SCHEDULE` `UNSCHED` | scheduling |

### Fitness & Mutation

| Word | Description |
|------|-------------|
| `FITNESS` `LEADERBOARD` `RATE` | scoring |
| `MUTATE` `MUTATE-WORD"` `UNDO-MUTATE` `MUTATIONS` | mutation |
| `SMART-MUTATE` `MUTATION-REPORT` `MUTATION-STATS` | smart mutation |
| `EVOLVE` `AUTO-EVOLVE` `BENCHMARK"` | fitness-driven evolution |

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

### Persistence

| Word | Description |
|------|-------------|
| `JSON-SNAPSHOT` `JSON-RESTORE` | save/load JSON snapshots |
| `HIBERNATE` | snapshot and exit |
| `AUTO-SNAPSHOT` | periodic auto-save |
| `SNAPSHOT-PATH` `JSON-SNAPSHOTS` | inspect storage |
| `EXPORT-GENOME` `IMPORT-GENOME"` | genome transfer |
| `SAVE` `LOAD-STATE` `RESET` | binary state management |
| `SNAPSHOT` `SNAPSHOTS` `RESTORE` | binary versioned backups |
| `AUTO-SAVE` | binary auto-save |

## Binary Sizes

| Target | Size |
|--------|------|
| Native (macOS arm64, release) | ~1.2 MB |
| WASM (browser) | ~338 KB |

## License

MIT — see [LICENSE](LICENSE).
