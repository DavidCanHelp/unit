# From ALife mesh to useful computation

Investigation: 2026-09-13. Experimental changes, not a production readiness claim.

## What the architecture already provides

The native VM is an interpreter plus a mesh identity. Its recruiter ledger retains
instructions until completion, ordered `ParallelJob` slots collect results, and
supervision retries dead or silent holders with bounded attempts. The same
recruit protocol can nest. These are useful foundations for independent,
idempotent tasks: parameter sweeps, search partitions, simulation replicates,
and small map/reduce workloads with compact inputs and results.

The design record's distinction between a uniform node and a central scheduler
can survive this transition. A submitting node must still own its job's state:
"no central coordinator" does not remove aggregation, retry, or durability
obligations. Current state is in memory; losing the root loses the job unless
its caller retains and resubmits it.

## Blockers found in code

1. **Memory pressure does not schedule CPU work.** `run_parallel` executes each
   part synchronously while below the resource ceiling. With a spare CPU budget,
   it can run the entire job serially. A million resident VMs are not a million
   simultaneous executors; `MultiUnitHost` also multiplexes its VMs serially.
2. **Dictionary drift can masquerade as success.** Unknown words previously
   printed an error (or nothing when silent) without recording a VM fault.
   `(MISSING-KERNEL 123)` demonstrably returned success with value 123. This
   investigation adds `Fault::UnknownWord` to interpreted and compiled paths.
   Direct stack primitives, division by zero, and invalid memory addresses also
   now record faults; signed MIN / -1 uses the same wrapping arithmetic policy
   as addition and multiplication instead of panicking the worker.
3. **Inputs are not reproducible programs yet.** Recruiting a named word assumes
   the recipient has the same implementation. Word gossip and mutation do not
   establish that. Versioned, immutable program bundles and capability matching
   are more valuable for external computation than evolving arbitrary words.
4. **Headroom is not a reservation.** Every recruiter can see the same free peer.
   The new scatter window only bounds assignments from one recruiter, not all
   recruiters. The worker needs explicit admission and bounded queues before
   multi-client throughput claims are meaningful.
5. **UDP is the data plane as well as discovery.** Sends have no application
   acknowledgement and requests/results fit one datagram, with a 16-bit payload
   length. Large data and code need a reliable transfer path and explicit limits.
   A seed peer entry can exist before the return identity is learned; benchmark
   discovery must wait for actual routes, not just nonzero peer counts.
6. **The sandbox is not a pure function boundary.** It restores stacks and some
   execution state, not the entire dictionary and memory. At-least-once execution
   and first-result-wins are suitable only when duplicate execution is acceptable.
   Untrusted multi-tenant execution is outside this experiment's scope.
7. **Useful work lacks a complete client lifecycle.** HTTP `/sexp` currently
   translates and evaluates directly in the live VM; it is not the recruiter
   seam or an asynchronous job API. A caller needs submit/status/result/release,
   durable identities, explicit failure, and bounded retention. Completed root
   jobs and ledger entries otherwise accumulate.

## Implemented scheduling experiment

`PARALLEL" (scatter (KERNEL input1) (KERNEL input2) ...)"` explicitly requests
independent parts. The existing `(parallel ...)` pressure behavior is unchanged.
Scatter sends at most one part to each sufficiently available peer, excluding
peers with pending assignments in this recruiter's ledger, before doing local
work. It retains at least one local part. Between local parts it collects only
result messages, refills idle peer slots, and otherwise continues local work.
Unrelated incoming work remains queued, preventing recursive work dispatch from
monopolizing this scheduler. Spare parts use the existing local
admission/overflow path. Thus the new bound applies to proactive scatter sends;
pressure-driven overflow and supervision retain their existing behavior.

Result order and envelopes are unchanged. Remote scatter instructions use the
same handler and deferred completion mechanism as parallel instructions. Use
homogeneous versions: older peers do not understand this new form. Start with
pure, independent, preinstalled kernels and small datagrams. There is no claim
that scheduling alone gives reproducibility, durability, or global admission.

