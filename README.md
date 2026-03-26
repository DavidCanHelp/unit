# unit

A software nanobot. The Forth interpreter *is* the agent.

**unit** is a minimal Forth interpreter that doubles as a self-replicating,
self-mutating networked agent. Every instance can execute code, communicate
with peers, replicate across a mesh, mutate its own definitions, interact
with the host system, and evolve toward higher fitness — all guided by
human operators.

## Build and Run

```sh
cargo build --release
./target/release/unit
```

```
unit v0.4.0 — seed online
Mesh node a1b2c3d4e5f67890 online with 0 peers (fitness=0 )
auto-claim: ON
> 2 3 + .
5 ok
```

## Mesh Networking

Units discover and communicate over UDP gossip.

```sh
UNIT_PORT=4201 cargo run                                    # seed node
UNIT_PORT=4202 UNIT_PEERS=127.0.0.1:4201 cargo run         # joins mesh
UNIT_PORT=4203 UNIT_PEERS=127.0.0.1:4201 cargo run         # gossip discovery
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

| Word              | Stack effect              | Description                            |
|-------------------|---------------------------|----------------------------------------|
| `GOAL{ <code> }`  | `( priority -- id )`      | Submit executable Forth goal           |
| `GOAL" <desc>"`   | `( priority -- id )`      | Submit description-only goal           |
| `CLAIM`           | `( -- task-id )`          | Claim and execute next task            |
| `AUTO-CLAIM`      | `( -- )`                  | Toggle automatic task execution        |
| `RESULT`          | `( task-id -- )`          | Show task result                       |
| `GOAL-RESULT`     | `( goal-id -- )`          | Show combined results for a goal       |
| `TIMEOUT`         | `( seconds -- )`          | Set execution timeout (default 10s)    |
| `GOALS`           | `( -- )`                  | List all goals                         |
| `TASKS`           | `( -- )`                  | List claimed tasks                     |
| `REPORT`          | `( -- )`                  | Mesh-wide progress summary             |

## Host I/O

Units can interact with the host system through file, HTTP, shell, and
environment primitives.

### File system

| Word                   | Stack effect           | Description                         |
|------------------------|------------------------|-------------------------------------|
| `FILE-READ" <path>"`  | `( -- addr n )`        | Read file into memory               |
| `FILE-WRITE" <path>"` | `( addr n -- )`        | Write memory to file                |
| `FILE-EXISTS" <path>"`| `( -- flag )`          | Check if file exists                |
| `FILE-LIST" <path>"`  | `( -- )`               | List directory contents             |
| `FILE-DELETE" <path>"`| `( -- flag )`          | Delete a file                       |

### HTTP (raw TCP, HTTP/1.1, no TLS)

| Word                  | Stack effect                  | Description                  |
|-----------------------|-------------------------------|------------------------------|
| `HTTP-GET" <url>"`   | `( -- addr n status )`        | Fetch a URL                  |
| `HTTP-POST" <url>"`  | `( addr n -- addr n status )` | POST body to URL             |

### System

| Word               | Stack effect            | Description                           |
|--------------------|-------------------------|---------------------------------------|
| `SHELL" <cmd>"`   | `( -- addr n exitcode )` | Execute shell command                 |
| `ENV" <name>"`    | `( -- addr n )`          | Read environment variable             |
| `TIMESTAMP`        | `( -- n )`               | Current unix timestamp                |
| `SLEEP`            | `( ms -- )`              | Sleep for N milliseconds              |
| `IO-LOG`           | `( -- )`                 | Show recent I/O operations            |

### Example: read a file across the mesh

```
> 5 GOAL{ FILE-READ" /etc/hostname" TYPE }
goal #101 created [exec]: FILE-READ" /etc/hostname" TYPE
[auto] claimed task #102 (goal #101): ...
[auto] output: myhost
[auto] task #102 done
```

## Security

The sandbox-by-default model prevents remote code from doing harm.

### Sandbox rules

| Operation      | REPL | Sandbox (untrusted) | Sandbox (trusted) |
|----------------|------|---------------------|-------------------|
| FILE-READ      | yes  | yes (read-only)     | yes               |
| FILE-WRITE     | yes  | **no**              | yes               |
| FILE-DELETE     | yes  | **no**              | yes               |
| HTTP-GET        | yes  | yes                 | yes               |
| HTTP-POST       | yes  | **no**              | yes               |
| SHELL           | if enabled | **never**     | **never**         |
| ENV             | yes  | yes                 | yes               |

### Trust words

| Word           | Description                                      |
|----------------|--------------------------------------------------|
| `SANDBOX-ON`   | Enable sandbox (blocks writes/POST)              |
| `SANDBOX-OFF`  | Disable sandbox                                  |
| `TRUST`        | `( peer-id -- )` whitelist a peer                |
| `TRUST-ALL`    | Clear trust list                                 |
| `TRUST-NONE`   | Clear trust list                                 |
| `SHELL-ENABLE` | Toggle shell access (REPL only, never in sandbox)|

### Key principles

- **Goal payloads always run sandboxed** — remote code can read but not write
- **SHELL is never available in sandbox** — even for trusted peers
- **SHELL-ENABLE** can only be toggled from the local REPL
- **All I/O is logged** — view with `IO-LOG`

## Mutation & Evolution

Units can mutate their own word definitions and evolve toward higher
fitness through a genetic-programming-style cycle.

### Mutation strategies

| Strategy           | Description                                   |
|--------------------|-----------------------------------------------|
| constant-tweak     | Adjust a literal value by ±1–10%              |
| word-swap          | Replace a word call with a different word      |
| instruction-delete | Remove one instruction from a word body        |
| instruction-dup    | Duplicate one instruction in a word body       |

### Mutation words

| Word                    | Description                                 |
|-------------------------|---------------------------------------------|
| `MUTATE`                | Mutate a random non-kernel word             |
| `MUTATE-WORD" <name>"`  | Mutate a specific word                     |
| `UNDO-MUTATE`           | Revert the last mutation                    |
| `MUTATIONS`             | List all mutations since boot               |

### Fitness tracking

Every task execution adjusts the unit's fitness score:
- Success: **+10** plus speed bonus (up to +5)
- Failure: **-5**
- Peer rating via `RATE ( task-id score -- )`

| Word              | Description                                     |
|-------------------|-------------------------------------------------|
| `FITNESS`         | `( -- n )` push fitness score                   |
| `LEADERBOARD`     | Show fitness scores of all mesh units            |
| `RATE`            | `( task-id score -- )` rate a task result        |

### Evolution

The evolution cycle mutates a word, benchmarks the result, and keeps
or reverts the change:

1. Apply a random mutation to a random word
2. Run the benchmark code
3. If performance improved or held: keep the mutation
4. If performance dropped: revert

| Word                   | Description                                |
|------------------------|--------------------------------------------|
| `EVOLVE`               | Run one evolution cycle                    |
| `AUTO-EVOLVE`          | Toggle automatic evolution                 |
| `BENCHMARK" <code>"`   | Set benchmark code for evaluating mutations|

### Example: evolve a unit

```
> BENCHMARK" 1000 0 DO I DROP LOOP "
benchmark set: 1000 0 DO I DROP LOOP
> EVOLVE
evolve: kept mutation (...): ...
evolve: own=15 avg=15 evolutions=1
> LEADERBOARD
--- leaderboard ---
  1. a1b2c3d4e5f67890 score=15 (you)
---
```

## Architecture

Zero external dependencies. Built entirely on `std`:
- `std::net::UdpSocket` — mesh gossip
- `std::net::TcpStream` — raw HTTP/1.1
- `std::fs` — file operations
- `std::process::Command` — shell execution
- `std::sync::{Arc, Mutex}` — shared mesh state
- `std::thread` — network thread

## License

CC0 1.0 Universal — see [LICENSE](LICENSE).
