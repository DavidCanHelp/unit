# unit — Word Reference

339 words in the live dictionary (run `WORDS` to list them). Organized by
category below. Stack effects use standard Forth notation `( before -- after )`;
words shown without one are `( -- )` (pure side effect / print). Words whose
name ends in `"` parse a trailing string argument up to a closing `"`.

## Stack

| Word | Effect | | Word | Effect |
|------|--------|-|------|--------|
| `DUP` | `( a -- a a )` | | `2DUP` | `( a b -- a b a b )` |
| `DROP` | `( a -- )` | | `2DROP` | `( a b -- )` |
| `SWAP` | `( a b -- b a )` | | `NIP` | `( a b -- b )` |
| `OVER` | `( a b -- a b a )` | | `TUCK` | `( a b -- b a b )` |
| `ROT` | `( a b c -- b c a )` | | `.S` | print stack |

## Arithmetic & Logic

| Word | Effect | | Word | Effect |
|------|--------|-|------|--------|
| `+` `-` `*` `/` `MOD` | arithmetic | | `=` `<` `>` `>=` | comparison |
| `AND` `OR` `NOT` `INVERT` | bitwise logic | | `ABS` `NEGATE` `MIN` `MAX` | math |
| `1+` `1-` `2*` `2/` | shortcuts | | `0=` `0<` `<>` `TRUE` `FALSE` | predicates |

`>=` is `( a b -- flag )`; `INVERT` is `( n -- ~n )` (bitwise complement, alias
of `NOT`).

## Memory

| Word | Description |
|------|-------------|
| `@` `!` | fetch / store |
| `HERE` `,` `C,` `ALLOT` `CELLS` | data space allocation |
| `VARIABLE` `CONSTANT` `CREATE` `DOES>` | data words |

