# unit-go

A Go organism that joins the Rust unit mesh. Different language, different mutation strategy, same protocol.

## Build

```
cd polyglot/go
go build -o unit-go .
```

## Run

```sh
# Start a Rust unit first
UNIT_PORT=4200 cargo run --release

# Start the Go organism, connecting to the Rust unit
./unit-go -peer 127.0.0.1:4200
```

## What it does

- Joins the Rust mesh via UDP S-expression protocol
- Appears in the Rust unit's `PEERS` list
- Receives challenges broadcast by Rust units
- Evolves solutions using arithmetic expression trees (not Forth)
- Broadcasts solutions that Rust units can verify
- Periodic gossip keeps the connection alive

## How it differs from Rust units

| | Rust unit | Go organism |
|---|-----------|------------|
| Language | Forth programs | Expression trees |
| Execution | Stack-based VM | Tree evaluation |
| Mutation | Token swap/insert/delete | Subtree replacement |
| Concurrency | Single-threaded | Goroutines |
| Wire format | S-expressions (shared) | S-expressions (shared) |

The S-expression mesh protocol enables interop between any language that can parse and emit `(message :key value)` over UDP.

## Flags

```
-port N     UDP port (default 4201)
-peer ADDR  seed peer address (e.g. 127.0.0.1:4200)
-id HEX     8-char hex node ID (random if omitted)
```
