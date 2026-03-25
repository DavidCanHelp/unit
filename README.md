# unit

A software nanobot. The Forth interpreter *is* the agent.

**unit** is a minimal Forth interpreter that doubles as a self-replicating
networked agent. Every instance is a living program — it can execute code,
communicate with peers, replicate itself across a mesh, and mutate its own
definitions at runtime.

## The Four Concerns

| Concern         | Description                                          |
|-----------------|------------------------------------------------------|
| **Execute**     | A complete Forth inner interpreter with dictionary    |
| **Communicate** | Send and receive messages across a peer mesh          |
| **Replicate**   | Serialize and transmit self to another node           |
| **Mutate**      | Rewrite word definitions at runtime                   |

## Bootstrap Lifecycle

```
seed → dictionary → full system
```

1. **Seed**: The Rust binary boots with a minimal set of kernel primitives
   hardcoded in native code (stack ops, arithmetic, memory, I/O, control flow).

2. **Dictionary**: The prelude (`src/prelude.fs`) is compiled at startup,
   extending the dictionary with higher-level words defined in Forth itself.

3. **Full system**: The REPL drops the user (or a remote peer) into an
   interactive session where arbitrary new words can be defined, and mesh
   primitives enable communication, replication, and mutation.

## Build and Run

```sh
cargo build --release
cargo run
```

Or directly:

```sh
./target/release/unit
```

You'll see:

```
unit v0.1.0 — seed online
Mesh node a1b2c3d4e5f67890 online with 0 peers
> 2 3 + .
5 ok
>
```

Type `BYE` to exit.

## Mesh Networking

Units discover and communicate with each other over UDP. Configure via
environment variables:

| Variable     | Description                                    | Default     |
|--------------|------------------------------------------------|-------------|
| `UNIT_PORT`  | UDP port to bind                               | 0 (random)  |
| `UNIT_PEERS` | Comma-separated seed peers (e.g. `host:port`)  | (none)      |

### Running a local mesh

```sh
# Terminal 1 — seed node
UNIT_PORT=4201 cargo run

# Terminal 2 — joins the mesh
UNIT_PORT=4202 UNIT_PEERS=127.0.0.1:4201 cargo run

# Terminal 3 — also joins (discovers terminal 2 via gossip)
UNIT_PORT=4203 UNIT_PEERS=127.0.0.1:4201 cargo run
```

### Mesh Forth words

| Word           | Stack effect         | Description                                 |
|----------------|----------------------|---------------------------------------------|
| `PEERS`        | `( -- n )`           | Number of known peers                       |
| `SEND`         | `( addr n peer -- )` | Send n bytes from addr to mesh              |
| `RECV`         | `( -- addr n peer )` | Receive next message (0 0 0 if none)        |
| `REPLICATE`    | `( -- )`             | Serialize state and broadcast to peers      |
| `PROPOSE`      | `( -- )`             | Start a consensus vote for replication      |
| `MESH-STATUS`  | `( -- )`             | Print mesh state, peers, and event log      |
| `LOAD`         | `( -- n )`           | Current load metric                         |
| `CAPACITY`     | `( -- n )`           | Capacity threshold                          |
| `ID`           | `( -- addr n )`      | This unit's hex ID as a string              |
| `TYPE`         | `( addr n -- )`      | Print n characters from memory              |

### Consensus Protocol

Any unit can propose replication when it observes high mesh load:

1. Proposer broadcasts `PROPOSE` with its serialized state
2. Each peer evaluates and votes `YES` or `NO`
3. Simple majority quorum (>50% of peers) required
4. If quorum reached within 5 seconds → `COMMIT` (state broadcast)
5. Otherwise → `REJECT`
6. 10-second cooldown between proposals per node
7. One active proposal per node at a time

## License

CC0 1.0 Universal — see [LICENSE](LICENSE).
