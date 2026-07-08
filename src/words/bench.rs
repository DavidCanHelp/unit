//! Headless benchmark primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Headless benchmark
    // -----------------------------------------------------------------------

    // (fmt_human_count is defined as a free fn below)

    /// Run a headless benchmark across the given populations. For each pop:
    ///   - model an all-to-all chatter graph: each of N units broadcasts once
    ///     per tick, recording fan-out (= peers seen) and total dispatches
    ///   - sample-time per-dispatch latency (capped to keep wall time bounded
    ///     — N² delivery at large N would take minutes)
    ///   - render the dashboard once per tick
    ///   - run a fixed number of `spawn.build_package` calls
    ///   - run `tick_dist_goals` (mesh.tick) once per tick
    ///
    /// The shape metrics are recorded as the *full theoretical* counts so the
    /// O(n²) growth is visible even though the bench doesn't actually invoke
    /// every receiver. Prints both a duration report and a values report per
    /// population, then a projected per-tick dispatch cost.
    /// Does not start the mesh, fork children, or touch persistent state.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run_bench(&mut self, populations: &[usize]) {
        println!("=== unit benchmark ===");
        println!("sizeof(VM) = {} bytes", std::mem::size_of::<VM>());
        println!(
            "sizeof(PeerInfo) = {} bytes (one entry in the per-unit peer table)",
            mesh::peer_info_size_bytes()
        );
        println!("self RSS at start: {} kB", crate::read_rss_kb());
        println!();

        // Measure the cost of one real fork once, up-front. Capture as locals
        // because metrics::reset() between populations would otherwise erase it.
        let (fork_ns, pkg_ns) = crate::bench_measure_one_fork(self);

        // Largest population for which we will *measurably* build a peer table
        // and run the chatter dispatch loop. Above this we project linearly.
        const SCALE_CAP: usize = 200_000;

        for &pop in populations {
            println!("###########################################################");
            println!("# population {}", pop);
            println!("###########################################################");

            if pop <= 10_000 {
                // Small enough for the original A/B model — run it straight.
                self.run_bench_one(pop, None, "all-to-all");
                self.run_bench_one(pop, Some(8), "gossip k=8");
            } else {
                // Very large: skip all-to-all (projected cost already prohibitive)
                // and run a measurable subset of gossip k=8, then project.
                let measure_pop = pop.min(SCALE_CAP);
                self.run_bench_one(measure_pop, Some(8), "gossip k=8 (capped)");
                if pop > SCALE_CAP {
                    crate::project_gossip_to(pop, measure_pop);
                }
            }

            // Memory + peer-table operations at scale.
            crate::run_scale_bench(pop, SCALE_CAP);
            // Spawn projection at this population.
            crate::project_spawn_to(pop, fork_ns, pkg_ns);
            println!();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn run_bench_one(&mut self, pop: usize, gossip_k: Option<usize>, label: &str) {
        metrics::reset();
        let ticks: usize = match pop {
            0..=99 => 200,
            100..=999 => 50,
            1000..=9999 => 10,
            _ => 3,
        };
        let spawn_iters: usize = 3;

        // Per-unit fan-out: under all-to-all every unit sends to N-1; under
        // bounded gossip every unit sends to min(k, N-1).
        let fanout_per_unit: u64 = match gossip_k {
            Some(k) => (k.min(pop.saturating_sub(1))) as u64,
            None => pop.saturating_sub(1) as u64,
        };

        // Sample of inbound messages we *actually* dispatch, to time per-call
        // handler latency. Per-call cost is independent of N, so a fixed
        // sample is sufficient. We don't actually run N² invocations.
        let sample_msgs: usize = 1000.min(
            (fanout_per_unit as usize).saturating_mul(pop),
        );
        let sample_template = format!(
            "(peer-hello :id \"sim0\" :gen 0 :peers {} :fitness 0)",
            pop.saturating_sub(1)
        );

        let total_start = std::time::Instant::now();
        for _ in 0..ticks {
            self.tick_dist_goals();

            // Record chatter shape under the chosen model.
            let mut tick_total: u64 = 0;
            for _ in 0..pop {
                metrics::record_value("chatter.fanout", fanout_per_unit);
                tick_total = tick_total.saturating_add(fanout_per_unit);
            }
            metrics::record_value("chatter.dispatch_per_tick", tick_total);

            for _ in 0..sample_msgs {
                self.process_chatter_msg(&sample_template);
            }

            // Discard dashboard output to avoid flooding stdout.
            let saved = self.output_buffer.take();
            self.output_buffer = Some(String::new());
            self.prim_dashboard();
            self.output_buffer = saved;
        }
        for _ in 0..spawn_iters {
            let _t = metrics::Timer::new("spawn.build_package");
            let state = self.build_state_for_spawn();
            let _ = spawn::build_package(&state);
        }
        let total = total_start.elapsed();

        println!(
            "--- population {} [{}] (ticks {}, sampled-dispatched {}/tick) — wall {:?} ---",
            pop, label, ticks, sample_msgs, total
        );
        println!("durations:");
        print!("{}", metrics::report());
        println!("counts:");
        print!("{}", metrics::report_values());

        let mean_dispatch = metrics::value_mean("chatter.dispatch_per_tick");
        let mean_proc_ns = metrics::duration_mean_ns("chatter.process");
        let projected_ns = (mean_dispatch as u128).saturating_mul(mean_proc_ns as u128);
        let projected_ms = projected_ns as f64 / 1e6;
        println!(
            "projected per-tick chatter dispatch [{}]: {} dispatches × {}ns ≈ {:.2}ms",
            label,
            crate::fmt_human_count(mean_dispatch),
            mean_proc_ns,
            projected_ms
        );

        let ratio = if pop > 0 {
            mean_dispatch as f64 / pop as f64
        } else {
            0.0
        };
        println!(
            "shape: dispatches/N = {:.1} (≈N → all-to-all O(N²); ≈k → bounded O(N·k))\n",
            ratio
        );
    }

}
