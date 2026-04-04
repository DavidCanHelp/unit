# unit-python

A Python organism that joins the Rust unit mesh. AST-based symbolic regression — a third cognitive substrate.

## Run

```sh
# Start a Rust unit first
UNIT_PORT=4200 cargo run --release

# Start the Python organism
cd polyglot/python
python main.py --peer 127.0.0.1:4200
```

## Test

```sh
cd polyglot/python
python -m unittest discover -v
```

## How it differs

| | Rust | Go | Python |
|---|------|-----|--------|
| Programs | Forth tokens | Expression trees | AST nodes |
| Mutation | Token swap/insert/delete | Subtree replacement | AST node mutation |
| Evaluation | Stack-based VM | Tree eval | `ast` module eval |
| Concurrency | Single-threaded | Goroutines | Threads |
| Wire format | S-expressions | S-expressions | S-expressions |

Three species, three cognitive substrates, one mesh.

## Flags

```
--port N     UDP port (default 4202)
--peer ADDR  seed peer address (e.g. 127.0.0.1:4200)
--id HEX     8-char hex node ID (random if omitted)
```
