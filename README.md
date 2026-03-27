# unit

[![CI](https://github.com/DavidCanHelp/unit/actions/workflows/ci.yml/badge.svg)](https://github.com/DavidCanHelp/unit/actions/workflows/ci.yml)

A software nanobot. The Forth interpreter *is* the agent.

**[Try it in your browser](https://davidcanhelp.github.io/unit/)** — 226KB WASM, loads instantly.

**unit** is a zero-dependency Forth interpreter that doubles as a self-replicating,
self-healing distributed ops agent. Every instance can monitor services, alert on
failures, self-remediate, replicate itself across machines, mutate and evolve its
own code, and compile to WebAssembly — all from a 673KB native binary.

### Binary Sizes

| Target | Size |
|--------|------|
| Native (Linux/macOS) | 673KB |
| WASM (browser) | 226KB |

## Quick Start

```sh
cargo build --release
UNIT_PORT=4201 ./target/release/unit
```

```
unit v0.7.0 — seed online
Mesh node a1b2c3d4e5f67890 gen=0 peers=0 fitness=0
auto-claim: ON
> 10 WATCH" http://myapp.local:8080/health"
watch #1 created (every 10s)
> 1 ON-ALERT" SHELL-ENABLE SHELL\" systemctl restart myapp\" DROP DROP DROP"
alert handler set for watch #1
> DASHBOARD
```

## Monitoring & Ops

The primary use case: infrastructure that manages itself.

### Watches

| Word                     | Stack effect             | Description                    |
|--------------------------|--------------------------|--------------------------------|
| `WATCH" <url>"`         | `( interval -- id )`     | Monitor a URL periodically     |
| `WATCH-FILE" <path>"`   | `( interval -- id )`     | Monitor a file for changes     |
| `WATCH-PROC" <name>"`   | `( interval -- id )`     | Monitor a process              |
| `WATCHES`                | `( -- )`                 | List all watches + status      |
| `UNWATCH`                | `( id -- )`              | Remove a watch                 |
| `WATCH-LOG`              | `( id -- )`              | Show history for a watch       |
| `UPTIME`                 | `( id -- )`              | Show uptime percentage         |

### Alerting

| Word                      | Description                                    |
|---------------------------|------------------------------------------------|
| `ON-ALERT" <code>"`      | `( watch-id -- )` Forth code to run on alert   |
| `ALERTS`                  | Show active alerts                             |
| `ACK`                     | `( alert-id -- )` acknowledge an alert         |
| `ALERT-HISTORY`           | Show past alerts                               |
| `HEAL`                    | Run handlers for all active alerts             |

### Dashboard

```
> DASHBOARD
╔══════════════════════════════════════╗
║         UNIT OPS DASHBOARD           ║
╚══════════════════════════════════════╝
─── watches ───
  #1 [UP  ] ▁▂▃▂▁▂▃▂ 45 myapp.local:8080/health
  #2 [DOWN] ▇▇▇▇▇▇▇▇ 0  database:5432
─── alerts ───
  [CRIT] watch #2: connection refused
─── mesh ───
  peers: 2  fitness: 45
```

| Word        | Description                                      |
|-------------|--------------------------------------------------|
| `DASHBOARD` | Formatted overview with sparkline trends         |
| `HEALTH`    | `( -- n )` overall health score 0-100            |
| `OPS`       | Combined: DASHBOARD + ALERTS + SCHEDULE          |

### Scheduler

| Word            | Description                                     |
|-----------------|-------------------------------------------------|
| `EVERY`         | `( secs -- id )` schedule recurring Forth code  |
| `SCHEDULE`      | List scheduled tasks                            |
| `UNSCHED`       | `( id -- )` cancel a scheduled task             |

```
> 30 EVERY DASHBOARD
schedule #5 every 30s: DASHBOARD
```

### Self-Healing

When a watch triggers a critical alert:
1. The alert broadcasts to the mesh
2. The local unit runs the ON-ALERT handler
3. If remediation fails, a GOAL is submitted to the mesh
4. Other units attempt the fix
5. Next check auto-resolves if the service recovers

```
> 10 WATCH" http://myapp:8080/health"
> 1 ON-ALERT" ." restarting..." CR"
> HEAL
--- heal cycle ---
  running handler for alert #2
  restarting...
--- heal done ---
```

### Example: Three-Node Monitoring

```sh
# Node 1: seed + monitor
UNIT_PORT=4201 ./target/release/unit
> 10 WATCH" http://myapp:8080"

# Node 2: joins mesh, shares watch data
UNIT_PORT=4202 UNIT_PEERS=127.0.0.1:4201 ./target/release/unit

# Node 3: joins mesh
UNIT_PORT=4203 UNIT_PEERS=127.0.0.1:4201 ./target/release/unit

# Any node can view the dashboard:
> DASHBOARD
> HEALTH .
85
```

## Mesh Networking

UDP gossip with consensus-based replication.

```sh
UNIT_PORT=4201 cargo run                           # seed
UNIT_PORT=4202 UNIT_PEERS=127.0.0.1:4201 cargo run # join
```

## True Self-Replication

```
> SPAWN
spawned child pid=12345 id=cafe0123deadbeef
> CHILDREN
  pid=12345 id=cafe0123deadbeef age=5s
```

A 624KB binary that reads itself, packages its state, and births new processes.

## Executable Goals & Task Decomposition

```
> 5 GOAL{ 2 3 + 4 * }                    \ distributed computation
> 5 GOAL{ 1000 10 SPLIT DO I LOOP }      \ 10 subtasks across mesh
```

## Persistence

```
> SAVE                  \ ~/.unit/<id>/state.bin
> SNAPSHOT              \ timestamped backup
$ ./target/release/unit
resumed identity a1b2c3d4e5f67890
restored from ~/.unit/a1b2c3d4e5f67890/state.bin
```

## Host I/O & Security

File, HTTP, shell, env. Sandbox by default for remote code.

## Mutation & Evolution

Self-mutating code with fitness-driven evolution.

## Browser Mesh

Browser WASM units join the same mesh as native units via WebSocket.

### Architecture

```
browser (WASM) ↔ WebSocket ↔ native unit ↔ UDP gossip ↔ native units
```

Each native unit runs a WebSocket server on `UNIT_PORT + 2000`. Browser
units connect and appear as peers — they submit goals, receive results,
and show in PEERS and DASHBOARD.

### Demo

```sh
# Terminal: start a native unit
UNIT_PORT=4201 cargo run
# ws-bridge: listening on port 6201
```

Open https://davidcanhelp.github.io/unit/ (or localhost:8080).
Click "Connect" with `ws://localhost:6201`.

```
# In browser:
> 5 GOAL{ 6 7 * }
[submitted to mesh: 6 7 *]

# Native terminal auto-claims:
[ws] goal #101 from browser: 6 7 *
[auto] claimed task #102 (goal #101): 6 7 *
[auto] stack: 42
```

### WebSocket words (native)

| Word                    | Description                        |
|-------------------------|------------------------------------|
| `WS-STATUS`             | Show bridge: port, clients, relay count |
| `WS-CLIENTS`            | List connected browsers            |
| `WS-PORT`               | `( -- n )` push WS port           |
| `WS-BROADCAST" <msg>"`  | Send message to all browsers       |

### Configuration

| Env var       | Default          | Description              |
|---------------|------------------|--------------------------|
| `UNIT_WS_PORT`| `UNIT_PORT+2000` | WebSocket server port    |

The WS bridge also serves the web UI — browse to `http://localhost:<WS_PORT>/`
to load the REPL and connect from the same origin.

### Browser compatibility

The WS bridge uses plain `ws://` (no TLS). Chrome 104+ blocks `ws://`
connections to localhost under its Private Network Access policy. Use
**Firefox** for local mesh testing, or launch Chrome with:

```sh
open -a "Google Chrome" --args --disable-features=PrivateNetworkAccessRespectPreflightResults
```

The protocol is correct (verified against RFC 6455). Future versions may
add TLS support for full Chrome compatibility.

## WASM Target

```sh
make build-wasm    # browser REPL in web/
```

## Testing

```sh
cargo test          # 69 Rust unit tests
./tests/integration.sh  # 104 bash integration tests
```

## Architecture

Zero external dependencies. ~9000 lines of Rust + Forth.

```
src/
├── main.rs              — VM struct, REPL, primitive dispatch
├── types.rs             — shared types: Cell, Entry, Instruction
├── mesh.rs              — UDP gossip, consensus, replication
├── goals.rs             — goal registry, task management
├── persist.rs           — state serialization, snapshots
├── spawn.rs             — self-replication, package format
├── features/
│   ├── io_words.rs      — file, HTTP, shell operations
│   ├── mutation.rs      — self-mutation engine
│   ├── fitness.rs       — fitness tracking, evolution
│   ├── monitor.rs       — watches, alerts, dashboard, scheduler
│   └── ws_bridge.rs     — WebSocket bridge for browser units
├── platform.rs          — native/WASM abstraction
└── wasm_entry.rs        — WASM FFI entry point
```

## License

CC0 1.0 Universal — see [LICENSE](LICENSE).
