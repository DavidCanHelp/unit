//! Snapshot & persistence primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Genome snapshot primitives (S-expression format; legacy JSON read-only)
    // -----------------------------------------------------------------------

    pub(crate) fn make_unit_snapshot(&self) -> snapshot::UnitSnapshot {
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

    pub(crate) fn restore_unit_snapshot(&mut self, snap: &snapshot::UnitSnapshot) {
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

    pub(crate) fn prim_json_snapshot(&mut self) {
        let snap = self.make_unit_snapshot();
        let text = snapshot::to_sexp(&snap);
        let id = self.node_id_cache.unwrap_or([0u8; 8]);
        match snapshot::save_snapshot_file(&id, &text) {
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

    pub(crate) fn prim_json_restore(&mut self) {
        let id = self.node_id_cache.unwrap_or([0u8; 8]);
        if let Some(text) = snapshot::load_snapshot_file(&id) {
            if let Some(snap) = snapshot::from_str(&text) {
                self.restore_unit_snapshot(&snap);
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

    pub(crate) fn prim_snapshot_path(&mut self) {
        let id = self.node_id_cache.unwrap_or([0u8; 8]);
        self.emit_str(&format!("{}\n", snapshot::snapshot_path(&id)));
    }

    pub(crate) fn prim_json_snapshots(&mut self) {
        let snapshots = snapshot::list_snapshot_files();
        if snapshots.is_empty() {
            self.emit_str("no snapshots\n");
        } else {
            for name in &snapshots {
                self.emit_str(&format!("  {}\n", name));
            }
        }
    }

    pub(crate) fn prim_auto_snapshot(&mut self) {
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

    pub(crate) fn prim_hibernate(&mut self) {
        let snap = self.make_unit_snapshot();
        let text = snapshot::to_sexp(&snap);
        if let Some(id) = self.node_id_cache {
            match snapshot::save_snapshot_file(&id, &text) {
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
            let _ = snapshot::save_snapshot_file(&[0u8; 8], &text);
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

    pub(crate) fn prim_export_genome(&mut self) {
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

    pub(crate) fn prim_import_genome(&mut self) {
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
    pub(crate) fn check_auto_snapshot(&mut self) {
        if self.auto_snapshot_secs == 0 {
            return;
        }
        if let Some(last) = self.auto_snapshot_last {
            if last.elapsed() >= Duration::from_secs(self.auto_snapshot_secs) {
                self.auto_snapshot_last = Some(Instant::now());
                let snap = self.make_unit_snapshot();
                let text = snapshot::to_sexp(&snap);
                if let Some(id) = self.node_id_cache {
                    let _ = snapshot::save_snapshot_file(&id, &text);
                }
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn check_auto_snapshot(&mut self) {
        // No timer on WASM — auto-snapshot is a no-op in the browser.
    }

    /// Try to resurrect from a genome snapshot (canonical sexp, or a legacy
    /// JSON file left by a pre-v0.34 unit). Returns true if restored.
    pub fn try_resurrect(&mut self) -> bool {
        if let Some(id) = self.node_id_cache {
            if let Some(text) = snapshot::load_snapshot_file(&id) {
                if let Some(snap) = snapshot::from_str(&text) {
                    self.restore_unit_snapshot(&snap);
                    return true;
                }
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Persistence primitives
    // -----------------------------------------------------------------------

    pub(crate) fn make_snapshot(&self) -> persist::VmSnapshot {
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

    /// `TRANSPORT` ( -- ) — the unit's chosen attempt to relocate itself to
    /// another coordinate, with confirm-before-release. Unit-invoked and
    /// GP-mutable like COURT/SAY!, NOT host-driven: the host offers the
    /// capability; the unit's own evolved code decides whether/when to flee
    /// local resource pressure. There is no host-side relocation scheduler.
    ///
    /// Sequence: sense local mislocation (over the ceiling) → sufficient-first
    /// destination from the gossiped peer view → if affordable, capture the
    /// complete self and transport it. The origin is released (marks
    /// `transported_out`) ONLY on a confirmed live copy. A starving unit can't
    /// afford the cost and no-ops — it cannot flee. Not mislocated, or no
    /// sufficient destination, is a safe no-op: the unit stays.
    ///
    /// On wasm32 the gossiped view is empty (no mesh) so this naturally
    /// no-ops, mirroring the MARK!/SENSE shims.
    pub(crate) fn prim_transport(&mut self) {
        let local = crate::resources::HostResources::measure();
        // The gossiped candidate view — peers and their advertised headroom.
        // A unit reads only its own view; no coordinator, no aggregation.
        let candidates: Vec<crate::transport::Candidate> = self
            .mesh
            .as_ref()
            .map(|m| {
                m.peer_resource_view()
                    .into_iter()
                    .map(|(_, _, headroom, addr)| crate::transport::Candidate {
                        headroom_pct: headroom,
                        addr,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let can_afford = self.energy.can_afford(energy::TRANSPORT_COST);
        // Derive the placement tie-break seed from this unit's own RNG (advanced
        // each call) BEFORE the send closure borrows `self` — passing a u64 by
        // value avoids an `&mut self.rng` / `&self` borrow collision.
        let tie_seed = self.rng.next_u64();

        // Capture + ship lazily inside the closure: we only serialize the
        // complete self when we will actually transport (mislocated, a
        // sufficient destination exists, and the cost is affordable).
        let attempt =
            crate::transport::attempt_transport(&local, &candidates, can_afford, tie_seed, |addr| {
                let payload = persist::serialize_snapshot(&self.make_snapshot());
                crate::transport::send_transport(&addr.to_string(), &payload)
            });

        match attempt {
            // Content where we are, nowhere sufficient to go, or starving:
            // safe no-ops, no energy charged.
            crate::transport::TransportAttempt::NotMislocated
            | crate::transport::TransportAttempt::NoDestination
            | crate::transport::TransportAttempt::CannotAfford => {}
            crate::transport::TransportAttempt::Attempted(outcome) => {
                // We shipped — charge the metabolic cost of the attempt.
                let _ = self.energy.spend(energy::TRANSPORT_COST, "transport");
                if crate::transport::should_release(&outcome) {
                    // A confirmed live copy exists elsewhere — release origin.
                    self.transported_out = true;
                    self.emit_str("transported: confirmed live copy, origin released\n");
                } else if let Err(e) = outcome {
                    self.emit_str(&format!("transport failed ({}); staying put\n", e));
                }
            }
        }
    }

    pub(crate) fn prim_save(&mut self) {
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

    pub(crate) fn prim_load_state(&mut self) {
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

    pub(crate) fn prim_auto_save(&mut self) {
        self.auto_save_enabled = !self.auto_save_enabled;
        self.emit_str(&format!(
            "auto-save: {} (every {} tasks)\n",
            if self.auto_save_enabled { "ON" } else { "OFF" },
            self.auto_save_interval
        ));
    }

    pub(crate) fn prim_reset(&mut self) {
        if let Some(id) = self.node_id_cache {
            let _ = persist::delete_state(&id);
        }
        let _ = persist::delete_node_id();
        self.emit_str("state and identity deleted — restart for fresh boot\n");
    }

    pub(crate) fn prim_snapshots(&mut self) {
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

    pub(crate) fn prim_snapshot(&mut self) {
        if let Some(id) = self.node_id_cache {
            let snap = self.make_snapshot();
            let data = persist::serialize_snapshot(&snap);
            match persist::save_snapshot(&id, &data) {
                Ok(name) => self.emit_str(&format!("snapshot: {}\n", name)),
                Err(e) => self.emit_str(&format!("snapshot failed: {}\n", e)),
            }
        }
    }

    pub(crate) fn prim_restore(&mut self) {
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

    pub(crate) fn restore_snapshot(&mut self, snap: persist::VmSnapshot) {
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

    pub(crate) fn check_auto_save(&mut self) {
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

}