`DOES>` attaches runtime behavior to the most recently `CREATE`d word
(seed-level approximation — the code after `DOES>` in a definition becomes that
word's action).

## I/O

| Word | Description |
|------|-------------|
| `.` `.S` `EMIT` `CR` `SPACE` `SPACES` `TYPE` | output |
| `KEY` `."` | input / string literal |
| `FILE-READ"` `FILE-WRITE"` `FILE-EXISTS"` `FILE-LIST"` `FILE-DELETE"` | filesystem |
| `HTTP-GET"` `HTTP-POST"` | raw HTTP/1.1 |
| `SHELL"` `ENV"` `TIMESTAMP` `SLEEP` | system |
| `IO-LOG` `SANDBOX-ON` `SANDBOX-OFF` `SHELL-ENABLE` | security |

## Control Flow

| Word | Description |
|------|-------------|
| `IF` `ELSE` `THEN` | conditional |
| `DO` `LOOP` `I` `J` | counted loop |
| `BEGIN` `UNTIL` `WHILE` `REPEAT` | indefinite loop |
| `:` `;` `RECURSE` | word definitions |
| `(` `\` | comments — `( ... )` inline, `\` to end of line |
| `WORDS` `SEE` `EVAL"` `HELP` | introspection |
| `BYE` `QUIT` | leave the REPL — `BYE` auto-saves first, `QUIT` exits immediately |

`HELP` prints the top-level guide; the topic pages are `HELP-STACK`
`HELP-MATH` `HELP-MESH` `HELP-GOALS` `HELP-MONITOR` `HELP-SPAWN` `HELP-IO`
`HELP-COLONY` `HELP-PERSIST` `HELP-EVOLVE` `HELP-DIST` `HELP-MEMORY`
`HELP-IMMUNE` (all `( -- )`).

## S-Expressions

| Word | Description |
|------|-------------|
| `SEXP"` | parse S-expression, translate to Forth, execute |
| `SEXP-EVAL"` | evaluate an S-expression in a sandbox, print its `(result ...)` envelope |
| `SEXP-SEND"` | broadcast S-expression to mesh peers |
| `SEXP-RECV` | drain inbound S-expression messages |

## Mesh & Gossip

| Word | Description |
|------|-------------|
| `PEERS` `MESH-STATUS` `ID` `MY-ADDR` `MESH-HELLO` | mesh info |
| `PEER-COUNT` | `( -- n )` count of connected peers |
| `MESH-AVG-FITNESS` | `( -- avg )` mean fitness across self + peers |
| `LOAD` `CAPACITY` | `( -- n )` local load metric / capacity threshold |
| `PEER-TABLE` `MESH-STATS` `MESH-KEY` | cross-machine |
| `CONNECT"` `DISCONNECT"` | manual peer management |
| `SEND` `RECV` | raw messaging |
| `PROPOSE` `REPLICATE` | broadcast this unit's serialized state (consensus / direct) |
| `DISCOVER` `AUTO-DISCOVER` | LAN discovery |
| `SHARE"` `SHARE-ALL` `AUTO-SHARE` `SHARED-WORDS` | word sharing |
| `SWARM-ON` `SWARM-OFF` `SWARM` `SWARM-STATUS` | swarm mode |
| `AUTO-SPAWN` `AUTO-CULL` | toggle population auto-grow / auto-shrink |
| `MIN-UNITS` `MAX-UNITS` | `( n -- )` population bounds for auto-spawn/cull |

## Distributed Computation

| Word | Description |
|------|-------------|
| `DIST-GOAL{` | distribute pipe-separated expressions across peers |
| `DIST-STATUS` | show active distributed goals |
| `DIST-CANCEL` | cancel all distributed goals |
| `RECRUIT"` | send `"<peer> <s-expr>"` as a recruit round-trip |
| `RECRUITS` | show outstanding and collected recruit round-trips |
| `PARALLEL"` | run `"(parallel (e1) (e2) ...)"` under local resource pressure, print collected results |

## Genetic Programming

| Word | Description |
|------|-------------|
| `GP-EVOLVE` | run 10 generations (call repeatedly to continue) |
| `GP-STATUS` `GP-BEST` | inspect evolution state |
| `GP-STOP` `GP-RESET` | control evolution |

## Immune System & Energy

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
| `GENERATORS` | list top generators by fitness and program |
| `META-EVOLVE` | run one generation of generator evolution |
| `SCORERS` | list top scoring functions (third-order) |
| `META-DEPTH` | evolution depth at all three levels |
| `GENERATE-CHALLENGE` | evolve and register a new challenge from best generator |
| `EVOLUTION-STATS` | combined summary: depth, generators, scorers, environment |
| `SOLUTIONS` | `( id -- )` list all solutions for a challenge |
| `DIVERSITY` | colony-wide solution diversity stats |
| `PERSONALITY` | current behavioral profile |

## Goals & Tasks

| Word | Description |
|------|-------------|
| `GOAL"` | `( priority -- id )` description-only goal |
| `GOAL{` `}` | `( priority -- id )` executable Forth goal |
| `GOALS` `TASKS` `REPORT` `CLAIM` `COMPLETE` `STATUS` | lifecycle |
| `TASK-STATUS` | `( goal-id -- )` task breakdown for one goal |
| `CANCEL` | `( goal-id -- )` cancel a goal and its tasks |
| `STEER` | `( goal-id priority -- )` change a goal's priority |
| `RESULT` | `( task-id -- )` print a completed task's result |
| `GOAL-RESULT` | `( goal-id -- )` combined results across a goal's tasks |
| `SUBTASK{` `FORK` `RESULTS` `REDUCE"` `PROGRESS` | decomposition |
| `AUTO-CLAIM` `TIMEOUT` | execution control |
| `GOAL-COUNT` | `( -- total pending active completed failed )` goal tallies |
| `TASK-COUNT` | `( -- total waiting running done failed )` task tallies |

Built-in demo goals (each `( -- id )`): `PING-GOAL` `MATH-GOAL` `STRESS-GOAL`
`WORDS-GOAL` `HELLO-WORLD`.

## Monitoring

| Word | Description |
|------|-------------|
| `WATCH"` `WATCH-FILE"` `WATCH-PROC"` | create watches |
| `WATCHES` `UNWATCH` `WATCH-LOG` `UPTIME` | manage watches |
| `ON-ALERT"` `ALERTS` `ACK` `ALERT-HISTORY` `HEAL` | alerting |
| `ALERT-THRESHOLD` | `( level -- )` set alert level; reads a trailing `watch-id"` |
| `CHECK-WATCHES` `RUN-HANDLERS` | `( -- )` run due checks / fire alert handlers |
| `WATCH-COUNT` `ALERT-COUNT` | `( -- n )` watch / active-alert counts |
| `DASHBOARD` `HEALTH` `OPS` | overview |
| `HEALTH-PORT` | `( -- port )` replication port (0 if mesh offline) |
| `EVERY` `SCHEDULE` `UNSCHED` | scheduling |

## Fitness & Mutation

| Word | Description |
|------|-------------|
| `FITNESS` `LEADERBOARD` `RATE` | scoring |
| `MUTATE` `MUTATE-WORD"` `UNDO-MUTATE` `MUTATIONS` | mutation |
| `MUTATE-RANDOM` | `( -- flag )` mutate a random word; `-1` on success, `0` if none |
| `UNDO-LAST-MUTATION` | `( -- )` revert the most recent mutation |
| `SMART-MUTATE` `MUTATION-REPORT` `MUTATION-STATS` | smart mutation |
| `EVOLVE` `AUTO-EVOLVE` `BENCHMARK"` | fitness-driven evolution |
| `RUN-BENCHMARK` | `( -- score )` run the benchmark, push its score |

## Spawn & Replication

| Word | Description |
|------|-------------|
| `SPAWN` `SPAWN-N` | local replication |
| `PACKAGE` `PACKAGE-SIZE` | build UREP package |
| `REPLICATE-TO"` | remote replication |
| `TRANSPORT` | self-relocate to a sufficient-first destination with confirm-before-release (costs 150; no-op when not mislocated, no destination, or starving). Unit-invoked, GP-mutable. See [self-replication.md](self-replication.md) |
| `CHILDREN` `FAMILY` `FAMILY-TREE` `GENERATION` `KILL-CHILD` | lineage |
| `CHILD-COUNT` | `( -- n )` number of local children |
| `SPAWN-TEST` | `( -- )` spawn one child and announce it (demo helper) |
| `ACCEPT-REPLICATE` `DENY-REPLICATE` `QUARANTINE` `MAX-CHILDREN` | safety |

## Reproduction

| Word | Description |
|------|-------------|
| `MATE` | initiate sexual reproduction with a mesh peer |
| `MATE-STATUS` | show pending mating requests and offspring count |
| `ACCEPT-MATE` `DENY-MATE` | control auto-accept for mating |
| `OFFSPRING` | list children produced by mating |

## Ecology

| Word | Description |
|------|-------------|
| `NICHE` | show niche profile: specializations and modifiers |
| `NICHE-HISTORY` | last 20 challenge outcomes with categories |
| `ECOLOGY` | colony-wide ecological diversity |

## Signaling

Inter-unit signaling — direct (peer inbox) and environmental layers.
See [signaling.md](signaling.md) for the design rationale.

| Word | Stack effect | Cost | Description |
|------|--------------|------|-------------|
| `SAY!` | `( v -- )` | 3 | broadcast value `v` to neighbors' inboxes |
| `LISTEN` | `( -- v -1 \| 0 )` | 0 | pop oldest inbox entry; push value+flag, or 0 if empty |
| `INBOX?` | `( -- n )` | 0 | push count of pending inbox entries |
| `MARK!` | `( v -- )` | 5 | deposit `v` into per-host environmental field, keyed by dominant niche (native only; WASM shim) |
| `SENSE` | `( -- v )` | 0 | read current environmental strength for this unit's niche (native only; WASM shim) |
| `COURT` | `( -- )` | 3 | prelude word: `: COURT FITNESS SAY! ;` — honest mate-finding signal |

## Trust & Consent

| Word | Description |
|------|-------------|
| `TRUST-ALL` `TRUST-MESH` `TRUST-FAMILY` `TRUST-NONE` | trust levels |
| `TRUST` | `( id -- )` add one node ID to the trusted-peer set |
| `TRUST-LEVEL` `REQUESTS` `ACCEPT` `DENY` `DENY-ALL` | consent flow |
| `REPLICATION-LOG` | audit trail |

## Persistence

| Word | Description |
|------|-------------|
| `JSON-SNAPSHOT` `JSON-RESTORE` | save/load the genome snapshot (S-expression since v0.34; names kept for compatibility — legacy JSON files still load) |
| `HIBERNATE` | snapshot and exit |
| `AUTO-SNAPSHOT` | periodic auto-save |
| `SNAPSHOT-PATH` `JSON-SNAPSHOTS` | inspect storage (lists both `.sexp` and legacy `.json`) |
| `EXPORT-GENOME` `IMPORT-GENOME"` | genome transfer |
| `SAVE` `LOAD-STATE` `RESET` | binary state management |
| `SNAPSHOT` `SNAPSHOTS` `RESTORE` | binary versioned backups |
| `AUTO-SAVE` | binary auto-save |
| `REIDENTIFY` | `( -- )` generate a new node ID and migrate saved state |
| `PERSIST-TEST` | `( -- )` `SAVE` and confirm (demo helper) |

## WebSocket Bridge

Live browser view of the mesh. All `( -- )`.

| Word | Description |
|------|-------------|
| `WS-STATUS` | bridge running state |
| `WS-CLIENTS` | connected browser clients |
| `WS-PORT` | `( -- port )` WebSocket port (0 if not running) |
| `WS-BROADCAST"` | push a `"message"` to all connected browsers |

## Resource Load

Load generator that forces the resource ceiling for recruit-path testing. Gated
by `ALLOC-ENABLE` (off by default) so evolved GP code cannot reach it.

| Word | Description |
|------|-------------|
| `ALLOC-ENABLE` | `( -- )` toggle the gate that lets `ALLOC-MB` allocate |
| `ALLOC-MB` | `( mb -- allocated )` allocate & retain N MiB; pushes MiB actually taken (0 if refused/disabled) |
| `RECLAIM-MB` | `( -- freed )` free all retained allocations; pushes chunk count freed |

## Colony, Persona & Lifecycle

Prelude-defined "creature" vocabulary — a unit is a nanobot, not a process.
All `( -- )` unless noted. See `HELP-COLONY`.

| Word | Description |
|------|-------------|
| `HELLO` | introduce this unit (id, generation, peers, fitness) |
| `HEADCOUNT` | how many units are in the mesh |
| `ROLL-CALL` | self report plus the fitness leaderboard |
| `WORKFORCE` | available units and pending task load |
| `PATROL` | check watches; run handlers if alerts, else "all clear" |
| `CHECKUP` | full status: `PATROL` `PROUD` `INTROSPECT` |
| `PROUD` | one-line fitness / generation / children |
| `STRETCH` | warm-up busy loop |
| `BORN` | birth announcement |
| `GROW` | `EVOLVE` then `MUTATE`, then report fitness |
| `REPRODUCE` | announce package size and `SPAWN` a child |
| `REST` `WAKE` | save state / load state (with a message) |
| `MORNING` `EVENING` | start a shift (`WAKE HELLO CHECKUP`) / end one (`REST`) |
| `SWARM` | swarm overview: `SWARM-STATUS MESH-STATUS LEADERBOARD` |
| `SECURE-SWARM` | `SWARM-ON` with mesh-only trust |
| `LOCKDOWN` | `TRUST-NONE` and `QUARANTINE` — block replication |
| `JOYFUL` | `( -- flag )` true when this unit has peers |
| `JOY` | express mesh-connection joy (varies by peer count) |
| `HOW-ARE-YOU` | status as a mood, varying by fitness and peers |
| `LONELY` `BUSY` | mood by peer count / task load |
| `SAY-SOMETHING` | state-driven utterance (cycles via `PERSONALITY-SEED`) |
| `PERSONALITY-SEED` | `( -- addr )` variable seeding `SAY-SOMETHING` |

## Self-Programming

Words that rewrite the unit's own Forth to match its current state. All `( -- )`
unless noted. See `HELP-COLONY`.

| Word | Description |
|------|-------------|
| `OBSERVE` | record one self-observation (bumps `OBS-COUNT`) |
| `OBS-COUNT` | `( -- addr )` variable counting adaptations/observations |
| `COMPOSE-ROUTINE` | pick a routine (social vs. solo) for the current state |
| `INVENT-GREETER` `INVENT-STRATEGY` | derive a greeter / strategy from state |
| `MY-ROUTINE` `MY-STRATEGY` `GREET` | default composable words (redefined by `ADAPT`) |
| `ADAPT` | recompose routine, greeter, and strategy for now |
| `TEACH` | `ADAPT` then `SHARE-ALL` with the mesh |
| `REFLECT` | decide whether adaptation is needed |
| `DREAM` | deep cycle: reflect, invent, compose, mutate, teach |
| `INTROSPECT` | mood plus adaptation-count history |
