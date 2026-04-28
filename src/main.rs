// unit — a software nanobot
// Minimal Forth interpreter that is also a self-replicating networked agent.

// --- Shared types ---
pub mod types;

// --- The Forth VM ---
pub mod vm;

// --- S-expression wire format ---
pub mod sexp;

// --- JSON snapshot persistence ---
pub mod snapshot;

// --- Genetic programming engine ---
pub mod evolve;

// --- Distributed goal computation ---
pub mod distgoal;

// --- Challenge registry (immune system) ---
pub mod challenges;

// --- Problem discovery ---
pub mod discovery;

// --- Metabolic energy system ---
pub mod energy;

// --- Dynamic fitness landscape ---
pub mod landscape;

// --- Timing instrumentation ---
pub mod metrics;

// --- Single-process multi-unit host (in-process port of the WASM browser model) ---
pub mod multi_unit;

// --- Integration tests ---
#[cfg(test)]
mod integration_tests;

// --- Core nanobot ---
#[allow(dead_code)]
pub mod goals;
#[allow(dead_code)]
pub mod mesh;

// --- Sexual reproduction ---
#[allow(dead_code)]
pub mod reproduction;

// --- Niche construction ---
#[allow(dead_code)]
pub mod niche;

/// Inter-unit signaling — direct (peer inbox) and environmental layers.
pub mod signaling;

// --- Replication & persistence ---
#[allow(dead_code)]
pub mod persist;
#[allow(dead_code)]
pub mod spawn;

// --- Feature layers ---
pub mod features {
    #[allow(dead_code)]
    pub mod fitness;
    #[allow(dead_code)]
    pub mod io_words;
    #[allow(dead_code)]
    pub mod monitor;
    #[allow(dead_code)]
    pub mod mutation;
    #[allow(dead_code)]
    pub mod ws_bridge;
}

#[allow(dead_code)]
mod platform;

// --- HTTP bridge (localhost only, opt-in via --features http) ---
#[cfg(all(feature = "http", not(target_arch = "wasm32")))]
pub mod http;

#[cfg(target_arch = "wasm32")]
mod wasm_entry;

use std::io::{self, BufRead, Write};

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    unsafe { kill(pid, sig) }
}
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use features::{fitness, io_words, monitor, mutation, ws_bridge};
use types::{Cell, Instruction, PAD};
use vm::VM;
use vm::*; // import P_* constants

// ===========================================================================
// Feature primitives — extend the core VM for mesh, goals, I/O, ops, etc.
// ===========================================================================

impl VM {
    // -----------------------------------------------------------------------
    // Atom primitives (raw data for Forth-level orchestration)
    // -----------------------------------------------------------------------

    /// GOAL-COUNT ( -- total pending active completed failed )
    fn prim_goal_count(&mut self) {
        if let Some(ref m) = self.mesh {
            let st = m.state_lock();
            let total = st.goals.goals.len() as Cell;
            let pending = st
                .goals
                .goals
                .values()
                .filter(|g| g.status == goals::GoalStatus::Pending)
                .count() as Cell;
            let active = st
                .goals
                .goals
                .values()
                .filter(|g| g.status == goals::GoalStatus::Active)
                .count() as Cell;
            let completed = st
                .goals
                .goals
                .values()
                .filter(|g| g.status == goals::GoalStatus::Completed)
                .count() as Cell;
            let failed = st
                .goals
                .goals
                .values()
                .filter(|g| g.status == goals::GoalStatus::Failed)
                .count() as Cell;
            drop(st);
            self.stack.push(total);
            self.stack.push(pending);
            self.stack.push(active);
            self.stack.push(completed);
            self.stack.push(failed);
        } else {
            for _ in 0..5 {
                self.stack.push(0);
            }
        }
    }

    /// TASK-COUNT ( -- total waiting running done failed )
    fn prim_task_count(&mut self) {
        if let Some(ref m) = self.mesh {
            let st = m.state_lock();
            let total = st.goals.tasks.len() as Cell;
            let waiting = st
                .goals
                .tasks
                .values()
                .filter(|t| t.status == goals::TaskStatus::Waiting)
                .count() as Cell;
            let running = st
                .goals
                .tasks
                .values()
                .filter(|t| t.status == goals::TaskStatus::Running)
                .count() as Cell;
            let done = st
                .goals
                .tasks
                .values()
                .filter(|t| t.status == goals::TaskStatus::Done)
                .count() as Cell;
            let failed = st
                .goals
                .tasks
                .values()
                .filter(|t| t.status == goals::TaskStatus::Failed)
                .count() as Cell;
            drop(st);
            self.stack.push(total);
            self.stack.push(waiting);
            self.stack.push(running);
            self.stack.push(done);
            self.stack.push(failed);
        } else {
            for _ in 0..5 {
                self.stack.push(0);
            }
        }
    }

    /// MESH-AVG-FITNESS ( -- avg )
    fn prim_mesh_avg_fitness(&mut self) {
        let avg = self
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
            .unwrap_or(0);
        self.stack.push(avg);
    }

    /// CHECK-WATCHES ( -- ) run all due watch checks.
    fn prim_check_watches(&mut self) {
        let due = self.monitor.due_watches();
        for wid in due {
            self.run_watch_check(wid);
        }
    }

    /// RUN-HANDLERS ( -- ) run alert handlers for active alerts.
    fn prim_run_handlers(&mut self) {
        let handlers: Vec<(u32, String)> = self
            .monitor
            .alerts
            .iter()
            .filter(|a| !a.acknowledged)
            .filter_map(|a| {
                self.monitor
                    .watches
                    .get(&a.watch_id)
                    .and_then(|w| w.alert_handler.clone())
                    .map(|h| (a.id, h))
            })
            .collect();
        for (_aid, handler) in &handlers {
            self.interpret_line(handler);
        }
    }

    /// MUTATE-RANDOM ( -- flag ) apply a random mutation, push -1 if success, 0 if fail.
    fn prim_mutate_random_atom(&mut self) {
        let mutable_indices: Vec<usize> = self
            .dictionary
            .iter()
            .enumerate()
            .filter(|(_, e)| mutation::is_mutable(e))
            .map(|(i, _)| i)
            .collect();
        if mutable_indices.is_empty() {
            self.stack.push(0);
            return;
        }
        let idx = mutable_indices[self.rng.next_usize(mutable_indices.len())];
        let dict_len = self.dictionary.len();
        if let Some(mut record) =
            mutation::mutate_entry(&mut self.dictionary[idx], &mut self.rng, dict_len)
        {
            record.word_index = idx;
            self.mutation_history.push(record);
            self.stack.push(-1); // success
        } else {
            self.stack.push(0); // fail
        }
    }

    // -----------------------------------------------------------------------
    // Smart mutation
    // -----------------------------------------------------------------------

