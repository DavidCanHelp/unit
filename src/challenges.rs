// challenges.rs — Challenge registry for emergent problem-solving
//
// Generalizes FitnessChallenge beyond the hardcoded fib10. Challenges can
// be built-in, discovered from failures, or received from mesh peers.
// The GP engine evolves solutions; solutions become dictionary words.

use std::collections::HashMap;
use crate::evolve::FitnessChallenge;
use crate::mesh::NodeId;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

pub type ChallengeId = u64;

#[derive(Clone, Debug, PartialEq)]
pub enum ChallengeOrigin {
    BuiltIn,
    Discovered {
        source_node: NodeId,
        discovered_at: u64,
    },
}

#[derive(Clone, Debug)]
pub struct Challenge {
    pub id: ChallengeId,
    pub name: String,
    pub description: String,
    pub target_output: String,
    pub test_input: Option<String>,
    pub max_steps: usize,
    pub seed_programs: Vec<String>,
    pub origin: ChallengeOrigin,
    pub reward: i64,
    pub solved: bool,
    pub solution: Option<String>,
    pub solver: Option<NodeId>,
    pub attempts: u32,
    pub solutions: Vec<(String, NodeId)>, // all verified solutions (program, solver)
}

// ---------------------------------------------------------------------------
// ChallengeRegistry
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ChallengeRegistry {
    pub challenges: HashMap<ChallengeId, Challenge>,
    id_counter: u64,
    pub active_challenge: Option<ChallengeId>,
}

impl ChallengeRegistry {
    pub fn new(node_id: &NodeId) -> Self {
        // Seed counter from node ID to avoid collisions (same pattern as GoalRegistry).
        let base = ((node_id[4] as u64) << 4 | (node_id[5] as u64 >> 4)) * 10 + 1;
        ChallengeRegistry {
            challenges: HashMap::new(),
            id_counter: base,
            active_challenge: None,
        }
    }

    fn next_id(&mut self) -> ChallengeId {
        let id = self.id_counter;
        self.id_counter += 1;
        id
    }

    pub fn register_builtin(
        &mut self,
        name: &str,
        target_output: &str,
        seeds: Vec<String>,
    ) -> ChallengeId {
        let id = self.next_id();
        self.challenges.insert(id, Challenge {
            id,
            name: name.to_string(),
            description: format!("built-in challenge: {}", name),
            target_output: target_output.to_string(),
            test_input: None,
            max_steps: 10000,
            seed_programs: seeds,
            origin: ChallengeOrigin::BuiltIn,
            reward: 100,
            solved: false,
            solution: None,
            solver: None,
            attempts: 0,
            solutions: vec![],
        });
        id
    }

    #[allow(clippy::too_many_arguments)]
    pub fn register_discovered(
        &mut self,
        name: &str,
        desc: &str,
        target_output: &str,
        test_input: Option<String>,
        seed_programs: Vec<String>,
        source_node: NodeId,
        reward: i64,
    ) -> ChallengeId {
        let id = self.next_id();
        self.challenges.insert(id, Challenge {
            id,
            name: name.to_string(),
            description: desc.to_string(),
            target_output: target_output.to_string(),
            test_input,
            max_steps: 10000,
            seed_programs,
            origin: ChallengeOrigin::Discovered {
                source_node,
                discovered_at: 0,
            },
            reward,
            solved: false,
            solution: None,
            solver: None,
            attempts: 0,
            solutions: vec![],
        });
        id
    }

    pub fn mark_solved(&mut self, id: ChallengeId, solution: &str, solver: NodeId) -> bool {
        if let Some(ch) = self.challenges.get_mut(&id) {
            let is_first = !ch.solved;
            if is_first {
                ch.solved = true;
                ch.solution = Some(solution.to_string());
                ch.solver = Some(solver);
            }
            // Track all distinct solutions (cap at 20).
            if ch.solutions.len() < 20
                && !ch.solutions.iter().any(|(p, _)| p == solution)
            {
                ch.solutions.push((solution.to_string(), solver));
            }
            return is_first;
        }
        false
    }

    pub fn solution_count(&self, id: ChallengeId) -> usize {
        self.challenges.get(&id).map_or(0, |c| c.solutions.len())
    }

