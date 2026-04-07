# unit — S-Expression Wire Protocol

Forth is the execution model. S-expressions are the wire format. Any
future nanobot implementation in any language can parse the mesh messages.

## Basics

```
> SEXP" (+ 10 32)" .
42  ok
> SEXP" (* 6 7)" .
42  ok
> SEXP-SEND" (event :type ping :data hello)"
sexp sent
```

## Message Format

Mesh messages are self-describing keyword-argument lists:

```
(peer-status :id "aaa" :peers 2 :fitness 10 :load 190 :capacity 100)
(sub-goal :id 1 :seq 0 :from "aaa" :expr "99 99 *")
(sub-result :id 1 :seq 0 :from "bbb" :result "9801")
(evolve-share :gen 100 :fitness 890 :program "0 1 10 0 DO OVER + SWAP LOOP DROP .")
(challenge-broadcast :id 11273 :name "fib15" :target "610 " :reward 150)
(solution-broadcast :id 11271 :solution "0 1 10 0 DO OVER + SWAP LOOP DROP ." :solver "aaa")
```

## Mating Protocol

```
(mating-request :from "aaa" :fitness 42 :words (("SQUARE" ": SQUARE DUP * ;") ("CUBE" ": CUBE DUP DUP * * ;")))
(mating-response :accepted true :from "bbb" :fitness 88 :words (("SOL-FIB10" ": SOL-FIB10 55 . ;")))
```

## Niche Broadcast

```
(niche-profile :from "aaa" :specializations (("fibonacci" 0.90) ("polynomial" 0.30)))
```

## Cross-Machine Mesh

Two machines, same mesh:

```sh
# Machine A
UNIT_PORT=4201 unit

# Machine B (discovers A, gossip finds the rest)
UNIT_PORT=4201 UNIT_PEERS=<A-ip>:4201 unit
```

DNS hostnames work: `UNIT_PEERS=myhost.example.com:4201`

NAT traversal: `UNIT_EXTERNAL_ADDR=203.0.113.5:4201`

Authentication: `UNIT_MESH_KEY=mysecret` on all machines.

Manual connect from the REPL:

```
> CONNECT" 192.168.1.10:4201"
connected to 192.168.1.10:4201
> PEER-TABLE
--- peer table ---
  cafe0123deadbeef @ 192.168.1.10:4201 fitness=45 seen=1s ago
```

Gossip self-assembles: A tells B about C, the mesh grows.

## Polyglot Organisms

The S-expression protocol is language-independent. Three species
coexist on one mesh, each with a different cognitive substrate.

```sh
# Terminal 1: Rust unit (Forth token sequences)
UNIT_PORT=4200 unit

# Terminal 2: Go organism (expression trees)
cd polyglot/go && go run . -peer 127.0.0.1:4200

# Terminal 3: Python organism (AST symbolic regression)
cd polyglot/python && python main.py --peer 127.0.0.1:4200
```

Each organism appears in the Rust unit's `PEERS` list, receives
challenges, evolves solutions using its own GP strategy, and
broadcasts results. Different languages, different mutation
strategies, same S-expression protocol.