    fn snapshot_word(&mut self, idx: usize) -> u64 {
        let body = self.dictionary[idx].body.clone();
        let mut combined = String::new();
        // Save the outer output buffer so callers don't lose accumulated output.
        let saved_outer_buffer = self.output_buffer.take();
        for test_stack in &[vec![], vec![1i64], vec![1, 2, 3]] {
            let saved = std::mem::take(&mut self.stack);
            self.stack = test_stack.clone();
            self.output_buffer = Some(String::new());
            self.timed_out = false;
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.deadline = Some(Instant::now() + Duration::from_millis(100));
            }
            self.execute_body(&body);
            combined.push_str(&self.output_buffer.take().unwrap_or_default());
            combined.push_str(&format!("{:?}", self.stack));
            self.stack = saved;
            self.deadline = None;
            self.timed_out = false;
        }
        // Restore the outer output buffer.
        self.output_buffer = saved_outer_buffer;
        mutation::hash_output(&combined)
    }

    fn prim_smart_mutate(&mut self) {
        let mutable_indices: Vec<usize> = self
            .dictionary
            .iter()
            .enumerate()
            .filter(|(_, e)| mutation::is_mutable(e))
            .map(|(i, _)| i)
            .collect();
        if mutable_indices.is_empty() {
            self.emit_str("no mutable words\n");
            self.stack.push(0);
            return;
        }
        let idx = mutable_indices[self.rng.next_usize(mutable_indices.len())];
        let word_name = self.dictionary[idx].name.clone();
        let before_hash = self.snapshot_word(idx);

        let dict_len = self.dictionary.len();
        let record =
            match mutation::mutate_entry(&mut self.dictionary[idx], &mut self.rng, dict_len) {
                Some(mut r) => {
                    r.word_index = idx;
                    r
                }
                None => {
                    self.stack.push(0);
                    return;
                }
            };

        let after_hash = self.snapshot_word(idx);
        let class = if after_hash == before_hash {
            mutation::MutationClass::Neutral
        } else {
            let score = self.run_benchmark();
            if score >= 0 {
                mutation::MutationClass::Beneficial
            } else {
                mutation::MutationClass::Harmful
            }
        };

        let kept = matches!(
            class,
            mutation::MutationClass::Neutral | mutation::MutationClass::Beneficial
        );
        if kept {
            self.mutation_history.push(record.clone());
        } else {
            mutation::undo_mutation(&mut self.dictionary[idx], &record);
        }

        self.mutation_stats.record(&class);
        self.last_mutation_result = Some(mutation::SmartMutationResult {
            word_name,
            strategy: record.strategy.clone(),
            class,
            before_hash,
            after_hash,
            kept,
            description: record.description,
        });
        self.stack.push(if kept { -1 } else { 0 });
    }

    fn prim_mutation_report(&mut self) {
        if let Some(ref r) = self.last_mutation_result {
            self.emit_str(&format!(
                "last: {} [{}] {} {}\n",
                r.word_name,
                r.strategy.label(),
                r.class.label(),
                if r.kept { "(kept)" } else { "(reverted)" }
            ));
        } else {
            self.emit_str("no mutations yet\n");
        }
    }

    // -----------------------------------------------------------------------
    // S-expression primitives
    // -----------------------------------------------------------------------

    /// SEXP" expr" — parse S-expression and translate to Forth, then execute.
    fn prim_sexp_eval(&mut self) {
        let sexp_str = self.parse_until('"');
        match crate::sexp::parse(&sexp_str) {
            Ok(sexp) => {
                let forth = crate::sexp::to_forth(&sexp);
                // Save outer input state — interpret_line overwrites these.
                let saved_buf = self.input_buffer.clone();
                let saved_pos = self.input_pos;
                self.interpret_line(&forth);
                // Restore so the rest of the outer line continues.
                self.input_buffer = saved_buf;
                self.input_pos = saved_pos;
            }
            Err(e) => {
                self.emit_str(&format!("sexp error: {}\n", e));
            }
        }
    }

    /// SEXP-SEND" expr" — broadcast an S-expression message to mesh peers.
    fn prim_sexp_send(&mut self) {
        let sexp_str = self.parse_until('"');
        // Validate it parses as a valid S-expression.
        match crate::sexp::parse(&sexp_str) {
            Ok(_) => {
                if let Some(ref m) = self.mesh {
                    m.send_sexp(&sexp_str);
                    self.emit_str("sexp sent\n");
                } else {
                    self.emit_str("no mesh\n");
                }
            }
            Err(e) => {
                self.emit_str(&format!("sexp error: {}\n", e));
            }
        }
    }

    /// SEXP-RECV — drain inbound S-expression messages, print them.
    fn prim_sexp_recv(&mut self) {
        if let Some(ref m) = self.mesh {
            let msgs = m.recv_sexp_messages();
            if msgs.is_empty() {
                self.emit_str("no sexp messages\n");
            } else {
                for msg in &msgs {
                    self.emit_str(msg);
                    self.emit_str("\n");
                }
            }
        } else {
            self.emit_str("no mesh\n");
        }
    }

    // -----------------------------------------------------------------------
    // JSON snapshot primitives
    // -----------------------------------------------------------------------

    fn make_json_snapshot(&self) -> snapshot::UnitSnapshot {
        let node_id = self
            .node_id_cache
            .map(|id| crate::mesh::id_to_hex(&id))
            .unwrap_or_else(|| "offline".to_string());
        #[cfg(not(target_arch = "wasm32"))]
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        #[cfg(target_arch = "wasm32")]
        let ts: u64 = 0;

        // Collect user-defined words (skip kernel + prelude words).
        let kernel_count = self.kernel_word_count;
        let words: Vec<(String, String)> = self.dictionary[kernel_count..]
            .iter()
            .filter(|e| !e.hidden)
            .map(|e| {
                let source = snapshot::decompile_word(e, &self.dictionary, &self.primitive_names);
                (e.name.clone(), source)
            })
            .collect();

        snapshot::UnitSnapshot {
            node_id,
            timestamp: ts,
            stack: self.stack.clone(),
            fitness: self.fitness.score,
            tasks_completed: self.fitness.tasks_completed,
            generation: self.spawn_state.generation,
            mutation_stats: snapshot::MutStats {
                total: self.mutation_stats.total,
                neutral: self.mutation_stats.neutral,
                beneficial: self.mutation_stats.beneficial,
                harmful: self.mutation_stats.harmful,
                lethal: self.mutation_stats.lethal,
            },
            words,
            memory_here: self.here,
            memory: self.memory[..self.here].to_vec(),
            energy: self.energy.energy,
            energy_max: self.energy.max_energy,
            energy_earned: self.energy.total_earned,
            energy_spent: self.energy.total_spent,
            landscape_depth: self.landscape.depth,
            landscape_generated: self.landscape.challenges_generated,
        }
    }

    fn restore_json_snapshot(&mut self, snap: &snapshot::UnitSnapshot) {
        // Restore simple fields.
        self.stack = snap.stack.clone();
        self.fitness.score = snap.fitness;
        self.fitness.tasks_completed = snap.tasks_completed;
        self.spawn_state.generation = snap.generation;
        self.mutation_stats.total = snap.mutation_stats.total;
        self.mutation_stats.neutral = snap.mutation_stats.neutral;
        self.mutation_stats.beneficial = snap.mutation_stats.beneficial;
        self.mutation_stats.harmful = snap.mutation_stats.harmful;
        self.mutation_stats.lethal = snap.mutation_stats.lethal;

        // Restore energy.
        self.energy.energy = snap.energy;
        self.energy.max_energy = snap.energy_max;
        self.energy.total_earned = snap.energy_earned;
        self.energy.total_spent = snap.energy_spent;

        // Restore landscape.
        self.landscape.depth = snap.landscape_depth;
        self.landscape.challenges_generated = snap.landscape_generated;

        // Restore memory.
        if snap.memory_here <= self.memory.len() {
            self.here = snap.memory_here;
            for (i, &v) in snap.memory.iter().enumerate() {
                if i < self.memory.len() {
                    self.memory[i] = v;
                }
            }
        }

        // Restore user-defined words by eval'ing their decompiled source.
        for (_, source) in &snap.words {
            let saved_buf = self.input_buffer.clone();
            let saved_pos = self.input_pos;
            let saved_silent = self.silent;
            self.silent = true;
            self.interpret_line(source);
            self.silent = saved_silent;
            self.input_buffer = saved_buf;
            self.input_pos = saved_pos;
        }
    }

    fn prim_json_snapshot(&mut self) {
        let snap = self.make_json_snapshot();
        let json = snapshot::to_json(&snap);
        let id = self.node_id_cache.unwrap_or([0u8; 8]);
        match snapshot::save_json_snapshot(&id, &json) {
            Ok(path) => {
                self.emit_str(&format!("snapshot saved to {}\n", path));
                if let Some(ref m) = self.mesh {
                    let sexp = crate::sexp::msg_snapshot(&id, snap.fitness, snap.generation);
                    m.send_sexp(&sexp.to_string());
                }
            }
            Err(e) => self.emit_str(&format!("snapshot failed: {}\n", e)),
        }
    }

    fn prim_json_restore(&mut self) {
        let id = self.node_id_cache.unwrap_or([0u8; 8]);
        if let Some(json) = snapshot::load_json_snapshot(&id) {
            if let Some(snap) = snapshot::from_json(&json) {
                self.restore_json_snapshot(&snap);
                self.emit_str(&format!(
                    "restored from snapshot (saved {}, fitness={}, gen={})\n",
                    snap.timestamp, snap.fitness, snap.generation
                ));
                if let Some(ref m) = self.mesh {
                    let sexp = crate::sexp::msg_resurrect(
                        &id,
                        snap.fitness,
                        snap.generation,
                        snap.timestamp,
                    );
                    m.send_sexp(&sexp.to_string());
                }
            } else {
                self.emit_str("restore: corrupt snapshot\n");
            }
        } else {
            self.emit_str("no snapshot found\n");
        }
    }

    fn prim_snapshot_path(&mut self) {
        let id = self.node_id_cache.unwrap_or([0u8; 8]);
        self.emit_str(&format!("{}\n", snapshot::snapshot_path(&id)));
    }

    fn prim_json_snapshots(&mut self) {
        let snapshots = snapshot::list_json_snapshots();
        if snapshots.is_empty() {
            self.emit_str("no snapshots\n");
        } else {
            for name in &snapshots {
                self.emit_str(&format!("  {}\n", name));
            }
        }
    }

    fn prim_auto_snapshot(&mut self) {
        let secs = self.pop();
        if secs <= 0 {
            self.auto_snapshot_secs = 0;
            self.auto_snapshot_last = None;
            self.emit_str("auto-snapshot: OFF\n");
        } else {
            self.auto_snapshot_secs = secs as u64;
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.auto_snapshot_last = Some(Instant::now());
            }
            self.emit_str(&format!("auto-snapshot: every {}s\n", secs));
        }
    }

    fn prim_hibernate(&mut self) {
        let snap = self.make_json_snapshot();
        let json = snapshot::to_json(&snap);
        if let Some(id) = self.node_id_cache {
            match snapshot::save_json_snapshot(&id, &json) {
                Ok(path) => {
                    self.emit_str(&format!("hibernating... saved to {}\n", path));
                    if let Some(ref m) = self.mesh {
                        let sexp = crate::sexp::msg_snapshot(&id, snap.fitness, snap.generation);
                        m.send_sexp(&sexp.to_string());
                    }
                }
                Err(e) => self.emit_str(&format!("hibernate failed: {}\n", e)),
            }
        } else {
            // No node ID — save to in-memory anyway.
            let _ = snapshot::save_json_snapshot(&[0u8; 8], &json);
            self.emit_str("hibernated (in-memory)\n");
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.running = false;
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.emit_str("(browser mode — snapshot saved, VM stays alive)\n");
        }
    }

    fn prim_export_genome(&mut self) {
        let kernel_count = self.kernel_word_count;
        let mut genome = String::new();
        for entry in &self.dictionary[kernel_count..] {
            if entry.hidden {
                continue;
            }
            let source = snapshot::decompile_word(entry, &self.dictionary, &self.primitive_names);
            genome.push_str(&source);
            genome.push('\n');
        }
        if genome.is_empty() {
            self.emit_str("(empty genome)\n");
        } else {
            self.emit_str(&genome);
        }
    }

    fn prim_import_genome(&mut self) {
        let source = self.parse_until('"');
        if source.trim().is_empty() {
            self.emit_str("import-genome: empty input\n");
            return;
        }
        let saved_buf = self.input_buffer.clone();
        let saved_pos = self.input_pos;
        let count_before = self.dictionary.len();
        for line in source.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                self.interpret_line(trimmed);
            }
        }
        self.input_buffer = saved_buf;
        self.input_pos = saved_pos;
        let imported = self.dictionary.len() - count_before;
        self.emit_str(&format!("imported {} words\n", imported));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn check_auto_snapshot(&mut self) {
        if self.auto_snapshot_secs == 0 {
            return;
        }
        if let Some(last) = self.auto_snapshot_last {
            if last.elapsed() >= Duration::from_secs(self.auto_snapshot_secs) {
                self.auto_snapshot_last = Some(Instant::now());
                let snap = self.make_json_snapshot();
                let json = snapshot::to_json(&snap);
                if let Some(id) = self.node_id_cache {
                    let _ = snapshot::save_json_snapshot(&id, &json);
                }
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn check_auto_snapshot(&mut self) {
        // No timer on WASM — auto-snapshot is a no-op in the browser.
    }

    /// Try to resurrect from a JSON snapshot. Returns true if restored.
    pub fn try_resurrect(&mut self) -> bool {
        if let Some(id) = self.node_id_cache {
            if let Some(json) = snapshot::load_json_snapshot(&id) {
                if let Some(snap) = snapshot::from_json(&json) {
                    self.restore_json_snapshot(&snap);
                    return true;
                }
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Evolution engine primitives
    // -----------------------------------------------------------------------

    fn evaluate_population(&mut self) {
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

    fn prim_gp_evolve(&mut self) {
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

    fn prim_gp_status(&mut self) {
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

    fn prim_gp_best(&mut self) {
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

    fn prim_gp_stop(&mut self) {
        if let Some(ref mut evo) = self.evolution {
            evo.running = false;
            self.emit_str("evolution stopped\n");
        } else {
            self.emit_str("no evolution running\n");
        }
    }

    fn prim_gp_reset(&mut self) {
        self.evolution = None;
        self.emit_str("evolution reset\n");
    }

    // -----------------------------------------------------------------------
    // Distributed goal primitives
    // -----------------------------------------------------------------------

    /// DIST-GOAL{ expr1 | expr2 | ... } — distribute and compute.
    fn prim_dist_goal(&mut self) {
        let input = self.parse_balanced_braces();
        let expressions = distgoal::parse_pipe_expressions(&input);
        if expressions.is_empty() {
            self.emit_str("dist-goal: no expressions\n");
            return;
        }

        // Get peer list.
        let peer_ids: Vec<String> = self
            .mesh
            .as_ref()
            .map(|m| {
                m.peer_details()
                    .iter()
                    .map(|(id, _, _)| id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let my_id = self
            .node_id_cache
            .map(|id| crate::mesh::id_to_hex(&id))
            .unwrap_or_else(|| "local".to_string());

        let goal_id = self.dist_engine.create_goal(expressions, &my_id, &peer_ids);

        // Send remote sub-goals as S-expressions.
        let remote = self.dist_engine.pending_remote_subgoals(goal_id);
        for (seq, expr, _peer) in &remote {
            if let Some(ref m) = self.mesh {
                let sexp = distgoal::sexp_sub_goal(goal_id, *seq, &my_id, expr);
                m.send_sexp(&sexp);
            }
        }
        let remote_count = remote.len();

        // Compute local sub-goals immediately.
        let local = self.dist_engine.pending_local_subgoals(goal_id);
        for (seq, expr) in &local {
            let result = self.execute_sandbox(expr);
            let output = result.output.trim().to_string();
            self.dist_engine.record_result(goal_id, *seq, &output);
        }

        // If all done (no remote, or no peers), deliver immediately.
        if self.dist_engine.is_complete(goal_id) {
            if let Some(combined) = self.dist_engine.combine_results(goal_id) {
                let total = self
                    .dist_engine
                    .goals
                    .get(&goal_id)
                    .map_or(0, |g| g.sub_goals.len());
                self.emit_str(&format!("{}\n", combined));
                if remote_count > 0 {
                    self.emit_str(&format!(
                        "(distributed {} sub-goals, {} local, {} remote)\n",
                        total,
                        total - remote_count,
                        remote_count
                    ));
                }
                // Broadcast completion.
                if let Some(ref m) = self.mesh {
                    let sexp = distgoal::sexp_dist_complete(goal_id, &combined, peer_ids.len());
                    m.send_sexp(&sexp);
                }
            }
        } else {
            self.emit_str(&format!(
                "dist-goal #{}: {} sub-goals distributed ({} local, {} remote)\n\
                 waiting for results... type DIST-STATUS to check\n",
                goal_id,
                self.dist_engine
                    .goals
                    .get(&goal_id)
                    .map_or(0, |g| g.sub_goals.len()),
                local.len(),
                remote_count
            ));
        }
    }

    fn prim_dist_status(&mut self) {
        let s = self.dist_engine.format_status();
        self.emit_str(&s);
    }

    fn prim_dist_cancel(&mut self) {
        self.dist_engine.goals.clear();
        self.emit_str("all distributed goals cancelled\n");
    }

    // -----------------------------------------------------------------------
    // Immune system primitives
    // -----------------------------------------------------------------------

    fn prim_challenges(&mut self) {
        let out = self.challenge_registry.format_challenges();
        self.emit_str(&out);
    }

    fn prim_immune_status(&mut self) {
        let total = self.challenge_registry.challenges.len();
        let solved = self
            .challenge_registry
            .challenges
            .values()
            .filter(|c| c.solved)
            .count();
        let unsolved = total - solved;
        let antibodies = self
            .dictionary
            .iter()
            .filter(|e| e.name.starts_with("SOL-"))
            .count();
        self.emit_str(&format!(
            "--- immune status ---\nchallenges: {} ({} solved, {} unsolved)\n\
             colony antibodies: {}\n",
            total, solved, unsolved, antibodies
        ));
        if let Some(active) = self.challenge_registry.active() {
            self.emit_str(&format!("active: #{} {}\n", active.id, active.name));
        }
        // List antibody words
        let sol_words: Vec<&str> = self
            .dictionary
            .iter()
            .filter(|e| e.name.starts_with("SOL-"))
            .map(|e| e.name.as_str())
            .collect();
        if !sol_words.is_empty() {
            self.emit_str(&format!("  words: {}\n", sol_words.join(" ")));
        }
    }

    fn prim_antibodies(&mut self) {
        let sol_words: Vec<String> = self
            .dictionary
            .iter()
            .filter(|e| e.name.starts_with("SOL-"))
            .map(|e| e.name.clone())
            .collect();
        if sol_words.is_empty() {
            self.emit_str("no antibodies yet\n");
        } else {
            self.emit_str(&format!("--- {} antibodies ---\n", sol_words.len()));
            for name in &sol_words {
                self.emit_str(&format!("  {}\n", name));
            }
        }
    }

    fn prim_metabolism(&mut self) {
        let out = format!(
            "--- metabolism ---\n\
             energy: {}/{}\n\
             lifetime earned: {}\n\
             lifetime spent: {}\n\
             efficiency: {:.2}\n\
             peak energy: {}\n\
             starving ticks: {}\n\
             throttled: {}\n\
             --- costs ---\n\
             \x20 spawn: {}\n\
             \x20 gp generation: {}\n\
             \x20 eval per 1000 steps: {}\n\
             \x20 mesh send: {}\n\
             --- rewards ---\n\
             \x20 task success: {}\n\
             \x20 challenge solved: {}\n\
             \x20 passive regen: {}/tick\n",
            self.energy.energy,
            self.energy.max_energy,
            self.energy.total_earned,
            self.energy.total_spent,
            self.energy.efficiency(),
            self.energy.peak_energy,
            self.energy.starving_ticks,
            if self.energy.throttled { "YES" } else { "no" },
            energy::SPAWN_COST,
            energy::GP_GENERATION_COST,
            energy::EVAL_STEP_COST_PER_1000,
            energy::MESH_SEND_COST,
            energy::TASK_REWARD,
            energy::CHALLENGE_SOLVE_REWARD,
            energy::PASSIVE_REGEN,
        );
        self.emit_str(&out);
    }

    /// Generate harder challenges from a solved one via the landscape engine.
    fn generate_landscape_challenges(&mut self, challenge_name: &str, solution: &str) {
        // Find the solved challenge by name.
        let solved = self
            .challenge_registry
            .challenges
            .values()
            .find(|c| c.name == challenge_name && c.solved)
            .cloned();
        let solved = match solved {
            Some(c) => c,
            None => return,
        };
        let all_solved: Vec<&challenges::Challenge> = self
            .challenge_registry
            .challenges
            .values()
            .filter(|c| c.solved)
            .collect();
        let new_challenges = self
            .landscape
            .on_challenge_solved(&solved, solution, &all_solved);
        if new_challenges.is_empty() {
            return;
        }
        let count = new_challenges.len();
        let depth = self.landscape.depth();
        let my_id = self.node_id_cache.unwrap_or([0; 8]);
        for ch in new_challenges {
            let id = self.challenge_registry.register_discovered(
                &ch.name,
                &ch.description,
                &ch.target_output,
                ch.test_input.clone(),
                ch.seed_programs.clone(),
                my_id,
                ch.reward,
            );
            // Broadcast to mesh.
            if let Some(ref m) = self.mesh {
                if let Some(registered) = self.challenge_registry.get_challenge(id) {
                    let sexp = challenges::sexp_challenge_broadcast(registered);
                    m.send_sexp(&sexp);
                }
            }
        }
        self.emit_str(&format!(
            "[landscape] depth {}: generated {} new challenges from '{}'\n",
            depth, count, challenge_name
        ));
    }

    /// Install a solved challenge as a dictionary word (sol-{name}).
    fn install_solution(&mut self, challenge_name: &str, program: &str) {
        let word_name = format!("SOL-{}", challenge_name.to_uppercase());
        // Check if already installed.
        if self.find_word(&word_name).is_some() {
            return;
        }
        let def = format!(": {} {} ;", word_name, program);
        self.interpret_line(&def);
        self.energy
            .earn(energy::CHALLENGE_SOLVE_REWARD, "challenge-solved");
        self.emit_str(&format!("[immune] learned word: {}\n", word_name));
    }

    /// Called during REPL tick to check for incoming sub-goal results and timeouts.
    fn tick_dist_goals(&mut self) {
        let _t_tick = metrics::Timer::new("mesh.tick");
        self.dist_engine.advance_tick();

        // Process incoming S-expression messages for sub-results.
        if let Some(ref m) = self.mesh {
            let msgs = m.recv_sexp_messages();
            for msg in &msgs {
                self.process_chatter_msg(msg);
            }
        }

        // Check for timed-out sub-goals and fall back to local.
        let goal_ids: Vec<u64> = self.dist_engine.goals.keys().copied().collect();
        for gid in goal_ids {
            let timed_out = self.dist_engine.timed_out_subgoals(gid);
            for (seq, expr) in timed_out {
                self.dist_engine.fallback_to_local(gid, seq);
                let result = self.execute_sandbox(&expr);
                let output = result.output.trim().to_string();
                self.dist_engine.record_result(gid, seq, &output);
                self.emit_str(&format!(
                    "(fallback: computed sub-goal {} locally — peer timeout)\n",
                    seq
                ));
                if self.dist_engine.is_complete(gid) {
                    if let Some(combined) = self.dist_engine.combine_results(gid) {
                        self.emit_str(&format!("dist-goal #{} complete: {}\n", gid, combined));
                    }
                }
            }
        }
    }

    /// Dispatch a single inbound chatter (S-expression) message. Extracted so
    /// the bench harness can call it directly with synthesized messages.
    pub fn process_chatter_msg(&mut self, msg: &str) {
        let _t_msg = metrics::Timer::new("chatter.process");
        if let Some(sexp) = crate::sexp::try_parse_mesh_msg(msg) {
            match crate::sexp::msg_type(&sexp) {
                        Some("sub-goal") => {
                            // A peer asked us to compute something.
                            let goal_id =
                                sexp.get_key(":id").and_then(|s| s.as_number()).unwrap_or(0) as u64;
                            let seq = sexp
                                .get_key(":seq")
                                .and_then(|s| s.as_number())
                                .unwrap_or(0) as usize;
                            let _from = sexp
                                .get_key(":from")
                                .and_then(|s| s.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let expr = sexp
                                .get_key(":expr")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();
                            if !expr.is_empty() {
                                let result = self.execute_sandbox(&expr);
                                let output = result.output.trim().to_string();
                                let my_id = self
                                    .node_id_cache
                                    .map(|id| crate::mesh::id_to_hex(&id))
                                    .unwrap_or_else(|| "local".to_string());
                                if let Some(ref m2) = self.mesh {
                                    let reply =
                                        distgoal::sexp_sub_result(goal_id, seq, &my_id, &output);
                                    m2.send_sexp(&reply);
                                }
                            }
                        }
                        Some("sub-result") => {
                            // A peer sent back a result.
                            let goal_id =
                                sexp.get_key(":id").and_then(|s| s.as_number()).unwrap_or(0) as u64;
                            let seq = sexp
                                .get_key(":seq")
                                .and_then(|s| s.as_number())
                                .unwrap_or(0) as usize;
                            let result_str = sexp
                                .get_key(":result")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();
                            self.dist_engine.record_result(goal_id, seq, &result_str);

                            // Check if goal is now complete.
                            if self.dist_engine.is_complete(goal_id) {
                                if let Some(combined) = self.dist_engine.combine_results(goal_id) {
                                    self.emit_str(&format!(
                                        "dist-goal #{} complete: {}\n",
                                        goal_id, combined
                                    ));
                                }
                            }
                        }
                        Some("mating-request") => {
                            let from_hex = sexp
                                .get_key(":from")
                                .and_then(|s| s.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let fitness = sexp
                                .get_key(":fitness")
                                .and_then(|s| s.as_number())
                                .unwrap_or(0);
                            let my_fitness = self.fitness.score;
                            // Auto-accept if requester fitness >= half of ours.
                            let accept =
                                self.mate_auto_accept && fitness >= my_fitness / 2;
                            if accept {
                                let my_id = self.node_id_cache.unwrap_or([0; 8]);
                                let words: Vec<(String, String)> = self
                                    .dictionary
                                    .iter()
                                    .filter(|e| !e.hidden && e.body.len() > 1)
                                    .take(50)
                                    .map(|e| (e.name.clone(), format!("{:?}", e.body)))
                                    .collect();
                                let resp = reproduction::MatingResponse {
                                    accepted: true,
                                    responder_id: my_id,
                                    responder_fitness: my_fitness,
                                    dictionary_words: words,
                                };
                                let reply = reproduction::sexp_mating_response(&resp);
                                if let Some(ref m2) = self.mesh {
                                    m2.send_sexp(&reply);
                                }
                                self.emit_str(&format!(
                                    "[mate] accepted mating request from {}\n",
                                    from_hex
                                ));
                            } else {
                                self.emit_str(&format!(
                                    "[mate] denied mating request from {}\n",
                                    from_hex
                                ));
                            }
                        }
                        Some("mating-response") => {
                            let accepted_str = sexp
                                .get_key(":accepted")
                                .and_then(|s| s.as_atom())
                                .unwrap_or("false");
                            let from_hex = sexp
                                .get_key(":from")
                                .and_then(|s| s.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            if accepted_str == "true" {
                                self.emit_str(&format!(
                                    "[mate] {} accepted! crossover offspring created\n",
                                    from_hex
                                ));
                                self.mating_offspring
                                    .push(("child".to_string(), from_hex));
                            } else {
                                self.emit_str(&format!(
                                    "[mate] {} denied mating request\n",
                                    from_hex
                                ));
                            }
                        }
                _ => {} // other sexp types handled elsewhere
            }
        }
    }

    // -----------------------------------------------------------------------
    // Cross-machine mesh primitives
    // -----------------------------------------------------------------------

    fn prim_my_addr(&mut self) {
        if let Some(ref m) = self.mesh {
            self.emit_str(&format!("{}\n", m.my_addr()));
        } else {
            self.emit_str("mesh offline\n");
        }
    }

    fn prim_peer_table(&mut self) {
        if let Some(ref m) = self.mesh {
            let table = m.peer_table();
            if table.is_empty() {
                self.emit_str("no peers\n");
            } else {
                self.emit_str("--- peer table ---\n");
                for (id, addr, fitness, age) in &table {
                    self.emit_str(&format!(
                        "  {} @ {} fitness={} seen={}s ago\n",
                        id, addr, fitness, age
                    ));
                }
            }
        } else {
            self.emit_str("mesh offline\n");
        }
    }

    fn prim_mesh_key(&mut self) {
        if let Some(ref m) = self.mesh {
            if m.mesh_key.is_some() {
                self.emit_str("mesh-key: enabled\n");
            } else {
                self.emit_str("mesh-key: disabled (open mesh)\n");
            }
        } else {
            self.emit_str("mesh offline\n");
        }
    }

    fn prim_connect(&mut self) {
        let addr_str = self.parse_until('"');
        let addr: SocketAddr = match addr_str.trim().parse().or_else(|_| {
            use std::net::ToSocketAddrs;
            addr_str
                .trim()
                .to_socket_addrs()
                .map_err(|e| e.to_string())
                .and_then(|mut a| a.next().ok_or_else(|| "no address".into()))
        }) {
            Ok(a) => a,
            Err(e) => {
                self.emit_str(&format!("connect: {}\n", e));
                return;
            }
        };
        if let Some(ref m) = self.mesh {
            m.connect_peer(addr);
            self.emit_str(&format!("connected to {}\n", addr));
        } else {
            self.emit_str("mesh offline\n");
        }
    }

    fn prim_disconnect(&mut self) {
        let hex_id = self.parse_until('"');
        if let Some(ref m) = self.mesh {
            if m.disconnect_peer(hex_id.trim()) {
                self.emit_str(&format!("disconnected {}\n", hex_id.trim()));
            } else {
                self.emit_str(&format!("peer {} not found\n", hex_id.trim()));
            }
        } else {
            self.emit_str("mesh offline\n");
        }
    }

    fn prim_mesh_stats(&mut self) {
        if let Some(ref m) = self.mesh {
            let (peers, port) = m.mesh_stats();
            self.emit_str(&format!(
                "--- mesh stats ---\nport: {}\npeers: {}\naddress: {}\nkey: {}\n",
                port,
                peers,
                m.my_addr(),
                if m.mesh_key.is_some() {
                    "enabled"
                } else {
                    "disabled"
                }
            ));
        } else {
            self.emit_str("mesh offline\n");
        }
    }

    // -----------------------------------------------------------------------
    // Swarm primitives
    // -----------------------------------------------------------------------

    fn prim_discover(&mut self) {
        if let Some(ref m) = self.mesh {
            m.send_discovery_beacon();
            self.emit_str("discovery beacon sent\n");
        }
    }

    fn prim_auto_discover(&mut self) {
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.auto_discover = !st.auto_discover;
            let on = st.auto_discover;
            drop(st);
            self.emit_str(&format!(
                "auto-discover: {}\n",
                if on { "ON" } else { "OFF" }
            ));
        }
    }

    fn prim_share_word(&mut self) {
        let name = self.parse_until('"');
        let upper = name.to_uppercase();
        // Find the word and reconstruct its source (simplified: use SEE-like decompilation).
        if let Some(idx) = self.find_word(&upper) {
            let _entry = &self.dictionary[idx];
            // Build a Forth source representation.
            let source = format!(": {} ;", upper); // simplified — real impl would decompile
            if let Some(ref m) = self.mesh {
                m.share_word(&upper, &source);
                self.emit_str(&format!("shared: {}\n", upper));
            }
        } else {
            self.emit_str(&format!("{}?\n", upper));
        }
    }

    fn prim_share_all(&mut self) {
        if let Some(ref m) = self.mesh {
            // Share all non-kernel words (words with more than one instruction).
            let mut count = 0;
            for entry in &self.dictionary {
                if entry.body.len() > 1 && !entry.hidden {
                    let source = format!(": {} ;", entry.name);
                    m.share_word(&entry.name, &source);
                    count += 1;
                }
            }
            self.emit_str(&format!("shared {} words\n", count));
        }
    }

    fn prim_auto_share(&mut self) {
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.auto_share = !st.auto_share;
            let on = st.auto_share;
            drop(st);
            self.emit_str(&format!("auto-share: {}\n", if on { "ON" } else { "OFF" }));
        }
    }

    fn prim_shared_words(&mut self) {
        if let Some(ref m) = self.mesh {
            let words = m.shared_words_list();
            if words.is_empty() {
                self.emit_str("  (no shared words)\n");
            } else {
                for (name, origin) in &words {
                    self.emit_str(&format!("  {} from {}\n", name, origin));
                }
            }
        }
    }

    fn prim_auto_spawn(&mut self) {
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.auto_spawn = !st.auto_spawn;
            let on = st.auto_spawn;
            drop(st);
            self.emit_str(&format!("auto-spawn: {}\n", if on { "ON" } else { "OFF" }));
        }
    }

    fn prim_auto_cull(&mut self) {
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.auto_cull = !st.auto_cull;
            let on = st.auto_cull;
            drop(st);
            self.emit_str(&format!("auto-cull: {}\n", if on { "ON" } else { "OFF" }));
        }
    }

    fn prim_min_units(&mut self) {
        let n = self.pop() as usize;
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.min_units = n.max(1);
            drop(st);
            self.emit_str(&format!("min-units: {}\n", n));
        }
    }

    fn prim_max_units(&mut self) {
        let n = self.pop() as usize;
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.max_units = n.max(1);
            drop(st);
            self.emit_str(&format!("max-units: {}\n", n));
        }
    }

    fn prim_swarm_status(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_swarm_status();
            self.emit_str(&s);
        } else {
            self.emit_str("swarm: offline\n");
        }
    }

    /// Compile shared words received from peers.
    fn process_shared_words(&mut self) {
        let words = self
            .mesh
            .as_ref()
            .map(|m| m.recv_shared_words())
            .unwrap_or_default();
        for word in words {
            // Compile the shared word source.
            self.interpret_line(&word.body_source);
        }
    }

    /// Swarm tick — process word shares and check autonomous behaviors.
    fn tick_swarm(&mut self) {
        self.process_shared_words();
    }

    // -----------------------------------------------------------------------
    // Replication consent primitives
    // -----------------------------------------------------------------------

    fn prim_trust_all_level(&mut self) {
        if let Some(ref m) = self.mesh {
            m.set_trust_level(mesh::TrustLevel::All);
            self.emit_str("trust: ALL (auto-accept everything)\n");
        }
    }

    fn prim_trust_mesh(&mut self) {
        if let Some(ref m) = self.mesh {
            m.set_trust_level(mesh::TrustLevel::Mesh);
            self.emit_str("trust: MESH (auto-accept known peers)\n");
        }
    }

    fn prim_trust_family(&mut self) {
        if let Some(ref m) = self.mesh {
            m.set_trust_level(mesh::TrustLevel::Family);
            self.emit_str("trust: FAMILY (auto-accept parent/children)\n");
        }
    }

    fn prim_trust_none_level(&mut self) {
        if let Some(ref m) = self.mesh {
            m.set_trust_level(mesh::TrustLevel::None);
            self.emit_str("trust: NONE (manual approval required)\n");
        }
    }

    fn prim_trust_level(&mut self) {
        if let Some(ref m) = self.mesh {
            let level = m.trust_level();
            self.stack.push(level.as_val());
            self.emit_str(&format!("trust: {}\n", level.label()));
        } else {
            self.stack.push(0);
        }
    }

    fn prim_requests(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_requests();
            self.emit_str(&s);
        }
    }

    fn prim_accept_req(&mut self) {
        if let Some(ref m) = self.mesh {
            if let Some((sender, rid)) = m.accept_oldest() {
                self.emit_str(&format!(
                    "accepted request #{} from {}\n",
                    rid,
                    mesh::id_to_hex(&sender)
                ));
            } else {
                self.emit_str("no pending requests\n");
            }
        }
    }

    fn prim_deny_req(&mut self) {
        if let Some(ref m) = self.mesh {
            if let Some(rid) = m.deny_oldest() {
                self.emit_str(&format!("denied request #{}\n", rid));
            } else {
                self.emit_str("no pending requests\n");
            }
        }
    }

    fn prim_deny_all_req(&mut self) {
        if let Some(ref m) = self.mesh {
            let n = m.deny_all_requests();
            self.emit_str(&format!("denied {} request(s)\n", n));
        }
    }

    fn prim_replication_log(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_replication_log();
            self.emit_str(&s);
        }
    }

    // -----------------------------------------------------------------------
    // Mesh primitives
    // -----------------------------------------------------------------------

    /// SEND ( addr n peer -- ) send n bytes from memory to all peers.
    /// The peer argument is reserved for future use (ignored, broadcast).
    fn prim_send(&mut self) {
        let _peer = self.pop(); // reserved
        let n = self.pop() as usize;
        let addr = self.pop() as usize;

        // Read n cells from memory, convert each to a byte.
        let mut data = Vec::with_capacity(n);
        for i in 0..n {
            let a = addr + i;
            if a < self.memory.len() {
                data.push(self.memory[a] as u8);
            }
        }

        if let Some(ref m) = self.mesh {
            m.send_data(&data);
        } else {
            eprintln!("SEND: mesh offline");
        }
    }

    /// RECV ( -- addr n peer ) receive next message.
    /// Copies data to PAD buffer. peer is the sender (0 = none).
    fn prim_recv(&mut self) {
        if let Some(ref m) = self.mesh {
            if let Some(msg) = m.recv_data() {
                // Copy data to PAD area in memory.
                let len = msg.data.len().min(self.memory.len() - PAD);
                for (i, &byte) in msg.data.iter().take(len).enumerate() {
                    self.memory[PAD + i] = byte as Cell;
                }
                self.stack.push(PAD as Cell);
                self.stack.push(len as Cell);
                // Push a nonzero "peer" value to indicate a message was received.
                self.stack.push(-1);
                return;
            }
        }
        // No message or mesh offline.
        self.stack.push(0);
        self.stack.push(0);
        self.stack.push(0);
    }

    /// PEERS ( -- n ) number of known peers.
    fn prim_peers(&mut self) {
        let count = self.mesh.as_ref().map_or(0, |m| m.peer_count());
        self.stack.push(count as Cell);
    }

    /// REPLICATE ( -- ) serialize this unit's state and broadcast to peers.
    fn prim_replicate(&mut self) {
        if let Some(ref m) = self.mesh {
            // Update load metric before serializing.
            let user_words = self.dictionary.len();
            m.set_load(user_words as u32);

            let goals = m.clone_goals();
            let state_bytes =
                mesh::serialize_state(&self.dictionary, &self.memory, self.here, Some(&goals));
            println!(
                "REPLICATE: serialized {} bytes ({} dictionary entries, {} memory cells)",
                state_bytes.len(),
                self.dictionary.len(),
                self.here
            );
            m.send_data(&state_bytes);
        } else {
            eprintln!("REPLICATE: mesh offline");
        }
    }

    /// MUTATE ( xt -- ) replace a word's definition at runtime.
    /// Stub: prints info about what would happen.
    fn prim_mutate(&mut self) {
        let xt = self.pop() as usize;
        if xt < self.dictionary.len() {
            let name = &self.dictionary[xt].name;
            eprintln!(
                "MUTATE: would replace definition of {} (xt={}). Not yet implemented.",
                name, xt
            );
        } else {
            eprintln!("MUTATE: invalid xt {}", xt);
        }
    }

    /// MESH-STATUS ( -- ) print mesh state.
    fn prim_mesh_status(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_status();
            self.emit_str(&s);
        } else {
            self.emit_str("mesh: offline\n");
        }
    }

    /// PROPOSE ( -- ) trigger a replication proposal via consensus.
    fn prim_propose(&mut self) {
        if let Some(ref m) = self.mesh {
            // Update load metric.
            let user_words = self.dictionary.len();
            m.set_load(user_words as u32);

            // Serialize state for the proposal.
            let goals = m.clone_goals();
            let state_bytes =
                mesh::serialize_state(&self.dictionary, &self.memory, self.here, Some(&goals));
            let reason = format!("load={} dict_size={}", user_words, self.dictionary.len());

            match m.propose_replicate(&reason, state_bytes) {
                Ok(()) => println!("PROPOSE: proposal submitted to mesh"),
                Err(e) => eprintln!("PROPOSE: {}", e),
            }
        } else {
            eprintln!("PROPOSE: mesh offline");
        }
    }

    /// LOAD ( -- n ) push current load metric.
    fn prim_mesh_load(&mut self) {
        let load = self.mesh.as_ref().map_or(0, |m| m.load());
        self.stack.push(load as Cell);
    }

    /// CAPACITY ( -- n ) push capacity threshold.
    fn prim_mesh_capacity(&mut self) {
        let cap = self.mesh.as_ref().map_or(0, |m| m.capacity());
        self.stack.push(cap as Cell);
    }

    /// ID ( -- addr n ) push this unit's ID string to PAD and return addr+len.
    fn prim_id(&mut self) {
        let id_str = self
            .mesh
            .as_ref()
            .map_or_else(|| "offline".to_string(), |m| m.id_hex().to_string());

        // Write to PAD area.
        let len = id_str.len().min(self.memory.len() - PAD);
        for (i, byte) in id_str.bytes().take(len).enumerate() {
            self.memory[PAD + i] = byte as Cell;
        }
        self.stack.push(PAD as Cell);
        self.stack.push(len as Cell);
    }

    /// TYPE ( addr n -- ) print n characters from memory starting at addr.
    fn prim_type(&mut self) {
        let n = self.pop() as usize;
        let addr = self.pop() as usize;
        for i in 0..n {
            let a = addr + i;
            if a < self.memory.len() {
                self.emit_char(self.memory[a] as u8 as char);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Sandbox execution engine
    // -----------------------------------------------------------------------

    /// Parse balanced braces from the input buffer. Returns the content
    /// between the opening { (already consumed) and the closing }.
    fn parse_balanced_braces(&mut self) -> String {
        let bytes = self.input_buffer.as_bytes();
        if self.input_pos < bytes.len() && bytes[self.input_pos] == b' ' {
            self.input_pos += 1;
        }
        let start = self.input_pos;
        let mut depth = 1i32;
        while self.input_pos < bytes.len() && depth > 0 {
            match bytes[self.input_pos] as char {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let result = self.input_buffer[start..self.input_pos].to_string();
                        self.input_pos += 1;
                        return result;
                    }
                }
                _ => {}
            }
            self.input_pos += 1;
        }
        self.input_buffer[start..self.input_pos].to_string()
    }

    /// Execute Forth code in a sandbox. Saves/restores VM state. Returns
    /// a TaskResult with the captured stack, output, and success status.
    fn execute_sandbox(&mut self, code: &str) -> goals::TaskResult {
        // Save state.
        let saved_stack = std::mem::take(&mut self.stack);
        let saved_rstack = std::mem::take(&mut self.rstack);
        let saved_silent = self.silent;
        let saved_compiling = self.compiling;
        let saved_current_def = self.current_def.take();
        let saved_output_buffer = self.output_buffer.take();
        let saved_deadline = self.deadline.take();
        let saved_timed_out = self.timed_out;
        let saved_sandbox = self.sandbox_active;

        // Set up sandbox.
        self.stack = Vec::with_capacity(256);
        self.rstack = Vec::with_capacity(256);
        self.output_buffer = Some(String::new());
        self.silent = true;
        self.sandbox_active = true; // remote code always sandboxed
        self.compiling = false;
        self.timed_out = false;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.deadline = Some(Instant::now() + Duration::from_secs(self.execution_timeout));
        }

        // Execute.
        for line in code.lines() {
            self.interpret_line(line);
            if self.timed_out || !self.running {
                break;
            }
        }

        // Capture results.
        let stack_snapshot = self.stack.clone();
        let output = self.output_buffer.take().unwrap_or_default();
        let success = !self.timed_out;
        let error = if self.timed_out {
            Some(format!("execution timeout ({}s)", self.execution_timeout))
        } else {
            None
        };

        // Restore state.
        self.stack = saved_stack;
        self.rstack = saved_rstack;
        self.silent = saved_silent;
        self.compiling = saved_compiling;
        self.current_def = saved_current_def;
        self.output_buffer = saved_output_buffer;
        self.deadline = saved_deadline;
        self.timed_out = saved_timed_out;
        self.sandbox_active = saved_sandbox;
        self.running = true; // task execution must not kill the unit

        goals::TaskResult {
            stack_snapshot,
            output,
            success,
            error,
        }
    }

    // -----------------------------------------------------------------------
    // Goal primitives
    // -----------------------------------------------------------------------

    /// GOAL" `<description>`" ( priority -- goal-id ) submit a description-only goal.
    fn prim_goal(&mut self) {
        let desc = self.parse_until('"');
        let priority = self.pop();
        if let Some(ref m) = self.mesh {
            let goal_id = m.create_goal(&desc, priority, None);
            m.set_load(self.dictionary.len() as u32);
            self.stack.push(goal_id as Cell);
            if !self.silent {
                println!("goal #{} created", goal_id);
            }
        } else {
            eprintln!("GOAL: mesh offline");
            self.stack.push(0);
        }
    }

    /// GOALS ( -- ) list all known goals.
    fn prim_goals(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_goals();
            self.emit_str(&s);
        } else {
            self.emit_str("  (mesh offline)\n");
        }
    }

    /// TASKS ( -- ) list this unit's current task queue.
    fn prim_tasks(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_tasks();
            self.emit_str(&s);
        } else {
            self.emit_str("  (mesh offline)\n");
        }
    }

    /// TASK-STATUS ( goal-id -- ) show task breakdown for a specific goal.
    fn prim_task_status(&mut self) {
        let goal_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            print!("{}", m.format_goal_tasks(goal_id));
            let _ = io::stdout().flush();
        } else {
            eprintln!("TASK-STATUS: mesh offline");
        }
    }

    /// CANCEL ( goal-id -- ) cancel a goal and all its tasks.
    fn prim_cancel(&mut self) {
        let goal_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            if m.cancel_goal(goal_id) {
                println!("goal #{} cancelled", goal_id);
            } else {
                eprintln!("goal #{} not found", goal_id);
            }
        } else {
            eprintln!("CANCEL: mesh offline");
        }
    }

    /// STEER ( goal-id priority -- ) change priority of a goal.
    fn prim_steer(&mut self) {
        let priority = self.pop();
        let goal_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            if m.steer_goal(goal_id, priority) {
                println!("goal #{} priority -> {}", goal_id, priority);
            } else {
                eprintln!("goal #{} not found", goal_id);
            }
        } else {
            eprintln!("STEER: mesh offline");
        }
    }

    /// REPORT ( -- ) mesh-wide progress summary.
    fn prim_report(&mut self) {
        if let Some(ref m) = self.mesh {
            print!("{}", m.format_report());
            let _ = io::stdout().flush();
        } else {
            println!("  (mesh offline)");
        }
    }

    /// CLAIM ( -- task-id ) claim the next available task, or 0 if none.
    /// CLAIM ( -- task-id ) claim and execute the next available task.
    fn prim_claim(&mut self) {
        // Extract claimed task info (releases mesh borrow).
        let claimed = self.mesh.as_ref().and_then(|m| m.claim_task());

        if let Some((task_id, goal_id, desc)) = claimed {
            println!("claimed task #{} (goal #{}): {}", task_id, goal_id, desc);
            // Check if the parent goal has executable code.
            let code = self.mesh.as_ref().and_then(|m| m.goal_code(goal_id));
            if let Some(code) = code {
                let result = self.execute_sandbox(&code);
                if !result.output.is_empty() {
                    println!("  output: {}", result.output.trim_end());
                }
                if !result.stack_snapshot.is_empty() {
                    print!("  stack: ");
                    for v in &result.stack_snapshot {
                        print!("{} ", v);
                    }
                    println!();
                }
                if !result.success {
                    println!("  FAILED: {}", result.error.as_deref().unwrap_or("unknown"));
                }
                if let Some(ref m) = self.mesh {
                    m.complete_task_with_result(task_id, result);
                }
            }
            self.stack.push(task_id as Cell);
        } else {
            println!("no tasks available");
            self.stack.push(0);
        }
    }

    /// COMPLETE ( task-id -- ) mark a task as done.
    fn prim_complete(&mut self) {
        let task_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            m.complete_task_with_result(
                task_id,
                goals::TaskResult {
                    stack_snapshot: vec![],
                    output: String::new(),
                    success: true,
                    error: None,
                },
            );
            println!("task #{} completed", task_id);
        } else {
            eprintln!("COMPLETE: mesh offline");
        }
    }

    /// GOAL{ `<forth code>` } ( priority -- goal-id ) submit an executable goal.
    /// Immediate: parses the code at compile time. In compile mode, stores
    /// the code in a side table and compiles Literal(index) + Primitive(RT).
    fn prim_goal_exec(&mut self) {
        let code = self.parse_balanced_braces();
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(code);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(P_GOAL_EXEC_RT));
            }
        } else {
            self.create_exec_goal(&code);
        }
    }

    /// Runtime primitive for compiled GOAL{. Pops code-string index from
    /// stack, looks up the code, then creates the goal.
    fn rt_goal_exec(&mut self) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let code = self.code_strings[idx].clone();
            self.create_exec_goal(&code);
        } else {
            eprintln!("GOAL{{: invalid code index");
            self.stack.push(0);
        }
    }

    fn create_exec_goal(&mut self, code: &str) {
        let priority = self.pop();

        // Check for SPLIT directive in the code.
        if let Some(split_pos) = code.find(" SPLIT ") {
            let before = &code[..split_pos];
            let after = &code[split_pos + 7..]; // skip " SPLIT "
                                                // Evaluate the "before" part to get total and N from the stack.
            let saved = self.stack.clone();
            self.interpret_line(before);
            let n = self.pop();
            let total = self.pop();
            self.stack = saved;

            if n > 0 && total > 0 {
                if let Some(ref m) = self.mesh {
                    let mut st = m.state_lock();
                    let goal_id =
                        st.goals
                            .create_split_goal(total, n, after, priority, m.id_bytes());
                    drop(st);
                    m.set_load(self.dictionary.len() as u32);
                    self.stack.push(goal_id as Cell);
                    if !self.silent {
                        println!(
                            "goal #{} created [split {}×{}]: {}",
                            goal_id,
                            n,
                            total / n,
                            after.chars().take(40).collect::<String>()
                        );
                    }
                    return;
                }
            }
        }

        // Normal (non-SPLIT) goal creation.
        if let Some(ref m) = self.mesh {
            let goal_id = m.create_goal(code, priority, Some(code.to_string()));
            m.set_load(self.dictionary.len() as u32);
            self.stack.push(goal_id as Cell);
            if !self.silent {
                println!(
                    "goal #{} created [exec]: {}",
                    goal_id,
                    code.chars().take(60).collect::<String>()
                );
            }
        } else {
            eprintln!("GOAL: mesh offline");
            self.stack.push(0);
        }
    }

    /// EVAL" `<forth code>`" ( -- ) evaluate a string of Forth immediately.
    fn prim_eval(&mut self) {
        let code = self.parse_until('"');
        self.interpret_line(&code);
    }

    /// RESULT ( task-id -- ) display the result of a completed task.
    fn prim_result(&mut self) {
        let task_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            let s = m.format_task_result(task_id);
            self.emit_str(&s);
        } else {
            eprintln!("RESULT: mesh offline");
        }
    }

    /// AUTO-CLAIM ( -- ) toggle automatic task claiming and execution.
    fn prim_auto_claim(&mut self) {
        self.auto_claim = !self.auto_claim;
        if !self.silent {
            println!("auto-claim: {}", if self.auto_claim { "ON" } else { "OFF" });
        }
    }

    /// TIMEOUT ( seconds -- ) set execution timeout for sandboxed tasks.
    fn prim_timeout(&mut self) {
        let secs = self.pop();
        if secs > 0 {
            self.execution_timeout = secs as u64;
            if !self.silent {
                println!("execution timeout: {}s", self.execution_timeout);
            }
        } else {
            eprintln!("TIMEOUT: must be > 0");
        }
    }

    /// GOAL-RESULT ( goal-id -- ) show combined results from all tasks of a goal.
    fn prim_goal_result(&mut self) {
        let goal_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            let s = m.format_goal_result(goal_id);
            self.emit_str(&s);
        } else {
            eprintln!("GOAL-RESULT: mesh offline");
        }
    }

    /// Check for and execute auto-claimed tasks.
    fn check_auto_claim(&mut self) {
        if !self.auto_claim {
            return;
        }
        // Extract the claimed task info while borrowing mesh immutably.
        let claimed = self.mesh.as_ref().and_then(|m| m.claim_executable_task());

        if let Some((task_id, goal_id, desc, code)) = claimed {
            println!(
                "[auto] claimed task #{} (goal #{}): {}",
                task_id,
                goal_id,
                desc.chars().take(50).collect::<String>()
            );
            // Execute in sandbox with timing.
            #[cfg(not(target_arch = "wasm32"))]
            let start = Instant::now();
            let result = self.execute_sandbox(&code);
            #[cfg(not(target_arch = "wasm32"))]
            let elapsed_ms = start.elapsed().as_millis() as u64;
            #[cfg(target_arch = "wasm32")]
            let elapsed_ms: u64 = 0;
            let success = result.success;

            // Record fitness and energy.
            if success {
                self.fitness.record_success(elapsed_ms);
                self.energy.earn(energy::TASK_REWARD, "task");
            } else {
                self.fitness.record_failure();
            }
            if !result.output.is_empty() {
                println!("[auto] output: {}", result.output.trim_end());
            }
            if !result.stack_snapshot.is_empty() {
                print!("[auto] stack: ");
                for v in &result.stack_snapshot {
                    print!("{} ", v);
                }
                println!();
            }
            if !success {
                println!(
                    "[auto] FAILED: {}",
                    result.error.as_deref().unwrap_or("unknown")
                );
            }
            // Now borrow mesh again to broadcast result.
            if let Some(ref m) = self.mesh {
                m.complete_task_with_result(task_id, result);
                m.set_fitness(self.fitness.score);
            }
            self.check_auto_save();
            println!("[auto] task #{} done", task_id);
        }
    }

    /// Check if auto-replication should be triggered by goal load.
    fn check_auto_replicate(&mut self) {
        let should = self
            .mesh
            .as_ref()
            .is_some_and(|m| m.should_auto_replicate());
        if should {
            if let Some(ref m) = self.mesh {
                m.clear_auto_replicate();
                m.set_load(self.dictionary.len() as u32);
                let goals = m.clone_goals();
                let state_bytes =
                    mesh::serialize_state(&self.dictionary, &self.memory, self.here, Some(&goals));
                let reason = format!("auto: goal_load dict={}", self.dictionary.len());
                match m.propose_replicate(&reason, state_bytes) {
                    Ok(()) => println!("auto-replication proposed"),
                    Err(e) => {
                        if !self.silent {
                            eprintln!("auto-replicate: {}", e);
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Host I/O primitives
    // -----------------------------------------------------------------------

    fn log_io(&mut self, msg: &str) {
        self.io_log.push_back(msg.to_string());
        if self.io_log.len() > 50 {
            self.io_log.pop_front();
        }
    }

    fn check_sandbox_write(&self, op: &str) -> bool {
        if self.sandbox_active {
            eprintln!("{}: blocked by sandbox", op);
            false
        } else {
            true
        }
    }

    fn check_shell_allowed(&self) -> bool {
        if self.sandbox_active {
            eprintln!("SHELL: blocked by sandbox");
            return false;
        }
        if !self.shell_enabled {
            eprintln!("SHELL: disabled (use SHELL-ENABLE from REPL)");
            return false;
        }
        true
    }

    /// Common handler for all immediate I/O words. Parses the string,
    /// and in compile mode stores it for runtime dispatch.
    fn io_immediate(&mut self, op: Cell) {
        let s = self.parse_until('"');
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(s);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Literal(op));
                def.body.push(Instruction::Primitive(P_IO_RT));
            }
        } else {
            self.execute_io(op, &s);
        }
    }

    /// Runtime dispatch for compiled I/O words.
    fn rt_io(&mut self) {
        let op = self.pop();
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let s = self.code_strings[idx].clone();
            self.execute_io(op, &s);
        }
    }

    fn execute_io(&mut self, op: Cell, s: &str) {
        match op {
            0 => self.do_file_read(s),
            1 => self.do_file_write(s),
            2 => self.do_file_exists(s),
            3 => self.do_file_list(s),
            4 => self.do_file_delete(s),
            5 => self.do_http_get(s),
            6 => self.do_http_post(s),
            7 => self.do_shell(s),
            8 => self.do_env(s),
            _ => {}
        }
    }

    fn do_file_read(&mut self, path: &str) {
        self.log_io(&format!("FILE-READ {}", path));
        match io_words::file_read(path) {
            Ok(data) => {
                let len = data.len().min(self.memory.len() - PAD);
                for (i, &byte) in data.iter().take(len).enumerate() {
                    self.memory[PAD + i] = byte as Cell;
                }
                self.stack.push(PAD as Cell);
                self.stack.push(len as Cell);
            }
            Err(e) => {
                if !self.silent {
                    eprintln!("FILE-READ: {}", e);
                }
                self.stack.push(0);
                self.stack.push(0);
            }
        }
    }

    fn do_file_write(&mut self, path: &str) {
        if !self.check_sandbox_write("FILE-WRITE") {
            return;
        }
        let n = self.pop() as usize;
        let addr = self.pop() as usize;
        let mut data = Vec::with_capacity(n);
        for i in 0..n {
            if addr + i < self.memory.len() {
                data.push(self.memory[addr + i] as u8);
            }
        }
        self.log_io(&format!("FILE-WRITE {} ({} bytes)", path, n));
        if let Err(e) = io_words::file_write(path, &data) {
            if !self.silent {
                eprintln!("FILE-WRITE: {}", e);
            }
        }
    }

    fn do_file_exists(&mut self, path: &str) {
        self.log_io(&format!("FILE-EXISTS {}", path));
        let flag = if io_words::file_exists(path) { -1 } else { 0 };
        self.stack.push(flag);
    }

    fn do_file_list(&mut self, path: &str) {
        self.log_io(&format!("FILE-LIST {}", path));
        match io_words::file_list(path) {
            Ok(names) => {
                for name in &names {
                    self.emit_str(&format!("  {}\n", name));
                }
            }
            Err(e) => {
                if !self.silent {
                    eprintln!("FILE-LIST: {}", e);
                }
            }
        }
    }

    fn do_file_delete(&mut self, path: &str) {
        if !self.check_sandbox_write("FILE-DELETE") {
            self.stack.push(0);
            return;
        }
        self.log_io(&format!("FILE-DELETE {}", path));
        let flag = if io_words::file_delete(path).is_ok() {
            -1
        } else {
            0
        };
        self.stack.push(flag);
    }

    fn do_http_get(&mut self, url: &str) {
        self.log_io(&format!("HTTP-GET {}", url));
        match io_words::http_get(url) {
            Ok((body, status)) => {
                let len = body.len().min(self.memory.len() - PAD);
                for (i, &byte) in body.iter().take(len).enumerate() {
                    self.memory[PAD + i] = byte as Cell;
                }
                self.stack.push(PAD as Cell);
                self.stack.push(len as Cell);
                self.stack.push(status as Cell);
            }
            Err(e) => {
                if !self.silent {
                    eprintln!("HTTP-GET: {}", e);
                }
                self.stack.push(0);
                self.stack.push(0);
                self.stack.push(0);
            }
        }
    }

    fn do_http_post(&mut self, url: &str) {
        if !self.check_sandbox_write("HTTP-POST") {
            self.stack.push(0);
            self.stack.push(0);
            self.stack.push(0);
            return;
        }
        let n = self.pop() as usize;
        let addr = self.pop() as usize;
        let mut body = Vec::with_capacity(n);
        for i in 0..n {
            if addr + i < self.memory.len() {
                body.push(self.memory[addr + i] as u8);
            }
        }
        self.log_io(&format!("HTTP-POST {} ({} bytes)", url, n));
        match io_words::http_post(url, &body) {
            Ok((resp, status)) => {
                let len = resp.len().min(self.memory.len() - PAD);
                for (i, &byte) in resp.iter().take(len).enumerate() {
                    self.memory[PAD + i] = byte as Cell;
                }
                self.stack.push(PAD as Cell);
                self.stack.push(len as Cell);
                self.stack.push(status as Cell);
            }
            Err(e) => {
                if !self.silent {
                    eprintln!("HTTP-POST: {}", e);
                }
                self.stack.push(0);
                self.stack.push(0);
                self.stack.push(0);
            }
        }
    }

    fn do_shell(&mut self, cmd: &str) {
        if !self.check_shell_allowed() {
            self.stack.push(0);
            self.stack.push(0);
            self.stack.push(-1);
            return;
        }
        self.log_io(&format!("SHELL {}", cmd));
        match io_words::shell_exec(cmd) {
            Ok((stdout, exit_code)) => {
                let len = stdout.len().min(self.memory.len() - PAD);
                for (i, &byte) in stdout.iter().take(len).enumerate() {
                    self.memory[PAD + i] = byte as Cell;
                }
                self.stack.push(PAD as Cell);
                self.stack.push(len as Cell);
                self.stack.push(exit_code as Cell);
            }
            Err(e) => {
                if !self.silent {
                    eprintln!("SHELL: {}", e);
                }
                self.stack.push(0);
                self.stack.push(0);
                self.stack.push(-1);
            }
        }
    }

    fn do_env(&mut self, name: &str) {
        self.log_io(&format!("ENV {}", name));
        if let Some(val) = io_words::env_var(name) {
            let len = val.len().min(self.memory.len() - PAD);
            for (i, byte) in val.bytes().take(len).enumerate() {
                self.memory[PAD + i] = byte as Cell;
            }
            self.stack.push(PAD as Cell);
            self.stack.push(len as Cell);
        } else {
            self.stack.push(0);
            self.stack.push(0);
        }
    }

    fn prim_timestamp(&mut self) {
        self.stack.push(io_words::timestamp());
    }

    fn prim_sleep(&mut self) {
        let ms = self.pop();
        #[cfg(target_arch = "wasm32")]
        {
            let _ = ms;
            self.emit_str("sleep not available in browser\n");
        }
        #[cfg(not(target_arch = "wasm32"))]
        if ms > 0 {
            std::thread::sleep(Duration::from_millis(ms as u64));
        }
    }

    fn prim_io_log(&mut self) {
        if self.io_log.is_empty() {
            self.emit_str("  (no I/O operations logged)\n");
        } else {
            self.emit_str("--- I/O log ---\n");
            let entries: Vec<String> = self.io_log.iter().cloned().collect();
            for entry in &entries {
                self.emit_str(&format!("  {}\n", entry));
            }
            self.emit_str("---\n");
        }
    }

    // -----------------------------------------------------------------------
    // Mutation primitives
    // -----------------------------------------------------------------------

    fn prim_mutate_rand(&mut self) {
        // Pick a random mutable word.
        let mutable_indices: Vec<usize> = self
            .dictionary
            .iter()
            .enumerate()
            .filter(|(_, e)| mutation::is_mutable(e))
            .map(|(i, _)| i)
            .collect();
        if mutable_indices.is_empty() {
            self.emit_str("no mutable words\n");
            return;
        }
        let idx = mutable_indices[self.rng.next_usize(mutable_indices.len())];
        let dict_len = self.dictionary.len();
        if let Some(mut record) =
            mutation::mutate_entry(&mut self.dictionary[idx], &mut self.rng, dict_len)
        {
            record.word_index = idx;
            self.emit_str(&format!("mutated: {}\n", record.format()));
            self.mutation_history.push(record);
        } else {
            self.emit_str("mutation failed (no applicable strategy)\n");
        }
    }

    fn prim_mutate_word(&mut self) {
        let name = self.parse_until('"');
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(name);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(P_MUTATE_WORD_RT));
            }
        } else {
            self.do_mutate_word(&name);
        }
    }

    fn rt_mutate_word(&mut self) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let name = self.code_strings[idx].clone();
            self.do_mutate_word(&name);
        }
    }

    fn do_mutate_word(&mut self, name: &str) {
        let upper = name.to_uppercase();
        if let Some(idx) = self.find_word(&upper) {
            if !mutation::is_mutable(&self.dictionary[idx]) {
                self.emit_str(&format!("{}: not mutable (kernel word)\n", upper));
                return;
            }
            let dict_len = self.dictionary.len();
            if let Some(mut record) =
                mutation::mutate_entry(&mut self.dictionary[idx], &mut self.rng, dict_len)
            {
                record.word_index = idx;
                self.emit_str(&format!("mutated: {}\n", record.format()));
                self.mutation_history.push(record);
            } else {
                self.emit_str("mutation failed\n");
            }
        } else {
            self.emit_str(&format!("{}?\n", upper));
        }
    }

    fn prim_undo_mutate(&mut self) {
        if let Some(record) = self.mutation_history.pop() {
            if record.word_index < self.dictionary.len() {
                mutation::undo_mutation(&mut self.dictionary[record.word_index], &record);
                self.emit_str(&format!(
                    "undone: {} [{}]\n",
                    record.word_name,
                    record.strategy.label()
                ));
            }
        } else {
            self.emit_str("nothing to undo\n");
        }
    }

    fn prim_mutations(&mut self) {
        if self.mutation_history.is_empty() {
            self.emit_str("  (no mutations)\n");
        } else {
            let lines: Vec<String> = self.mutation_history.iter().map(|r| r.format()).collect();
            for line in &lines {
                self.emit_str(&format!("{}\n", line));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Fitness / Evolution primitives
    // -----------------------------------------------------------------------

    fn prim_leaderboard(&mut self) {
        if let Some(ref m) = self.mesh {
            let peer_fitness = m.peer_fitness_list();
            let s = fitness::format_leaderboard(&m.id_bytes(), self.fitness.score, &peer_fitness);
            self.emit_str(&s);
        } else {
            self.emit_str(&format!("  (offline) score={}\n", self.fitness.score));
        }
    }

    fn prim_rate(&mut self) {
        let score = self.pop();
        let _task_id = self.pop() as u64;
        // For now, rating adjusts local fitness (the rated peer would
        // receive the rating via gossip in a fuller implementation).
        self.fitness.record_rating(score);
        self.emit_str(&format!("rated: fitness adjusted by {}\n", score));
    }

    fn prim_evolve(&mut self) {
        self.do_evolve();
    }

    fn prim_auto_evolve(&mut self) {
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

    fn prim_benchmark(&mut self) {
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

    fn rt_benchmark(&mut self) {
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

    fn prim_trust(&mut self) {
        // Expect a node ID on the stack (as a number).
        let id_val = self.pop() as u64;
        let id_bytes = id_val.to_be_bytes();
        self.trusted_peers.insert(id_bytes);
        self.emit_str(&format!("trusted: {:016x}\n", id_val));
    }

    /// Run one evolution cycle.
    fn do_evolve(&mut self) {
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
    fn run_benchmark(&mut self) -> i64 {
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

    fn check_auto_evolve(&mut self) {
        if self.fitness.should_auto_evolve() {
            self.do_evolve();
        }
    }

    // -----------------------------------------------------------------------
    // WebSocket bridge primitives
    // -----------------------------------------------------------------------

    fn prim_ws_status(&mut self) {
        if let Some(ref st) = self.ws_state {
            let s = st.lock().unwrap().format_status();
            self.emit_str(&s);
        } else {
            self.emit_str("ws-bridge: not running\n");
        }
    }

    fn prim_ws_clients(&mut self) {
        if let Some(ref st) = self.ws_state {
            let s = st.lock().unwrap().format_clients();
            self.emit_str(&s);
        } else {
            self.emit_str("  (ws-bridge not running)\n");
        }
    }

    fn prim_ws_broadcast(&mut self) {
        let msg = self.parse_until('"');
        // The broadcast happens by updating the mesh_json which gets
        // pushed to all connected browsers on the next 2s tick.
        if let Ok(mut json) = self.ws_mesh_json.lock() {
            *json = format!(
                r#"{{"type":"broadcast","message":"{}"}}"#,
                msg.replace('"', "\\\"")
            );
        }
        self.emit_str(&format!("ws broadcast: {}\n", msg));
    }

    fn update_ws_mesh_json(&mut self) {
        let id_hex = self
            .node_id_cache
            .map(|id| mesh::id_to_hex(&id))
            .unwrap_or_default();
        let peer_details = self
            .mesh
            .as_ref()
            .map(|m| m.peer_details())
            .unwrap_or_default();
        let goal_stats = self
            .mesh
            .as_ref()
            .map(|m| m.goal_stats())
            .unwrap_or((0, 0, 0, 0));
        let recent = self
            .mesh
            .as_ref()
            .map(|m| m.drain_recent_events())
            .unwrap_or_default();
        let children: Vec<(String, u32)> = self
            .spawn_state
            .children
            .iter()
            .map(|c| (mesh::id_to_hex(&c.node_id), self.spawn_state.generation + 1))
            .collect();
        let json = ws_bridge::build_mesh_json(
            &id_hex,
            self.fitness.score,
            self.spawn_state.generation,
            &peer_details,
            goal_stats,
            &recent,
            &children,
            self.monitor.watches.len(),
            self.monitor.alerts.len(),
        );
        if let Ok(mut j) = self.ws_mesh_json.lock() {
            *j = json;
        }
    }

    fn poll_ws_events(&mut self) {
        // Process incoming WS events (goal submissions from browsers).
        let events: Vec<ws_bridge::WsEvent> = self
            .ws_events
            .as_ref()
            .map(|rx| {
                let mut evts = Vec::new();
                while let Ok(e) = rx.try_recv() {
                    evts.push(e);
                }
                evts
            })
            .unwrap_or_default();

        for event in events {
            match event {
                ws_bridge::WsEvent::GoalSubmit { code, priority } => {
                    if let Some(ref m) = self.mesh {
                        let gid = m.create_goal(&code, priority, Some(code.clone()));
                        println!(
                            "[ws] goal #{} from browser: {}",
                            gid,
                            code.chars().take(40).collect::<String>()
                        );
                    }
                }
                ws_bridge::WsEvent::ClientConnected { id } => {
                    println!("[ws] browser connected: {}", id);
                }
                ws_bridge::WsEvent::ClientDisconnected { id } => {
                    println!("[ws] browser disconnected: {}", id);
                }
                ws_bridge::WsEvent::Heartbeat { .. } => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Monitoring & Ops primitives
    // -----------------------------------------------------------------------

    fn prim_watch(&mut self, kind: i32) {
        let target = self.parse_until('"');
        let rt_prim = match kind {
            0 => P_WATCH_URL_RT,
            1 => P_WATCH_FILE_RT,
            2 => P_WATCH_PROC_RT,
            _ => P_WATCH_URL_RT,
        };
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(target);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(rt_prim));
            }
        } else {
            self.do_add_watch(kind, &target);
        }
    }

    fn rt_watch(&mut self, kind: i32) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let target = self.code_strings[idx].clone();
            self.do_add_watch(kind, &target);
        }
    }

    fn do_add_watch(&mut self, kind: i32, target: &str) {
        let interval = self.pop() as u64;
        let wk = match kind {
            0 => monitor::WatchKind::Url(target.to_string()),
            1 => monitor::WatchKind::File(target.to_string()),
            2 => monitor::WatchKind::Process(target.to_string()),
            _ => return,
        };
        let id = self.monitor.add_watch(wk, interval.max(1));
        self.stack.push(id as Cell);
        self.emit_str(&format!("watch #{} created (every {}s)\n", id, interval));
    }

    fn prim_on_alert(&mut self) {
        let code = self.parse_until('"');
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(code);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Primitive(P_ON_ALERT_RT));
                def.body.push(Instruction::Literal(idx as Cell));
            }
        } else {
            let watch_id = self.pop() as u32;
            self.monitor.set_alert_handler(watch_id, code);
            self.emit_str(&format!("alert handler set for watch #{}\n", watch_id));
        }
    }

    fn rt_on_alert(&mut self) {
        let idx = self.pop() as usize;
        let watch_id = self.pop() as u32;
        if idx < self.code_strings.len() {
            let code = self.code_strings[idx].clone();
            self.monitor.set_alert_handler(watch_id, code);
        }
    }

    fn prim_alert_threshold(&mut self) {
        let target = self.parse_until('"');
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(target);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(P_ALERT_THRESHOLD_RT));
            }
        } else if let Ok(watch_id) = target.trim().parse::<u32>() {
            let level = self.pop();
            self.monitor
                .set_alert_level(watch_id, monitor::AlertLevel::from_val(level));
            self.emit_str(&format!("alert threshold set for watch #{}\n", watch_id));
        }
    }

    fn rt_alert_threshold(&mut self) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            if let Ok(watch_id) = self.code_strings[idx].trim().parse::<u32>() {
                let level = self.pop();
                self.monitor
                    .set_alert_level(watch_id, monitor::AlertLevel::from_val(level));
            }
        }
    }

    fn prim_dashboard(&mut self) {
        let _t = metrics::Timer::new("dashboard.render");
        let peer_count = self.mesh.as_ref().map(|m| m.peer_count()).unwrap_or(0);
        let goal_summary = self
            .mesh
            .as_ref()
            .map(|m| m.format_goals())
            .unwrap_or_default();
        let s = self
            .monitor
            .format_dashboard(peer_count, self.fitness.score, &goal_summary);
        self.emit_str(&s);
    }

    fn prim_health(&mut self) {
        let peer_count = self.mesh.as_ref().map(|m| m.peer_count()).unwrap_or(0);
        let score = self.monitor.health_score(peer_count, self.fitness.score);
        self.stack.push(score);
    }

    fn prim_every(&mut self) {
        let interval = self.pop() as u64;
        // Consume the rest of the input line as the code to schedule.
        let remaining = self.input_buffer[self.input_pos..].trim().to_string();
        self.input_pos = self.input_buffer.len(); // consume it
        if remaining.is_empty() {
            self.emit_str("EVERY: no code to schedule\n");
            return;
        }
        let id = self
            .monitor
            .add_schedule(remaining.clone(), interval.max(1));
        self.stack.push(id as Cell);
        self.emit_str(&format!(
            "schedule #{} every {}s: {}\n",
            id,
            interval,
            remaining.chars().take(40).collect::<String>()
        ));
    }

    fn rt_every(&mut self) {
        // For compiled EVERY, not yet supported — would need code string storage.
        self.emit_str("EVERY only works at the REPL\n");
    }

    fn prim_heal(&mut self) {
        self.emit_str("--- heal cycle ---\n");
        // Check all watches.
        let due = self.monitor.due_watches();
        if due.is_empty() {
            self.emit_str("  no watches due\n");
        }
        for wid in &due {
            self.run_watch_check(*wid);
        }
        // Run handlers for active alerts.
        let handlers: Vec<(u32, String)> = self
            .monitor
            .alerts
            .iter()
            .filter(|a| !a.acknowledged)
            .filter_map(|a| {
                self.monitor
                    .watches
                    .get(&a.watch_id)
                    .and_then(|w| w.alert_handler.clone())
                    .map(|h| (a.id, h))
            })
            .collect();
        for (aid, handler) in &handlers {
            self.emit_str(&format!("  running handler for alert #{}\n", aid));
            self.interpret_line(handler);
        }
        self.emit_str("--- heal done ---\n");
    }

    /// Execute a watch check for a specific watch ID.
    fn run_watch_check(&mut self, watch_id: u32) {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = watch_id;
            return;
        } // watches require native I/O
        #[cfg(not(target_arch = "wasm32"))]
        {
            let kind = match self.monitor.watches.get(&watch_id) {
                Some(w) => w.kind.clone(),
                None => return,
            };
            let start = Instant::now();
            let status = match kind {
                monitor::WatchKind::Url(ref url) => match io_words::http_get(url) {
                    Ok((_, code)) => {
                        let ms = start.elapsed().as_millis() as u64;
                        if (200..400).contains(&code) {
                            monitor::WatchStatus::up(code as i32, ms, format!("{}", code))
                        } else {
                            monitor::WatchStatus::down(code as i32, format!("HTTP {}", code))
                        }
                    }
                    Err(e) => monitor::WatchStatus::down(-1, e),
                },
                monitor::WatchKind::File(ref path) => {
                    if io_words::file_exists(path) {
                        let ms = start.elapsed().as_millis() as u64;
                        match std::fs::metadata(path) {
                            Ok(m) => monitor::WatchStatus::up(0, ms, format!("{}b", m.len())),
                            Err(e) => monitor::WatchStatus::down(-1, e.to_string()),
                        }
                    } else {
                        monitor::WatchStatus::down(-1, "not found".into())
                    }
                }
                monitor::WatchKind::Process(ref name) => {
                    match io_words::shell_exec(&format!(
                        "pgrep -x {} >/dev/null 2>&1 && echo UP || echo DOWN",
                        name
                    )) {
                        Ok((stdout, _)) => {
                            let ms = start.elapsed().as_millis() as u64;
                            let out = String::from_utf8_lossy(&stdout).trim().to_string();
                            if out.contains("UP") {
                                monitor::WatchStatus::up(0, ms, "running".into())
                            } else {
                                monitor::WatchStatus::down(-1, "not running".into())
                            }
                        }
                        Err(e) => monitor::WatchStatus::down(-1, e),
                    }
                }
            };

            // Record the check result.
            if let Some(alert) = self.monitor.record_check(watch_id, status.clone()) {
                self.emit_str(&format!(
                    "ALERT [{}] watch #{}: {}\n",
                    alert.level.label(),
                    watch_id,
                    alert.message
                ));
                // Run alert handler if defined.
                let handler = self
                    .monitor
                    .watches
                    .get(&watch_id)
                    .and_then(|w| w.alert_handler.clone());
                if let Some(code) = handler {
                    self.interpret_line(&code);
                    // Fitness bonus for attempted remediation.
                    self.fitness.score += 15;
                }
            }
        } // end #[cfg(not(wasm32))]
    }

    /// Tick the monitor: check due watches and run due schedules.
    fn tick_monitor(&mut self) {
        // Check due watches.
        let due_watches = self.monitor.due_watches();
        for wid in due_watches {
            self.run_watch_check(wid);
        }

        // Run due schedules.
        let due_scheds = self.monitor.due_schedules();
        for (_sid, code) in due_scheds {
            self.interpret_line(&code);
        }
    }

    // -----------------------------------------------------------------------
    // Spawn / Replication primitives
    // -----------------------------------------------------------------------

    fn build_state_for_spawn(&self) -> Vec<u8> {
        let snap = self.make_snapshot();
        persist::serialize_snapshot(&snap)
    }

    fn prim_spawn(&mut self) {
        let _t = metrics::Timer::new("spawn.total");
        // Energy check.
        if !self.energy.can_afford(energy::SPAWN_COST) {
            self.emit_str(&format!(
                "insufficient energy to spawn (need {}, have {})\n",
                energy::SPAWN_COST,
                self.energy.energy
            ));
            return;
        }
        if let Err(e) = self.spawn_state.can_spawn() {
            self.emit_str(&format!("SPAWN: {}\n", e));
            return;
        }

        // Spawn economics: parent pays SPAWN_COST (200), child starts with
        // parent_remaining/3 capped at INITIAL_ENERGY (1000). Both parent
        // and child are in a more constrained metabolic state after reproduction.
        self.energy.spend(energy::SPAWN_COST, "spawn");
        let parent_energy = self.energy.energy;
        let child_energy = (parent_energy / 3).min(energy::INITIAL_ENERGY);

        // Temporarily set child's energy state for serialization.
        let saved_energy = self.energy.energy;
        let saved_earned = self.energy.total_earned;
        let saved_spent = self.energy.total_spent;
        let saved_peak = self.energy.peak_energy;
        let saved_starving = self.energy.starving_ticks;
        self.energy.energy = child_energy;
        self.energy.total_earned = 0;
        self.energy.total_spent = 0;
        self.energy.peak_energy = child_energy;
        self.energy.starving_ticks = 0;

        let state = self.build_state_for_spawn();

        // Restore parent's energy state.
        self.energy.energy = saved_energy;
        self.energy.total_earned = saved_earned;
        self.energy.total_spent = saved_spent;
        self.energy.peak_energy = saved_peak;
        self.energy.starving_ticks = saved_starving;

        let package = {
            let _t = metrics::Timer::new("spawn.build_package");
            match spawn::build_package(&state) {
                Ok(p) => p,
                Err(e) => {
                    self.emit_str(&format!("SPAWN: {}\n", e));
                    return;
                }
            }
        };
        let parent_port = self.mesh.as_ref().map(|m| m.local_port()).unwrap_or(0);
        let child_gen = self.spawn_state.generation + 1;

        let _t_fork = metrics::Timer::new("spawn.fork");
        match spawn::spawn_local_with_energy(&package, parent_port, child_gen, Some(child_energy)) {
            Ok((pid, port, child_id)) => {
                self.spawn_state.children.push(spawn::ChildInfo {
                    pid,
                    port,
                    node_id: child_id,
                    spawned_at: Instant::now(),
                });
                self.spawn_state.last_spawn = Some(Instant::now());
                self.emit_str(&format!(
                    "spawned child pid={} id={} (energy: {})\n",
                    pid,
                    mesh::id_to_hex(&child_id),
                    child_energy
                ));
            }
            Err(e) => self.emit_str(&format!("SPAWN: {}\n", e)),
        }
    }

    fn prim_spawn_n(&mut self) {
        let n = self.pop() as usize;
        for i in 0..n {
            self.prim_spawn();
            // Override cooldown for batch spawns.
            if i < n - 1 {
                self.spawn_state.last_spawn = None;
            }
        }
    }

    fn prim_package(&mut self) {
        let state = self.build_state_for_spawn();
        match spawn::build_package(&state) {
            Ok(pkg) => {
                let len = pkg.len().min(self.memory.len() - PAD);
                for (i, &byte) in pkg.iter().take(len).enumerate() {
                    self.memory[PAD + i] = byte as Cell;
                }
                self.stack.push(PAD as Cell);
                self.stack.push(len as Cell);
                self.emit_str(&format!("package: {} bytes\n", pkg.len()));
            }
            Err(e) => {
                self.emit_str(&format!("PACKAGE: {}\n", e));
                self.stack.push(0);
                self.stack.push(0);
            }
        }
    }

    fn prim_package_size(&mut self) {
        let state = self.build_state_for_spawn();
        match spawn::package_size_estimate(state.len()) {
            Ok(size) => {
                self.stack.push(size as Cell);
                self.emit_str(&format!("package size: {} bytes\n", size));
            }
            Err(e) => {
                self.emit_str(&format!("PACKAGE-SIZE: {}\n", e));
                self.stack.push(0);
            }
        }
    }

    fn prim_children(&mut self) {
        if self.spawn_state.children.is_empty() {
            self.emit_str("  (no children)\n");
        } else {
            let lines: Vec<String> = self
                .spawn_state
                .children
                .iter()
                .map(|c| {
                    format!(
                        "  pid={} id={} age={}s\n",
                        c.pid,
                        mesh::id_to_hex(&c.node_id),
                        c.spawned_at.elapsed().as_secs()
                    )
                })
                .collect();
            for line in &lines {
                self.emit_str(line);
            }
        }
    }

    fn prim_family(&mut self) {
        let self_id = self
            .node_id_cache
            .map(|id| mesh::id_to_hex(&id))
            .unwrap_or_else(|| "?".to_string());
        let parent = self
            .spawn_state
            .parent_id
            .map(|id| mesh::id_to_hex(&id))
            .unwrap_or_else(|| "none".to_string());
        self.emit_str(&format!(
            "id: {} gen: {} parent: {} children: {}\n",
            self_id,
            self.spawn_state.generation,
            parent,
            self.spawn_state.children.len(),
        ));
    }

    fn prim_kill_child(&mut self) {
        let pid = self.pop() as u32;
        #[cfg(unix)]
        {
            unsafe {
                libc_kill(pid as i32, 15); // SIGTERM
            }
        }
        self.spawn_state.children.retain(|c| c.pid != pid);
        self.emit_str(&format!("sent SIGTERM to pid {}\n", pid));
    }

    fn prim_replicate_to(&mut self) {
        let addr = self.parse_until('"');
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(addr);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(P_REPLICATE_TO));
            }
            return;
        }
        let state = self.build_state_for_spawn();
        let package = match spawn::build_package(&state) {
            Ok(p) => p,
            Err(e) => {
                self.emit_str(&format!("REPLICATE-TO: {}\n", e));
                return;
            }
        };
        match spawn::send_package(&addr, &package) {
            Ok(()) => self.emit_str(&format!("sent {} bytes to {}\n", package.len(), addr)),
            Err(e) => self.emit_str(&format!("REPLICATE-TO: {}\n", e)),
        }
    }

    /// Check for and handle incoming replication packages.
    fn check_incoming_replications(&mut self) {
        if self.spawn_state.quarantine || !self.spawn_state.accept_replicate {
            return;
        }
        let pkg = self.mesh.as_ref().and_then(|m| m.recv_replication());
        if let Some(pkg) = pkg {
            let parent_port = self.mesh.as_ref().map(|m| m.local_port()).unwrap_or(0);
            let child_gen = self.spawn_state.generation + 1;
            match spawn::spawn_local(&pkg, parent_port, child_gen) {
                Ok((pid, _, child_id)) => {
                    self.spawn_state.children.push(spawn::ChildInfo {
                        pid,
                        port: 0,
                        node_id: child_id,
                        spawned_at: Instant::now(),
                    });
                    println!(
                        "[repl] spawned child pid={} id={}",
                        pid,
                        mesh::id_to_hex(&child_id)
                    );
                }
                Err(e) => eprintln!("[repl] spawn failed: {}", e),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Identity
    // -----------------------------------------------------------------------

    /// REIDENTIFY ( -- ) generate a new node ID, migrate saved state.
    fn prim_reidentify(&mut self) {
        let old_id = self.node_id_cache;
        // Generate a new random ID.
        let mut new_id = [0u8; 8];
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            use std::io::Read;
            let _ = f.read_exact(&mut new_id);
        }
        // Migrate state directory.
        if let Some(oid) = old_id {
            let _ = persist::rename_state(&oid, &new_id);
        }
        // Save the new ID.
        let _ = persist::save_node_id(&new_id);
        self.node_id_cache = Some(new_id);
        self.rng = mutation::SimpleRng::new(u64::from_be_bytes(new_id));
        self.emit_str(&format!(
            "reidentified: {} -> {}\n",
            old_id
                .map(|id| mesh::id_to_hex(&id))
                .unwrap_or_else(|| "none".into()),
            mesh::id_to_hex(&new_id),
        ));
    }

    // -----------------------------------------------------------------------
    // Persistence primitives
    // -----------------------------------------------------------------------

    fn make_snapshot(&self) -> persist::VmSnapshot {
        let node_id = self.node_id_cache.unwrap_or([0u8; 8]);
        let goals = self
            .mesh
            .as_ref()
            .map(|m| m.clone_goals())
            .unwrap_or_else(goals::GoalRegistry::empty);
        persist::VmSnapshot {
            node_id,
            dictionary: self.dictionary.clone(),
            memory: self.memory.clone(),
            here: self.here,
            goals,
            fitness: self.fitness.clone(),
            code_strings: self.code_strings.clone(),
        }
    }

    fn prim_save(&mut self) {
        if let Some(id) = self.node_id_cache {
            let snap = self.make_snapshot();
            let data = persist::serialize_snapshot(&snap);
            match persist::save_state(&id, &data) {
                Ok(()) => self.emit_str(&format!(
                    "saved {} bytes to {}\n",
                    data.len(),
                    persist::state_dir(&id)
                )),
                Err(e) => self.emit_str(&format!("save failed: {}\n", e)),
            }
        } else {
            self.emit_str("save: no node ID (mesh offline)\n");
        }
    }

    fn prim_load_state(&mut self) {
        if let Some(id) = self.node_id_cache {
            if let Some(data) = persist::load_state(&id) {
                if let Some(snap) = persist::deserialize_snapshot(&data) {
                    self.restore_snapshot(snap);
                    self.emit_str("state restored\n");
                } else {
                    self.emit_str("load: corrupt state file\n");
                }
            } else {
                self.emit_str("load: no saved state\n");
            }
        } else {
            self.emit_str("load: no node ID\n");
        }
    }

    fn prim_auto_save(&mut self) {
        self.auto_save_enabled = !self.auto_save_enabled;
        self.emit_str(&format!(
            "auto-save: {} (every {} tasks)\n",
            if self.auto_save_enabled { "ON" } else { "OFF" },
            self.auto_save_interval
        ));
    }

    fn prim_reset(&mut self) {
        if let Some(id) = self.node_id_cache {
            let _ = persist::delete_state(&id);
        }
        let _ = persist::delete_node_id();
        self.emit_str("state and identity deleted — restart for fresh boot\n");
    }

    fn prim_snapshots(&mut self) {
        if let Some(id) = self.node_id_cache {
            let snaps = persist::list_snapshots(&id);
            if snaps.is_empty() {
                self.emit_str("  (no snapshots)\n");
            } else {
                for name in &snaps {
                    self.emit_str(&format!("  {}\n", name));
                }
            }
        }
    }

    fn prim_snapshot(&mut self) {
        if let Some(id) = self.node_id_cache {
            let snap = self.make_snapshot();
            let data = persist::serialize_snapshot(&snap);
            match persist::save_snapshot(&id, &data) {
                Ok(name) => self.emit_str(&format!("snapshot: {}\n", name)),
                Err(e) => self.emit_str(&format!("snapshot failed: {}\n", e)),
            }
        }
    }

    fn prim_restore(&mut self) {
        let snap_id = self.pop();
        if let Some(id) = self.node_id_cache {
            let name = format!("{}", snap_id);
            if let Some(data) = persist::load_snapshot(&id, &name) {
                if let Some(snap) = persist::deserialize_snapshot(&data) {
                    self.restore_snapshot(snap);
                    self.emit_str(&format!("restored snapshot {}\n", name));
                } else {
                    self.emit_str("restore: corrupt snapshot\n");
                }
            } else {
                self.emit_str(&format!("snapshot {} not found\n", name));
            }
        }
    }

    fn restore_snapshot(&mut self, snap: persist::VmSnapshot) {
        self.dictionary = snap.dictionary;
        self.memory = snap.memory;
        self.here = snap.here;
        self.fitness = snap.fitness;
        self.code_strings = snap.code_strings;
        // Restore goals into mesh state if available.
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.goals = snap.goals;
        }
    }

    fn check_auto_save(&mut self) {
        if !self.auto_save_enabled {
            return;
        }
        self.tasks_since_save += 1;
        if self.tasks_since_save >= self.auto_save_interval {
            self.tasks_since_save = 0;
            if let Some(id) = self.node_id_cache {
                let snap = self.make_snapshot();
                let data = persist::serialize_snapshot(&snap);
                let _ = persist::save_state(&id, &data);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Task decomposition primitives
    // -----------------------------------------------------------------------

    /// SUBTASK{ `<code>` } ( goal-id -- task-id ) add a subtask to a goal.
    fn prim_subtask(&mut self) {
        let code = self.parse_balanced_braces();
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(code);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(P_SUBTASK));
            }
        } else {
            let goal_id = self.pop() as u64;
            let result = self.mesh.as_ref().and_then(|m| {
                let mut st = m.state_lock();
                st.goals
                    .create_subtask(goal_id, code.clone(), Some(code.clone()))
            });
            if let Some(tid) = result {
                self.emit_str(&format!("subtask #{} added to goal #{}\n", tid, goal_id));
                self.stack.push(tid as Cell);
            } else {
                self.emit_str(&format!("goal #{} not found\n", goal_id));
                self.stack.push(0);
            }
        }
    }

    /// FORK ( goal-id n -- ) split an existing goal into n tasks.
    fn prim_fork(&mut self) {
        let n = self.pop() as usize;
        let goal_id = self.pop() as u64;
        let ok = self.mesh.as_ref().is_some_and(|m| {
            let mut st = m.state_lock();
            st.goals.fork_goal(goal_id, n)
        });
        if ok {
            self.emit_str(&format!("goal #{} forked into {} tasks\n", goal_id, n));
        } else {
            self.emit_str(&format!(
                "fork failed: goal #{} not found or no code\n",
                goal_id
            ));
        }
    }

    /// RESULTS ( goal-id -- ) show all subtask results.
    fn prim_results(&mut self) {
        let goal_id = self.pop() as u64;
        let out = if let Some(ref m) = self.mesh {
            let st = m.state_lock();
            let results = st.goals.collect_results(goal_id);
            if results.is_empty() {
                format!("goal #{}: no results\n", goal_id)
            } else {
                let mut s = format!("goal #{}: {} results\n", goal_id, results.len());
                for (tid, result) in &results {
                    s.push_str(&format!("  task #{}:", tid));
                    if let Some(r) = result {
                        if !r.stack_snapshot.is_empty() {
                            s.push_str(" stack=");
                            for v in &r.stack_snapshot {
                                s.push_str(&format!("{} ", v));
                            }
                        }
                        if !r.output.is_empty() {
                            s.push_str(&format!(" output=\"{}\"", r.output.trim_end()));
                        }
                        s.push('\n');
                    } else {
                        s.push_str(" (pending)\n");
                    }
                }
                s
            }
        } else {
            "mesh offline\n".to_string()
        };
        self.emit_str(&out);
    }

    /// REDUCE" `<forth code>`" ( goal-id -- ) apply reduction across subtask results.
    fn prim_reduce(&mut self) {
        let code = self.parse_until('"');
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(code);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(P_REDUCE_RT));
            }
        } else {
            self.do_reduce(&code);
        }
    }

    fn rt_reduce(&mut self) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let code = self.code_strings[idx].clone();
            self.do_reduce(&code);
        }
    }

    fn do_reduce(&mut self, reduce_code: &str) {
        let goal_id = self.pop() as u64;
        // Collect all stack results from completed subtasks.
        let values: Vec<Cell> = if let Some(ref m) = self.mesh {
            let st = m.state_lock();
            let results = st.goals.collect_results(goal_id);
            results
                .iter()
                .filter_map(|(_, r)| r.as_ref())
                .flat_map(|r| r.stack_snapshot.iter().copied())
                .collect()
        } else {
            vec![]
        };

        if values.is_empty() {
            self.emit_str("reduce: no values to reduce\n");
            return;
        }

        // Push first value, then for each subsequent value push it and run reduce_code.
        self.stack.push(values[0]);
        for &val in &values[1..] {
            self.stack.push(val);
            self.interpret_line(reduce_code);
        }
        let result = self.stack.last().copied().unwrap_or(0);
        self.emit_str(&format!("reduce: {} values -> {}\n", values.len(), result));
    }

    /// PROGRESS ( goal-id -- ) show completion progress.
    fn prim_progress(&mut self) {
        let goal_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            let st = m.state_lock();
            let s = st.goals.format_progress(goal_id);
            drop(st);
            self.emit_str(&s);
        }
    }

    // -----------------------------------------------------------------------
    // (load_prelude is defined in vm/compiler.rs)

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
        println!("self RSS at start: {} kB", read_rss_kb());
        println!();

        // Measure the cost of one real fork once, up-front. Capture as locals
        // because metrics::reset() between populations would otherwise erase it.
        let (fork_ns, pkg_ns) = bench_measure_one_fork(self);

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
                    project_gossip_to(pop, measure_pop);
                }
            }

            // Memory + peer-table operations at scale.
            run_scale_bench(pop, SCALE_CAP);
            // Spawn projection at this population.
            project_spawn_to(pop, fork_ns, pkg_ns);
            println!();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run_bench_one(&mut self, pop: usize, gossip_k: Option<usize>, label: &str) {
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
            fmt_human_count(mean_dispatch),
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

    // -----------------------------------------------------------------------
    // REPL
    // -----------------------------------------------------------------------
}

