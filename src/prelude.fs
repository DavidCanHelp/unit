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
: MESH-HELLO ." Mesh node " ID TYPE ."  online with " PEERS . ." peers" CR ;

\ --- Boot banner ---
." unit v0.1.0 — seed online" CR
MESH-HELLO
