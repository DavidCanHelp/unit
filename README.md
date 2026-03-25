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

## Human Guidance

Humans set direction, the mesh navigates. The goal system lets a human
operator submit high-level objectives and watch the mesh organize around them.

### Philosophy

A goal is a human-provided intention — a thing that needs to happen. When
submitted, the goal is broadcast to every unit in the mesh. Each goal
spawns one or more tasks. Units claim tasks, work on them, and report
results. When all tasks for a goal are done, the goal is complete.

The mesh self-scales: when there are more pending tasks than units, the
system automatically proposes replication via consensus.

### Goal Forth words

| Word                        | Stack effect           | Description                              |
|-----------------------------|------------------------|------------------------------------------|
| `GOAL" <desc>"`            | `( priority -- )`      | Submit a goal with a priority (1-10)     |
| `GOALS`                     | `( -- )`               | List all known goals and their status    |
| `TASKS`                     | `( -- )`               | List this unit's claimed tasks           |
| `TASK-STATUS`               | `( goal-id -- )`       | Show task breakdown for a goal           |
| `CLAIM`                     | `( -- task-id )`       | Claim the next available task            |
| `COMPLETE`                  | `( task-id -- )`       | Mark a task as done                      |
| `CANCEL`                    | `( goal-id -- )`       | Cancel a goal and its tasks              |
| `STEER`                     | `( goal-id priority -- )` | Change a goal's priority              |
| `REPORT`                    | `( -- )`               | Mesh-wide progress summary               |
| `STATUS`                    | `( -- )`               | Combined mesh + goals + tasks view       |

### Example session

```
> 5 GOAL" analyze incoming sensor data"
goal #4201 created
 ok

> 8 GOAL" optimize replication protocol"
goal #4202 created
 ok

> GOALS
  #4201 [pending] p=5 (0/1 tasks): analyze incoming sensor data
  #4202 [pending] p=8 (0/1 tasks): optimize replication protocol
 ok

> CLAIM
claimed task #4203 (goal #4202): optimize replication protocol
 ok

> TASKS
  task #4203 [running] goal #4202: optimize replication protocol
 ok

> 4203 COMPLETE
task #4203 completed
 ok

> REPORT
--- mesh progress report ---
goals: 2 total (1 pending, 0 active, 1 completed, 0 failed)
tasks: 2 total (1 waiting, 0 running, 1 done, 0 failed)
workers: 0 active units
---
 ok
```

### Goal distribution

Goals propagate through the mesh via gossip:

- **On creation**: the goal is broadcast to all peers immediately
- **On peer join**: active goals are sent to newly discovered peers
- **Conflict resolution**: status transitions are monotonic
  (pending → active → completed/failed), so stale updates are ignored
- **Task claiming**: units claim the highest-priority unclaimed task;
  claims are broadcast to prevent double-assignment
- **Auto-replication**: when pending tasks outnumber units, the mesh
  proposes replication through the consensus protocol

## License

CC0 1.0 Universal — see [LICENSE](LICENSE).