// ---------------------------------------------------------------------------
// Bench scale-up support
// ---------------------------------------------------------------------------

/// Read this process's RSS in kilobytes via `ps`. macOS + Linux compatible.
/// Returns 0 if the call fails.
#[cfg(not(target_arch = "wasm32"))]
fn read_rss_kb() -> u64 {
    let pid = std::process::id().to_string();
    match std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse::<u64>()
            .unwrap_or(0),
        Err(_) => 0,
    }
}

/// Format a kB value as kB/MB/GB.
#[cfg(not(target_arch = "wasm32"))]
fn fmt_kb(kb: u64) -> String {
    if kb >= 1_000_000 {
        format!("{:.2} GB", kb as f64 / 1_000_000.0)
    } else if kb >= 1_000 {
        format!("{:.2} MB", kb as f64 / 1_000.0)
    } else {
        format!("{} kB", kb)
    }
}

/// Format a wall-time duration as ms/s/min/h.
#[cfg(not(target_arch = "wasm32"))]
fn fmt_wall(ns: u128) -> String {
    let s = ns as f64 / 1e9;
    if s >= 3600.0 {
        format!("{:.2} h", s / 3600.0)
    } else if s >= 60.0 {
        format!("{:.2} min", s / 60.0)
    } else if s >= 1.0 {
        format!("{:.2} s", s)
    } else {
        format!("{:.2} ms", s * 1e3)
    }
}

