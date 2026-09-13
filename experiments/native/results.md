# Native mesh experiment results

Follow-up: [pipeline comparison and distinct-host results](pipeline-results.md).

2026-09-13. The native queue simulation supports useful coarse computation through
ordinary units. It does not establish an advantage over a local thread pool or
scaling across physical machines. Forth remains programmable; the native kernel
is a versioned primitive, with opt-in admission and existing recruitment,
supervision, and energy accounting.

## Measurements

Twelve deterministic queue simulations per job, three repeats per size. Times
below are medians in milliseconds. All results were checked against the same
serial native kernel; a separate event-list oracle and golden fixture test the
kernel itself. Startup/discovery is excluded; scheduling and collection are
included. Serial, three threads, and three computing units use the same kernel.

Three Docker containers share one host (Apple Silicon, eight Docker CPUs).
They provide process isolation, not additional physical CPU or independent
failure domains. Raw observations: [ordinary](docker.csv),
[netem](docker-netem.csv).

| Customers per task | Serial | Threads | Mesh | Serial / mesh |
| ---: | ---: | ---: | ---: | ---: |
| 10,000 | 2.477 | 1.381 | 11.127 | 0.22× |
| 1,000,000 | 37.914 | 14.099 | 33.115 | 1.14× |
| 10,000,000 | 390.753 | 137.521 | 204.321 | 1.91× |
| 100,000,000 | 3970.300 | 1437.535 | 2032.285 | 1.95× |

Netem was verified with `tc qdisc show` on every container: 40 ms ±10 ms delay,
1% loss, and 5% reorder on egress.

| Customers per task | Serial | Threads | Mesh | Serial / mesh |
| ---: | ---: | ---: | ---: | ---: |
| 10,000 | 2.394 | 1.659 | 92.626 | 0.03× |
| 1,000,000 | 39.972 | 13.852 | 93.013 | 0.43× |
| 10,000,000 | 375.187 | 137.538 | 344.689 | 1.09× |
| 100,000,000 | 3952.763 | 1536.752 | 2080.160 | 1.90× |

Three repeats are exploratory evidence, not a tail-latency estimate. One medium
netem job took 1295.523 ms, consistent with the drill's one-second supervision
retry interval. Coarse mesh runs took 2027–2084 ms; local threads were faster.
The largest runs sent six of twelve tasks remotely. The issuer's remaining local
work is consistent with the roughly twofold speedup ceiling; this is a placement
hypothesis to test, not proof of a hardware limit.

## Recovery and bounded admission

Both [ordinary](drill.json) and [netem](drill-netem.json) drills passed:

- All 36 benchmark observations per leg were numerically correct.
- Two recruiters submitted 18 requests to one unit; all completed. Outstanding
  work peaked at three, explicit busy responses occurred, and pending returned
  to zero. This demonstrates the shared admission bound, not fairness under
  sustained overload.
- Unsupported kernel versions failed explicitly.
- Pausing a holder triggered existing supervision and reassignment; recovery
  took 2.109 seconds ordinarily and 1.975 seconds with netem. The test timeout
  is one second; production's default is 60 seconds.
- Resumed late replies did not change settled results. Killing a holder also
  recovered the same numerical result.
- A new Forth definition still evaluated correctly after worker failure.

Regression validation: 581 unit tests and two HTTP integration tests passed,
with one performance test intentionally ignored; all 114 existing shell
integration checks and both idle-tick regressions passed in disposable containers.
The local ordinary-process benchmark also completed all 36 observations correctly
(used as a smoke check, not a timing sample). Standard HTTP-feature
Clippy passed with warnings denied, and the WebAssembly target compiled.

## Decision

Keep this capability opt-in and use coarse, independent simulation sweeps with
small inputs and results. Fine-grained native work should remain local. The
Docker result clears an overhead/recovery feasibility check, but misses a strict
2× three-host gate and does not substitute for physical-host measurements.

Next, test the same contract on three physical hosts with 1/2/3 participants,
then compare the present placement against one that reserves an equal coarse
share for each capable unit. Retain the local thread baseline and failure drills.
Only keep a placement change if throughput improves without weakening bounded
admission or ordinary unit behavior. Before calling this a dependable external
service, add root restart/resubmission and bounded completed-job retention;
the current stable task IDs and 64-entry worker replay cache are not durable
job storage. No claim is made about untrusted execution or the multi-unit host
bridge.
