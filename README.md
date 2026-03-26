# unit

A software nanobot. The Forth interpreter *is* the agent.

**unit** is a minimal, zero-dependency Forth interpreter that doubles as a
self-replicating, self-mutating, self-persisting networked agent. Every
instance can execute code, communicate with peers over a gossip mesh,
interact with the host system, replicate itself through consensus, mutate
its own definitions, evolve toward higher fitness, persist and resume its
state, decompose tasks across the mesh — and compile to WebAssembly to
run in a browser.

## Build and Run

```sh
make build-native       # or: cargo build --release
make run                # or: cargo run
```

```
unit v0.5.0 — seed online
Mesh node a1b2c3d4e5f67890 online with 0 peers (fitness=0 )
auto-claim: ON
> 2 3 + .
5 ok
```

### WASM target

```sh
rustup target add wasm32-unknown-unknown
make build-wasm
# Serve web/ directory and open index.html
```

## Mesh Networking

Units discover and communicate over UDP gossip.

```sh
UNIT_PORT=4201 cargo run                                    # seed node
UNIT_PORT=4202 UNIT_PEERS=127.0.0.1:4201 cargo run         # joins mesh
```

## Executable Goals

Goals carry Forth code as distributed task payloads:

```
> 5 GOAL{ 2 3 + 4 * }
goal #4201 created [exec]: 2 3 + 4 *
[auto] claimed task #4202 (goal #4201): 2 3 + 4 *
[auto] stack: 20
[auto] task #4202 done
```

## Task Decomposition

Goals can be split into subtasks distributed across the mesh.

### SPLIT — automatic decomposition

```
> 5 GOAL{ 1000 10 SPLIT DO I LOOP }
goal #101 created [split 10×100]: DO I LOOP
```

This creates 10 subtasks, each iterating a chunk of the range:
- Task 1: `0 100 DO I LOOP`
- Task 2: `100 200 DO I LOOP`
- ...
- Task 10: `900 1000 DO I LOOP`

### Decomposition words