/// Build a synthetic peer-table HashMap of size `n` and time the operations
/// `send_sexp` performs on it (collect-addrs, gossip-sample). Also synthesize
/// an inbox of size `n` and time the drain. Reads RSS before/after.
///
/// If `pop > cap`, only `cap` entries are actually built — caller is
/// responsible for projecting from the measured cost.
#[cfg(not(target_arch = "wasm32"))]
fn run_scale_bench(pop: usize, cap: usize) {
    use std::collections::{HashMap, VecDeque};
    use std::net::SocketAddr;

    let actual = pop.min(cap);
    let projected = pop > cap;
    let label = if projected {
        format!("scale (measured at {}, projected to {})", actual, pop)
    } else {
        format!("scale (measured at {})", actual)
    };

    let rss_before = read_rss_kb();

    // PeerInfo is private to mesh; build a same-shape stub for memory accounting.
    // Real PeerInfo size is reported via mesh::peer_info_size_bytes().
    #[allow(dead_code)]
    struct PeerStub {
        addr: SocketAddr,
        id: [u8; 8],
        load: u32,
        capacity: u32,
        peer_count: u16,
        fitness: i64,
        last_seen: std::time::Instant,
    }

    let populate_start = std::time::Instant::now();
    let mut peer_table: HashMap<[u8; 8], PeerStub> = HashMap::with_capacity(actual);
    for i in 0..actual {
        let mut id = [0u8; 8];
        id.copy_from_slice(&(i as u64).to_le_bytes());
        peer_table.insert(
            id,
            PeerStub {
                addr: SocketAddr::from(([127, 0, 0, 1], 1024 + (i % 60000) as u16)),
                id,
                load: 0,
                capacity: 100,
                peer_count: 0,
                fitness: 0,
                last_seen: std::time::Instant::now(),
            },
        );
    }
    let populate_elapsed = populate_start.elapsed();

    // The legacy collect-addrs step (full O(N) Vec materialization).
    let collect_iters = if actual >= 100_000 { 10 } else { 100 };
    let collect_start = std::time::Instant::now();
    for _ in 0..collect_iters {
        let _v: Vec<SocketAddr> = peer_table.values().map(|p| p.addr).collect();
    }
    let collect_per_call = collect_start.elapsed() / collect_iters as u32;

    // Reservoir sampling (Vitter R) on the HashMap iterator. O(N) iteration
    // with O(k) allocation. This is what send_sexp used to do.
    let reservoir_iters = collect_iters;
    let reservoir_start = std::time::Instant::now();
    let mut rng_state: u64 = 0xdeadbeefcafebabe;
    for _ in 0..reservoir_iters {
        let mut reservoir: Vec<SocketAddr> = Vec::with_capacity(8);
        for (i, p) in peer_table.values().enumerate() {
            if i < 8 {
                reservoir.push(p.addr);
            } else {
                rng_state ^= rng_state << 13;
                rng_state ^= rng_state >> 7;
                rng_state ^= rng_state << 17;
                if rng_state == 0 {
                    rng_state = 0xdeadbeefcafebabe;
                }
                let j = (rng_state as usize) % (i + 1);
                if j < 8 {
                    reservoir[j] = p.addr;
                }
            }
        }
        std::hint::black_box(reservoir);
    }
    let reservoir_per_call = reservoir_start.elapsed() / reservoir_iters as u32;

    // Indexable Vec sampling: rejection-sample k random indices in 0..len and
    // read those slots directly. True O(k) — no iteration over N. This is
    // what send_sexp does now on the gossip path.
    let indexed_addrs: Vec<SocketAddr> = peer_table.values().map(|p| p.addr).collect();
    let indexed_iters = 100_000usize;
    let indexed_start = std::time::Instant::now();
    let mut idx_rng: u64 = 0xfeedface12345678;
    for _ in 0..indexed_iters {
        let n = indexed_addrs.len();
        let mut out: Vec<SocketAddr> = Vec::with_capacity(8);
        let mut chosen: Vec<usize> = Vec::with_capacity(8);
        while out.len() < 8 {
            idx_rng ^= idx_rng << 13;
            idx_rng ^= idx_rng >> 7;
            idx_rng ^= idx_rng << 17;
            if idx_rng == 0 {
                idx_rng = 0xdeadbeefcafebabe;
            }
            let i = (idx_rng as usize) % n;
            if !chosen.contains(&i) {
                chosen.push(i);
                out.push(indexed_addrs[i]);
            }
        }
        std::hint::black_box(out);
    }
    let indexed_per_call = indexed_start.elapsed() / indexed_iters as u32;

    // Inbox drain: with gossip k=8, expected inbox per tick is ~k = 8 (constant
    // independent of N). But model the worst case: a flood of N messages.
    let inbox_n = actual.min(50_000);
    let mut inbox: VecDeque<String> = (0..inbox_n)
        .map(|i| format!("(peer-hello :id \"peer{}\" :gen 0)", i))
        .collect();
    let drain_start = std::time::Instant::now();
    let _drained: Vec<String> = inbox.drain(..).collect();
    let drain_elapsed = drain_start.elapsed();

    let rss_after = read_rss_kb();

    println!("--- {} ---", label);
    println!(
        "  peer-table populate ({} inserts):       {}",
        actual,
        fmt_wall(populate_elapsed.as_nanos())
    );
    println!(
        "  legacy collect_peers (per call):        {}  [O(N) iter + O(N) alloc]",
        fmt_wall(collect_per_call.as_nanos())
    );
    println!(
        "  reservoir k=8 (per call):               {}  [O(N) iter + O(k) alloc]",
        fmt_wall(reservoir_per_call.as_nanos())
    );
    println!(
        "  indexed Vec sample k=8 (per call):      {}  [O(k) — current send_sexp]",
        fmt_wall(indexed_per_call.as_nanos())
    );
    let res_speedup = if reservoir_per_call.as_nanos() > 0 {
        collect_per_call.as_nanos() as f64 / reservoir_per_call.as_nanos() as f64
    } else {
        0.0
    };
    let idx_speedup = if indexed_per_call.as_nanos() > 0 {
        collect_per_call.as_nanos() as f64 / indexed_per_call.as_nanos() as f64
    } else {
        0.0
    };
    println!(
        "  reservoir vs legacy: {:.2}x   indexed vs legacy: {:.2}x   indexed vs reservoir: {:.2}x",
        res_speedup,
        idx_speedup,
        if indexed_per_call.as_nanos() > 0 {
            reservoir_per_call.as_nanos() as f64 / indexed_per_call.as_nanos() as f64
        } else {
            0.0
        }
    );
    println!(
        "  inbox drain ({} msgs):                   {}",
        inbox_n,
        fmt_wall(drain_elapsed.as_nanos())
    );
    println!(
        "  RSS delta (peer table only):            {} → {} (Δ {})",
        fmt_kb(rss_before),
        fmt_kb(rss_after),
        fmt_kb(rss_after.saturating_sub(rss_before))
    );

    // Per-unit projection: at full N, each unit holds an N-entry peer table
    // (assuming epidemic discovery converges to full mesh).
    let per_entry_bytes = mesh::peer_info_size_bytes() + 24; // + ~24B HashMap overhead
    let per_unit_peer_mem = (pop as u128) * (per_entry_bytes as u128);
    let total_peer_mem = (pop as u128) * per_unit_peer_mem;
    println!(
        "  projected per-unit peer table at N={}: {}",
        pop,
        fmt_kb((per_unit_peer_mem / 1024) as u64)
    );
    println!(
        "  projected aggregate peer-table memory:  {}  [O(N²) WALL]",
        fmt_kb((total_peer_mem / 1024) as u64)
    );

    // Projections at full N. Legacy/reservoir scale linearly in N (both
    // iterate the table). Indexed sampling is O(k) — N-independent per call.
    let scale = if actual > 0 {
        pop as f64 / actual as f64
    } else {
        1.0
    };
    let legacy_at_full_ns = (collect_per_call.as_nanos() as f64) * scale;
    let reservoir_at_full_ns = (reservoir_per_call.as_nanos() as f64) * scale;
    // Indexed sampling per-call is independent of N — same measurement holds.
    let indexed_at_full_ns = indexed_per_call.as_nanos() as f64;
    let legacy_per_tick_ns = legacy_at_full_ns * pop as f64;
    let reservoir_per_tick_ns = reservoir_at_full_ns * pop as f64;
    let indexed_per_tick_ns = indexed_at_full_ns * pop as f64;
    println!(
        "  projected legacy collect at full N:     {}  per call",
        fmt_wall(legacy_at_full_ns as u128)
    );
    println!(
        "  projected reservoir at full N:          {}  per call",
        fmt_wall(reservoir_at_full_ns as u128)
    );
    println!(
        "  projected indexed at full N:            {}  per call  [N-independent]",
        fmt_wall(indexed_at_full_ns as u128)
    );
    println!(
        "  projected per-tick (legacy):     {}",
        fmt_wall(legacy_per_tick_ns as u128)
    );
    println!(
        "  projected per-tick (reservoir):  {}",
        fmt_wall(reservoir_per_tick_ns as u128)
    );
    println!(
        "  projected per-tick (indexed):    {}",
        fmt_wall(indexed_per_tick_ns as u128)
    );
}

