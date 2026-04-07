# unit — Word Reference

309 words. Organized by category.

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
| `+` `-` `*` `/` `MOD` | arithmetic | | `=` `<` `>` | comparison |
| `AND` `OR` `NOT` | bitwise logic | | `ABS` `NEGATE` `MIN` `MAX` | math |
| `1+` `1-` `2*` `2/` | shortcuts | | `0=` `0<` `<>` `TRUE` `FALSE` | predicates |

## Memory

| Word | Description |
|------|-------------|
| `@` `!` | fetch / store |
| `HERE` `,` `C,` `ALLOT` `CELLS` | data space allocation |
| `VARIABLE` `CONSTANT` `CREATE` | data words |

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
| `WORDS` `SEE` `EVAL"` | introspection |

## S-Expressions

| Word | Description |
|------|-------------|
| `SEXP"` | parse S-expression, translate to Forth, execute |
| `SEXP-SEND"` | broadcast S-expression to mesh peers |
| `SEXP-RECV` | drain inbound S-expression messages |

## Mesh & Gossip

| Word | Description |
|------|-------------|
| `PEERS` `MESH-STATUS` `ID` `MY-ADDR` | mesh info |
| `PEER-TABLE` `MESH-STATS` `MESH-KEY` | cross-machine |
| `CONNECT"` `DISCONNECT"` | manual peer management |
| `SEND` `RECV` | raw messaging |
| `DISCOVER` `AUTO-DISCOVER` | LAN discovery |
| `SHARE"` `SHARE-ALL` `AUTO-SHARE` `SHARED-WORDS` | word sharing |
| `SWARM-ON` `SWARM-OFF` `SWARM-STATUS` | swarm mode |

## Distributed Computation

| Word | Description |
|------|-------------|
| `DIST-GOAL{` | distribute pipe-separated expressions across peers |
| `DIST-STATUS` | show active distributed goals |
| `DIST-CANCEL` | cancel all distributed goals |

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
| `GOALS` `TASKS` `REPORT` `CLAIM` `COMPLETE` | lifecycle |
| `SUBTASK{` `FORK` `RESULTS` `REDUCE"` `PROGRESS` | decomposition |
| `AUTO-CLAIM` `TIMEOUT` | execution control |

## Monitoring

| Word | Description |
|------|-------------|
| `WATCH"` `WATCH-FILE"` `WATCH-PROC"` | create watches |
| `WATCHES` `UNWATCH` `WATCH-LOG` `UPTIME` | manage watches |
| `ON-ALERT"` `ALERTS` `ACK` `ALERT-HISTORY` `HEAL` | alerting |
| `DASHBOARD` `HEALTH` `OPS` | overview |
| `EVERY` `SCHEDULE` `UNSCHED` | scheduling |

## Fitness & Mutation

| Word | Description |
|------|-------------|
| `FITNESS` `LEADERBOARD` `RATE` | scoring |
| `MUTATE` `MUTATE-WORD"` `UNDO-MUTATE` `MUTATIONS` | mutation |
| `SMART-MUTATE` `MUTATION-REPORT` `MUTATION-STATS` | smart mutation |
| `EVOLVE` `AUTO-EVOLVE` `BENCHMARK"` | fitness-driven evolution |

## Spawn & Replication

| Word | Description |
|------|-------------|
| `SPAWN` `SPAWN-N` | local replication |
| `PACKAGE` `PACKAGE-SIZE` | build UREP package |
| `REPLICATE-TO"` | remote replication |
| `CHILDREN` `FAMILY` `GENERATION` `KILL-CHILD` | lineage |
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

## Trust & Consent

| Word | Description |
|------|-------------|
| `TRUST-ALL` `TRUST-MESH` `TRUST-FAMILY` `TRUST-NONE` | trust levels |
| `TRUST-LEVEL` `REQUESTS` `ACCEPT` `DENY` `DENY-ALL` | consent flow |
| `REPLICATION-LOG` | audit trail |

## Persistence

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
