\ unit prelude — Forth words defined in Forth itself
\ This file is compiled at boot before the REPL starts.

\ --- Stack utilities ---
: NIP  ( a b -- b )       SWAP DROP ;
: TUCK ( a b -- b a b )   SWAP OVER ;
: 2DUP ( a b -- a b a b ) OVER OVER ;
: 2DROP ( a b -- )         DROP DROP ;

\ --- Arithmetic utilities ---
: NEGATE ( n -- -n )   0 SWAP - ;
: ABS    ( n -- |n| )  DUP 0 < IF NEGATE THEN ;
: MIN    ( a b -- min ) 2DUP > IF SWAP THEN DROP ;
: MAX    ( a b -- max ) 2DUP < IF SWAP THEN DROP ;
: 1+     ( n -- n+1 )  1 + ;
: 1-     ( n -- n-1 )  1 - ;
: 2*     ( n -- n*2 )  2 * ;
: 2/     ( n -- n/2 )  2 / ;
: 0=     ( n -- flag ) 0 = ;
: 0<     ( n -- flag ) 0 < ;
: <>     ( a b -- flag ) = NOT ;

\ --- Boolean ---
: TRUE  ( -- -1 ) -1 ;
: FALSE ( -- 0 )   0 ;
: INVERT ( n -- ~n ) NOT ;

\ --- I/O helpers ---
: SPACE  ( -- ) 32 EMIT ;
: SPACES ( n -- ) 0 DO SPACE LOOP ;

\ --- Mesh helpers ---
: MESH-HELLO ." Mesh node " ID TYPE ."  gen=" GENERATION . ." peers=" PEERS . ." fitness=" FITNESS . CR ;

\ --- Orchestration (Forth-level composition of atom primitives) ---

: REPORT ( -- )
  CR ." --- mesh progress ---" CR
  GOAL-COUNT
  ." goals: " 4 0 DO . ." / " LOOP . ."  (total/pend/active/done/fail)" CR
  TASK-COUNT
  ." tasks: " 4 0 DO . ." / " LOOP . ."  (total/wait/run/done/fail)" CR
  ." ---" CR
;

: FAMILY ( -- )
  ." id: " ID TYPE ."  gen: " GENERATION .
  ."  children: " CHILD-COUNT . CR
;

: DASHBOARD ( -- )
  CR ." === UNIT OPS ===" CR
  ." watches: " WATCH-COUNT . ."  alerts: " ALERT-COUNT . CR
  ." peers: " PEER-COUNT . ."  fitness: " FITNESS . CR
  GOAL-COUNT ." goals: " 4 0 DO . ." / " LOOP . CR
  ." ---" CR
;

: HEAL ( -- )
  ." --- heal ---" CR
  CHECK-WATCHES
  RUN-HANDLERS
  ." --- done ---" CR
;

: >= ( a b -- flag ) < NOT ;

: EVOLVE ( -- )
  RUN-BENCHMARK
  MUTATE-RANDOM IF
    RUN-BENCHMARK
    2DUP >= IF
      ." kept (" . ." -> " . ." )" CR
    ELSE
      ." reverted (" . ." -> " . ." )" CR UNDO-LAST-MUTATION
    THEN
  ELSE
    DROP ." no mutation" CR
  THEN
;

: STATUS  ( -- ) MESH-STATUS GOALS TASKS FAMILY ;

\ --- Built-in executable goals ---
: PING-GOAL     ( -- id ) 5 GOAL{ ." pong" } ;
: MATH-GOAL     ( -- id ) 5 GOAL{ 2 3 + 4 * } ;
: STRESS-GOAL   ( -- id ) 3 GOAL{ 1000000 0 DO LOOP ." done" } ;
: WORDS-GOAL    ( -- id ) 5 GOAL{ WORDS } ;
: HELLO-WORLD   ( -- id ) 5 GOAL{ ." Hello from the mesh!" } ;
: PERSIST-TEST  ( -- )    SAVE ." state saved" CR ;

\ --- Spawn helpers ---
: FAMILY-TREE  ( -- ) FAMILY CHILDREN ;
: SPAWN-TEST   ( -- ) SPAWN ." spawned child" CR ;

\ --- Ops helpers ---
: OPS      ( -- ) DASHBOARD ALERTS SCHEDULE ;

\ --- Swarm helpers ---
: SWARM-ON  ( -- ) AUTO-DISCOVER AUTO-SHARE AUTO-SPAWN TRUST-ALL ." swarm mode active" CR ;
: SWARM-OFF ( -- ) ." swarm mode disabled" CR ;
: SWARM     ( -- ) SWARM-STATUS MESH-STATUS LEADERBOARD ;