/// Project chatter dispatch cost from the measured population to the requested
/// one. Both grow as N (gossip is O(N·k) per tick). Per-call latency is constant.
#[cfg(not(target_arch = "wasm32"))]
fn project_gossip_to(target_pop: usize, measured_pop: usize) {
    let mean_proc_ns = metrics::duration_mean_ns("chatter.process");
    let dispatches_per_tick = (target_pop as u128).saturating_mul(8);
    let projected_ns = dispatches_per_tick.saturating_mul(mean_proc_ns as u128);
    println!(
        "projected per-tick chatter dispatch (gossip k=8) at N={} (from N={}):",
        target_pop, measured_pop
    );
    println!(
        "  {} dispatches × {}ns ≈ {}",
        fmt_human_count(dispatches_per_tick.min(u64::MAX as u128) as u64),
        mean_proc_ns,
        fmt_wall(projected_ns)
    );
}

/// Measure one real fork via spawn_local_with_energy and clean up the child
/// and its on-disk artifacts. Returns (fork_ns, pkg_build_ns).
#[cfg(not(target_arch = "wasm32"))]
fn bench_measure_one_fork(vm: &mut VM) -> (u64, u64) {
    let pkg_start = std::time::Instant::now();
    let state = vm.build_state_for_spawn();
    let package = match spawn::build_package(&state) {
        Ok(p) => p,
        Err(_) => return (0, 0),
    };
    let pkg_ns = pkg_start.elapsed().as_nanos().min(u64::MAX as u128) as u64;

    let fork_start = std::time::Instant::now();
    let res = spawn::spawn_local_with_energy(&package, 0, 1, Some(1000));
    let fork_ns = fork_start.elapsed().as_nanos().min(u64::MAX as u128) as u64;

    match res {
        Ok((pid, _port, child_id)) => {
            // Kill child immediately.
            #[cfg(unix)]
            unsafe {
                libc_kill(pid as i32, 9);
            }
            #[cfg(not(unix))]
            let _ = pid;
            // Clean up the spawn artifacts so repeated benches don't bloat ~/.unit.
            let hex: String = child_id.iter().map(|b| format!("{:02x}", b)).collect();
            if let Ok(home) = std::env::var("HOME") {
                let _ = std::fs::remove_dir_all(format!("{}/.unit/spawn/{}", home, hex));
                let _ = std::fs::remove_dir_all(format!("{}/.unit/{}", home, hex));
            }
            println!(
                "single fork measurement: pkg-build {} + fork+exec {} = {} per unit (cleaned up)",
                fmt_wall(pkg_ns as u128),
                fmt_wall(fork_ns as u128),
                fmt_wall((pkg_ns + fork_ns) as u128)
            );
        }
        Err(e) => {
            println!("single fork measurement: SKIPPED ({})", e);
        }
    }
    (fork_ns, pkg_ns)
}

