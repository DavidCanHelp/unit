# Native computation as a unit capability

Latest evidence: [pipeline comparison and distinct-host results](pipeline-results.md).

This extends the ordinary unit. Its Forth dictionary still chooses what to do,
its mesh still discovers peers, its recruiter still supervises work, and its
metabolism still pays for execution. There is no new worker daemon, master role,
central queue, executable upload, or dependency. `--bench-native` is an experiment
driver that uses the same ordinary units and recruit protocol.

The first kernel is a deterministic discrete-event single-server queue simulation.
A parameter sweep explores the latency consequences of service time approaching
or exceeding interarrival time. Both distributions are uniform integer durations
from 1 through twice their respective parameter. This is not an M/M/1 model.

## Forth remains the programming interface

```forth
: SCENARIO 100 80 1000000 42 QUEUE-SIM ;
SCENARIO . . .
NATIVE-ON
NATIVE-STATUS
```

`QUEUE-SIM` consumes `(arrival service customers seed)` and pushes
`(total-wait max-wait final-departure)`, so `. . .` prints departure first.
The word can be named, composed, shared, and inherited like other primitive-based
programs. The kernel itself is installed with the binary, like arithmetic
primitives; the genome chooses kernels, parameters, and recruitment behavior.
A named native kernel/version is immutable even if a local dictionary overrides
the Forth word's name. That distinction gives recruited computation a reproducible
contract while preserving local dictionary programmability.

`NATIVE-ON` opens this unit's native admission. It lazily starts one execution
thread when the first task arrives. At most three requests can be outstanding
(one computing and two waiting, including any uncollected completion).
`NATIVE-OFF` refuses new work and lets accepted work finish. A unit that has never
enabled native admission starts no native thread and emits no capability chatter.
The Forth sandbox cannot enable or disable this admission policy.

At the REPL, after enabling native admission on the peers and allowing discovery:

```forth
PARALLEL" (scatter (native :kernel queue-sim :version 1 :arrival 100 :service 80 :customers 10000000 :seed 42 :task queue-sim/v1/100/80/10000000/42) (native :kernel queue-sim :version 1 :arrival 100 :service 100 :customers 10000000 :seed 42 :task queue-sim/v1/100/100/10000000/42) (native :kernel queue-sim :version 1 :arrival 100 :service 110 :customers 10000000 :seed 42 :task queue-sim/v1/100/110/10000000/42))"
RECRUITS-SEXP
```

The individual instruction also works with `SEXP-EVAL"` or `RECRUIT"`.
For structured envelopes, the stack is top-first, as with existing S-expression
results: `:value (final-departure max-wait total-wait)`.

## Contract and admission

- The canonical task ID encodes every input and the kernel version, with no hash
  collisions. Workers reject mismatched IDs, duplicate/unknown fields, unsupported
  versions, and invalid ranges. Successful replies must match the retained task ID.
- Arrival/service parameters are 1..10,000, customers 1..100,000,000, and seed
  1..i64::MAX. Admission proves the worst-case total wait fits i64; extreme
  combinations are rejected before execution. The xorshift64 stream and integer
  event recurrence are specified in the kernel, with no host clock or floating
  point dependence. Forth step budgets also bound native simulation steps.
- Admission is local to each unit and shared across all its recruiters. A full
  unit returns `recruit-busy` with a retry hint. The recruiter retries without
  resetting the original supervision deadline or buying another bounty. Existing
  timeout/reassignment/terminal-failure rules remain authoritative.
- The native pipeline experiment permits two pending requests per peer, using
  round-robin initial placement and available advertised slots. This counts
  pending work from every job owned by the recruiter. Other recruiters can
  race with those advertisements; the receiver still enforces its shared
  three-request admission limit and returns busy. Ordinary Forth scatter
  retains its one-request window. The normal native window also remains one;
  heterogeneous hosts need placement informed by observed execution speed.
- Native capability advertisements report free reserved slots and expire after
  five seconds. They do not pretend to be memory-headroom observations. The
  existing resource-pressure placement and replication ceilings are unchanged.
- Execution costs `1 + customers / 1,000,000` energy. This is a logical price, not
  a measured electricity charge. Bounties use the existing successful-recruit wage
  path. Duplicate replay does not repeat the execution charge or wage.