\ --- OPS (recomposed from atoms) ---
: OPS  ( -- ) DASHBOARD ALERTS SCHEDULE ;
: SECURE-SWARM ( -- ) SWARM-ON TRUST-MESH ." swarm with mesh trust" CR ;
: LOCKDOWN  ( -- ) TRUST-NONE QUARANTINE ." replication locked" CR ;

\ === HELP system ===

: HELP
  CR
  ." === unit -- a self-replicating software nanobot ===" CR CR
  ." BASICS" CR
  ."   2 3 + .                       Add two numbers, print result" CR
  ."   : SQUARE DUP * ;              Define a new word" CR
  ."   7 SQUARE .                    Use it (49)" CR
  ."   SEE SQUARE                    Inspect a word's definition" CR
  ."   WORDS                         List all words" CR CR
  ." STACK" CR
  ."   DUP DROP SWAP OVER ROT        Core stack operations" CR
  ."   .S                            Show stack without consuming" CR
  ."   NIP TUCK 2DUP 2DROP           Extended stack ops" CR CR
  ." CONTROL FLOW" CR
  ."   10 0 DO I . LOOP              Loop 0 to 9" CR
  ."   IF ... ELSE ... THEN          Conditional" CR
  ."   BEGIN ... UNTIL               Loop until true" CR
  ."   BEGIN ... WHILE ... REPEAT    While loop" CR CR
  ." MESH" CR
  ."   MESH-STATUS                   Peers, port, event log" CR
  ."   SWARM-ON                      Discovery + sharing + auto-spawn" CR
  ."   SHARE" ." MYWORD" ."              Share a word with peers" CR CR
  ." GOALS" CR
  ."   5 GOAL{ 6 7 * }               Distribute Forth as work" CR
  ."   GOALS                         List all goals" CR
  ."   DASHBOARD                     Ops overview with sparklines" CR CR
  ." MONITORING" CR
  ."   30 WATCH" ." http://x.com" ."     Monitor a URL" CR
  ."   ALERTS                        Active alerts" CR
  ."   HEAL                          Run alert handlers" CR CR
  ." REPLICATION" CR
  ."   SPAWN                         Birth a child process" CR
  ."   SAVE / SNAPSHOT               Persist state to disk" CR
  ."   TRUST-LEVEL                   Show trust setting" CR CR
  ." MORE: HELP-STACK HELP-MATH HELP-MESH HELP-GOALS" CR
  ."       HELP-MONITOR HELP-SPAWN HELP-IO" CR
;

: HELP-STACK
  CR ." === Stack Operations ===" CR CR
  ."   DUP   ( a -- a a )          Duplicate top" CR
  ."   DROP  ( a -- )              Discard top" CR
  ."   SWAP  ( a b -- b a )        Swap top two" CR
  ."   OVER  ( a b -- a b a )      Copy second to top" CR
  ."   ROT   ( a b c -- b c a )    Rotate third to top" CR
  ."   NIP   ( a b -- b )          Drop second" CR
  ."   TUCK  ( a b -- b a b )      Copy top under second" CR
  ."   2DUP  ( a b -- a b a b )    Duplicate pair" CR
  ."   2DROP ( a b -- )            Drop pair" CR
  ."   .S    ( -- )                Print stack (non-destructive)" CR CR
  ."   Example: 1 2 3 .S  =>  <3> 1 2 3" CR
;

: HELP-MATH
  CR ." === Arithmetic & Logic ===" CR CR
  ."   + - * / MOD                 Basic arithmetic" CR
  ."   = < >                       Comparison (true = -1)" CR
  ."   AND OR NOT                  Bitwise logic" CR
  ."   ABS NEGATE MIN MAX          Numeric utilities" CR
  ."   1+ 1- 2* 2/                 Shortcuts" CR
  ."   0= 0< <>                    Predicates" CR
  ."   TRUE FALSE INVERT           Boolean constants" CR CR
  ."   Example: -7 ABS .  =>  7" CR
  ."   Example: 3 5 > .   =>  0 (false)" CR
;

: HELP-MESH
  CR ." === Mesh Networking ===" CR CR
  ."   MESH-STATUS                 Show peers, port, events" CR
  ."   PEERS .                     Count of connected peers" CR
  ."   ID TYPE                     Print this node's hex ID" CR
  ."   SWARM-ON                    Enable discovery+sharing+spawn" CR
  ."   SWARM-STATUS                Show swarm configuration" CR
  ."   DISCOVER                    Send discovery beacon" CR
  ."   SHARE" ." WORD" ."              Broadcast word to peers" CR
  ."   SHARE-ALL                   Share all non-kernel words" CR
  ."   SHARED-WORDS                List words from peers" CR
  ."   LEADERBOARD                 Fitness rankings" CR CR
  ."   Env vars: UNIT_PORT, UNIT_PEERS, UNIT_WS_PORT" CR
  ."   Start:    UNIT_PORT=4201 unit" CR
  ."   Join:     UNIT_PORT=4202 unit --peers 127.0.0.1:4201" CR
