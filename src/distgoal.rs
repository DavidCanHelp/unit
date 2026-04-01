// distgoal.rs — Distributed goal computation for unit
//
// A unit breaks a problem into sub-goals, distributes them as S-expressions
// to mesh peers, collects results, and combines the answer. Map-reduce
// with nanobots.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

pub type GoalId = u64;

#[derive(Clone, Debug, PartialEq)]
pub enum DistStatus {
    Pending,
    Running,
    Complete,
    Failed,
}

#[derive(Clone, Debug)]
pub struct SubGoal {
    pub seq: usize,
    pub expr: String,        // Forth code to evaluate
    pub assigned_to: String, // "local" or peer hex ID
    pub result: Option<String>,
    pub sent_at: u64,        // counter-based (not time)
}

#[derive(Clone, Debug)]
pub struct DistGoal {
    pub id: GoalId,
    pub parent_id: String,   // node that initiated
    pub sub_goals: Vec<SubGoal>,
    pub status: DistStatus,
    pub combiner: Combiner,
    pub tick: u64,            // monotonic counter for timeouts
}

#[derive(Clone, Debug)]
pub enum Combiner {
    List,    // collect all results as a list
    Sum,     // sum numeric results
    Concat,  // concatenate output strings
}

#[derive(Clone, Debug, Default)]
pub struct DistEngine {
    pub goals: HashMap<GoalId, DistGoal>,
    next_id: u64,
    pub tick: u64,
    pub timeout_ticks: u64, // ticks before fallback (default ~50)
}

impl DistEngine {
    pub fn new() -> Self {
        DistEngine {
            goals: HashMap::new(),
            next_id: 1,
            tick: 0,
            timeout_ticks: 50,
        }
    }

    pub fn next_id(&mut self) -> GoalId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn advance_tick(&mut self) {
        self.tick += 1;
    }

    /// Create a new distributed goal from pipe-separated expressions.
    pub fn create_goal(
        &mut self,
        expressions: Vec<String>,
        parent_id: &str,
        peer_ids: &[String], // available peer hex IDs
    ) -> GoalId {
        let id = self.next_id();
        let mut sub_goals = Vec::new();
        let all_workers: Vec<String> = {
            let mut w = vec!["local".to_string()];
            w.extend(peer_ids.iter().cloned());
            w
        };

        for (i, expr) in expressions.iter().enumerate() {
            let worker = &all_workers[i % all_workers.len()];
            sub_goals.push(SubGoal {
                seq: i,
                expr: expr.trim().to_string(),
                assigned_to: worker.clone(),
                result: None,
                sent_at: self.tick,
            });
        }

        let goal = DistGoal {
            id,
            parent_id: parent_id.to_string(),
            sub_goals,
            status: DistStatus::Running,
            combiner: Combiner::List,
            tick: self.tick,
        };
        self.goals.insert(id, goal);
        id
    }

    /// Record a result for a sub-goal.
    pub fn record_result(&mut self, goal_id: GoalId, seq: usize, result: &str) -> bool {
        if let Some(goal) = self.goals.get_mut(&goal_id) {
            if let Some(sg) = goal.sub_goals.iter_mut().find(|sg| sg.seq == seq) {
                sg.result = Some(result.to_string());
                // Check if all done
                if goal.sub_goals.iter().all(|sg| sg.result.is_some()) {
                    goal.status = DistStatus::Complete;
                }
                return true;
            }
        }
        false
    }

