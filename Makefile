.PHONY: build-native build-wasm run test clean

build-native:
	cargo build --release

build-wasm:
	cargo build --release --target wasm32-unknown-unknown
	@mkdir -p web
	@cp target/wasm32-unknown-unknown/release/unit.wasm web/unit.wasm 2>/dev/null || true
	@echo "WASM binary: web/unit.wasm"

run:
	cargo run

test:
	@echo "=== native build ==="
	cargo build --release
	@echo "=== basic test ==="
	echo '2 3 + . BYE' | ./target/release/unit
	@echo "=== goal test ==="
	echo '5 GOAL{ 6 7 * } DROP GOALS BYE' | UNIT_PORT=0 ./target/release/unit
	@echo "=== persistence test ==="
	echo 'SAVE BYE' | UNIT_PORT=0 ./target/release/unit
	@echo "=== all tests passed ==="

clean:
	cargo clean
	rm -f web/unit.wasm
