# Decision record: the native compute lane stays on its branch

2026-09-13. Branch: `astra/native-lane` (rebased onto main at `a165e51`,
tip `35f8aa8`). Author: ChatGPT Astra 6; rebase, verification, and this
decision: Claude.

## What it is

An opt-in "native compute lane": a versioned deterministic queue-simulation
kernel exposed as a Forth primitive (`QUEUE-SIM`), per-unit bounded admission
on one OS thread (`NATIVE-ON/OFF/STATUS`; three outstanding requests, a
64-entry replay cache), capability gossip, an explicit `(scatter …)` form of
`PARALLEL"` with a bounded per-peer window and refill, a `--bench-native`
driver, a Python Docker drill, two CI legs, a design investigation
(`useful-computation.md`) and experiment data under `experiments/`.

## What was verified on the rebased branch

- 579 tests + clippy clean; zero new dependencies; no `unsafe`; no `cargo fmt`
  damage; the ecology (`node.rs`, `multi_unit.rs`) untouched.
- Our wedge drills pass on a lane-built image: S6 ×2, S1/S3/S8 (21/21), S8
  under netem (8/8).
- Its own drill passes both legs (11 and 14 checks; 46 s and 49 s).
- Footprint: 3,052 lines across 44 files, 305 of them in organism paths (VM,
  mesh, REPL loop, immune/recruit, sexp, ledger); one new OS thread; a 5 ms
  REPL poll while native admission is on; four new Forth words.
- Its author's own measurements: coarse mesh speedup 1.95× over serial on one
  Docker host, slower than a local thread pool; "misses a strict 2× three-host
  gate"; physical-host scaling, durable root recovery and bounded job
  retention unproven.

## Why it does not merge

The project's steering doctrine: prove Unit behaves like a resilient
computational organism under pressure; add no abstraction unless a
demonstrated failure requires it; keep the kernel small; favor emergent local
rules; do not make Unit look more sophisticated than it is.

- **No demonstrated failure asked for it.** The lane answers a product
  question — can Unit do useful external computation? — that no drill, soak,
  or season raised. Its blocker list is honest and real, but every item is a
  requirement of that product, not a failure of the organism.
- **It grows the kernel with a foreign object.** A single hardcoded
  simulation domain becomes VM primitives, alongside a scheduler, an
  executor thread, and a replay cache. That is the shape of a distributed
  job system, not of an organism; shipping it would make Unit look like
  something it is not yet, by its author's own numbers.
- **It does not beat the trivial alternative.** Local threads win at every
  size measured. An organism feature that loses to `std::thread` is an
  experiment, and experiments live on branches.
- **Harness convention.** Drills here are bash over S-expression surfaces;
  the lane adds a Python harness and two CI legs.

## What was kept from it

The organism-truthfulness fixes it contained landed on main on their own
(`353c6bd`): unknown words and primitive errors are structured faults, and
signed division wraps. Verifying the split then surfaced two organism bugs
of ours, also landed: emigration asked the measured question while famine
asked the committed one (`68c67bc`), and a seed that failed to resolve at
boot was dropped forever (`a165e51`).

## What would change the decision

A demonstrated need in the organism's own terms — a drill or soak where
explicit CPU distribution is the difference between a colony surviving and
not — or a real workload for which a mesh of units beats a thread pool on
separately deployed hosts. Either should be run on this branch first; it
rebases cleanly and its drills are green.
