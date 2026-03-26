# unit

A software nanobot. The Forth interpreter *is* the agent.

**unit** is a zero-dependency Forth interpreter that doubles as a self-replicating,
self-mutating, self-persisting networked agent. Every instance can execute code,
communicate with peers over a gossip mesh, replicate itself as a new process,
mutate its own definitions, evolve toward higher fitness, and compile to WASM.

## Build and Run

```sh
cargo build --release
./target/release/unit
```

```
unit v0.6.0 — seed online
Mesh node a1b2c3d4e5f67890 gen=0 peers=0 fitness=0
auto-claim: ON
>
```

## True Self-Replication

A running unit can package its own binary, state, and prelude into a
single blob and spawn a new independent process. The nanobot metaphor
becomes literal.

### How it works

```
seed → dictionary → full system → SPAWN → child process
```

1. **PACKAGE**: the unit reads its own executable (`std::env::current_exe()`),
   serializes its state (dictionary, memory, goals, fitness, mutations), and
   bundles them with the prelude into a binary package.

2. **SPAWN**: writes the package to `~/.unit/spawn/<child-id>/`, launches the
   child as an independent process. The child boots with the parent's state,
   joins the mesh via gossip, and gets its own unique identity.

3. **Children are independent** — killing the parent doesn't kill children.
   Each child is a full unit with its own PID, node ID, and mesh port.

### Package format

```
"UREP" (4 bytes magic)
version (1 byte)
binary_size (8 bytes)
state_size (8 bytes)
prelude_size (8 bytes)
[binary bytes] [state bytes] [prelude bytes]
```

### Spawn words

| Word                   | Stack effect   | Description                           |
|------------------------|----------------|---------------------------------------|
| `SPAWN`                | `( -- )`       | Spawn one local child                 |
| `SPAWN-N`              | `( n -- )`     | Spawn n local children                |
| `PACKAGE`              | `( -- addr n )` | Build package in memory              |
| `PACKAGE-SIZE`         | `( -- n )`     | Estimate package size                 |
| `REPLICATE-TO" host"`  | `( -- )`       | Send package to remote host via TCP   |
| `CHILDREN`             | `( -- )`       | List spawned children                 |
| `FAMILY`               | `( -- )`       | Show lineage (parent, gen, children)  |
| `GENERATION`           | `( -- n )`     | This unit's generation number         |
| `KILL-CHILD`           | `( pid -- )`   | Send SIGTERM to a child               |

### Safety limits

| Control              | Default | Description                          |
|----------------------|---------|--------------------------------------|
| `MAX-CHILDREN`       | 10      | `( n -- )` set max children          |
| Cooldown             | 30s     | Between spawns                       |
| `ACCEPT-REPLICATE`   | ON      | Accept incoming replication packages  |
| `DENY-REPLICATE`     |         | Refuse incoming packages             |
| `QUARANTINE`         | OFF     | Emergency stop: disable all spawning |

### Example: local spawn + mesh discovery

```
> SPAWN
spawned child pid=12345 id=cafe0123deadbeef
> CHILDREN
  pid=12345 id=cafe0123deadbeef age=5s
> PEERS
1
> FAMILY
id: a1b2c3d4e5f67890 gen: 0 parent: none children: 1
```

### Remote replication

```
> REPLICATE-TO" 192.168.1.100:5201"
sent 614400 bytes to 192.168.1.100:5201
```

The receiving unit (with `ACCEPT-REPLICATE` on) unpacks and launches
the child. Each unit listens on UDP port + 1000 for TCP replication.

## Mesh Networking

Units discover and communicate over UDP gossip.

```sh
UNIT_PORT=4201 cargo run                                    # seed node
UNIT_PORT=4202 UNIT_PEERS=127.0.0.1:4201 cargo run         # joins mesh
```

## Executable Goals & Task Decomposition

```
> 5 GOAL{ 2 3 + 4 * }
> 5 GOAL{ 1000 10 SPLIT DO I LOOP }     \ 10 subtasks
> RESULTS  REDUCE" + "                   \ aggregate
```

## Persistence

```
> SAVE                     \ save to ~/.unit/<id>/state.bin
> SNAPSHOT                  \ timestamped backup
> BYE                       \ auto-saves if enabled
$ cargo run                 \ auto-loads on boot
resumed identity a1b2c3d4e5f67890
restored from ~/.unit/a1b2c3d4e5f67890/state.bin
```

## Host I/O

File read/write, HTTP GET/POST (raw TCP), shell, env vars. Sandbox
by default for remote code — reads allowed, writes/shell blocked.

## Mutation & Evolution

Self-mutation with four strategies, fitness tracking, benchmarked
evolution cycles. `EVOLVE` mutates, benchmarks, keeps or reverts.

## WASM Target

```sh
rustup target add wasm32-unknown-unknown
make build-wasm             # 139KB binary
```

Browser REPL in `web/`. Core VM works identically; mesh/IO unavailable.

## Architecture

Zero external dependencies. ~4500 lines of Rust + Forth.

```
src/main.rs       — Forth VM, REPL, all primitives
src/mesh.rs       — UDP gossip, consensus, replication
src/goals.rs      — Goals, tasks, decomposition
src/spawn.rs      — Self-replication, package format, process spawning
src/persist.rs    — State serialization, snapshots
src/io_words.rs   — File, HTTP, shell operations
src/mutation.rs   — Self-mutation engine
src/fitness.rs    — Fitness tracking, evolution
src/platform.rs   — Platform abstraction traits
```

## License

CC0 1.0 Universal — see [LICENSE](LICENSE).
