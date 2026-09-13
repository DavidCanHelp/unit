# Results: 2026-09-13

Host: Mac15,12, 8 logical CPUs, 16 GiB RAM, macOS 26.6.2 arm64. Rust 1.95.0. Release profile (`opt-level=z`), LTO disabled. Three repeats per cell, median elapsed milliseconds below. Four VMs on independent threads use actual loopback UDP; this is not a four-machine test. Fixed headroom isolates scheduling. No performance thresholds are asserted.

| Work per task | Tasks | Serial parallel | Scatter once | Scatter refill | Speedup vs serial |
|---|---:|---:|---:|---:|---:|
| 100 loop iterations | 4 | 0.329 | 2.815 | 2.744 | 0.12x |
| 100 loop iterations | 16 | 1.186 | 3.602 | 3.494 | 0.34x |
| 10,000 loop iterations | 4 | 14.423 | 6.782 | 6.704 | 2.15x |
| 10,000 loop iterations | 16 | 44.226 | 37.552 | 22.179 | 1.99x |
| 200,000 loop iterations | 4 | 216.190 | 62.515 | 63.758 | 3.39x |
| 200,000 loop iterations | 16 | 868.285 | 708.570 | 339.875 | 2.55x |

All 54 synthetic timing runs matched the analytical oracle in every ordered slot. Tiny tasks lose: fixed messaging and polling overhead dominates. Larger tasks benefit, and refilling a bounded window prevents the root from retaining most of a 16-part job.

| Prime search [0, 20000) | Serial VM | Scatter refill | Speedup |
|---|---:|---:|---:|
| 4 partitions | 170.601 ms | 58.631 ms | 2.91x |
| 16 partitions | 171.316 ms | 64.420 ms | 2.66x |

All 12 prime-search runs matched an independently computed result for every partition and a total of **2,262 primes**. Native Rust oracle calculation took **0.253 ms** (four partitions) and **0.262 ms** (16 partitions), one measurement each. The native algorithm uses trial division by all integers; the Forth kernel skips even divisors. These are not identical instruction sequences or a tuned native benchmark. Even so, the roughly 230-fold gap to the fastest distributed VM run is too large to treat scheduling as the principal remaining compute bottleneck.

Fault experiment: deliberately discard one recruit request while the peer remains live. With an injected 100 ms supervision clock, the job completed correctly in **104.165 ms**. A late conflicting duplicate did not overwrite the settled result. This tests application-level loss and retry, not a partition, process crash, or production timeout: the normal timeout remains 60 seconds.

Correctness regression reproduced before the fix: `(MISSING-KERNEL 123)` returned `(result :ok 1 :value (123) :output "")`. It now returns a runtime failure. Additional tests cover direct stack primitive underflow, division by zero, invalid addresses, wrapping signed division, no-peer scatter, saturation/abandonment, nested recruit routing, reopening a settled window, and preserving unrelated inbox messages.

Validation: `cargo test --features http` passed **574 unit tests and 2 HTTP integration tests**, with the performance test skipped by default. The ignored release experiment was run explicitly and passed. `cargo clippy --features http -- -D warnings` and `cargo check --target wasm32-unknown-unknown` passed. The optional broader `--all-targets` lint encountered an existing `clippy::never_loop` in `src/multi_unit.rs:2271` (the heredity test); that unrelated code is unchanged.

Raw samples: [final.csv](final.csv). [initial.csv](initial.csv) and [refill.csv](refill.csv) retain the exploratory iterations. Some initial trials overlapped compilation/testing, and their work sizes differ; use within-run comparisons and the final table, not cross-run elapsed-time comparisons.