    /// Get sub-goals that need to be sent to remote peers.
    pub fn pending_remote_subgoals(&self, goal_id: GoalId) -> Vec<(usize, String, String)> {
        // Returns (seq, expr, peer_id) for sub-goals assigned to non-local peers
        if let Some(goal) = self.goals.get(&goal_id) {
            goal.sub_goals.iter()
                .filter(|sg| sg.assigned_to != "local" && sg.result.is_none())
                .map(|sg| (sg.seq, sg.expr.clone(), sg.assigned_to.clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get sub-goals assigned to "local" that haven't been computed yet.
    pub fn pending_local_subgoals(&self, goal_id: GoalId) -> Vec<(usize, String)> {
        if let Some(goal) = self.goals.get(&goal_id) {
            goal.sub_goals.iter()
                .filter(|sg| sg.assigned_to == "local" && sg.result.is_none())
                .map(|sg| (sg.seq, sg.expr.clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get sub-goals that have timed out (assigned to remote, no result after timeout_ticks).
    pub fn timed_out_subgoals(&self, goal_id: GoalId) -> Vec<(usize, String)> {
        if let Some(goal) = self.goals.get(&goal_id) {
            goal.sub_goals.iter()
                .filter(|sg| sg.assigned_to != "local" && sg.result.is_none()
                    && self.tick - sg.sent_at > self.timeout_ticks)
                .map(|sg| (sg.seq, sg.expr.clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Reassign a timed-out sub-goal to local.
    pub fn fallback_to_local(&mut self, goal_id: GoalId, seq: usize) {
        if let Some(goal) = self.goals.get_mut(&goal_id) {
            if let Some(sg) = goal.sub_goals.iter_mut().find(|sg| sg.seq == seq) {
                sg.assigned_to = "local".to_string();
                sg.sent_at = self.tick;
            }
        }
    }

    /// Is the goal complete?
    pub fn is_complete(&self, goal_id: GoalId) -> bool {
        self.goals.get(&goal_id)
            .map_or(false, |g| g.status == DistStatus::Complete)
    }

    /// Combine results into final output.
    pub fn combine_results(&self, goal_id: GoalId) -> Option<String> {
        let goal = self.goals.get(&goal_id)?;
        let results: Vec<String> = goal.sub_goals.iter()
            .filter_map(|sg| sg.result.clone())
            .collect();
        if results.len() != goal.sub_goals.len() {
            return None; // not all results in
        }
        Some(match goal.combiner {
            Combiner::List => results.join(" "),
            Combiner::Sum => {
                let total: i64 = results.iter()
                    .filter_map(|r| r.trim().parse::<i64>().ok())
                    .sum();
                format!("{}", total)
            }
            Combiner::Concat => results.join(""),
        })
    }

    /// Format status for display.
    pub fn format_status(&self) -> String {
        if self.goals.is_empty() {
            return "no distributed goals\n".to_string();
        }
        let mut out = String::new();
        for (id, goal) in &self.goals {
            let done = goal.sub_goals.iter().filter(|sg| sg.result.is_some()).count();
            let total = goal.sub_goals.len();
            out.push_str(&format!(
                "goal #{}: {:?} ({}/{} complete)\n",
                id, goal.status, done, total
            ));
            for sg in &goal.sub_goals {
                let status = if sg.result.is_some() { "done" } else { "pending" };
                out.push_str(&format!(
                    "  [{}] {} -> {} ({})\n",
                    sg.seq,
                    if sg.expr.len() > 30 { format!("{}...", &sg.expr[..30]) } else { sg.expr.clone() },
                    sg.assigned_to,
                    status
                ));
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Parse pipe-separated expressions from DIST-GOAL{ ... }
// ---------------------------------------------------------------------------

pub fn parse_pipe_expressions(input: &str) -> Vec<String> {
    input.split('|')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// S-expression message constructors
// ---------------------------------------------------------------------------

pub fn sexp_sub_goal(goal_id: GoalId, seq: usize, from: &str, expr: &str) -> String {
    format!(
        "(sub-goal :id {} :seq {} :from \"{}\" :expr \"{}\")",
        goal_id, seq, from, expr.replace('"', "\\\"")
    )
}

pub fn sexp_sub_result(goal_id: GoalId, seq: usize, from: &str, result: &str) -> String {
    format!(
        "(sub-result :id {} :seq {} :from \"{}\" :result \"{}\")",
        goal_id, seq, from, result.replace('"', "\\\"")
    )
}

pub fn sexp_dist_complete(goal_id: GoalId, results: &str, peers: usize) -> String {
    format!(
        "(dist-complete :id {} :results \"{}\" :peers {})",
        goal_id, results.replace('"', "\\\""), peers
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pipe_expressions() {
        let exprs = parse_pipe_expressions("10 10 * | 20 20 * | 30 30 *");
        assert_eq!(exprs.len(), 3);
        assert_eq!(exprs[0], "10 10 *");
        assert_eq!(exprs[1], "20 20 *");
        assert_eq!(exprs[2], "30 30 *");
    }

    #[test]
    fn test_local_fallback() {
        let mut eng = DistEngine::new();
        let id = eng.create_goal(
            vec!["1 2 +".into(), "3 4 +".into()],
            "self",
            &[], // no peers
        );
        // All should be local
        let local = eng.pending_local_subgoals(id);
        assert_eq!(local.len(), 2);
        assert!(eng.pending_remote_subgoals(id).is_empty());
    }

    #[test]
    fn test_round_robin() {
        let mut eng = DistEngine::new();
        let peers = vec!["aaa".into(), "bbb".into()];
        let id = eng.create_goal(
            vec!["a".into(), "b".into(), "c".into(), "d".into(), "e".into(), "f".into()],
            "self",
            &peers,
        );
        let goal = &eng.goals[&id];
        // 3 workers: local, aaa, bbb → round robin
        assert_eq!(goal.sub_goals[0].assigned_to, "local");
        assert_eq!(goal.sub_goals[1].assigned_to, "aaa");
        assert_eq!(goal.sub_goals[2].assigned_to, "bbb");
        assert_eq!(goal.sub_goals[3].assigned_to, "local");
        assert_eq!(goal.sub_goals[4].assigned_to, "aaa");
        assert_eq!(goal.sub_goals[5].assigned_to, "bbb");
    }

    #[test]
    fn test_result_collection() {
        let mut eng = DistEngine::new();
        let id = eng.create_goal(vec!["a".into(), "b".into()], "self", &[]);
        assert!(!eng.is_complete(id));
        eng.record_result(id, 0, "100");
        assert!(!eng.is_complete(id));
        eng.record_result(id, 1, "200");
        assert!(eng.is_complete(id));
    }

    #[test]
    fn test_combine_list() {
        let mut eng = DistEngine::new();
        let id = eng.create_goal(vec!["a".into(), "b".into(), "c".into()], "self", &[]);
        eng.record_result(id, 0, "100");
        eng.record_result(id, 1, "200");
        eng.record_result(id, 2, "300");
        assert_eq!(eng.combine_results(id), Some("100 200 300".into()));
    }

    #[test]
    fn test_combine_sum() {
        let mut eng = DistEngine::new();
        let id = eng.create_goal(vec!["a".into(), "b".into()], "self", &[]);
        eng.goals.get_mut(&id).unwrap().combiner = Combiner::Sum;
        eng.record_result(id, 0, "100");
        eng.record_result(id, 1, "200");
        assert_eq!(eng.combine_results(id), Some("300".into()));
    }

    #[test]
    fn test_timeout_detection() {
        let mut eng = DistEngine::new();
        eng.timeout_ticks = 5;
        let peers = vec!["aaa".into()];
        let id = eng.create_goal(vec!["a".into(), "b".into()], "self", &peers);
        // Advance ticks past timeout
        for _ in 0..10 { eng.advance_tick(); }
        let timed_out = eng.timed_out_subgoals(id);
        // Only the remote one (assigned to "aaa") should time out
        assert_eq!(timed_out.len(), 1);
        assert_eq!(timed_out[0].0, 1); // seq 1 was assigned to aaa
    }

    #[test]
    fn test_fallback_to_local() {
        let mut eng = DistEngine::new();
        eng.timeout_ticks = 5;
        let peers = vec!["aaa".into()];
        let id = eng.create_goal(vec!["x".into(), "y".into()], "self", &peers);
        for _ in 0..10 { eng.advance_tick(); }
        eng.fallback_to_local(id, 1);
        let goal = &eng.goals[&id];
        assert_eq!(goal.sub_goals[1].assigned_to, "local");
    }

    #[test]
    fn test_sexp_messages() {
        let sg = sexp_sub_goal(42, 0, "aaa", "10 10 *");
        assert!(sg.contains("sub-goal"));
        assert!(sg.contains(":id 42"));
        assert!(sg.contains(":expr \"10 10 *\""));

        let sr = sexp_sub_result(42, 0, "bbb", "100");
        assert!(sr.contains("sub-result"));
        assert!(sr.contains(":result \"100\""));
    }

    #[test]
    fn test_single_subgoal() {
        let mut eng = DistEngine::new();
        let id = eng.create_goal(vec!["42 .".into()], "self", &[]);
        eng.record_result(id, 0, "42");
        assert!(eng.is_complete(id));
        assert_eq!(eng.combine_results(id), Some("42".into()));
    }

    #[test]
    fn test_empty_expressions() {
        let exprs = parse_pipe_expressions("");
        assert!(exprs.is_empty());
    }
}
