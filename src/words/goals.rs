//! Goal & task-decomposition primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Goal primitives
    // -----------------------------------------------------------------------

    /// GOAL" `<description>`" ( priority -- goal-id ) submit a description-only goal.
    pub(crate) fn prim_goal(&mut self) {
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
    pub(crate) fn prim_goals(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_goals();
            self.emit_str(&s);
        } else {
            self.emit_str("  (mesh offline)\n");
        }
    }

    /// TASKS ( -- ) list this unit's current task queue.
    pub(crate) fn prim_tasks(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_tasks();
            self.emit_str(&s);
        } else {
            self.emit_str("  (mesh offline)\n");
        }
    }

    /// TASK-STATUS ( goal-id -- ) show task breakdown for a specific goal.
    pub(crate) fn prim_task_status(&mut self) {
        let goal_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            print!("{}", m.format_goal_tasks(goal_id));
            let _ = io::stdout().flush();
        } else {
            eprintln!("TASK-STATUS: mesh offline");
        }
    }

    /// CANCEL ( goal-id -- ) cancel a goal and all its tasks.
    pub(crate) fn prim_cancel(&mut self) {
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
    pub(crate) fn prim_steer(&mut self) {
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
    pub(crate) fn prim_report(&mut self) {
        if let Some(ref m) = self.mesh {
            print!("{}", m.format_report());
            let _ = io::stdout().flush();
        } else {
            println!("  (mesh offline)");
        }
    }

    /// CLAIM ( -- task-id ) claim the next available task, or 0 if none.
    /// CLAIM ( -- task-id ) claim and execute the next available task.
    pub(crate) fn prim_claim(&mut self) {
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
    pub(crate) fn prim_complete(&mut self) {
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
    pub(crate) fn prim_goal_exec(&mut self) {
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
    pub(crate) fn rt_goal_exec(&mut self) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let code = self.code_strings[idx].clone();
            self.create_exec_goal(&code);
        } else {
            eprintln!("GOAL{{: invalid code index");
            self.stack.push(0);
        }
    }

    pub(crate) fn create_exec_goal(&mut self, code: &str) {
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
    pub(crate) fn prim_eval(&mut self) {
        let code = self.parse_until('"');
        self.interpret_line(&code);
    }

    /// RESULT ( task-id -- ) display the result of a completed task.
    pub(crate) fn prim_result(&mut self) {
        let task_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            let s = m.format_task_result(task_id);
            self.emit_str(&s);
        } else {
            eprintln!("RESULT: mesh offline");
        }
    }

    /// AUTO-CLAIM ( -- ) toggle automatic task claiming and execution.
    pub(crate) fn prim_auto_claim(&mut self) {
        self.auto_claim = !self.auto_claim;
        if !self.silent {
            println!("auto-claim: {}", if self.auto_claim { "ON" } else { "OFF" });
        }
    }

    /// TIMEOUT ( seconds -- ) set execution timeout for sandboxed tasks.
    pub(crate) fn prim_timeout(&mut self) {
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
    pub(crate) fn prim_goal_result(&mut self) {
        let goal_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            let s = m.format_goal_result(goal_id);
            self.emit_str(&s);
        } else {
            eprintln!("GOAL-RESULT: mesh offline");
        }
    }

    /// Check for and execute auto-claimed tasks.
    pub(crate) fn check_auto_claim(&mut self) {
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
    pub(crate) fn check_auto_replicate(&mut self) {
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
    // Task decomposition primitives
    // -----------------------------------------------------------------------

    /// SUBTASK{ `<code>` } ( goal-id -- task-id ) add a subtask to a goal.
    pub(crate) fn prim_subtask(&mut self) {
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
    pub(crate) fn prim_fork(&mut self) {
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
    pub(crate) fn prim_results(&mut self) {
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
    pub(crate) fn prim_reduce(&mut self) {
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

    pub(crate) fn rt_reduce(&mut self) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let code = self.code_strings[idx].clone();
            self.do_reduce(&code);
        }
    }

    pub(crate) fn do_reduce(&mut self, reduce_code: &str) {
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
    pub(crate) fn prim_progress(&mut self) {
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

}
