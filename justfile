# Default: show available recipes
default:
    @just --list

# Build native release
build:
    cargo build --release

# Build WASM target
wasm:
    cargo build --target wasm32-unknown-unknown --release

# Run all tests
test:
    cargo test

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