    pub fn format_solutions(&self, id: ChallengeId) -> String {
        match self.challenges.get(&id) {
            Some(ch) if !ch.solutions.is_empty() => {
                let mut out = format!("--- {} solutions for {} ---\n", ch.solutions.len(), ch.name);
                for (i, (prog, solver)) in ch.solutions.iter().enumerate() {
                    let tokens = prog.split_whitespace().count();
                    out.push_str(&format!(
                        "  {}. \"{}\" ({} tokens) from {}\n",
                        i + 1, prog, tokens, crate::mesh::id_to_hex(solver)
                    ));
                }
                out
            }
            _ => format!("no solutions for challenge #{}\n", id),
        }
    }

    pub fn colony_diversity(&self) -> String {
        let solved: Vec<&Challenge> = self.challenges.values().filter(|c| c.solved).collect();
        if solved.is_empty() { return "no solved challenges yet\n".to_string(); }
        let total_solutions: usize = solved.iter().map(|c| c.solutions.len()).sum();
        let avg = total_solutions as f64 / solved.len() as f64;
        let most_diverse = solved.iter().max_by_key(|c| c.solutions.len());
        let mut out = format!(
            "--- colony diversity ---\nchallenges solved: {}\ntotal solutions: {}\navg solutions per challenge: {:.1}\n",
            solved.len(), total_solutions, avg
        );
        if let Some(md) = most_diverse {
            out.push_str(&format!("most diverse: {} ({} solutions)\n", md.name, md.solutions.len()));
        }
        out
    }

    pub fn get_unsolved(&self) -> Vec<&Challenge> {
        let mut unsolved: Vec<&Challenge> = self.challenges.values()
            .filter(|c| !c.solved)
            .collect();
        unsolved.sort_by(|a, b| b.reward.cmp(&a.reward));
        unsolved
    }

    pub fn get_challenge(&self, id: ChallengeId) -> Option<&Challenge> {
        self.challenges.get(&id)
    }

    /// Convert a Challenge to the FitnessChallenge format consumed by the GP engine.
    pub fn to_fitness_challenge(&self, id: ChallengeId) -> Option<FitnessChallenge> {
        let ch = self.challenges.get(&id)?;
        Some(FitnessChallenge {
            name: ch.name.clone(),
            target_output: ch.target_output.clone(),
            max_steps: ch.max_steps,
            seed_programs: ch.seed_programs.clone(),
        })
    }

    /// Merge a challenge received from a peer. Accept if new or if solved
    /// status is more advanced (same pattern as GoalRegistry::merge_goal).
    pub fn merge_challenge(&mut self, challenge: Challenge) {
        if let Some(existing) = self.challenges.get(&challenge.id) {
            // Update if incoming is solved and ours isn't.
            if challenge.solved && !existing.solved {
                self.challenges.insert(challenge.id, challenge);
            }
        } else {
            if challenge.id >= self.id_counter {
                self.id_counter = challenge.id + 1;
            }
            self.challenges.insert(challenge.id, challenge);
        }
    }

    pub fn format_challenges(&self) -> String {
        if self.challenges.is_empty() {
            return "no challenges\n".to_string();
        }
        let mut out = format!("--- {} challenges ---\n", self.challenges.len());
        let mut sorted: Vec<&Challenge> = self.challenges.values().collect();
        sorted.sort_by_key(|c| c.id);
        for ch in sorted {
            let status = if ch.solved { "SOLVED" } else { "unsolved" };
            out.push_str(&format!(
                "  #{} {} [{}] reward={} attempts={}\n",
                ch.id, ch.name, status, ch.reward, ch.attempts
            ));
            if let Some(ref sol) = ch.solution {
                out.push_str(&format!("    solution: {}\n", sol));
            }
        }
        if let Some(active) = self.active_challenge {
            out.push_str(&format!("active: #{}\n", active));
        }
        out
    }

    pub fn active(&self) -> Option<&Challenge> {
        self.active_challenge.and_then(|id| self.challenges.get(&id))
    }

    pub fn set_active(&mut self, id: ChallengeId) -> bool {
        if self.challenges.contains_key(&id) {
            self.active_challenge = Some(id);
            true
        } else {
            false
        }
    }

    /// Pick the highest-reward unsolved challenge and set it as active.
    pub fn next_unsolved(&mut self) -> Option<ChallengeId> {
        let best = self.get_unsolved().first().map(|c| c.id);
        if let Some(id) = best {
            self.active_challenge = Some(id);
        }
        best
    }
}

// ---------------------------------------------------------------------------
// S-expression constructors
// ---------------------------------------------------------------------------

