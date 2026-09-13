# Useful computation experiments

Run the real UDP loopback experiment (four independently executing VMs, one OS
process, no ALife loop):

```sh
CARGO_PROFILE_RELEASE_LTO=false cargo test --release compute_mesh_experiment -- --ignored --nocapture
```

LTO is disabled to shorten experiment rebuilds; all compared modes use the same
binary. Discovery and VM initialization are outside the timer. Resource readings
are deliberately fixed below the ceiling and workers advertise 90% headroom:
this isolates CPU scheduling from host load-average lag. Every output is checked
against an independent sum-of-squares formula, including per-slot ordering.
This kernel measures interpreter compute throughput, not application usefulness:
the closed form is preferable for actually computing this particular answer.

These are real UDP messages and concurrent VMs, but not separate hosts or
process failure domains. There is no WAN, GP background work, or global admission
control in this experiment. The tests fail on missing results, rather than
counting dispatched work as completed throughput. Normal `cargo test` skips the
performance run; correctness regressions run normally.

The experiment compares original pressure-driven `parallel`, initial
`scatter-once`, and scatter with window refill. It then runs partitioned prime
counting and deliberately drops one request to exercise supervision. The final
[results and raw samples](results.md) record all three stages of the investigation.

For a manual homogeneous mesh, load `prime-count.fs` on each worker (for example,
`unit --file experiments/compute/prime-count.fs --port 4201`, using distinct ports
and `--peers` for the other processes). At the issuer's REPL, once the peers know
each other's identities:

```forth
PARALLEL" (scatter (COUNT-PRIMES 0 5000) (COUNT-PRIMES 5000 10000) (COUNT-PRIMES 10000 15000) (COUNT-PRIMES 15000 20000))"
RECRUITS
```

Add the four returned counts for 2,262. `:ok 0` in the initial aggregate can mean
pending work; the asynchronous completion reports the final ordered envelopes.
`COUNT-PRIMES` returns zero for empty or reversed intervals and is intended for
the small nonnegative integer ranges in this fixture. All workers must have the
same kernel source; the new unknown-word fault prevents silent input echo when
a worker is missing it, but does not verify that an installed implementation is
the correct version.
