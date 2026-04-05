// evolve.rs — Genetic programming engine for unit
//
// Evolves Forth programs through mutation and selection to solve
// fitness challenges. The default challenge: find the shortest
// program that computes the 10th Fibonacci number (55).

use crate::features::mutation::SimpleRng;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct FitnessChallenge {
    pub name: String,
    pub target_output: String,
    pub max_steps: usize,
    pub seed_programs: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub program: String,
    pub fitness: f64,
    pub generation: u32,
    pub parent: Option<String>,
}

impl Candidate {
    pub fn new(program: &str) -> Self {
        Candidate {
            program: program.to_string(),
            fitness: 0.0,
            generation: 0,
            parent: None,
        }
    }

    pub fn token_count(&self) -> usize {
        self.program.split_whitespace().count()
    }
}

#[derive(Clone, Debug)]
pub struct EvolutionState {
    pub challenge: FitnessChallenge,
    pub population: Vec<Candidate>,
    pub generation: u32,
    pub best: Option<Candidate>,
    pub max_generations: u32,
    pub running: bool,
    pub immigrants: u32,
}

impl EvolutionState {
    pub fn new(challenge: FitnessChallenge, max_gen: u32) -> Self {
        EvolutionState {
            challenge,
            population: Vec::new(),
            generation: 0,
            best: None,
            max_generations: max_gen,
            running: false,
            immigrants: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Default challenge: fib10
// ---------------------------------------------------------------------------

pub fn fib10_challenge() -> FitnessChallenge {
    FitnessChallenge {
        name: "fib10".into(),
        target_output: "55 ".into(), // Forth . prints number followed by space
        max_steps: 10000,
        seed_programs: vec![
            // Correct but long
            "0 1 10 0 DO OVER + SWAP LOOP DROP .".into(),
            // Partially correct (fib5 = 8)
            "0 1 5 0 DO OVER + SWAP LOOP DROP .".into(),
            // Random arithmetic
            "1 1 + DUP * .".into(),
            // Blank slate
            "0 .".into(),
            // Near-correct (fib9 = 34)
            "0 1 9 0 DO OVER + SWAP LOOP DROP .".into(),
        ],
    }
}

// ---------------------------------------------------------------------------
// Fitness scoring
// ---------------------------------------------------------------------------

/// Score a candidate. Returns (fitness, output_string).
/// This is called by the VM which provides the sandbox evaluator.
pub fn score_candidate(output: &str, success: bool, target: &str, token_count: usize) -> f64 {
    if !success {
        return 0.0; // crashed or timed out
    }
    let trimmed = output.trim();
    let target_trimmed = target.trim();
    if trimmed == target_trimmed {
        // Correct! Reward shorter programs.
        1000.0 - (token_count as f64 * 10.0)
    } else if !trimmed.is_empty() {
        // Produced output but wrong — survived, slight credit
        1.0
    } else {
        0.5 // produced nothing
    }
}

// ---------------------------------------------------------------------------
// Mutation operators (token-level)
// ---------------------------------------------------------------------------

const VOCAB: &[&str] = &[
    // Numbers
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "20", "55", // Arithmetic
    "+", "-", "*", // Stack ops
    "DUP", "DROP", "SWAP", "OVER", "ROT", // Comparison
    "<", ">", "=", // Control flow
    "IF", "THEN", "ELSE", "DO", "LOOP", "I", // Output
    ".",
];

fn random_token(rng: &mut SimpleRng) -> &'static str {
    VOCAB[rng.next_usize(VOCAB.len())]
}

pub fn tokenize(program: &str) -> Vec<String> {
    program.split_whitespace().map(|s| s.to_string()).collect()
}

pub fn detokenize(tokens: &[String]) -> String {
    tokens.join(" ")
}

pub fn mutate(program: &str, rng: &mut SimpleRng) -> String {
    let mut tokens = tokenize(program);
    if tokens.is_empty() {
        tokens.push(random_token(rng).to_string());
        return detokenize(&tokens);
    }

    let op = rng.next_usize(5);
    match op {
        0 => {
            // Token swap
            if tokens.len() >= 2 {
                let a = rng.next_usize(tokens.len());
                let mut b = rng.next_usize(tokens.len());
                while b == a && tokens.len() > 1 {
                    b = rng.next_usize(tokens.len());
                }
                tokens.swap(a, b);
            }
        }
        1 => {
            // Token insert
            let pos = rng.next_usize(tokens.len() + 1);
            tokens.insert(pos, random_token(rng).to_string());
        }
        2 => {
            // Token delete
            if tokens.len() > 1 {
                let pos = rng.next_usize(tokens.len());
                tokens.remove(pos);
            }
        }
        3 => {
            // Token replace
            let pos = rng.next_usize(tokens.len());
            tokens[pos] = random_token(rng).to_string();
        }
        _ => {
            // Double mutation
            let pos = rng.next_usize(tokens.len());
            tokens[pos] = random_token(rng).to_string();
            if tokens.len() >= 2 {
                let pos2 = rng.next_usize(tokens.len());
                tokens[pos2] = random_token(rng).to_string();
            }
        }
    }
    detokenize(&tokens)
}

pub fn crossover(a: &str, b: &str, rng: &mut SimpleRng) -> String {
    let ta = tokenize(a);
    let tb = tokenize(b);
    if ta.is_empty() {
        return b.to_string();
    }
    if tb.is_empty() {
        return a.to_string();
    }
    let cut_a = rng.next_usize(ta.len());
    let cut_b = rng.next_usize(tb.len());
    let mut result: Vec<String> = ta[..cut_a].to_vec();
    result.extend_from_slice(&tb[cut_b..]);
    if result.is_empty() {
        result.push(".".to_string());
    }
    // Limit length to prevent bloat
    if result.len() > 30 {
        result.truncate(30);
    }
    detokenize(&result)
}

// ---------------------------------------------------------------------------
// Selection
// ---------------------------------------------------------------------------

pub fn tournament_select<'a>(pop: &'a [Candidate], rng: &mut SimpleRng) -> &'a Candidate {
    let mut best_idx = rng.next_usize(pop.len());
    for _ in 0..3 {
        let idx = rng.next_usize(pop.len());
        if pop[idx].fitness > pop[best_idx].fitness {
            best_idx = idx;
        }
    }
    &pop[best_idx]
}

