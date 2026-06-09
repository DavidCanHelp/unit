# Default: show available recipes
default:
    @just --list

# Build native release
build:
    cargo build --release

# Build WASM target and stage it for the web demo (web/unit.wasm is
# untracked; CI builds its own copy for the Pages deploy)
wasm:
    cargo build --target wasm32-unknown-unknown --release
    @mkdir -p web
    @cp target/wasm32-unknown-unknown/release/unit.wasm web/unit.wasm
    @echo "WASM binary: web/unit.wasm"

# Run all tests
test:
    cargo test

# Quick end-to-end smoke: pipe a few words through the real release binary
# (arithmetic, goals, persistence) — cheaper than tests/integration.sh
smoke: build
    @echo "=== basic test ==="
    echo '2 3 + . BYE' | ./target/release/unit
    @echo "=== goal test ==="
    echo '5 GOAL{ 6 7 * } DROP GOALS BYE' | UNIT_PORT=0 ./target/release/unit
    @echo "=== persistence test ==="
    echo 'SAVE BYE' | UNIT_PORT=0 ./target/release/unit
    @echo "=== smoke passed ==="

# Run clippy (must be warning-free)
lint:
    cargo clippy -- -D warnings

# Format code
fmt:
    cargo fmt

# Check formatting without changing files
fmt-check:
    cargo fmt -- --check

# Run the REPL
run *ARGS:
    cargo run -- {{ARGS}}

# Run a swarm of N units (default 3)
swarm n="3":
    #!/usr/bin/env bash
    for i in $(seq 1 {{n}}); do
      port=$((4200 + i))
      if [ "$i" -eq 1 ]; then
        UNIT_PORT=$port cargo run --release &
      else
        UNIT_PORT=$port UNIT_PEERS=127.0.0.1:4201 cargo run --release &
      fi
    done
    wait

# Full CI check (what GitHub Actions runs)
ci: fmt-check lint test

# Publish to crates.io (dry run)
publish-dry:
    cargo publish --dry-run

# Clean build artifacts
clean:
    cargo clean