/// Project total wall time to bring N units online from a single measured
/// fork+pkg-build cost. Linear projection — an upper bound that ignores any
/// per-process overhead growth (file-descriptor pressure, fork rate limits).
#[cfg(not(target_arch = "wasm32"))]
fn project_spawn_to(pop: usize, fork_ns: u64, pkg_ns: u64) {
    let per_unit_ns = (fork_ns as u128) + (pkg_ns as u128);
    let serial_ns = (pop as u128).saturating_mul(per_unit_ns);
    // 8-way parallel disk + exec; optimistic floor.
    let parallel_ns = serial_ns / 8;
    println!(
        "spawn projection at N={}: per-unit {} (pkg {} + fork {})",
        pop,
        fmt_wall(per_unit_ns),
        fmt_wall(pkg_ns as u128),
        fmt_wall(fork_ns as u128)
    );
    println!("  serial bring-up:    {}", fmt_wall(serial_ns));
    println!(
        "  8-way parallel:     {}  (optimistic; disk I/O may dominate)",
        fmt_wall(parallel_ns)
    );
}

// ---------------------------------------------------------------------------
// --multi-unit smoke demo
// ---------------------------------------------------------------------------
//
// Spawns N VMs in one process, dispatches a few goals via least-busy worker
// selection, and demonstrates share_word + teach_from. Reports RSS so users
// can compare per-unit memory to the native fork model. Mirrors the WASM
// browser demo's lifecycle but with no upper bound below the configured cap.