| Word                 | Stack effect             | Description                        |
|----------------------|--------------------------|------------------------------------|
| `SPLIT`              | inside GOAL{ only        | Split iterations into N subtasks   |
| `SUBTASK{ <code> }`  | `( goal-id -- task-id )` | Add a subtask to a goal            |
| `FORK`               | `( goal-id n -- )`       | Split goal into N identical tasks  |
| `RESULTS`            | `( goal-id -- )`         | Show all subtask results           |
| `REDUCE" <code>"`    | `( goal-id -- )`         | Reduce subtask results with code   |
| `PROGRESS`           | `( goal-id -- )`         | Show completion progress           |

### Example: distributed reduce

```
> 5 GOAL{ 100 10 SPLIT DO I LOOP } DROP
> ( wait for completion... )
> 101 RESULTS
> 101 REDUCE" + "
reduce: 10 values -> 4950
```

## Persistence

Units can save and restore their full state — dictionary, memory, goals,
fitness, and mutations survive restarts.

### Persistence words

| Word          | Description                                     |
|---------------|-------------------------------------------------|
| `SAVE`        | Save state to `~/.unit/<node-id>/state.bin`     |
| `LOAD-STATE`  | Restore state from disk                         |
| `AUTO-SAVE`   | Toggle auto-save (every 5 completed tasks)      |
| `RESET`       | Wipe saved state                                |
| `SNAPSHOT`    | Create a timestamped snapshot                   |
| `SNAPSHOTS`   | List available snapshots                        |
| `RESTORE`     | `( snapshot-id -- )` restore from a snapshot    |

### Auto-persistence

- On boot: automatically restores from saved state if present
- On `BYE`: auto-saves if auto-save is enabled
- After every 5 completed tasks (when auto-save is on)

### Example: save and resume

```
> : DOUBLE DUP + ;
> 5 DOUBLE .
10 ok
> SAVE
saved 2048 bytes to /home/user/.unit/a1b2c3d4e5f67890
> BYE

$ cargo run
restored from /home/user/.unit/a1b2c3d4e5f67890/state.bin
> 5 DOUBLE .
10 ok
```

## WASM Seed

The unit compiles to WebAssembly for browser execution. The core VM
(stacks, dictionary, interpreter) is platform-independent. On WASM:

- Mesh networking: unavailable (WebSocket planned)
- File I/O: unavailable (localStorage planned)
- Shell: permanently unavailable
- HTTP: unavailable (use browser fetch)

The WASM binary provides a C-compatible API:
- `boot()` → creates a VM
- `eval(ptr, len)` → evaluates Forth, returns output
- `is_running(ptr)` → check if VM is alive
- `destroy(ptr)` → free VM

The `web/` directory contains a terminal-style browser REPL.

## Host I/O

| Word                   | Stack effect                  | Description           |
|------------------------|-------------------------------|-----------------------|
| `FILE-READ" <path>"`  | `( -- addr n )`               | Read file             |
| `FILE-WRITE" <path>"` | `( addr n -- )`               | Write file            |
| `FILE-EXISTS" <path>"`| `( -- flag )`                 | Check file exists     |
| `FILE-LIST" <path>"`  | `( -- )`                      | List directory        |
| `FILE-DELETE" <path>"`| `( -- flag )`                 | Delete file           |
| `HTTP-GET" <url>"`    | `( -- addr n status )`        | GET request           |
| `HTTP-POST" <url>"`   | `( addr n -- addr n status )` | POST request          |
| `SHELL" <cmd>"`       | `( -- addr n exitcode )`      | Shell command         |
| `ENV" <name>"`        | `( -- addr n )`               | Environment variable  |
| `TIMESTAMP`            | `( -- n )`                    | Unix timestamp        |
| `SLEEP`                | `( ms -- )`                   | Sleep milliseconds    |

## Security

| Operation   | REPL | Sandbox (remote) | Notes                            |
|-------------|------|-------------------|----------------------------------|
| FILE-READ   | yes  | yes (read-only)   |                                  |
| FILE-WRITE  | yes  | **no**            | Blocked in sandbox               |
| HTTP-GET    | yes  | yes               |                                  |
| HTTP-POST   | yes  | **no**            | Blocked in sandbox               |
| SHELL       | if enabled | **never**    | SHELL-ENABLE from REPL only      |

| Word           | Description                              |
|----------------|------------------------------------------|
| `SANDBOX-ON`   | Enable sandbox                           |
| `SANDBOX-OFF`  | Disable sandbox                          |
| `SHELL-ENABLE` | Toggle shell access                      |
| `IO-LOG`       | Show I/O operation log                   |

## Mutation & Evolution

| Word                   | Description                             |
|------------------------|-----------------------------------------|
| `MUTATE`               | Mutate a random non-kernel word         |
| `MUTATE-WORD" <name>"` | Mutate a specific word                  |
| `UNDO-MUTATE`          | Revert last mutation                    |
| `MUTATIONS`            | List mutation history                   |
| `FITNESS`              | `( -- n )` push fitness score           |
| `LEADERBOARD`          | Show mesh-wide fitness rankings         |
| `EVOLVE`               | Run one evolution cycle                 |
| `AUTO-EVOLVE`          | Toggle automatic evolution              |
| `BENCHMARK" <code>"`   | Set benchmark for evolution evaluation  |

## Architecture

Zero external dependencies. Built entirely on `std`:

```
src/main.rs      — Forth VM, REPL, all primitive words
src/mesh.rs      — UDP gossip, consensus, replication
src/goals.rs     — Goal/task management, decomposition
src/io_words.rs  — File, HTTP, shell operations
src/mutation.rs  — Self-mutation engine
src/fitness.rs   — Fitness tracking, evolution
src/persist.rs   — State serialization, snapshots
src/platform.rs  — Platform abstraction traits
src/wasm_entry.rs — WASM entry point
src/prelude.fs   — Forth prelude (compiled at boot)
web/             — Browser REPL (HTML + JS)
```

## License

CC0 1.0 Universal — see [LICENSE](LICENSE).