// ---------------------------------------------------------------------------
// Population initialization
// ---------------------------------------------------------------------------

pub fn init_population(
    challenge: &FitnessChallenge,
    pop_size: usize,
    rng: &mut SimpleRng,
) -> Vec<Candidate> {
    let mut pop = Vec::with_capacity(pop_size);
    // Add seeds
    for seed in &challenge.seed_programs {
        pop.push(Candidate::new(seed));
    }
    // Fill rest with mutations of seeds + random programs
    while pop.len() < pop_size {
        if !challenge.seed_programs.is_empty() && rng.next_usize(3) < 2 {
            // Mutate a seed
            let seed = &challenge.seed_programs[rng.next_usize(challenge.seed_programs.len())];
            let mutant = mutate(seed, rng);
            pop.push(Candidate::new(&mutant));
        } else {
            // Random program
            let len = 3 + rng.next_usize(10);
            let tokens: Vec<&str> = (0..len).map(|_| random_token(rng)).collect();
            pop.push(Candidate::new(&tokens.join(" ")));
        }
    }
    pop
}

// ---------------------------------------------------------------------------
// Generation step (produces next generation from current)
// ---------------------------------------------------------------------------

pub fn next_generation(pop: &[Candidate], gen: u32, rng: &mut SimpleRng) -> Vec<Candidate> {
    let pop_size = pop.len();
    let elite_count = 5.min(pop_size);

    // Sort by fitness descending
    let mut sorted: Vec<&Candidate> = pop.iter().collect();
    sorted.sort_by(|a, b| {
        b.fitness
            .partial_cmp(&a.fitness)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut next = Vec::with_capacity(pop_size);

    // Keep elites
    for c in sorted.iter().take(elite_count) {
        let mut elite = (*c).clone();
        elite.generation = gen;
        next.push(elite);
    }

    // Produce offspring
    while next.len() < pop_size {
        let parent = tournament_select(pop, rng);
        let child_program = if rng.next_usize(10) < 2 {
            // Crossover
            let other = tournament_select(pop, rng);
            crossover(&parent.program, &other.program, rng)
        } else {
            // Mutation
            mutate(&parent.program, rng)
        };
        next.push(Candidate {
            program: child_program,
            fitness: 0.0,
            generation: gen,
            parent: Some(parent.program.clone()),
        });
    }

    next
}

// ---------------------------------------------------------------------------
// Serialization for snapshots
// ---------------------------------------------------------------------------

pub fn serialize_best(state: &EvolutionState) -> String {
    if let Some(ref best) = state.best {
        format!(
            "gen={} fitness={} tokens={} program={}",
            best.generation,
            best.fitness,
            best.token_count(),
            best.program
        )
    } else {
        "no evolution state".into()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_correct() {
        let f = score_candidate("55 ", true, "55 ", 10);
        assert!(f >= 900.0); // 1000 - 10*10 = 900
                             // Shorter program scores higher
        let f2 = score_candidate("55 ", true, "55 ", 5);
        assert!(f2 > f); // 1000 - 5*10 = 950 > 900
    }

    #[test]
    fn test_score_wrong() {
        let f = score_candidate("42 ", true, "55 ", 10);
        assert_eq!(f, 1.0);
    }

    #[test]
    fn test_score_crash() {
        let f = score_candidate("", false, "55 ", 10);
        assert_eq!(f, 0.0);
    }

    #[test]
    fn test_score_empty() {
        let f = score_candidate("", true, "55 ", 10);
        assert_eq!(f, 0.5);
    }

    #[test]
    fn test_mutate_produces_program() {
        let mut rng = SimpleRng::new(42);
        let prog = "1 2 + .";
        for _ in 0..20 {
            let m = mutate(prog, &mut rng);
            assert!(!m.is_empty());
        }
    }

    #[test]
    fn test_crossover() {
        let mut rng = SimpleRng::new(99);
        let a = "0 1 10 0 DO OVER + SWAP LOOP DROP .";
        let b = "1 DUP * .";
        let c = crossover(a, b, &mut rng);
        assert!(!c.is_empty());
        assert!(c.split_whitespace().count() <= 30);
    }

    #[test]
    fn test_init_population() {
        let ch = fib10_challenge();
        let mut rng = SimpleRng::new(1);
        let pop = init_population(&ch, 50, &mut rng);
        assert_eq!(pop.len(), 50);
        // Seeds should be in there
        assert!(pop.iter().any(|c| c.program.contains("10 0 DO")));
    }

    #[test]
    fn test_tokenize_detokenize() {
        let prog = "0 1 10 0 DO OVER + SWAP LOOP DROP .";
        assert_eq!(detokenize(&tokenize(prog)), prog);
    }

    #[test]
    fn test_seed_programs_valid() {
        let ch = fib10_challenge();
        for seed in &ch.seed_programs {
            let tokens = tokenize(seed);
            assert!(!tokens.is_empty(), "seed should not be empty: {}", seed);
        }
    }

    #[test]
    fn test_next_generation() {
        let mut rng = SimpleRng::new(42);
        let ch = fib10_challenge();
        let mut pop = init_population(&ch, 20, &mut rng);
        // Give some fitness scores
        for (i, c) in pop.iter_mut().enumerate() {
            c.fitness = i as f64;
        }
        let next = next_generation(&pop, 1, &mut rng);
        assert_eq!(next.len(), 20);
        // Elites should be preserved (top 5)
        assert!(next.iter().any(|c| c.fitness == 19.0));
    }
}