#[cfg(not(target_arch = "wasm32"))]
fn run_multi_unit_demo(n: usize) {
    use crate::multi_unit::MultiUnitHost;

    let cap = n.max(1);
    println!("=== unit --multi-unit {} ===", n);
    println!("(single process, no fork, no UDP; cap = {})", cap);

    let rss_before = read_rss_kb();
    println!("RSS before spawn: {}", fmt_kb(rss_before));

    let spawn_start = std::time::Instant::now();
    let mut host = MultiUnitHost::new(cap);
    let spawned = host.spawn_n(n);
    let spawn_elapsed = spawn_start.elapsed();

    let rss_after = read_rss_kb();
    let rss_delta = rss_after.saturating_sub(rss_before);
    let per_unit_kb = if spawned > 0 {
        rss_delta / spawned as u64
    } else {
        0
    };

    println!(
        "spawned {} units in {} ({} per unit, total Δ {})",
        spawned,
        fmt_wall(spawn_elapsed.as_nanos()),
        fmt_kb(per_unit_kb),
        fmt_kb(rss_delta)
    );
    println!("RSS after spawn:  {}", fmt_kb(rss_after));

    if spawned == 0 {
        return;
    }

    // Demo 1: dispatch a few goals. Verify least-busy picker spreads work.
    let goals = ["2 3 + .", "10 4 - .", "6 7 * .", "100 5 / ."];
    println!("\n--- goal dispatch (least-busy worker) ---");
    for code in &goals {
        if let Some(r) = host.execute_goal(code) {
            println!(
                "  unit #{:<4} `{}` → {}",
                r.unit_index,
                code,
                r.output.trim().replace('\n', " ")
            );
        }
    }

    // Demo 2: share a word across every unit (zero-copy &str).
    println!("\n--- share_word ---");
    host.share_word(": DOUBLE 2 * ;");
    let probe_idx = spawned.saturating_sub(1);
    let out = host.units[probe_idx].vm.eval("21 DOUBLE .");
    println!(
        "  defined DOUBLE on every unit; unit #{} evaluates `21 DOUBLE .` → {}",
        probe_idx,
        out.trim()
    );

    // Demo 3: teach_from — define a word on one unit only, then teach it.
    println!("\n--- teach_from ---");
    host.define_on(0, ": TRIPLE 3 * ;");
    let taught = host.teach_from(0, &["TRIPLE"]);
    println!("  unit #0 taught: {:?}", taught);
    if spawned > 1 {
        let out = host.units[1].vm.eval("7 TRIPLE .");
        println!(
            "  unit #1 evaluates `7 TRIPLE .` → {}",
            out.trim()
        );
    }

    println!("\nfinal RSS: {}", fmt_kb(read_rss_kb()));
}

// ---------------------------------------------------------------------------
// --multi-unit + --port: bridged demo (in-process units + mesh peer)
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
fn run_multi_unit_mesh_demo(n: usize, cli: &CliArgs) {
    use crate::multi_unit::MultiUnitNode;
    use std::time::{Duration, Instant};

    let port = cli.port.unwrap_or(0);
    // Reuse the same --peers parsing as the normal mesh path.
    let peers_str = cli.peers.clone().unwrap_or_default();
    let seed_peers: Vec<SocketAddr> = peers_str
        .split(',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            let s = s.trim();
            s.parse().ok().or_else(|| {
                use std::net::ToSocketAddrs;
                s.to_socket_addrs().ok().and_then(|mut a| a.next())
            })
        })
        .collect();

    println!("=== unit --multi-unit {} --port {} ===", n, port);
    println!("(in-process units + mesh peer; seeds = {:?})", seed_peers);

    let mut node = match MultiUnitNode::new(n.max(1), Some(port), seed_peers) {
        Ok(node) => node,
        Err(e) => {
            eprintln!("multi-unit-mesh: failed to start mesh: {}", e);
            std::process::exit(1);
        }
    };
    let spawned = node.spawn_n(n);
    let host_hex = node.host_id_hex().unwrap_or_default();
    println!(
        "host id: {}  port: {}  units: {}",
        host_hex,
        node.mesh_port().unwrap_or(0),
        spawned
    );

    // Brief discovery window: process inbound mesh messages and re-advertise.
    println!("\nlistening for peers (5s)...");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let dispatched = node.drain_and_dispatch();
        for ev in &dispatched {
            println!(
                "[recv] from {} → unit #{} → {}",
                ev.from_host_hex,
                ev.unit_index,
                ev.output.trim().replace('\n', " ")
            );
        }
        if let Some(ref m) = node.mesh {
            m.force_heartbeat();
        }
        std::thread::sleep(Duration::from_millis(250));
    }

    println!("\n--- discovered remote processes ---");
    let remotes = node.remote_processes();
    if remotes.is_empty() {
        println!("  (none)");
    } else {
        for r in &remotes {
            println!(
                "  host {} @ {}  units={}",
                r.host_id_hex, r.addr, r.units_hosted
            );
        }
    }

    // Demo the per-unit Forth queries.
    println!("\n--- per-unit Forth queries (unit #0) ---");
    if spawned > 0 {
        let q = node.host.units[0].vm.eval("HOST-ID");
        println!("  HOST-ID            → {}", q.trim());
        let s = node.host.units[0].vm.eval("SIBLING-COUNT .");
        println!("  SIBLING-COUNT      → {}", s.trim());
        let r = node.host.units[0].vm.eval("MESH-PROCESS-COUNT .");
        println!("  MESH-PROCESS-COUNT → {}", r.trim());
    }

    // If any remote is visible, send it a probe goal.
    if let Some(r) = remotes.first() {
        println!("\n--- cross-process send to {} ---", r.host_id_hex);
        if node.send_to_process(&r.host_id, "21 2 * .") {
            println!("  sent: `21 2 * .`");
        }
        std::thread::sleep(Duration::from_millis(200));
        for ev in node.drain_and_dispatch() {
            println!(
                "  reply path (recv): from {} → unit #{} → {}",
                ev.from_host_hex,
                ev.unit_index,
                ev.output.trim().replace('\n', " ")
            );
        }
    }

    println!("\nfinal RSS: {}", fmt_kb(read_rss_kb()));
}

// ---------------------------------------------------------------------------
// --bench-two-tier: characterize the bridged MultiUnitNode deployment
// ---------------------------------------------------------------------------
//
// Spins M MultiUnitNodes in one process, each with N in-process units, all
// peered into one mesh on loopback. Reports peer-table size, gossip
// bandwidth, cross-process send_to_process latency p50/p95, aggregate
// spawn time, and any non-linear scaling. The single-process model is
// chosen for simplicity; per-MultiUnitNode metrics are inferred by
// dividing process-wide counters by M.

