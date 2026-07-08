//! Evolution / fitness primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Evolution engine primitives
    // -----------------------------------------------------------------------

    pub(crate) fn evaluate_population(&mut self) {
        // Extract programs and target to avoid borrow conflicts with execute_sandbox.
        let (target, programs) = match self.evolution.as_ref() {
            Some(evo) => (
                evo.challenge.target_output.clone(),
                evo.population
                    .iter()
                    .map(|c| c.program.clone())
                    .collect::<Vec<_>>(),
            ),
            None => return,
        };

        // Evaluate each candidate in the sandbox.
        let mut scores = Vec::with_capacity(programs.len());
        for prog in &programs {
            let result = self.execute_sandbox(prog);
            let tc = evolve::tokenize(prog).len();
            scores.push(evolve::score_candidate(
                &result.output,
                result.success,
                &target,
                tc,
            ));
        }

        // Apply scores and update best.
        let evo = self.evolution.as_mut().unwrap();
        for (i, score) in scores.into_iter().enumerate() {
            evo.population[i].fitness = score;
        }
        for c in &evo.population {
            if evo.best.as_ref().is_none_or(|b| c.fitness > b.fitness) {
                evo.best = Some(c.clone());
            }
        }
    }

    pub(crate) fn prim_gp_evolve(&mut self) {
        // Initialize if not running.
        if self.evolution.is_none() {
            // Try to pick from challenge registry first.
            let challenge = if let Some(ch_id) = self.challenge_registry.next_unsolved() {
                if let Some(mut fc) = self.challenge_registry.to_fitness_challenge(ch_id) {
                    // Apply environment modifiers.
                    fc.max_steps = self.landscape.environment.apply_to_max_steps(fc.max_steps);
                    fc
                } else {
                    evolve::fib10_challenge()
                }
            } else {
                evolve::fib10_challenge()
            };
            let mut evo = evolve::EvolutionState::new(challenge.clone(), 1000);
            evo.population = evolve::init_population(&challenge, 50, &mut self.rng);
            evo.running = true;
            self.evolution = Some(evo);
        }

        let mut messages: Vec<String> = Vec::new();
        let mut sexp_broadcasts: Vec<String> = Vec::new();

        // Run batches of 10 generations.
        for _ in 0..10 {
            {
                let evo = self.evolution.as_ref().unwrap();
                if evo.generation >= evo.max_generations || !evo.running {
                    break;
                }
            }

            // Energy cost per generation.
            if !self.energy.can_afford(energy::GP_GENERATION_COST) {
                self.emit_str("[energy] evolution paused — insufficient energy\n");
                break;
            }
            self.energy.spend(energy::GP_GENERATION_COST, "gp-gen");

            // Evaluate fitness.
            self.evaluate_population();

            // Collect state for reporting.
            let evo = self.evolution.as_ref().unwrap();
            let gen = evo.generation;
            let best_fitness = evo.best.as_ref().map_or(0.0, |b| b.fitness);
            let best_prog = evo
                .best
                .as_ref()
                .map_or(String::new(), |b| b.program.clone());
            let best_tokens = evo.best.as_ref().map_or(0, |b| b.token_count());
            let pop_size = evo.population.len();
            let challenge_name = evo.challenge.name.clone();

            // Report every 100 generations.
            if gen.is_multiple_of(100) {
                messages.push(format!(
                    "[gen {}] best: {:.0} | pop: {} | \"{}\" ({} tokens)\n",
                    gen, best_fitness, pop_size, best_prog, best_tokens
                ));
                if best_fitness > 0.0 {
                    sexp_broadcasts.push(format!(
                        "(evolve-share :gen {} :fitness {:.0} :program \"{}\" :challenge \"{}\")",
                        gen,
                        best_fitness,
                        best_prog.replace('"', "\\\""),
                        challenge_name
                    ));
                }
            }

            // Check for winner.
            if best_fitness >= 800.0 && best_tokens <= 20 {
                messages.push(format!(
                    "[gen {}] WINNER: \"{}\" (fitness={:.0}, {} tokens)\n",
                    gen, best_prog, best_fitness, best_tokens
                ));
                // Install solution and mark challenge solved.
                if let Some(active_id) = self.challenge_registry.active_challenge {
                    let solver = self.node_id_cache.unwrap_or([0; 8]);
                    self.challenge_registry
                        .mark_solved(active_id, &best_prog, solver);
                    if let Some(ch) = self.challenge_registry.get_challenge(active_id) {
                        if let Ok(t) = ch.target_output.trim().parse::<i64>() {
                            self.last_solved_target = Some(t);
                        }
                        let ch_name = ch.name.clone();
                        // Broadcast solution to mesh.
                        if let Some(ref m) = self.mesh {
                            let hex = m.id_hex().to_string();
                            let sexp =
                                challenges::sexp_solution_broadcast(active_id, &best_prog, &hex);
                            sexp_broadcasts.push(sexp);
                        }
                        // Install as dictionary word (deferred to after borrow).
                        messages.push(format!("__INSTALL_SOL__{}|{}\n", ch_name, best_prog));
                    }
                }
                self.evolution.as_mut().unwrap().running = false;
                break;
            }

            // Produce next generation.
            let evo = self.evolution.as_mut().unwrap();
            let next = evolve::next_generation(&evo.population, gen + 1, &mut self.rng);
            evo.population = next;
            evo.generation = gen + 1;
        }

        // Emit collected messages and install solutions.
        for msg in &messages {
            if let Some(stripped) = msg.strip_prefix("__INSTALL_SOL__") {
                let rest = stripped.trim_end();
                if let Some(idx) = rest.find('|') {
                    let name = &rest[..idx];
                    let prog = &rest[idx + 1..];
                    self.install_solution(name, prog);
                    // Track niche: record solved challenge category.
                    let category = niche::categorize_challenge(name);
                    self.niche_profile
                        .challenge_history
                        .push((category, true));
                    niche::update_niche(&mut self.niche_profile);
                    // Generate harder challenges from the solution.
                    self.generate_landscape_challenges(name, prog);
                }
            } else {
                self.emit_str(msg);
            }
        }

        // Broadcast to mesh.
        for sexp in &sexp_broadcasts {
            if let Some(ref m) = self.mesh {
                m.send_sexp(sexp);
            }
        }

        // Final status.
        let evo = self.evolution.as_ref().unwrap();
        if evo.running && evo.generation < evo.max_generations {
            self.emit_str(&format!(
                "[gen {}] evolving... type GP-EVOLVE to continue, GP-STATUS for details\n",
                evo.generation
            ));
        } else if !evo.running || evo.generation >= evo.max_generations {
            if messages.is_empty() {
                let gen = evo.generation;
                let best = evo.best.as_ref().map_or("(none)".to_string(), |b| {
                    format!(
                        "\"{}\" (fitness={:.0}, {} tokens)",
                        b.program,
                        b.fitness,
                        b.token_count()
                    )
                });
                self.emit_str(&format!(
                    "[gen {}] evolution complete: {}\n",
                    gen, best
                ));
            }
            self.evolution.as_mut().unwrap().running = false;
        }
    }

    pub(crate) fn prim_gp_status(&mut self) {
        match &self.evolution {
            Some(evo) => {
                let best = evo.best.as_ref().map_or("(none)".to_string(), |b| {
                    format!(
                        "\"{}\" (fitness={:.0}, {} tokens)",
                        b.program,
                        b.fitness,
                        b.token_count()
                    )
                });
                self.emit_str(&format!(
                    "--- evolution ---\nchallenge: {}\ngeneration: {}/{}\nrunning: {}\nbest: {}\npop: {}\nimmigrants: {}\n",
                    evo.challenge.name, evo.generation, evo.max_generations,
                    evo.running, best, evo.population.len(), evo.immigrants
                ));
            }
            None => self.emit_str("no evolution running\n"),
        }
    }

    pub(crate) fn prim_gp_best(&mut self) {
        match &self.evolution {
            Some(evo) => match &evo.best {
                Some(best) => self.emit_str(&format!(
                    "{}\n(fitness={:.0}, gen={}, {} tokens)\n",
                    best.program,
                    best.fitness,
                    best.generation,
                    best.token_count()
                )),
                None => self.emit_str("no best candidate yet\n"),
            },
            None => self.emit_str("no evolution running\n"),
        }
    }

    pub(crate) fn prim_gp_stop(&mut self) {
        if let Some(ref mut evo) = self.evolution {
            evo.running = false;
            self.emit_str("evolution stopped\n");
        } else {
            self.emit_str("no evolution running\n");
        }
    }

    pub(crate) fn prim_gp_reset(&mut self) {
        self.evolution = None;
        self.emit_str("evolution reset\n");
    }

    // -----------------------------------------------------------------------
    // Fitness / Evolution primitives
    // -----------------------------------------------------------------------

    pub(crate) fn prim_leaderboard(&mut self) {
        if let Some(ref m) = self.mesh {
            let peer_fitness = m.peer_fitness_list();
            let s = fitness::format_leaderboard(&m.id_bytes(), self.fitness.score, &peer_fitness);
            self.emit_str(&s);
        } else {
            self.emit_str(&format!("  (offline) score={}\n", self.fitness.score));
        }
    }

    pub(crate) fn prim_rate(&mut self) {
        let score = self.pop();
        let _task_id = self.pop() as u64;
        // For now, rating adjusts local fitness (the rated peer would
        // receive the rating via gossip in a fuller implementation).
        self.fitness.record_rating(score);
        self.emit_str(&format!("rated: fitness adjusted by {}\n", score));
    }

    pub(crate) fn prim_evolve(&mut self) {
        self.do_evolve();
    }

    pub(crate) fn prim_auto_evolve(&mut self) {
        self.fitness.auto_evolve = !self.fitness.auto_evolve;
        self.emit_str(&format!(
            "auto-evolve: {}\n",
            if self.fitness.auto_evolve {
                "ON"
            } else {
                "OFF"
            }
        ));
    }

    pub(crate) fn prim_benchmark(&mut self) {
        let code = self.parse_until('"');
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(code);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(P_BENCHMARK_RT));
            }
        } else {
            self.fitness.benchmark_code = Some(code.clone());
            self.emit_str(&format!(
                "benchmark set: {}\n",
                code.chars().take(50).collect::<String>()
            ));
        }
    }

    pub(crate) fn rt_benchmark(&mut self) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let code = self.code_strings[idx].clone();
            self.fitness.benchmark_code = Some(code.clone());
            self.emit_str(&format!(
                "benchmark set: {}\n",
                code.chars().take(50).collect::<String>()
            ));
        }
    }

    pub(crate) fn prim_trust(&mut self) {
        // Expect a node ID on the stack (as a number).
        let id_val = self.pop() as u64;
        let id_bytes = id_val.to_be_bytes();
        self.trusted_peers.insert(id_bytes);
        self.emit_str(&format!("trusted: {:016x}\n", id_val));
    }

    /// Run one evolution cycle.
    pub(crate) fn do_evolve(&mut self) {
        // Get mesh average fitness.
        let avg_fitness = self
            .mesh
            .as_ref()
            .map(|m| {
                let peers = m.peer_fitness_list();
                if peers.is_empty() {
                    self.fitness.score
                } else {
                    let total: i64 =
                        peers.iter().map(|p| p.score).sum::<i64>() + self.fitness.score;
                    total / (peers.len() as i64 + 1)
                }
            })
            .unwrap_or(self.fitness.score);

        // Run benchmark before mutation.
        let before_score = self.run_benchmark();

        // Apply a random mutation.
        let mutable_indices: Vec<usize> = self
            .dictionary
            .iter()
            .enumerate()
            .filter(|(_, e)| mutation::is_mutable(e))
            .map(|(i, _)| i)
            .collect();
        if mutable_indices.is_empty() {
            self.emit_str("evolve: no mutable words\n");
            return;
        }
        let idx = mutable_indices[self.rng.next_usize(mutable_indices.len())];
        let dict_len = self.dictionary.len();
        if let Some(mut record) =
            mutation::mutate_entry(&mut self.dictionary[idx], &mut self.rng, dict_len)
        {
            record.word_index = idx;

            // Run benchmark after mutation.
            let after_score = self.run_benchmark();

            if after_score >= before_score {
                self.emit_str(&format!(
                    "evolve: kept mutation ({} -> {}): {}\n",
                    before_score,
                    after_score,
                    record.format()
                ));
                self.mutation_history.push(record);
            } else {
                mutation::undo_mutation(&mut self.dictionary[idx], &record);
                self.emit_str(&format!(
                    "evolve: reverted mutation ({} -> {})\n",
                    before_score, after_score
                ));
            }
        } else {
            self.emit_str("evolve: mutation failed\n");
        }
        self.fitness.mark_evolved();
        self.emit_str(&format!(
            "evolve: own={} avg={} evolutions={}\n",
            self.fitness.score, avg_fitness, self.fitness.evolution_count
        ));
    }

    /// Run the benchmark code and return a score (stack depth after execution).
    pub(crate) fn run_benchmark(&mut self) -> i64 {
        let code = match self.fitness.benchmark_code.clone() {
            Some(c) => c,
            None => return 0,
        };
        #[cfg(not(target_arch = "wasm32"))]
        let start = Instant::now();
        let result = self.execute_sandbox(&code);
        #[cfg(not(target_arch = "wasm32"))]
        let elapsed = start.elapsed().as_millis() as i64;
        #[cfg(target_arch = "wasm32")]
        let elapsed: i64 = 0;
        // Score = stack depth * 10 - elapsed_ms (reward correct output, penalize slowness).
        let depth_score = result.stack_snapshot.len() as i64 * 10;
        let time_penalty = (elapsed / 100).min(50);
        if result.success {
            depth_score - time_penalty
        } else {
            -100
        }
    }

    pub(crate) fn check_auto_evolve(&mut self) {
        if self.fitness.should_auto_evolve() {
            self.do_evolve();
        }
    }

}
