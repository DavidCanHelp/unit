// discovery.rs — Problem detection for emergent problem-solving
//
// Detects problems when Forth evaluation fails: goal task failures,
// distributed sub-goal timeouts/errors, and manual reports. Detected
// problems are queued for registration as challenges.

use crate::evolve;
use crate::features::mutation::SimpleRng;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum ProblemSource {
    GoalFailure { goal_id: u64, task_id: u64 },
    DistGoalTimeout { goal_id: u64, seq: usize },
    DistGoalError { goal_id: u64, seq: usize },
    Manual { description: String },
}

#[derive(Clone, Debug)]
pub struct DiscoveredProblem {
    pub source: ProblemSource,
    pub failed_code: String,
    pub expected_output: Option<String>,
    pub error_message: String,
    pub timestamp: u64,
}

// ---------------------------------------------------------------------------
// Simple hash for dedup (FNV-1a inspired, no dependencies)
// ---------------------------------------------------------------------------

fn hash_code(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---------------------------------------------------------------------------
// ProblemDetector
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ProblemDetector {
    pending: Vec<DiscoveredProblem>,
    max_pending: usize,
    recent_hashes: HashSet<u64>,
    max_hashes: usize,
}

impl Default for ProblemDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ProblemDetector {
    pub fn new() -> Self {
        ProblemDetector {
            pending: Vec::new(),
            max_pending: 20,
            recent_hashes: HashSet::new(),
            max_hashes: 50,
        }
    }

    fn is_duplicate(&mut self, code: &str) -> bool {
        let h = hash_code(code);
        if self.recent_hashes.contains(&h) {
            return true;
        }
        // Rotate out if full.
        if self.recent_hashes.len() >= self.max_hashes {
            self.recent_hashes.clear();
        }
        self.recent_hashes.insert(h);
        false
    }

    fn push(&mut self, problem: DiscoveredProblem) {
        if self.pending.len() < self.max_pending {
            self.pending.push(problem);
        }
    }

    pub fn detect_goal_failure(
        &mut self,
        goal_id: u64,
        task_id: u64,
        code: &str,
        error: &str,
        expected_output: Option<&str>,
    ) {
        if code.split_whitespace().count() < 3 {
            return;
        }
        if self.is_duplicate(code) {
            return;
        }
        self.push(DiscoveredProblem {
            source: ProblemSource::GoalFailure { goal_id, task_id },
            failed_code: code.to_string(),
            expected_output: expected_output.map(|s| s.to_string()),
            error_message: error.to_string(),
            timestamp: 0,
        });
    }

    pub fn detect_dist_timeout(&mut self, goal_id: u64, seq: usize, expr: &str) {
        if self.is_duplicate(expr) {
            return;
        }
        self.push(DiscoveredProblem {
            source: ProblemSource::DistGoalTimeout { goal_id, seq },
            failed_code: expr.to_string(),
            expected_output: None,
            error_message: "timeout".to_string(),
            timestamp: 0,
        });
    }

    pub fn detect_dist_error(&mut self, goal_id: u64, seq: usize, expr: &str, error: &str) {
        if self.is_duplicate(expr) {
            return;
        }
        self.push(DiscoveredProblem {
            source: ProblemSource::DistGoalError { goal_id, seq },
            failed_code: expr.to_string(),
            expected_output: None,
            error_message: error.to_string(),
            timestamp: 0,
        });
    }

    pub fn detect_manual(&mut self, code: &str, description: &str) {
        if self.is_duplicate(code) {
            return;
        }
        self.push(DiscoveredProblem {
            source: ProblemSource::Manual {
                description: description.to_string(),
            },
            failed_code: code.to_string(),
            expected_output: None,
            error_message: "manual report".to_string(),
            timestamp: 0,
        });
    }

    pub fn drain_pending(&mut self) -> Vec<DiscoveredProblem> {
        std::mem::take(&mut self.pending)
    }

    /// Convert a discovered problem into challenge registration parameters.
    /// Returns (name, description, target_output, test_input, seed_programs, reward).
    pub fn problem_to_challenge_params(
        problem: &DiscoveredProblem,
    ) -> (String, String, String, Option<String>, Vec<String>, i64) {
        let h = hash_code(&problem.failed_code);
        let name = format!("auto-{:08x}", h & 0xFFFFFFFF);
        let desc = match &problem.source {
            ProblemSource::GoalFailure { .. } => {
                format!("goal task failed: {}", problem.error_message)
            }
            ProblemSource::DistGoalTimeout { .. } => "distributed sub-goal timeout".into(),
            ProblemSource::DistGoalError { .. } => {
                format!("distributed sub-goal error: {}", problem.error_message)
            }
            ProblemSource::Manual { description } => description.clone(),
        };
        let target = problem.expected_output.clone().unwrap_or_default();

        // Generate seed programs: the failed code + mutations.
        let mut seeds = vec![problem.failed_code.clone()];
        let mut rng = SimpleRng::new(h);
        for _ in 0..2 {
            seeds.push(evolve::mutate(&problem.failed_code, &mut rng));
        }

        let reward = match problem.source {
            ProblemSource::Manual { .. } => 100,
            ProblemSource::GoalFailure { .. } => 50,
            _ => 30,
        };

        (name, desc, target, None, seeds, reward)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_goal_failure() {
        let mut det = ProblemDetector::new();
        det.detect_goal_failure(
            1,
            2,
            "10 0 DO I . LOOP",
            "timeout",
            Some("0 1 2 3 4 5 6 7 8 9"),
        );
        assert_eq!(det.pending.len(), 1);
        assert_eq!(det.pending[0].failed_code, "10 0 DO I . LOOP");
    }

    #[test]
    fn test_duplicate_ignored() {
        let mut det = ProblemDetector::new();
        det.detect_goal_failure(1, 2, "10 0 DO I . LOOP", "err", None);
        det.detect_goal_failure(1, 3, "10 0 DO I . LOOP", "err", None);
        assert_eq!(det.pending.len(), 1);
    }

    #[test]
    fn test_short_code_ignored() {
        let mut det = ProblemDetector::new();
        det.detect_goal_failure(1, 2, "42", "err", None);
        assert_eq!(det.pending.len(), 0);
    }

    #[test]
    fn test_drain_pending() {
        let mut det = ProblemDetector::new();
        det.detect_manual("1 2 + 3 * .", "test problem");
        assert_eq!(det.pending.len(), 1);
        let drained = det.drain_pending();
        assert_eq!(drained.len(), 1);
        assert_eq!(det.pending.len(), 0);
    }

    #[test]
    fn test_max_pending_cap() {
        let mut det = ProblemDetector::new();
        for i in 0..30 {
            det.detect_manual(&format!("code {} {} {}", i, i + 1, i + 2), "test");
        }
        assert_eq!(det.pending.len(), 20);
    }

    #[test]
    fn test_problem_to_challenge_params() {
        let problem = DiscoveredProblem {
            source: ProblemSource::GoalFailure {
                goal_id: 1,
                task_id: 2,
            },
            failed_code: "10 0 DO I . LOOP".to_string(),
            expected_output: Some("0 1 2 3 4 5 6 7 8 9 ".to_string()),
            error_message: "timeout".to_string(),
            timestamp: 0,
        };
        let (name, desc, target, _test_input, seeds, reward) =
            ProblemDetector::problem_to_challenge_params(&problem);
        assert!(name.starts_with("auto-"));
        assert!(desc.contains("goal task failed"));
        assert_eq!(target, "0 1 2 3 4 5 6 7 8 9 ");
        assert!(seeds.len() >= 3);
        assert_eq!(seeds[0], "10 0 DO I . LOOP");
        assert_eq!(reward, 50);
    }

    #[test]
    fn test_dist_timeout_detection() {
        let mut det = ProblemDetector::new();
        det.detect_dist_timeout(5, 2, "99 99 * . complex");
        assert_eq!(det.pending.len(), 1);
        match &det.pending[0].source {
            ProblemSource::DistGoalTimeout { goal_id, seq } => {
                assert_eq!(*goal_id, 5);
                assert_eq!(*seq, 2);
            }
            _ => panic!("wrong source type"),
        }
    }
}