See [experiment instructions](../../experiments/compute/README.md) for exact
reproduction commands and measurement limits.

## Results and the next decision

The [measurements](../../experiments/compute/results.md) support keeping scatter
opt-in. Four coarse synthetic tasks achieved 3.39x speedup; a 16-task refill
run achieved 2.55x. Tiny tasks were slower remotely. Prime search achieved
2.91x over the serial VM, yet the native oracle was about 230x faster than the
best distributed VM run. The next investment should therefore be a native
execution lane, not further multiplication of VMs or fine tuning of gossip.

A concrete next experiment sequence:

1. **Preinstalled native kernel plus typed inputs.** Keep Forth as orchestration
   and S-expressions as the contract. Advertise kernel/version capabilities and
   require the exact version in each instruction. Install binaries through the
   existing human-controlled host boundary; do not ship binaries over gossip.
   Benchmark against the *same native kernel* run serially and in a local thread
   pool. Use tasks taking 100–1000 ms, and measure the crossover by varying
   input size. Reject the feature if mesh execution cannot amortize its overhead.
2. **Worker admission and fair progress.** Implement a bounded worker queue with
   explicit busy/retry responses, then test two simultaneous recruiters. The
   current recruiter-local window cannot reserve global capacity. Keep receiver
   progress separate from long evaluations, so long jobs do not delay all result
   collection and supervision. Measure queue depth, p95 completion, useful CPU
   time, duplicate work, and memory, not node counts or metabolic fitness.
3. **A complete reproducible job lifecycle.** Add submit/status/result/release to
   the actual recruit seam; use stable job identities, kernel IDs, input IDs,
   deadlines, explicit errors, and a bounded/durable result store. Retain work
   at the submitting edge until completion. Test restart and resubmission before
   claiming durable computation. The current in-memory root is a failure point.
4. **Multi-host falsification.** Repeat on three separately deployed hosts with
   a serial-native and local-pool baseline, fixed total work, and 1/2/3 hosts.
   Inject latency, loss, worker death, a slow worker, root restart, and two clients.
   Require oracle-correct results, bounded queues, explicit terminal failures,
   no duplicate result application, and stable retained memory after 10,000 jobs.
   A sensible provisional performance gate is at least 2x throughput on three
   hosts for a coarse native workload, including transfer and retry costs.

The plausible first product is a trusted pool for coarse independent search,
parameter sweeps, or simulation replicates with small inputs/results. Large
shared datasets, tightly coupled numerical solvers, and untrusted marketplace
compute require substantially different data, scheduling, and trust machinery.
The prime-count fixture demonstrates the execution pattern, not a competitive
prime-counting service.

ALife mechanisms can still help as experimental policy search: evolve placement
or chunking policies offline, score them on validated useful throughput and
recovery, and only install policies that pass fixed workload regressions. An
energy bounty or an evolved word is not itself evidence of correct external work.

## Native follow-up completed

The [native capability experiment](../../experiments/native/README.md) now implements
a versioned queue-simulation primitive, shared bounded admission, capability
advertisements, and busy/retry handling through ordinary units. The existing
Docker REPL/netem process passed contention, pause, kill, and late-result checks.
[Measurements](../../experiments/native/results.md) show 1.95× coarse speedup over
serial (1.90× with netem), while local threads remain faster. These are containers
on one host. Physical-host scaling, durable root recovery, and bounded completed
job retention remain unproven. Forth dictionaries and genomes continue to control
behavior; execution does not introduce a permanent master or separate worker role.

## Placement and distinct-host follow-up

The [next experiment](../../experiments/native/pipeline-results.md) tested a
bounded two-request native pipeline. It improved coarse Docker work from about
2.05 to 1.48 seconds and survived netem, but the Mac/cloud-VM run was slower
than local serial and two control attempts hit a missing-completion deadline.
The default window remains one. Prioritize bounded acknowledged delivery and
measured per-peer service rates before widening placement globally. The
three-host experiment still needs a third host; the existing unit model is
preserved.
