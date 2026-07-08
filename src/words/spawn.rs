//! Spawn / replication primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Spawn / Replication primitives
    // -----------------------------------------------------------------------

    pub(crate) fn build_state_for_spawn(&self) -> Vec<u8> {
        let snap = self.make_snapshot();
        persist::serialize_snapshot(&snap)
    }

    pub(crate) fn prim_spawn(&mut self) {
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
        // Binding-constraint ceiling: refuse to replicate onto a host at/over
        // 80% utilization, and fail closed if the host can't be measured. The
        // pre-existing quarantine/max_children/cooldown guards still apply.
        let res = crate::resources::HostResources::measure();
        if let Err(e) = self.spawn_state.can_spawn_within(&res) {
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

    pub(crate) fn prim_spawn_n(&mut self) {
        // Validate before popping: a bare SPAWN-N must fail clean rather than
        // act on pop's substitute-0 or a leftover stack value.
        if self.stack.is_empty() {
            self.fault = Some(vm::Fault::StackUnderflow);
            self.emit_str("SPAWN-N: stack underflow (expected n)\n");
            return;
        }
        let n = self.pop();
        // Bound n: a negative cell cast to usize is ~2^64, which would loop
        // effectively forever on energy-refusal no-ops. max_children is the
        // most a batch could ever land anyway.
        if n < 1 || n > self.spawn_state.max_children as Cell {
            self.emit_str(&format!(
                "SPAWN-N: n out of range (expected 1..={}, got {})\n",
                self.spawn_state.max_children, n
            ));
            return;
        }
        let n = n as usize;
        for i in 0..n {
            self.prim_spawn();
            // Override cooldown for batch spawns.
            if i < n - 1 {
                self.spawn_state.last_spawn = None;
            }
        }
    }

    pub(crate) fn prim_package(&mut self) {
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

    pub(crate) fn prim_package_size(&mut self) {
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

    pub(crate) fn prim_children(&mut self) {
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

    pub(crate) fn prim_family(&mut self) {
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

    pub(crate) fn prim_kill_child(&mut self) {
        // Validate before popping: a bare KILL-CHILD must fail clean. pop's
        // substitute-0 here would mean kill(0, SIGTERM) — the entire process
        // group — and a leftover stack value is a SIGTERM aimed at an
        // arbitrary host process.
        if self.stack.is_empty() {
            self.fault = Some(vm::Fault::StackUnderflow);
            self.emit_str("KILL-CHILD: stack underflow (expected child pid)\n");
            return;
        }
        let pid = self.pop() as u32;
        // Never signal a pid this node didn't spawn. This is the real safety
        // boundary: depth validation can't tell a leftover value from an
        // intended argument, but the children ledger can.
        if !self.spawn_state.children.iter().any(|c| c.pid == pid) {
            self.emit_str(&format!(
                "KILL-CHILD: pid {} is not a child of this node (no signal sent)\n",
                pid
            ));
            return;
        }
        #[cfg(unix)]
        {
            unsafe {
                crate::libc_kill(pid as i32, 15); // SIGTERM
            }
        }
        self.spawn_state.children.retain(|c| c.pid != pid);
        self.emit_str(&format!("sent SIGTERM to pid {}\n", pid));
    }

    pub(crate) fn prim_replicate_to(&mut self) {
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
    pub(crate) fn check_incoming_replications(&mut self) {
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

}
