//! Atom & orchestration primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Atom primitives (raw data for Forth-level orchestration)
    // -----------------------------------------------------------------------

    /// GOAL-COUNT ( -- total pending active completed failed )
    pub(crate) fn prim_goal_count(&mut self) {
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
    pub(crate) fn prim_task_count(&mut self) {
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
    pub(crate) fn prim_mesh_avg_fitness(&mut self) {
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
    pub(crate) fn prim_check_watches(&mut self) {
        let due = self.monitor.due_watches();
        for wid in due {
            self.run_watch_check(wid);
        }
    }

    /// RUN-HANDLERS ( -- ) run alert handlers for active alerts.
    pub(crate) fn prim_run_handlers(&mut self) {
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
    pub(crate) fn prim_mutate_random_atom(&mut self) {
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

}