- A worker retains at most 64 completed replay records, keyed by recruiter and
  job slot and checked against the canonical task ID. After eviction or restart,
  a pure task may execute again. Result application stays first-write-wins.
- The native thread never reads or edits the VM dictionary, stacks, or goals.
  The VM drains completions and handles routing/accounting. Fast native polling
  preserves the original 250 ms ecological tick cadence. Long Forth evaluations
  can still delay polling; this is not a real-time VM or general preemption.

Old binary snapshots receive missing native primitives by appending them. Existing
instruction indices and user-defined overrides remain intact. Ephemeral queues
and OS threads are not heritable state; a genome can choose to enable the
capability again. This prototype targets the canonical one-unit-per-process path;
the multi-unit host bridge is not claimed as a native execution deployment.

## Reproduce the measurements and drills

```sh
cargo build --release
unit --bench-native
```

The driver launches two ordinary unit processes, enables their native capability
through stdin/Forth, and participates locally as the third computing unit. It uses
fresh `UNIT_STATE_DIR` directories and cleans up only its children and temporary
state. It does not load or overwrite your normal organism's saved identity/genome.

Every workload uses the same compiled kernel in three modes: serial, three local
threads, and the three-unit mesh. The 12 tasks sweep six service parameters with
two seeds. Three repeats at each of four sizes check every result against the
serial result. A separate event-list implementation and a golden answer test
validate the kernel. Each benchmark job supplies an explicit 1,000-energy issuer
budget so serial/thread/mesh timings do not turn into a fuel-depletion experiment.
Discovery and process startup are outside the timer; evaluation, scheduling,
messaging, and result collection are inside it. Remote-dispatch counts must be
nonzero. The native threads baseline uses the same number of computing participants.

For existing units, enable `NATIVE-ON` and use
`unit --bench-native --peers host-b:4200,host-c:4200`. All peers must run the same
binary/kernel version. This is the entry point for a later physical-machine test.

The Docker test reuses the project's existing image, FIFO-injected REPL entrypoint,
`docker pause` wedge, and optional netem layer:

```sh
just native-drill
just native-drill 1
```

Or build `docker/Dockerfile` as `unit-native-drill:latest`, then run
`python3 docker/native-drill.py` with `DRILL_NETEM=0` or `1`. The isolated
`unit-native` Compose project is cleaned up in `finally`. Other containers and
existing drill projects are untouched. Startup requires fresh identities in fresh
logs and ordered seed DNS readiness.

Tests include two simultaneous recruiters filling one worker, busy/retry behavior,
version rejection, queue high-water, deterministic results, pause/re-recruit,
late duplicate suppression, actual worker kill, and subsequent Forth definition
and execution. The injected supervision timeout is one second, not production's
60 seconds. Netem adds 40 ms ±10 ms delay, 1% loss and 5% reorder on each container's
egress. CI reuses the existing drill image for both native test legs.

These are three containers sharing one Docker host. They establish process
isolation and recovery, not independent machines or extra physical CPU capacity.
See [measured results and the next decision](results.md), with the adjacent CSV
and drill JSON files as evidence. Native admission
is intended for a trusted mesh: the protocol has not gained a new authentication
boundary. Stable task IDs are not a durable job store; root restart/resubmission,
large-data transfer, and general-purpose native plugins remain future work.

## Compare native pipeline windows

The same binary retains a one-request control for the benchmark driver:

```sh
DRILL_LABEL=window1 UNIT_BENCH_NATIVE_WINDOW=1 python3 docker/native-drill.py
DRILL_LABEL=window2 UNIT_BENCH_NATIVE_WINDOW=2 python3 docker/native-drill.py
DRILL_NETEM=1 DRILL_LABEL=window1 UNIT_BENCH_NATIVE_WINDOW=1 python3 docker/native-drill.py
DRILL_NETEM=1 DRILL_LABEL=window2 UNIT_BENCH_NATIVE_WINDOW=2 python3 docker/native-drill.py
```

Build the current `unit-native-drill:latest` image first. Labels preserve earlier
CSV/JSON observations. `UNIT_BENCH_NATIVE_WINDOW` accepts only 1 or 2 and affects
the benchmark issuer; it does not reconfigure normal units. Native scatter's
normal window remains one; the two-request policy is retained as an experiment.
This pipeline hides latency within a fixed bound. It does not establish that
equal shares are optimal for machines of different speeds.
