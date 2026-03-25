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

1. **Seed**: The Rust binary boots with kernel primitives in native code.
2. **Dictionary**: The prelude (`src/prelude.fs`) extends the dictionary at startup.
3. **Full system**: The REPL accepts new definitions, mesh primitives enable
   communication, and executable goals turn the mesh into a distributed
   computation engine.

## Build and Run

```sh
cargo build --release
cargo run
```

You'll see:

```
unit v0.3.0 — seed online
Mesh node a1b2c3d4e5f67890 online with 0 peers
auto-claim: ON
> 2 3 + .
5 ok
>
```

Type `BYE` to exit.

## Mesh Networking

Units discover and communicate over UDP. Configure via environment variables:

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

# Terminal 3 — discovers terminal 2 via gossip
UNIT_PORT=4203 UNIT_PEERS=127.0.0.1:4201 cargo run
```

### Mesh words

| Word           | Stack effect         | Description                                 |
|----------------|----------------------|---------------------------------------------|
| `PEERS`        | `( -- n )`           | Number of known peers                       |
| `SEND`         | `( addr n peer -- )` | Send n bytes from addr to mesh              |
| `RECV`         | `( -- addr n peer )` | Receive next message (0 0 0 if none)        |
| `REPLICATE`    | `( -- )`             | Serialize state and broadcast to peers      |
| `PROPOSE`      | `( -- )`             | Start a consensus vote for replication      |
| `MESH-STATUS`  | `( -- )`             | Print mesh state, peers, and event log      |
| `ID`           | `( -- addr n )`      | This unit's hex ID as a string              |
| `TYPE`         | `( addr n -- )`      | Print n characters from memory              |

### Consensus Protocol

1. Proposer broadcasts `PROPOSE` with serialized state
2. Peers vote `YES` or `NO` (majority quorum required)
3. Quorum reached within 5s → `COMMIT`, otherwise → `REJECT`
4. 10s cooldown between proposals; one active proposal per node

## Human Guidance

Humans set direction, the mesh navigates.

### Goal words

| Word                        | Stack effect              | Description                              |
|-----------------------------|---------------------------|------------------------------------------|
| `GOAL" <desc>"`            | `( priority -- goal-id )` | Submit a description-only goal           |
| `GOAL{ <code> }`           | `( priority -- goal-id )` | Submit an executable Forth goal          |
| `GOALS`                     | `( -- )`                  | List all known goals                     |
| `TASKS`                     | `( -- )`                  | List this unit's claimed tasks           |
| `TASK-STATUS`               | `( goal-id -- )`          | Show task breakdown for a goal           |
| `CLAIM`                     | `( -- task-id )`          | Claim the next available task            |
| `COMPLETE`                  | `( task-id -- )`          | Mark a task as done                      |
| `CANCEL`                    | `( goal-id -- )`          | Cancel a goal and its tasks              |
| `STEER`                     | `( goal-id priority -- )` | Change a goal's priority                 |
| `REPORT`                    | `( -- )`                  | Mesh-wide progress summary               |
| `STATUS`                    | `( -- )`                  | Combined mesh + goals + tasks view       |

## Executable Goals

Goals can carry Forth code as a payload. When a unit claims such a task,
it executes the code in a **sandbox** — a fresh stack isolated from the
unit's own state — and captures the result.

### How it works

```
> 5 GOAL{ 2 3 + 4 * }
goal #4201 created [exec]: 2 3 + 4 *
```

When a unit (this one or a peer) claims the task, it:

1. Saves its current stack
2. Creates a fresh execution context (empty stack, shared dictionary)
3. Compiles and runs the Forth payload
4. Captures the resulting stack and any printed output
5. Restores the original stack
6. Broadcasts the result to the mesh

### Execution words

| Word           | Stack effect         | Description                                   |
|----------------|----------------------|-----------------------------------------------|
| `GOAL{ <code> }` | `( priority -- id )` | Submit Forth code as a distributed goal     |
| `EVAL" <code>"`  | `( -- )`            | Evaluate a Forth string immediately           |
| `RESULT`       | `( task-id -- )`     | Display a completed task's result             |
| `GOAL-RESULT`  | `( goal-id -- )`     | Display combined results for all tasks        |
| `AUTO-CLAIM`   | `( -- )`             | Toggle automatic task claiming (on by default)|
| `TIMEOUT`      | `( seconds -- )`     | Set execution timeout (default: 10s)          |

### Sandboxed execution

- **Isolation**: task code runs on a fresh stack; the unit's own stack
  is saved and restored
- **Timeout**: execution is limited (default 10 seconds) — infinite loops
  fail the task, they don't crash the unit
- **Error handling**: any error marks the task as failed with a message;
  the unit continues running normally
- **Output capture**: all printed output (`.`, `."`, `EMIT`, etc.) is
  captured into the task result, not printed to the REPL

### Built-in test goals

```
> PING-GOAL DROP       \ sends ." pong" to the mesh
> MATH-GOAL DROP       \ sends 2 3 + 4 * — result: stack [20]
> STRESS-GOAL DROP     \ loop 1M times, print "done"
> WORDS-GOAL DROP      \ list the dictionary
```

### Example: distributed computation

```sh
# Terminal 1 — submit work
UNIT_PORT=4201 cargo run
> 5 GOAL{ 100 0 DO I LOOP DEPTH }
goal #4201 created [exec]: 100 0 DO I LOOP DEPTH

# Terminal 2 — auto-claims and executes
UNIT_PORT=4202 UNIT_PEERS=127.0.0.1:4201 cargo run
[auto] claimed task #4202 (goal #4201): 100 0 DO I LOOP DEPTH
[auto] stack: 0 1 2 3 ... 99 100
[auto] task #4202 done
```

Back on terminal 1:

```
> GOALS
  #4201 [completed] [exec] p=5 (1/1 tasks): 100 0 DO I LOOP DEPTH
> 4201 GOAL-RESULT
goal #4201 [completed]: 100 0 DO I LOOP DEPTH
  task #4202:
  status: ok
  stack: 0 1 2 ... 99 100
```

### Auto-claim

Auto-claim is **on by default**. When enabled, each unit automatically
grabs and executes the highest-priority unclaimed executable task after
each REPL command. Toggle with `AUTO-CLAIM`.

The mesh self-scales: when pending tasks outnumber available units,
auto-replication is proposed through consensus.

## License

CC0 1.0 Universal — see [LICENSE](LICENSE).
