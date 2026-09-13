# Pipeline placement and distinct-host follow-up

2026-09-13. A two-request native pipeline improves coarse Docker throughput,
but the distinct-host test does not justify making it the default. The normal
native and ordinary scatter windows remain one. Forth, dictionary/genome
behavior, energy charges, and receiver admission remain unchanged.

## Controlled Docker comparison

The same kernel and benchmark compare windows of one and two pending requests
per peer. The experimental scheduler makes round-robin initial passes, retains
local work, refills between local evaluations, and counts all jobs owned by its
recruiter against the window. Advertised free capacity can reduce the window;
the worker still authoritatively limits all recruiters to three outstanding
requests. A regression covers other jobs, reduced capacity, stale capabilities,
and unchanged ordinary-work eligibility.

Twelve tasks per sweep, three repeats, medians in milliseconds. Each window has
its own serial/thread controls in the raw CSV. Runs were sequential to avoid
CPU contention between experiments; they were not randomized or a tail-latency
study. All four legs checked all 36 observations against serial results.

| Customers per task | Window 1 | Window 2 | Window 1 + netem | Window 2 + netem |
| ---: | ---: | ---: | ---: | ---: |
| 10,000 | 13.444 | 13.484 | 86.997 | 106.046 |
| 1,000,000 | 31.620 | 26.800 | 96.332 | 115.796 |
| 10,000,000 | 204.259 | 148.361 | 345.217 | 293.270 |
| 100,000,000 | 2049.060 | 1478.314 | 2034.380 | 1479.019 |

The largest ordinary job improves from 2049.060 to 1478.314 ms (28% less time).
Its serial control is 3993.416 ms and three-thread control 1403.923 ms: 2.70×
serial speedup, still slower than local threads. Remote assignments rise from
about six to eight of twelve. With verified netem (40 ms ±10 ms, 1% loss, 5%
reorder), the largest job improves from 2034.380 to 1479.019 ms. Tiny jobs
remain slower than local execution and receive no benefit from the wider window.

All four contention/recovery drills passed: two recruiters and 18 requests,
worker high-water at most three and eventual zero pending, explicit busy,
version rejection, pause/reassignment, late-result suppression, worker kill,
and a subsequent ordinary Forth definition. The drill timeout is one second;
normal supervision defaults to 60 seconds. These are containers on one host.

Raw evidence: [window 1](docker-window1.csv), [window 2](docker-window2.csv),
[window 1 netem](docker-window1-netem.csv), [window 2 netem](docker-window2-netem.csv).
Drills: [control](drill-window1.json), [pipeline](drill-window2.json),
[control netem](drill-window1-netem.json), [pipeline netem](drill-window2-netem.json).

## Two distinct hosts exposed different limits

The issuer was this eight-CPU Apple Silicon Mac. The peer was `dev-server`, a
separate two-vCPU x86_64 cloud VM. A release build used the same native kernel
on each architecture, with direct UDP and fresh temporary organism state.
No tunnel, netem, new persistent service, or third host was used. Hardware,
compiler versions, and cloud CPU availability were not controlled; these are
feasibility observations, not a homogeneous scaling comparison.

All 36 observations in the two-request run were correct:

| Customers per task | Local serial | Two local threads | Two-host mesh |
| ---: | ---: | ---: | ---: |
| 10,000 | 1.860 | 0.693 | 52.437 |
| 1,000,000 | 41.998 | 19.034 | 89.799 |
| 10,000,000 | 397.971 | 199.618 | 841.744 |
| 100,000,000 | 3908.991 | 2028.622 | 6593.722 |

The largest mesh run takes 1.69× as long as local serial. Capacity slots alone
do not establish comparable compute speed. This run does not isolate hardware
speed from network effects or prove that the wider window caused the slowdown.

The one-request control was attempted twice, once with the previously used
worker and once with a fresh worker. Both hit the benchmark's 30-second
deadline, so neither is a complete performance sample. In the fresh attempt,
six earlier remote tasks completed; task seven remained pending at the issuer.
The worker reported accepted=6, pending=0, high-water=1, busy=0. That suggests
a request-delivery or routing failure rather than an overloaded execution queue,
but no packet capture established the cause. The benchmark exits before the
normal 60-second supervision interval can recover a silent loss. The Docker
drill's injected one-second timeout conceals that practical latency gap.

Evidence: [successful samples](two-host-window2.csv), [host metadata](two-host.json),
[first incomplete control](two-host-window1-incomplete.log),
[fresh incomplete control with issuer diagnostics](two-host-window1-fresh-incomplete.log),
and [worker status](two-host-worker-control.log). Both workers were stopped and
the remote temporary build/state directory removed; experiment sockets were gone.

## Decision and next experiment

Keep the wider pipeline behind the benchmark's explicit experiment parameter.
Do not infer that more queued remote work helps arbitrary peers. The next
implementation should distinguish receipt/admission from completion: acknowledge
accepted native task IDs, retry unacknowledged delivery within a bounded job
deadline, and retain replay/first-result-wins semantics without duplicate wages.
First reproduce the observed missing-admission case with controlled request
loss under the normal supervision setting; also test lost acknowledgements
and replies, queue-full responses, and bounded retries.

Then measure per-peer execution/turnaround for these versioned kernels and let
each recruiting unit choose a bounded share based on observed service rate.
Test a deliberately slow peer against local serial, rather than assuming equal
capability slots mean equal speed. This belongs in local unit policy; no central
master or replacement genome model is needed. Root restart/durable job retention
remain separate unsolved requirements.

The three-host test remains incomplete: only the Mac and `dev-server` were
available, and a third SSH host was requested. Do not count containers as that
missing physical deployment.

Validation after the scheduling experiment: 582 unit tests and two HTTP
integration tests passed (one performance test intentionally ignored), standard
Clippy passed with warnings denied, and the WebAssembly release target built.
The benchmark now includes pending job/ledger diagnostics when its deadline
expires. Reproduction commands are in [the capability README](README.md).