;

: HELP-GOALS
  CR ." === Goals & Tasks ===" CR CR
  ."   5 GOAL{ 6 7 * }            Submit executable goal (priority 5)" CR
  ."   5 GOAL" ." desc" ."            Description-only goal" CR
  ."   GOALS                       List all goals" CR
  ."   TASKS                       List claimed tasks" CR
  ."   CLAIM                       Claim next available task" CR
  ."   COMPLETE ( id -- )          Mark task done" CR
  ."   REPORT                      Mesh-wide progress" CR
  ."   DASHBOARD                   Ops overview" CR CR
  ."   Decomposition:" CR
  ."   5 GOAL{ 100 10 SPLIT DO I LOOP }  Split into 10 subtasks" CR
  ."   FORK ( goal-id n -- )       Split goal into n tasks" CR
  ."   RESULTS ( goal-id -- )      Show subtask results" CR
  ."   REDUCE" ." +" ." ( goal-id -- )  Reduce results" CR
;

: HELP-MONITOR
  CR ." === Monitoring & Alerting ===" CR CR
  ."   30 WATCH" ." http://x.com" ."   Check URL every 30s" CR
  ."   10 WATCH-FILE" ." /var/log/x" ."  Monitor file" CR
  ."   5 WATCH-PROC" ." nginx" ."       Monitor process" CR
  ."   WATCHES                     List all watches" CR
  ."   UNWATCH ( id -- )           Remove a watch" CR
  ."   UPTIME ( id -- )            Show uptime percentage" CR CR
  ."   1 ON-ALERT" ." code" ."         Set handler for watch #1" CR
  ."   ALERTS                      Active alerts" CR
  ."   ACK ( id -- )               Acknowledge alert" CR
  ."   HEAL                        Run all alert handlers" CR
  ."   DASHBOARD                   Overview with sparklines" CR
  ."   60 EVERY DASHBOARD          Auto-refresh every 60s" CR
;

: HELP-SPAWN
  CR ." === Self-Replication ===" CR CR
  ."   SPAWN                       Birth a child process" CR
  ."   SPAWN-N ( n -- )            Spawn n children" CR
  ."   CHILDREN                    List spawned children" CR
  ."   FAMILY                      Show lineage" CR
  ."   GENERATION .                This unit's generation" CR
  ."   PACKAGE-SIZE .              Replication package size" CR CR
  ."   Trust levels:" CR
  ."   TRUST-ALL                   Auto-accept (default)" CR
  ."   TRUST-MESH                  Accept known peers only" CR
  ."   TRUST-FAMILY                Accept parent/children" CR
  ."   TRUST-NONE                  Manual approval required" CR CR
  ."   QUARANTINE                  Emergency: block all replication" CR
  ."   10 MAX-CHILDREN             Limit children (default 10)" CR
;

: HELP-IO
  CR ." === Host I/O ===" CR CR
  ."   FILE-READ" ." path" ." ( -- addr n )   Read file" CR
  ."   FILE-WRITE" ." path" ." ( addr n -- )  Write file" CR
  ."   FILE-EXISTS" ." path" ." ( -- flag )   Check exists" CR
  ."   FILE-LIST" ." path" ."                 List directory" CR CR
  ."   HTTP-GET" ." url" ." ( -- addr n status )" CR
  ."   HTTP-POST" ." url" ." ( addr n -- addr n status )" CR CR
  ."   SHELL" ." cmd" ." ( -- addr n exit )   Run shell command" CR
  ."   ENV" ." name" ." ( -- addr n )         Read env var" CR
  ."   TIMESTAMP ( -- n )          Unix timestamp" CR
  ."   SLEEP ( ms -- )             Sleep milliseconds" CR CR
  ."   Security: sandbox ON by default for remote code" CR
  ."   SANDBOX-ON / SANDBOX-OFF    Toggle sandbox" CR
  ."   SHELL-ENABLE                Allow shell (REPL only)" CR
  ."   IO-LOG                      Show I/O audit trail" CR
;

\ --- Boot ---
." unit v0.12.2 -- seed online" CR
MESH-HELLO
AUTO-CLAIM