#[cfg(not(target_arch = "wasm32"))]
fn run_two_tier_bench(configs: &[(usize, usize)], gossip_k: Option<usize>) {
    use crate::multi_unit::MultiUnitNode;
    use std::net::SocketAddr;
    use std::time::{Duration, Instant};

    let mode_label = match gossip_k {
        Some(k) => format!("gossip k={}", k),
        None => "all-to-all".to_string(),
    };
    println!("=== two-tier scaling bench [{}] ===", mode_label);
    println!(
        "(M MultiUnitNodes × N units, all in one process; M peer tables on loopback)\n"
    );
    println!("sizeof(PeerInfo) = {} bytes", mesh::peer_info_size_bytes());
    println!("self RSS at start: {}\n", fmt_kb(read_rss_kb()));

    for &(m, n) in configs {
        println!("###########################################################");
        println!("# M={} processes × N={} units = {} aggregate", m, n, m * n);
        println!("###########################################################");

        let rss_before = read_rss_kb();
        metrics::reset();

        // ---- (4) aggregate spawn time ----
        let spawn_start = Instant::now();
        let mut nodes: Vec<MultiUnitNode> = Vec::with_capacity(m);
        for i in 0..m {
            // Seed each new node with up to 4 of the already-running peers,
            // so transitive gossip can fill in the rest. This keeps the
            // bootstrap O(M·k) instead of O(M²).
            let seeds: Vec<SocketAddr> = nodes
                .iter()
                .rev()
                .take(4)
                .filter_map(|nd| nd.mesh_port())
                .map(|p| format!("127.0.0.1:{}", p).parse().unwrap())
                .collect();
            let mut node = MultiUnitNode::new(n, Some(0), seeds)
                .expect("MultiUnitNode start failed");
            // Apply bounded-k gossip to BOTH send_sexp and send_heartbeat.
            if let Some(k) = gossip_k {
                if let Some(ref mesh_node) = node.mesh {
                    mesh_node.set_gossip_fanout(Some(k));
                }
            }
            node.spawn_n(n);
            nodes.push(node);
            // Periodic heartbeat during ramp so newcomers learn quickly.
            if i % 10 == 9 {
                for nd in &nodes {
                    if let Some(ref mesh_node) = nd.mesh {
                        mesh_node.force_heartbeat();
                    }
                }
            }
        }
        let spawn_elapsed = spawn_start.elapsed();
        let _rss_after_spawn = read_rss_kb();

        // ---- discovery convergence ----
        let conv_start = Instant::now();
        for _ in 0..8 {
            for nd in &nodes {
                if let Some(ref mesh_node) = nd.mesh {
                    mesh_node.force_heartbeat();
                }
            }
            std::thread::sleep(Duration::from_millis(60));
        }
        let conv_elapsed = conv_start.elapsed();

        // Drain stale envelopes so they don't pollute later measurements.
        for nd in nodes.iter_mut() {
            let _ = nd.drain_and_dispatch();
        }

        // ---- (1) peer-table size + memory ----
        let peer_counts: Vec<usize> = nodes.iter().map(|nd| nd.remote_processes().len()).collect();
        let peer_min = *peer_counts.iter().min().unwrap_or(&0);
        let peer_max = *peer_counts.iter().max().unwrap_or(&0);
        let peer_sum: usize = peer_counts.iter().sum();
        let peer_mean = if !peer_counts.is_empty() {
            peer_sum as f64 / peer_counts.len() as f64
        } else {
            0.0
        };
        let per_entry_bytes = mesh::peer_info_size_bytes() + 24 + 16; // Vec slot + HashMap overhead
        let per_proc_table_bytes = peer_max as u128 * per_entry_bytes as u128;
        let total_peer_mem_bytes = (peer_sum as u128) * per_entry_bytes as u128;

        // ---- (2) gossip bandwidth: sample over a steady-state window ----
        // Rely on the network thread's natural HEARTBEAT_INTERVAL (2s); also
        // force-tick to pump traffic for a denser sample. Reset metrics first.
        let bw_window = Duration::from_secs(3);
        let bytes_sent_before = metrics::value_total("mesh.bytes_sent");
        let msgs_sent_before = metrics::value_count("mesh.bytes_sent");
        let bytes_recv_before = metrics::value_total("mesh.bytes_recv");
        let msgs_recv_before = metrics::value_count("mesh.bytes_recv");
        let bw_start = Instant::now();
        while bw_start.elapsed() < bw_window {
            for nd in &nodes {
                if let Some(ref mesh_node) = nd.mesh {
                    mesh_node.force_heartbeat();
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let bw_elapsed = bw_start.elapsed().as_secs_f64();
        let bytes_sent = metrics::value_total("mesh.bytes_sent") - bytes_sent_before;
        let msgs_sent = metrics::value_count("mesh.bytes_sent") - msgs_sent_before;
        let bytes_recv = metrics::value_total("mesh.bytes_recv") - bytes_recv_before;
        let msgs_recv = metrics::value_count("mesh.bytes_recv") - msgs_recv_before;
        // Process-wide totals; per-process = total / M.
        let per_proc_send_bps = (bytes_sent as f64) / bw_elapsed / m as f64;
        let per_proc_send_mps = (msgs_sent as f64) / bw_elapsed / m as f64;
        let per_proc_recv_bps = (bytes_recv as f64) / bw_elapsed / m as f64;
        let per_proc_recv_mps = (msgs_recv as f64) / bw_elapsed / m as f64;

        // ---- (3) cross-process send_to_process latency ----
        // From node 0, send probes to a sweep of targets one-at-a-time and
        // poll the target's inbox until the probe arrives, recording elapsed
        // wall time. This measures actual end-to-end delivery (send →
        // network thread recv → sexp_inbox enqueue → drain), not the time
        // until our outer loop bothers to look.
        let epoch = Instant::now();
        let probe_targets = (1..m.min(11)).collect::<Vec<_>>(); // up to 10 targets
        let probes_per_target = 50usize;
        let poll_timeout = Duration::from_millis(50);
        for &target_i in &probe_targets {
            if target_i >= nodes.len() {
                continue;
            }
            let target_id = nodes[target_i].host_id().expect("target host id");
            for seq in 0..probes_per_target {
                let send_ns = epoch.elapsed().as_nanos();
                let payload = format!("__BENCH_PROBE_{}_{}", seq, send_ns);
                nodes[0].send_to_process(&target_id, &payload);
                // Poll-drain target until we see THIS probe (or timeout).
                let probe_deadline = Instant::now() + poll_timeout;
                let target_mesh = nodes[target_i].mesh.as_ref().expect("target mesh");
                let mut found = false;
                while !found && Instant::now() < probe_deadline {
                    let raw_msgs = target_mesh.recv_sexp_messages();
                    for raw in raw_msgs {
                        let parsed = match crate::sexp::try_parse_mesh_msg(&raw) {
                            Some(s) => s,
                            None => continue,
                        };
                        let p = match parsed.get_key(":payload").and_then(|s| s.as_str()) {
                            Some(p) => p.to_string(),
                            None => continue,
                        };
                        if let Some(rest) = p.strip_prefix("__BENCH_PROBE_") {
                            if let Some((_seq_str, send_ns_str)) = rest.split_once('_') {
                                if let Ok(this_send_ns) = send_ns_str.parse::<u128>() {
                                    let recv_ns = epoch.elapsed().as_nanos();
                                    let lat = (recv_ns - this_send_ns) as u64;
                                    metrics::record("send_to_process.latency", lat);
                                    if this_send_ns == send_ns {
                                        found = true;
                                    }
                                }
                            }
                        }
                    }
                    if !found {
                        std::thread::sleep(Duration::from_micros(200));
                    }
                }
            }
        }
        let lat_count = metrics::value_count("dummy"); // unused, just to keep API in scope
        let _ = lat_count;
        let lat_p50 = metrics::histogram_percentile_ns("send_to_process.latency", 0.50);
        let lat_p95 = metrics::histogram_percentile_ns("send_to_process.latency", 0.95);
        let lat_max = metrics::histogram_max_ns("send_to_process.latency");
        let lat_n = metrics::histogram_count("send_to_process.latency");

        let rss_after_bw = read_rss_kb();
        let rss_delta = rss_after_bw.saturating_sub(rss_before);
        let per_unit_kb = if m * n > 0 {
            rss_delta / (m * n) as u64
        } else {
            0
        };

        // ---- print report ----
        println!("spawn:");
        println!(
            "  aggregate spawn:          {} ({} per node, {} per unit)",
            fmt_wall(spawn_elapsed.as_nanos()),
            fmt_wall(spawn_elapsed.as_nanos() / m.max(1) as u128),
            fmt_wall(spawn_elapsed.as_nanos() / (m * n).max(1) as u128)
        );
        println!(
            "  discovery convergence:    {} ({} forced-heartbeat rounds)",
            fmt_wall(conv_elapsed.as_nanos()),
            8
        );
        println!("memory:");
        println!(
            "  RSS delta:                {}  ({} per unit)",
            fmt_kb(rss_delta),
            fmt_kb(per_unit_kb)
        );
        println!(
            "  per-process peer table:   {} (max peers = {})",
            fmt_kb((per_proc_table_bytes / 1024) as u64),
            peer_max
        );
        println!(
            "  aggregate peer-table:     {}",
            fmt_kb((total_peer_mem_bytes / 1024) as u64)
        );
        println!("peer table:");
        println!(
            "  observed peers / process: min {} mean {:.1} max {} (target M-1 = {})",
            peer_min,
            peer_mean,
            peer_max,
            m.saturating_sub(1)
        );
        println!("gossip bandwidth (steady-state, per process):");
        println!(
            "  send: {:.0} msg/s, {} (over {:.2}s window)",
            per_proc_send_mps,
            fmt_bps(per_proc_send_bps),
            bw_elapsed
        );
        println!(
            "  recv: {:.0} msg/s, {}",
            per_proc_recv_mps,
            fmt_bps(per_proc_recv_bps)
        );
        println!("cross-process latency ({} samples):", lat_n);
        println!(
            "  p50: {}   p95: {}   max: {}",
            fmt_wall(lat_p50 as u128),
            fmt_wall(lat_p95 as u128),
            fmt_wall(lat_max as u128)
        );

        // ---- (5) non-linear flags ----
        let expected_peers = m.saturating_sub(1);
        if peer_max < expected_peers {
            println!(
                "  ⚠ discovery did not fully converge: max peers {} < expected {} (need more rounds or larger seed fanout)",
                peer_max, expected_peers
            );
        }
        if lat_n == 0 {
            println!("  ⚠ no latency samples — check probe parsing or send_to_process");
        }

        println!();
        // Drop nodes between configs to free sockets/threads.
        drop(nodes);
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Format bytes-per-second as B/s, kB/s, MB/s.
#[cfg(not(target_arch = "wasm32"))]
fn fmt_bps(bps: f64) -> String {
    if bps >= 1_000_000.0 {
        format!("{:.2} MB/s", bps / 1_000_000.0)
    } else if bps >= 1_000.0 {
        format!("{:.2} kB/s", bps / 1_000.0)
    } else {
        format!("{:.0} B/s", bps)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fmt_human_count(v: u64) -> String {
    if v >= 1_000_000_000 {
        format!("{:.2}G", v as f64 / 1e9)
    } else if v >= 1_000_000 {
        format!("{:.2}M", v as f64 / 1e6)
    } else if v >= 1_000 {
        format!("{:.2}k", v as f64 / 1e3)
    } else {
        format!("{}", v)
    }
}

// ===========================================================================
// REPL
// ===========================================================================

impl VM {
    fn repl(&mut self) {
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        let _ = write!(stdout, "> ");
        let _ = stdout.flush();

        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    self.interpret_line(&line);
                    if !self.running {
                        break;
                    }
                    if !self.compiling {
                        self.check_auto_claim();
                        self.check_auto_replicate();
                        self.check_auto_evolve();
                        self.check_incoming_replications();
                        self.energy.tick();
                        self.landscape.tick();
                        self.tick_monitor();
                        self.tick_swarm();
                        self.check_auto_snapshot();
                        self.tick_dist_goals();
                        self.poll_ws_events();
                        self.update_ws_mesh_json();
                    }
                    if self.compiling {
                        let _ = write!(stdout, "  ");
                    } else {
                        let _ = write!(stdout, " ok\n> ");
                    }
                    let _ = stdout.flush();
                }
                Err(_) => break,
            }
        }
        println!();
    }
}

// ===========================================================================
// CLI argument parsing
// ===========================================================================

const VERSION: &str = "unit v0.27.0";

fn print_help() {
    println!("{}", VERSION);
    println!("A self-replicating software nanobot.\n");
    println!("USAGE:");
    println!("  unit                        Start interactive REPL");
    println!("  unit --eval \"2 3 + .\"       Evaluate and print result");
    println!("  unit --port 4201 --swarm    Start swarm node on port 4201");
    println!("  unit --file script.fs       Load a Forth script\n");
    println!("OPTIONS:");
    println!("  -h, --help                  Show this help");
    println!("  -v, --version               Print version and exit");
    println!("  -q, --quiet                 Suppress boot banner");
    println!("  --port PORT                 Set mesh UDP port (or UNIT_PORT env)");
    println!("  --peers HOST:PORT,...       Set seed peers (or UNIT_PEERS env)");
    println!("  --ws-port PORT             Set WebSocket bridge port");
    println!("  --eval \"FORTH CODE\"         Evaluate code, print output, exit");
    println!("  --file PATH                Load a .fs file, then start REPL");
    println!("  --no-mesh                  Start without mesh networking");
    println!("  --no-prelude               Start without loading prelude.fs");
    println!("  --swarm                    Start with SWARM-ON");
    println!("  --trust LEVEL              Set trust: all, mesh, family, none");
    println!("  --serve [PORT]             Start HTTP bridge on 127.0.0.1 (default :9898)");
    println!("                             (requires: cargo build --features http)");
    println!("  --bench [SIZES]            Run headless timing bench at the given");
    println!("                             populations (comma-separated, default");
    println!("                             10,100,1000,10000) and exit. Reports both");
    println!("                             all-to-all and bounded-k gossip modes.");
    println!("  --gossip-k K               Use bounded random gossip with fan-out K");
    println!("                             on the live mesh (default: all-to-all).");
    println!("  --multi-unit N             Spawn N units in a single process (no fork,");
    println!("                             no UDP). Combine with --port and --peers to");
    println!("                             also participate in the mesh as one peer");
    println!("                             process advertising N units. Runs a smoke");
    println!("                             demo and exits.");
    println!("  --bench-two-tier [CONFIGS] Two-tier scaling bench. CONFIGS is a comma-");
    println!("                             separated list of MxN pairs (e.g.");
    println!("                             10x10,100x100). Default:");
    println!("                             10x10,10x100,100x10,100x100,50x200.");
}

struct CliArgs {
    port: Option<u16>,
    peers: Option<String>,
    ws_port: Option<u16>,
    eval_code: Option<String>,
    file_path: Option<String>,
    no_mesh: bool,
    no_prelude: bool,
    swarm: bool,
    trust: Option<String>,
    quiet: bool,
    /// None = not serving, Some(p) = serve on 127.0.0.1:p.
    serve_port: Option<u16>,
    /// None = no bench, Some(sizes) = run bench at those populations.
    bench_pops: Option<Vec<usize>>,
    /// None = all-to-all (legacy); Some(k) = bounded random gossip with k peers.
    gossip_k: Option<usize>,
    /// None = no multi-unit run; Some(n) = spawn n units in-process and demo.
    multi_unit_n: Option<usize>,
    /// None = no two-tier bench; Some(configs) = run bench with these (M, N) pairs.
    bench_two_tier: Option<Vec<(usize, usize)>>,
}

fn parse_args() -> Option<CliArgs> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cli = CliArgs {
        port: None,
        peers: None,
        ws_port: None,
        eval_code: None,
        file_path: None,
        no_mesh: false,
        no_prelude: false,
        swarm: false,
        trust: None,
        quiet: false,
        serve_port: None,
        bench_pops: None,
        gossip_k: None,
        multi_unit_n: None,
        bench_two_tier: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "-v" | "--version" => {
                println!("{}", VERSION);
                std::process::exit(0);
            }
            "-q" | "--quiet" => cli.quiet = true,
            "--port" => {
                i += 1;
                cli.port = args.get(i).and_then(|s| s.parse().ok());
            }
            "--peers" => {
                i += 1;
                cli.peers = args.get(i).cloned();
            }
            "--ws-port" => {
                i += 1;
                cli.ws_port = args.get(i).and_then(|s| s.parse().ok());
            }
            "--eval" => {
                i += 1;
                cli.eval_code = args.get(i).cloned();
            }
            "--file" => {
                i += 1;
                cli.file_path = args.get(i).cloned();
            }
            "--no-mesh" => cli.no_mesh = true,
            "--no-prelude" => cli.no_prelude = true,
            "--swarm" => cli.swarm = true,
            "--trust" => {
                i += 1;
                cli.trust = args.get(i).cloned();
            }
            "--serve" => {
                // Optional PORT: consume next arg only if it parses as u16.
                let port = match args.get(i + 1).and_then(|s| s.parse::<u16>().ok()) {
                    Some(p) => {
                        i += 1;
                        p
                    }
                    None => 9898,
                };
                cli.serve_port = Some(port);
            }
            "--bench" => {
                // Optional SIZES (comma-separated). If next arg parses as a
                // comma-separated list of usize, consume it; otherwise default.
                let pops: Vec<usize> = match args.get(i + 1) {
                    Some(s) if s.split(',').all(|p| p.parse::<usize>().is_ok()) => {
                        i += 1;
                        s.split(',').filter_map(|p| p.parse().ok()).collect()
                    }
                    _ => vec![10, 100, 1000, 10000],
                };
                cli.bench_pops = Some(pops);
            }
            "--gossip-k" => {
                i += 1;
                cli.gossip_k = args.get(i).and_then(|s| s.parse().ok());
            }
            "--multi-unit" => {
                i += 1;
                cli.multi_unit_n = args.get(i).and_then(|s| s.parse().ok());
                if cli.multi_unit_n.is_none() {
                    eprintln!("--multi-unit requires a positive integer N");
                    std::process::exit(1);
                }
            }
            "--bench-two-tier" => {
                // Optional CONFIGS — comma-separated MxN pairs. If next arg
                // parses as such, consume it; otherwise default.
                let parsed: Option<Vec<(usize, usize)>> = args.get(i + 1).and_then(|s| {
                    let pairs: Option<Vec<(usize, usize)>> = s
                        .split(',')
                        .map(|tok| {
                            tok.split_once('x').and_then(|(a, b)| {
                                Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                            })
                        })
                        .collect();
                    pairs
                });
                cli.bench_two_tier = match parsed {
                    Some(p) if !p.is_empty() => {
                        i += 1;
                        Some(p)
                    }
                    _ => Some(vec![
                        (10, 10),
                        (10, 100),
                        (100, 10),
                        (100, 100),
                        (50, 200),
                        (200, 50),
                    ]),
                };
            }
            other => {
                eprintln!("unknown option: {}", other);
                std::process::exit(1);
            }
        }
        i += 1;
    }
    Some(cli)
}

// ===========================================================================
// Entry point
// ===========================================================================

fn main() {
    let cli = parse_args().unwrap();

    // --bench: headless timing run, no mesh, no REPL. Native only — the
    // metrics module is a no-op on wasm32 (no Instant), so bench would
    // produce all zeros.
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(ref pops) = cli.bench_pops {
            let mut vm = VM::new();
            vm.silent = true;
            vm.load_prelude();
            vm.silent = false;
            vm.run_bench(pops);
            return;
        }
        if let Some(n) = cli.multi_unit_n {
            // If --port is also set, run the bridged demo (in-process units +
            // mesh peer). Otherwise run the strictly intra-process demo.
            if cli.port.is_some() {
                run_multi_unit_mesh_demo(n, &cli);
            } else {
                run_multi_unit_demo(n);
            }
            return;
        }
        if let Some(ref cfgs) = cli.bench_two_tier {
            run_two_tier_bench(cfgs, cli.gossip_k);
            return;
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = cli.bench_pops;
        let _ = cli.multi_unit_n;
        let _ = cli.bench_two_tier;
    }

    let mut vm = VM::new();
    vm.silent = cli.quiet;

    // Port: CLI flag > env var > default 0.
    let port: u16 = cli
        .port
        .or_else(|| std::env::var("UNIT_PORT").ok().and_then(|s| s.parse().ok()))
        .unwrap_or(0);

    let peers_str = cli
        .peers
        .or_else(|| std::env::var("UNIT_PEERS").ok())
        .or_else(|| std::env::var("UNIT_SEEDS").ok())
        .unwrap_or_default();
    let seed_peers: Vec<SocketAddr> = peers_str
        .split(',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            let s = s.trim();
            // Try direct parse first, then DNS resolution.
            s.parse().ok().or_else(|| {
                use std::net::ToSocketAddrs;
                match s.to_socket_addrs() {
                    Ok(mut addrs) => addrs.next(),
                    Err(e) => {
                        eprintln!("resolve {}: {}", s, e);
                        None
                    }
                }
            })
        })
        .collect();

    // Start mesh unless --no-mesh.
    if !cli.no_mesh {
        let env_node_id: Option<[u8; 8]> = std::env::var("UNIT_NODE_ID").ok().and_then(|hex| {
            if hex.len() != 16 {
                return None;
            }
            let mut id = [0u8; 8];
            for i in 0..8 {
                id[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
            }
            Some(id)
        });

        let persisted_id = env_node_id.or_else(persist::load_node_id);
        let resumed = persisted_id.is_some() && env_node_id.is_none();

        match mesh::MeshNode::start_with_id(persisted_id, port, seed_peers) {
            Ok(node) => {
                let id = node.id_bytes();
                let seed = u64::from_be_bytes(id);
                vm.rng = mutation::SimpleRng::new(seed);
                vm.node_id_cache = Some(id);
                vm.challenge_registry = challenges::ChallengeRegistry::new(&id);
                // Register fib10 as a built-in challenge.
                let fib = challenges::fib10_as_challenge();
                vm.challenge_registry.register_builtin(
                    &fib.name,
                    &fib.target_output,
                    fib.seed_programs,
                );
                let _ = persist::save_node_id(&id);
                if resumed && !cli.quiet {
                    eprintln!("resumed identity {}", mesh::id_to_hex(&id));
                }
                vm.mesh = Some(node);

                // Set external address for NAT traversal.
                if let Ok(ext) = std::env::var("UNIT_EXTERNAL_ADDR") {
                    if let Ok(addr) = ext.parse::<SocketAddr>() {
                        if let Some(ref mut m) = vm.mesh {
                            m.external_addr = Some(addr);
                        }
                        if !cli.quiet {
                            eprintln!("external address: {}", addr);
                        }
                    }
                }

                // Set mesh authentication key.
                if let Ok(key) = std::env::var("UNIT_MESH_KEY") {
                    if !key.is_empty() {
                        if let Some(ref mut m) = vm.mesh {
                            m.mesh_key = Some(key);
                        }
                        if !cli.quiet {
                            eprintln!("mesh-key: enabled");
                        }
                    }
                }

                // Apply --gossip-k bounded fan-out (applies to both send_sexp
                // and send_heartbeat).
                if let Some(k) = cli.gossip_k {
                    if let Some(ref m) = vm.mesh {
                        m.set_gossip_fanout(Some(k));
                    }
                    if !cli.quiet {
                        eprintln!("gossip: bounded fan-out k={}", k);
                    }
                }

                let ws_port: u16 = cli
                    .ws_port
                    .or_else(|| {
                        std::env::var("UNIT_WS_PORT")
                            .ok()
                            .and_then(|s| s.parse().ok())
                    })
                    .unwrap_or_else(|| if port > 0 { port + 2000 } else { 0 });
                if ws_port > 0 {
                    match ws_bridge::start_ws_bridge(ws_port, vm.ws_mesh_json.clone()) {
                        Ok((ws_st, ws_rx)) => {
                            vm.ws_state = Some(ws_st);
                            vm.ws_events = Some(ws_rx);
                            if !cli.quiet {
                                eprintln!("ws-bridge: listening on port {}", ws_port);
                            }
                        }
                        Err(e) => {
                            if !cli.quiet {
                                eprintln!("ws-bridge: {}", e);
                            }
                        }
                    }
                }

                if let Ok(gen_str) = std::env::var("UNIT_GENERATION") {
                    if let Ok(gen) = gen_str.parse::<u32>() {
                        vm.spawn_state.generation = gen;
                    }
                }
                if let Ok(parent_hex) = std::env::var("UNIT_PARENT_ID") {
                    if parent_hex.len() == 16 {
                        let mut pid = [0u8; 8];
                        let mut ok = true;
                        for i in 0..8 {
                            match u8::from_str_radix(&parent_hex[i * 2..i * 2 + 2], 16) {
                                Ok(b) => pid[i] = b,
                                Err(_) => {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        if ok {
                            vm.spawn_state.parent_id = Some(pid);
                        }
                    }
                }
                if let Ok(energy_str) = std::env::var("UNIT_CHILD_ENERGY") {
                    if let Ok(energy) = energy_str.parse::<i64>() {
                        vm.energy.energy = energy;
                    }
                }
            }
            Err(e) => {
                if !cli.quiet {
                    eprintln!("mesh: failed to start: {}", e);
                }
            }
        }
    }

    if let Some(ref m) = vm.mesh {
        m.set_load(vm.dictionary.len() as u32);
    }

    // Restore state or load prelude.
    let mut restored = false;
    if let Some(id) = vm.node_id_cache {
        if let Some(data) = persist::load_state(&id) {
            if let Some(snap) = persist::deserialize_snapshot(&data) {
                vm.dictionary = snap.dictionary;
                vm.memory = snap.memory;
                vm.here = snap.here;
                vm.fitness = snap.fitness;
                vm.code_strings = snap.code_strings;
                if let Some(ref m) = vm.mesh {
                    let mut st = m.state_lock();
                    st.goals = snap.goals;
                }
                restored = true;
                if !cli.quiet {
                    eprintln!("restored from {}/state.bin", persist::state_dir(&id));
                }
            }
        }
    }

    if !restored && !cli.no_prelude {
        // Suppress prelude output for --eval and --quiet modes.
        let suppress = cli.eval_code.is_some() || cli.quiet;
        if suppress {
            vm.output_buffer = Some(String::new());
        }
        vm.load_prelude();
        if suppress {
            vm.output_buffer = None;
        }
    }
    // Record kernel+prelude dictionary size so snapshots only save user words.
    vm.kernel_word_count = vm.dictionary.len();
    vm.silent = false;

    // Try JSON resurrection (only if not already restored from binary state).
    if !restored && vm.try_resurrect() {
        if !cli.quiet {
            eprintln!("resurrected from snapshot");
        }
        // Broadcast resurrection to mesh.
        if let Some(id) = vm.node_id_cache {
            if let Some(json) = snapshot::load_json_snapshot(&id) {
                if let Some(snap) = snapshot::from_json(&json) {
                    if let Some(ref m) = vm.mesh {
                        let sexp =
                            sexp::msg_resurrect(&id, snap.fitness, snap.generation, snap.timestamp);
                        m.send_sexp(&sexp.to_string());
                    }
                }
            }
        }
    }

    // Apply --trust.
    if let Some(ref level) = cli.trust {
        match level.as_str() {
            "all" => vm.interpret_line("TRUST-ALL"),
            "mesh" => vm.interpret_line("TRUST-MESH"),
            "family" => vm.interpret_line("TRUST-FAMILY"),
            "none" => vm.interpret_line("TRUST-NONE"),
            _ => eprintln!("unknown trust level: {}", level),
        }
    }

    // Apply --swarm.
    if cli.swarm {
        vm.interpret_line("SWARM-ON");
    }

    // --file: load a Forth script.
    if let Some(ref path) = cli.file_path {
        match std::fs::read_to_string(path) {
            Ok(source) => {
                for line in source.lines() {
                    vm.interpret_line(line);
                }
            }
            Err(e) => {
                eprintln!("error: cannot read {}: {}", path, e);
                std::process::exit(1);
            }
        }
    }

    // --eval: evaluate and exit.
    if let Some(ref code) = cli.eval_code {
        let output = vm.eval(code);
        if !output.is_empty() {
            print!("{}", output);
        }
        return;
    }

    // --serve: run as HTTP bridge instead of starting the REPL.
    if let Some(port) = cli.serve_port {
        #[cfg(feature = "http")]
        {
            http::serve(vm, port);
        }
        #[cfg(not(feature = "http"))]
        {
            let _ = port;
            eprintln!("--serve requires building with --features http");
            std::process::exit(1);
        }
    }

    vm.repl();
}
