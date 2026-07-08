//! Distributed goal primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Distributed goal primitives
    // -----------------------------------------------------------------------

    /// DIST-GOAL{ expr1 | expr2 | ... } — distribute and compute.
    pub(crate) fn prim_dist_goal(&mut self) {
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

    pub(crate) fn prim_dist_status(&mut self) {
        let s = self.dist_engine.format_status();
        self.emit_str(&s);
    }

    pub(crate) fn prim_dist_cancel(&mut self) {
        self.dist_engine.goals.clear();
        self.emit_str("all distributed goals cancelled\n");
    }

}