pub fn sexp_challenge_broadcast(ch: &Challenge) -> String {
    let seeds: Vec<String> = ch.seed_programs.iter()
        .map(|s| format!("\"{}\"", s.replace('"', "\\\"")))
        .collect();
    format!(
        "(challenge :id {} :name \"{}\" :desc \"{}\" :target \"{}\" :reward {} :seeds ({}))",
        ch.id,
        ch.name.replace('"', "\\\""),
        ch.description.replace('"', "\\\""),
        ch.target_output.replace('"', "\\\""),
        ch.reward,
        seeds.join(" ")
    )
}

pub fn sexp_solution_broadcast(challenge_id: ChallengeId, solution: &str, solver_hex: &str) -> String {
    format!(
        "(solution :challenge-id {} :program \"{}\" :solver \"{}\")",
        challenge_id,
        solution.replace('"', "\\\""),
        solver_hex
    )
}

// ---------------------------------------------------------------------------
// Conversion from existing fib10
// ---------------------------------------------------------------------------

pub fn fib10_as_challenge() -> Challenge {
    let fc = crate::evolve::fib10_challenge();
    Challenge {
        id: 0, // will be assigned by register_builtin
        name: fc.name,
        description: "find the shortest program that outputs 55 (10th Fibonacci)".into(),
        target_output: fc.target_output,
        test_input: None,
        max_steps: fc.max_steps,
        seed_programs: fc.seed_programs,
        origin: ChallengeOrigin::BuiltIn,
        reward: 100,
        solved: false,
        solution: None,
        solver: None,
        attempts: 0,
        solutions: vec![],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_node_id() -> NodeId {
        [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]
    }

    #[test]
    fn test_register_and_lookup() {
        let mut reg = ChallengeRegistry::new(&test_node_id());
        let id = reg.register_builtin("test", "42 ", vec!["42 .".into()]);
        assert!(reg.get_challenge(id).is_some());
        assert_eq!(reg.get_challenge(id).unwrap().name, "test");
    }

    #[test]
    fn test_mark_solved_lifecycle() {
        let mut reg = ChallengeRegistry::new(&test_node_id());
        let id = reg.register_builtin("test", "42 ", vec![]);
        assert!(!reg.get_challenge(id).unwrap().solved);
        assert!(reg.mark_solved(id, "42 .", test_node_id()));
        assert!(reg.get_challenge(id).unwrap().solved);
        assert_eq!(reg.get_challenge(id).unwrap().solution.as_deref(), Some("42 ."));
        // Can't solve again
        assert!(!reg.mark_solved(id, "other", test_node_id()));
    }

    #[test]
    fn test_merge_challenge_new() {
        let mut reg = ChallengeRegistry::new(&test_node_id());
        let ch = Challenge {
            id: 999,
            name: "remote".into(),
            description: "from peer".into(),
            target_output: "10 ".into(),
            test_input: None,
            max_steps: 5000,
            seed_programs: vec![],
            origin: ChallengeOrigin::Discovered { source_node: [0; 8], discovered_at: 0 },
            reward: 50,
            solved: false,
            solution: None,
            solver: None,
            attempts: 0,
            solutions: vec![],
        };
        reg.merge_challenge(ch);
        assert!(reg.get_challenge(999).is_some());
        assert!(reg.id_counter >= 1000);
    }

    #[test]
    fn test_merge_challenge_solved_update() {
        let mut reg = ChallengeRegistry::new(&test_node_id());
        let id = reg.register_builtin("test", "42 ", vec![]);
        let mut solved = reg.get_challenge(id).unwrap().clone();
        solved.solved = true;
        solved.solution = Some("42 .".into());
        reg.merge_challenge(solved);
        assert!(reg.get_challenge(id).unwrap().solved);
    }

    #[test]
    fn test_merge_challenge_duplicate_ignore() {
        let mut reg = ChallengeRegistry::new(&test_node_id());
        let id = reg.register_builtin("test", "42 ", vec!["seed".into()]);
        let dup = reg.get_challenge(id).unwrap().clone();
        reg.merge_challenge(dup);
        // Still just one challenge
        assert_eq!(reg.challenges.len(), 1);
    }

    #[test]
    fn test_to_fitness_challenge_roundtrip() {
        let mut reg = ChallengeRegistry::new(&test_node_id());
        let fib = fib10_as_challenge();
        let id = reg.register_builtin(&fib.name, &fib.target_output, fib.seed_programs.clone());
        let fc = reg.to_fitness_challenge(id).unwrap();
        assert_eq!(fc.name, "fib10");
        assert_eq!(fc.target_output, "55 ");
        assert_eq!(fc.seed_programs.len(), 5);
    }

    #[test]
    fn test_get_unsolved_ordering() {
        let mut reg = ChallengeRegistry::new(&test_node_id());
        reg.register_builtin("low", "1 ", vec![]);
        let high_id = reg.register_discovered(
            "high", "important", "99 ", None, vec![], [0; 8], 200
        );
        let unsolved = reg.get_unsolved();
        assert_eq!(unsolved.len(), 2);
        assert_eq!(unsolved[0].id, high_id); // higher reward first
    }

    #[test]
    fn test_next_unsolved_picks_highest() {
        let mut reg = ChallengeRegistry::new(&test_node_id());
        reg.register_builtin("low", "1 ", vec![]);
        let high_id = reg.register_discovered(
            "high", "desc", "99 ", None, vec![], [0; 8], 200
        );
        let picked = reg.next_unsolved();
        assert_eq!(picked, Some(high_id));
        assert_eq!(reg.active_challenge, Some(high_id));
    }

    #[test]
    fn test_sexp_challenge_broadcast() {
        let ch = Challenge {
            id: 42,
            name: "test-challenge".into(),
            description: "a test".into(),
            target_output: "55 ".into(),
            test_input: None,
            max_steps: 10000,
            seed_programs: vec!["0 .".into(), "1 .".into()],
            origin: ChallengeOrigin::BuiltIn,
            reward: 100,
            solved: false,
            solution: None,
            solver: None,
            attempts: 0,
            solutions: vec![],
        };
        let sexp = sexp_challenge_broadcast(&ch);
        assert!(sexp.contains("challenge"));
        assert!(sexp.contains(":id 42"));
        assert!(sexp.contains(":name \"test-challenge\""));
        assert!(sexp.contains(":reward 100"));
        assert!(sexp.contains(":seeds"));
    }

    #[test]
    fn test_sexp_solution_broadcast() {
        let sexp = sexp_solution_broadcast(42, "0 1 10 0 DO OVER + SWAP LOOP DROP .", "aabbccdd");
        assert!(sexp.contains("solution"));
        assert!(sexp.contains(":challenge-id 42"));
        assert!(sexp.contains(":solver \"aabbccdd\""));
    }

    #[test]
    fn test_multiple_solutions() {
        let mut reg = ChallengeRegistry::new(&test_node_id());
        let id = reg.register_builtin("test", "42 ", vec![]);
        // First solution marks solved.
        assert!(reg.mark_solved(id, "42 .", test_node_id()));
        assert_eq!(reg.solution_count(id), 1);
        // Second distinct solution is recorded.
        assert!(!reg.mark_solved(id, "6 7 * .", [0xBB; 8]));
        assert_eq!(reg.solution_count(id), 2);
        // Duplicate solution is ignored.
        assert!(!reg.mark_solved(id, "42 .", [0xCC; 8]));
        assert_eq!(reg.solution_count(id), 2);
    }

    #[test]
    fn test_solution_cap() {
        let mut reg = ChallengeRegistry::new(&test_node_id());
        let id = reg.register_builtin("test", "42 ", vec![]);
        for i in 0..25u8 {
            let prog = format!("{} 42 + 42 - .", i);
            reg.mark_solved(id, &prog, [i; 8]);
        }
        assert_eq!(reg.solution_count(id), 20); // capped
    }

    #[test]
    fn test_colony_diversity() {
        let mut reg = ChallengeRegistry::new(&test_node_id());
        let id = reg.register_builtin("test", "42 ", vec![]);
        reg.mark_solved(id, "42 .", test_node_id());
        reg.mark_solved(id, "6 7 * .", [0xBB; 8]);
        let out = reg.colony_diversity();
        assert!(out.contains("challenges solved: 1"));
        assert!(out.contains("total solutions: 2"));
    }

    #[test]
    fn test_format_challenges() {
        let mut reg = ChallengeRegistry::new(&test_node_id());
        assert!(reg.format_challenges().contains("no challenges"));
        reg.register_builtin("fib10", "55 ", vec![]);
        let out = reg.format_challenges();
        assert!(out.contains("fib10"));
        assert!(out.contains("unsolved"));
    }
}
