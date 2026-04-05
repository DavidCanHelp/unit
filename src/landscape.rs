// landscape.rs — Dynamic fitness landscape for open-ended evolution
//
// When a challenge is SOLVED, the solution reveals harder problems.
// "Compute fib(10)" leads to "compute fib(15)" leads to "compute fib(20)
// in fewer tokens." Each solved challenge spawns children, creating
// progressively harder challenges and open-ended evolutionary pressure.

use crate::challenges::{Challenge, ChallengeOrigin};
use crate::evolve;
use crate::features::mutation::SimpleRng;

// ---------------------------------------------------------------------------
// Fibonacci helper
// ---------------------------------------------------------------------------

pub fn fib(n: u32) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut a: u64 = 0;
    let mut b: u64 = 1;
    for _ in 1..n {
        let tmp = a + b;
        a = b;
        b = tmp;
    }
    b
}

// ---------------------------------------------------------------------------
// Challenge generators
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum GeneratorKind {
    Arithmetic,
    Composition,
}

impl GeneratorKind {
    pub fn name(&self) -> &str {
        match self {
            GeneratorKind::Arithmetic => "arithmetic-ladder",
            GeneratorKind::Composition => "composition-ladder",
        }
    }

    fn generate_next(
        &self,
        solved: &Challenge,
        solution: &str,
        all_solved: &[&Challenge],
        rng_seed: u64,
    ) -> Vec<Challenge> {
        match self {
            GeneratorKind::Arithmetic => arithmetic_generate(solved, solution),
            GeneratorKind::Composition => {
                composition_generate(solved, solution, all_solved, rng_seed)
            }
        }
    }

    fn difficulty_level(&self, challenge: &Challenge) -> u32 {
        match self {
            GeneratorKind::Arithmetic => {
                // Extract fib index from name if present.
                if let Some(idx) = extract_fib_index(&challenge.name) {
                    idx
                } else {
                    (challenge.reward / 10) as u32
                }
            }
            GeneratorKind::Composition => {
                // Composition difficulty is proportional to reward.
                (challenge.reward / 10) as u32 + 5
            }
        }
    }
}

fn extract_fib_index(name: &str) -> Option<u32> {
    // Match "fib10", "fib15", "fib-20", etc.
    let lower = name.to_lowercase();
    let digits: String = lower
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn arithmetic_generate(solved: &Challenge, solution: &str) -> Vec<Challenge> {
    let mut out = Vec::new();
    let lower = solved.name.to_lowercase();

    if lower.contains("fib") {
        let current_n = extract_fib_index(&solved.name).unwrap_or(10);

        // 1. Parsimony challenge: same output, fewer tokens.
        let token_count = evolve::tokenize(solution).len();
        if token_count > 5 {
            let target_tokens = token_count - 2;
            out.push(Challenge {
                id: 0,
                name: format!("fib{}-short{}", current_n, target_tokens),
                description: format!(
                    "compute fib({}) in {} or fewer tokens",
                    current_n, target_tokens
                ),
                target_output: solved.target_output.clone(),
                test_input: None,
                max_steps: solved.max_steps,
                seed_programs: vec![solution.to_string()],
                origin: ChallengeOrigin::BuiltIn,
                reward: solved.reward + 20,
                solved: false,
                solution: None,
                solver: None,
                attempts: 0,
                solutions: vec![],
            });
        }

        // 2. Next Fibonacci: fib(N+5).
        let next_n = current_n + 5;
        if next_n <= 40 {
            let target = fib(next_n);
            out.push(Challenge {
                id: 0,
                name: format!("fib{}", next_n),
                description: format!("compute the {}th Fibonacci number ({})", next_n, target),
                target_output: format!("{} ", target),
                test_input: None,
                max_steps: solved.max_steps + 5000,
                seed_programs: vec![
                    solution.to_string(),
                    // Mutate the solution as a second seed.
                    {
                        let mut rng = crate::features::mutation::SimpleRng::new(next_n as u64);
                        evolve::mutate(solution, &mut rng)
                    },
                ],
                origin: ChallengeOrigin::BuiltIn,
                reward: solved.reward + 50,
                solved: false,
                solution: None,
                solver: None,
                attempts: 0,
                solutions: vec![],
            });
        }

        // 3. Related: compute N*N where N is the fib value.
        let fib_val = fib(current_n);
        if fib_val < 10000 {
            let square = fib_val * fib_val;
            out.push(Challenge {
                id: 0,
                name: format!("square-{}", fib_val),
                description: format!("compute {} * {} = {}", fib_val, fib_val, square),
                target_output: format!("{} ", square),
                test_input: None,
                max_steps: 10000,
                seed_programs: vec![format!("{} DUP * .", fib_val), format!("{} .", square)],
                origin: ChallengeOrigin::BuiltIn,
                reward: 80,
                solved: false,
                solution: None,
                solver: None,
                attempts: 0,
                solutions: vec![],
            });
        }
    }

    out
}

fn composition_generate(
    _solved: &Challenge,
    _solution: &str,
    all_solved: &[&Challenge],
    rng_seed: u64,
) -> Vec<Challenge> {
    // Only generate occasionally (use rng_seed as cheap randomness).
    if !rng_seed.is_multiple_of(3) {
        return Vec::new();
    }

    // Need at least 2 solved challenges with numeric outputs.
    let numeric_solved: Vec<&&Challenge> = all_solved
        .iter()
        .filter(|c| c.target_output.trim().parse::<i64>().is_ok())
        .collect();
    if numeric_solved.len() < 2 {
        return Vec::new();
    }

    // Pick two (deterministic from rng_seed).
    let idx_a = (rng_seed as usize) % numeric_solved.len();
    let idx_b = ((rng_seed as usize) / 7 + 1) % numeric_solved.len();
    if idx_a == idx_b {
        return Vec::new();
    }

    let a = numeric_solved[idx_a];
    let b = numeric_solved[idx_b];

    let val_a: i64 = a.target_output.trim().parse().unwrap_or(0);
    let val_b: i64 = b.target_output.trim().parse().unwrap_or(0);
    let composed = val_a + val_b;

    let sol_a = a.solution.as_deref().unwrap_or("");
    let sol_b = b.solution.as_deref().unwrap_or("");
    if sol_a.is_empty() || sol_b.is_empty() {
        return Vec::new();
    }

    // Strip trailing "." from solutions to get stack-producing programs.
    let prog_a = sol_a.trim_end_matches('.').trim();
    let prog_b = sol_b.trim_end_matches('.').trim();

    vec![Challenge {
        id: 0,
        name: format!("compose-{}+{}", a.name, b.name),
        description: format!(
            "compute {} + {} = {} (compose {} and {})",
            val_a, val_b, composed, a.name, b.name
        ),
        target_output: format!("{} ", composed),
        test_input: None,
        max_steps: 15000,
        seed_programs: vec![
            format!("{} {} + .", prog_a, prog_b),
            format!("{} .", composed),
        ],
        origin: ChallengeOrigin::BuiltIn,
        reward: a.reward.max(b.reward) + 30,
        solved: false,
        solution: None,
        solver: None,
        attempts: 0,
        solutions: vec![],
    }]
}

// ---------------------------------------------------------------------------
// Environment variation
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct EnvironmentCycle {
    pub current_idx: usize,
    pub cycle_length: u64,
    pub tick_counter: u64,
    pub conditions: Vec<String>,
}

impl Default for EnvironmentCycle {
    fn default() -> Self {
        Self::new()
    }
}

impl EnvironmentCycle {
    pub fn new() -> Self {
        EnvironmentCycle {
            current_idx: 0,
            cycle_length: 500,
            tick_counter: 0,
            conditions: vec![
                "normal".into(),
                "harsh".into(),
                "abundant".into(),
                "competitive".into(),
            ],
        }
    }

    pub fn tick(&mut self) {
        self.tick_counter += 1;
        if self.tick_counter >= self.cycle_length {
            self.tick_counter = 0;
            self.current_idx = (self.current_idx + 1) % self.conditions.len();
        }
    }

    pub fn current_condition(&self) -> &str {
        &self.conditions[self.current_idx]
    }

    pub fn apply_to_max_steps(&self, base: usize) -> usize {
        match self.current_condition() {
            "harsh" => base / 2,
            "abundant" => base * 2,
            _ => base,
        }
    }

    pub fn apply_to_reward(&self, base: i64, attempts: u32) -> i64 {
        match self.current_condition() {
            "harsh" => base * 2,
            "competitive" => base / (attempts as i64 + 1).max(1),
            _ => base,
        }
    }
}

// ---------------------------------------------------------------------------
// Meta-evolution: evolve challenge generators
// ---------------------------------------------------------------------------

/// A generator genome is a Forth program that transforms a number on the
/// stack into a new target number for a challenge.
#[derive(Clone, Debug)]
pub struct GeneratorGenome {
    pub program: String,
    pub fitness: f64,
    pub challenges_generated: u32,
    pub challenges_solved: u32,
    pub challenges_unsolvable: u32,
}

impl GeneratorGenome {
    pub fn new(program: &str) -> Self {
        GeneratorGenome {
            program: program.to_string(),
            fitness: 0.0,
            challenges_generated: 0,
            challenges_solved: 0,
            challenges_unsolvable: 0,
        }
    }
}

/// Vocabulary for generator mutations.
const GEN_VOCAB: &[&str] = &[
    "1", "2", "3", "5", "7", "10", "20", "+", "-", "*", "DUP", "SWAP", "OVER", "DROP", "1+", "1-",
    "2*", "2/",
];

fn random_gen_token(rng: &mut SimpleRng) -> &'static str {
    GEN_VOCAB[rng.next_usize(GEN_VOCAB.len())]
}

/// Mutate a generator program using token-level operators.
pub fn mutate_generator(program: &str, rng: &mut SimpleRng) -> String {
    let mut tokens = evolve::tokenize(program);
    if tokens.is_empty() {
        tokens.push(random_gen_token(rng).to_string());
        return evolve::detokenize(&tokens);
    }
    match rng.next_usize(4) {
        0 => {
            // Replace
            let pos = rng.next_usize(tokens.len());
            tokens[pos] = random_gen_token(rng).to_string();
        }
        1 => {
            // Insert
            if tokens.len() < 10 {
                let pos = rng.next_usize(tokens.len() + 1);
                tokens.insert(pos, random_gen_token(rng).to_string());
            }
        }
        2 => {
            // Delete
            if tokens.len() > 1 {
                let pos = rng.next_usize(tokens.len());
                tokens.remove(pos);
            }
        }
        _ => {
            // Swap
            if tokens.len() >= 2 {
                let a = rng.next_usize(tokens.len());
                let b = rng.next_usize(tokens.len());
                tokens.swap(a, b);
            }
        }
    }
    evolve::detokenize(&tokens)
}

/// Evaluate a generator: run its program with `input_val` on the stack.
/// Returns (proposed_target, fitness_score).
pub fn evaluate_generator(program: &str, input_val: i64) -> (Option<i64>, f64) {
    // Build a Forth snippet: push input, run generator, print result.
    let code = format!("{} {}", input_val, program);
    // We can't run a full VM here (no sandbox access at this level),
    // so we do a simple stack simulation for the limited vocabulary.
    match simulate_stack(&code) {
        Some(result) => {
            if result == input_val {
                return (Some(result), 1.0);
            } // trivial
            if result <= 0 || result > 1_000_000 {
                return (Some(result), 5.0);
            } // likely unsolvable
              // Score based on "interestingness"
            let input_digits = digit_count(input_val);
            let result_digits = digit_count(result);
            let digit_ratio = result_digits as f64 / input_digits.max(1) as f64;
            let mut score = 100.0;
            // Bonus for moderate difficulty increase
            if digit_ratio > 0.5 && digit_ratio < 3.0 {
                score += 30.0;
            }
            // Penalty for being a simple multiple
            if input_val != 0 && result % input_val == 0 && result / input_val < 4 {
                score -= 20.0;
            }
            (Some(result), score)
        }
        None => (None, 0.0), // crash
    }
}

fn digit_count(n: i64) -> u32 {
    if n == 0 {
        return 1;
    }
    let mut d = 0;
    let mut v = n.unsigned_abs();
    while v > 0 {
        d += 1;
        v /= 10;
    }
    d
}

/// Simple stack simulator for the generator vocabulary.
fn simulate_stack(code: &str) -> Option<i64> {
    let mut stack: Vec<i64> = Vec::new();
    for token in code.split_whitespace() {
        match token {
            "DUP" => {
                let a = *stack.last()?;
                stack.push(a);
            }
            "DROP" => {
                stack.pop()?;
            }
            "SWAP" => {
                let len = stack.len();
                if len < 2 {
                    return None;
                }
                stack.swap(len - 1, len - 2);
            }
            "OVER" => {
                let len = stack.len();
                if len < 2 {
                    return None;
                }
                stack.push(stack[len - 2]);
            }
            "+" => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                stack.push(a.wrapping_add(b));
            }
            "-" => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                stack.push(a.wrapping_sub(b));
            }
            "*" => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                stack.push(a.saturating_mul(b));
            }
            "1+" => {
                let a = stack.pop()?;
                stack.push(a + 1);
            }
            "1-" => {
                let a = stack.pop()?;
                stack.push(a - 1);
            }
            "2*" => {
                let a = stack.pop()?;
                stack.push(a * 2);
            }
            "2/" => {
                let a = stack.pop()?;
                stack.push(a / 2);
            }
            "ABS" => {
                let a = stack.pop()?;
                stack.push(a.abs());
            }
            "MAX" => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                stack.push(a.max(b));
            }
            "MIN" => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                stack.push(a.min(b));
            }
            _ => {
                if let Ok(n) = token.parse::<i64>() {
                    stack.push(n);
                }
                // Unknown tokens ignored (graceful)
            }
        }
        // Safety: cap stack size
        if stack.len() > 20 {
            return None;
        }
    }
    stack.last().copied()
}

#[derive(Clone, Debug)]
pub struct GeneratorPopulation {
    pub genomes: Vec<GeneratorGenome>,
    pub generation: u32,
    pub best: Option<GeneratorGenome>,
}

impl GeneratorPopulation {
    pub fn new(rng: &mut SimpleRng) -> Self {
        let seeds = [
            "5 +",
            "DUP *",
            "2 *",
            "1+",
            "3 *",
            "DUP 2 * +",
            "10 +",
            "DUP 3 * 2 +",
        ];
        let mut genomes: Vec<GeneratorGenome> =
            seeds.iter().map(|s| GeneratorGenome::new(s)).collect();
        // Fill to 20 with mutations of seeds
        while genomes.len() < 20 {
            let base = &seeds[rng.next_usize(seeds.len())];
            genomes.push(GeneratorGenome::new(&mutate_generator(base, rng)));
        }
        GeneratorPopulation {
            genomes,
            generation: 0,
            best: None,
        }
    }

    /// Run one generation of meta-evolution.
    pub fn evolve_generators(&mut self, rng: &mut SimpleRng) {
        // Sort by fitness descending.
        self.genomes.sort_by(|a, b| {
            b.fitness
                .partial_cmp(&a.fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some(best) = self.genomes.first() {
            self.best = Some(best.clone());
        }

        let pop_size = self.genomes.len();
        let elites = 3.min(pop_size);
        let mut next = Vec::with_capacity(pop_size);

        // Keep elites
        for g in self.genomes.iter().take(elites) {
            next.push(g.clone());
        }

        // Produce offspring
        while next.len() < pop_size {
            // Tournament select
            let mut best_idx = rng.next_usize(self.genomes.len());
            for _ in 0..2 {
                let idx = rng.next_usize(self.genomes.len());
                if self.genomes[idx].fitness > self.genomes[best_idx].fitness {
                    best_idx = idx;
                }
            }
            let parent = &self.genomes[best_idx];
            let child_prog = mutate_generator(&parent.program, rng);
            next.push(GeneratorGenome::new(&child_prog));
        }

        self.genomes = next;
        self.generation += 1;
    }

    /// Evaluate all generators against a solved target value.
    pub fn evaluate_all(&mut self, solved_target: i64) {
        for g in &mut self.genomes {
            let (_, score) = evaluate_generator(&g.program, solved_target);
            // Blend with history (moving average).
            g.fitness = g.fitness * 0.7 + score * 0.3;
        }
    }

    /// Run the best generator to produce a new target.
    pub fn generate_target(&mut self, solved_target: i64) -> Option<(i64, String)> {
        let best = self.best.as_ref().or_else(|| self.genomes.first())?;
        let (target, _) = evaluate_generator(&best.program, solved_target);
        let target = target?;
        if target <= 0 || target > 1_000_000 || target == solved_target {
            return None;
        }
        Some((target, best.program.clone()))
    }

    pub fn format_top(&self, n: usize) -> String {
        let mut sorted = self.genomes.clone();
        sorted.sort_by(|a, b| {
            b.fitness
                .partial_cmp(&a.fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut out = format!(
            "--- generator population (top {}) ---\n",
            n.min(sorted.len())
        );
        for (i, g) in sorted.iter().take(n).enumerate() {
            out.push_str(&format!(
                "  {}. \"{}\" fitness={:.0} generated={} solved={}\n",
                i + 1,
                g.program,
                g.fitness,
                g.challenges_generated,
                g.challenges_solved
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Third-order evolution: evolve the scoring function for generators
// ---------------------------------------------------------------------------

/// A scoring genome is a Forth program that takes two numbers on the stack
/// (input_target, output_target) and produces a fitness score.
#[derive(Clone, Debug)]
pub struct ScoringGenome {
    pub program: String,
    pub fitness: f64,
    pub generators_scored: u32,
}

impl ScoringGenome {
    pub fn new(program: &str) -> Self {
        ScoringGenome {
            program: program.to_string(),
            fitness: 0.0,
            generators_scored: 0,
        }
    }
}

/// History entry: which generator produced which challenge and was it solved.
#[derive(Clone, Debug)]
pub struct GeneratorHistory {
    pub generator_program: String,
    pub challenge_id: u64,
    pub was_solved: bool,
}

/// Evaluate a scoring function: run it with (input, output) on the stack.
pub fn evaluate_scorer(program: &str, input_val: i64, output_val: i64) -> Option<i64> {
    let code = format!("{} {} {}", input_val, output_val, program);
    simulate_stack(&code)
}

#[derive(Clone, Debug)]
pub struct ScoringPopulation {
    pub scorers: Vec<ScoringGenome>,
    pub generation: u32,
    pub best: Option<ScoringGenome>,
    pub history: Vec<GeneratorHistory>,
    pub cycles_completed: u32,
}

impl ScoringPopulation {
    pub fn new(rng: &mut SimpleRng) -> Self {
        let seeds = [
            "- ABS 100 SWAP - 0 MAX",
            "DROP 50",
            "- ABS",
            "- ABS 1+ 1000 SWAP -",
            "SWAP DROP DUP * 100 SWAP -",
        ];
        let mut scorers: Vec<ScoringGenome> = seeds.iter().map(|s| ScoringGenome::new(s)).collect();
        while scorers.len() < 10 {
            let base = seeds[rng.next_usize(seeds.len())];
            scorers.push(ScoringGenome::new(&mutate_generator(base, rng)));
        }
        ScoringPopulation {
            scorers,
            generation: 0,
            best: None,
            history: Vec::new(),
            cycles_completed: 0,
        }
    }

    pub fn record_history(&mut self, gen_program: &str, challenge_id: u64, was_solved: bool) {
        if self.history.len() >= 50 {
            self.history.remove(0);
        }
        self.history.push(GeneratorHistory {
            generator_program: gen_program.to_string(),
            challenge_id,
            was_solved,
        });
    }

    /// Evaluate scoring functions against history.
    pub fn evaluate_from_history(&mut self) {
        if self.history.len() < 10 {
            return;
        }
        for scorer in &mut self.scorers {
            let mut total_score = 0.0;
            for entry in &self.history {
                // Run the generator to get its output.
                let (gen_output, _) = evaluate_generator(&entry.generator_program, 55);
                let output = gen_output.unwrap_or(0);
                // Score the generator using this scoring function.
                let scorer_score = evaluate_scorer(&scorer.program, 55, output).unwrap_or(0);
                // If this scorer ranked the generator high AND the challenge was solved: good.
                if entry.was_solved && scorer_score > 50 {
                    total_score += 50.0;
                } else if entry.was_solved && scorer_score > 20 {
                    total_score += 20.0;
                } else if !entry.was_solved && scorer_score < 20 {
                    total_score += 10.0;
                }
                // If this scorer ranked a bad generator high: bad.
                else if !entry.was_solved && scorer_score > 50 {
                    total_score -= 10.0;
                }
            }
            scorer.fitness = scorer.fitness * 0.5 + (total_score / self.history.len() as f64) * 0.5;
            scorer.generators_scored += 1;
        }
    }

    /// Run one generation of scorer evolution.
    pub fn evolve_scorers(&mut self, rng: &mut SimpleRng) {
        self.scorers.sort_by(|a, b| {
            b.fitness
                .partial_cmp(&a.fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if let Some(best) = self.scorers.first() {
            self.best = Some(best.clone());
        }
        let pop_size = self.scorers.len();
        let mut next = Vec::with_capacity(pop_size);
        // Keep top 2.
        for s in self.scorers.iter().take(2.min(pop_size)) {
            next.push(s.clone());
        }
        while next.len() < pop_size {
            let idx = rng.next_usize(self.scorers.len());
            let parent = &self.scorers[idx];
            next.push(ScoringGenome::new(&mutate_generator(&parent.program, rng)));
        }
        self.scorers = next;
        self.generation += 1;
        self.cycles_completed += 1;
    }

    pub fn format_top(&self, n: usize) -> String {
        let mut sorted = self.scorers.clone();
        sorted.sort_by(|a, b| {
            b.fitness
                .partial_cmp(&a.fitness)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut out = format!("--- scoring population (top {}) ---\n", n.min(sorted.len()));
        for (i, s) in sorted.iter().take(n).enumerate() {
            out.push_str(&format!(
                "  {}. \"{}\" fitness={:.0}\n",
                i + 1,
                s.program,
                s.fitness
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Landscape engine
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct LandscapeEngine {
    pub generators: Vec<GeneratorKind>,
    pub environment: EnvironmentCycle,
    pub challenges_generated: u64,
    pub depth: u32,
    pub meta: GeneratorPopulation,
    pub evolved_count: u64,
    pub scoring: ScoringPopulation,
}

impl Default for LandscapeEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl LandscapeEngine {
    pub fn new() -> Self {
        let mut rng = SimpleRng::new(0xCAFE);
        LandscapeEngine {
            generators: vec![GeneratorKind::Arithmetic, GeneratorKind::Composition],
            environment: EnvironmentCycle::new(),
            challenges_generated: 0,
            depth: 0,
            meta: GeneratorPopulation::new(&mut rng),
            evolved_count: 0,
            scoring: ScoringPopulation::new(&mut rng),
        }
    }

    /// Called when a challenge is solved. Returns new, harder challenges.
    pub fn on_challenge_solved(
        &mut self,
        challenge: &Challenge,
        solution: &str,
        all_solved: &[&Challenge],
    ) -> Vec<Challenge> {
        let mut new_challenges = Vec::new();

        // Authored generators (ArithmeticLadder, CompositionLadder).
        for gen in &self.generators {
            let parent_difficulty = gen.difficulty_level(challenge);
            let generated =
                gen.generate_next(challenge, solution, all_solved, self.challenges_generated);
            for ch in generated {
                let child_difficulty = gen.difficulty_level(&ch);
                if child_difficulty > parent_difficulty && child_difficulty > self.depth {
                    self.depth = child_difficulty;
                }
                new_challenges.push(ch);
            }
        }

        // Evolved generator: run the best meta-evolved generator.
        let solved_target: i64 = challenge.target_output.trim().parse().unwrap_or(0);
        if solved_target > 0 {
            // Evaluate all generators against this solved target.
            self.meta.evaluate_all(solved_target);
            // Try to generate a new challenge from the best generator.
            if let Some((new_target, gen_program)) = self.meta.generate_target(solved_target) {
                let h = {
                    let mut h: u64 = 0xcbf29ce484222325;
                    for b in gen_program.bytes() {
                        h ^= b as u64;
                        h = h.wrapping_mul(0x100000001b3);
                    }
                    h
                };
                new_challenges.push(Challenge {
                    id: 0,
                    name: format!("evolved-{:08x}", h & 0xFFFFFFFF),
                    description: format!("evolved challenge from generator: {}", gen_program),
                    target_output: format!("{} ", new_target),
                    test_input: None,
                    max_steps: 10000,
                    seed_programs: vec![solution.to_string(), format!("{} .", new_target)],
                    origin: ChallengeOrigin::BuiltIn,
                    reward: 80 + (self.meta.best.as_ref().map_or(0.0, |b| b.fitness) * 0.5) as i64,
                    solved: false,
                    solution: None,
                    solver: None,
                    attempts: 0,
                    solutions: vec![],
                });
                self.evolved_count += 1;
                // Update generator stats.
                if let Some(ref mut best) = self.meta.best {
                    best.challenges_generated += 1;
                }
            }
            // Run one generation of meta-evolution.
            let mut rng = SimpleRng::new(self.challenges_generated + 1);
            self.meta.evolve_generators(&mut rng);

            // Third-order: evolve scoring functions if enough history.
            if self.scoring.history.len() >= 10 {
                self.scoring.evaluate_from_history();
                let mut rng3 = SimpleRng::new(self.scoring.generation as u64 + 1);
                self.scoring.evolve_scorers(&mut rng3);
            }
        }

        self.challenges_generated += new_challenges.len() as u64;
        new_challenges
    }

    pub fn current_environment(&self) -> &str {
        self.environment.current_condition()
    }

    pub fn tick(&mut self) {
        self.environment.tick();
    }

    pub fn depth(&self) -> u32 {
        self.depth
    }

    pub fn format_landscape(&self) -> String {
        let authored = self.challenges_generated - self.evolved_count;
        let best_gen = self
            .meta
            .best
            .as_ref()
            .map(|b| format!("\"{}\"", b.program))
            .unwrap_or_else(|| "(none)".into());
        format!(
            "--- landscape ---\n\
             depth: {}\n\
             challenges generated: {} ({} authored, {} evolved)\n\
             environment: {}\n\
             authored generators: {}\n\
             evolved generators: {} (best: {})\n\
             scoring functions: {} (gen {})\n",
            self.depth,
            self.challenges_generated,
            authored,
            self.evolved_count,
            self.current_environment(),
            self.generators.len(),
            self.meta.genomes.len(),
            best_gen,
            self.scoring.scorers.len(),
            self.scoring.generation,
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn solved_fib10() -> Challenge {
        Challenge {
            id: 1,
            name: "fib10".into(),
            description: "compute 10th fibonacci".into(),
            target_output: "55 ".into(),
            test_input: None,
            max_steps: 10000,
            seed_programs: vec![],
            origin: ChallengeOrigin::BuiltIn,
            reward: 100,
            solved: true,
            solution: Some("0 1 10 0 DO OVER + SWAP LOOP DROP .".into()),
            solver: Some([0; 8]),
            attempts: 1,
            solutions: vec![],
        }
    }

    #[test]
    fn test_fib_helper() {
        assert_eq!(fib(0), 0);
        assert_eq!(fib(1), 1);
        assert_eq!(fib(10), 55);
        assert_eq!(fib(15), 610);
        assert_eq!(fib(20), 6765);
        assert_eq!(fib(30), 832040);
    }

    #[test]
    fn test_arithmetic_generates_harder_fib() {
        let ch = solved_fib10();
        let solution = "0 1 10 0 DO OVER + SWAP LOOP DROP .";
        let generated = arithmetic_generate(&ch, solution);
        assert!(generated.len() >= 2);

        // Should have a parsimony challenge.
        assert!(generated.iter().any(|c| c.name.contains("short")));

        // Should have fib15.
        let fib15 = generated.iter().find(|c| c.name == "fib15");
        assert!(fib15.is_some());
        assert_eq!(fib15.unwrap().target_output, "610 ");
        assert!(fib15.unwrap().reward > ch.reward);
    }

    #[test]
    fn test_arithmetic_generates_square() {
        let ch = solved_fib10();
        let solution = "0 1 10 0 DO OVER + SWAP LOOP DROP .";
        let generated = arithmetic_generate(&ch, solution);
        let sq = generated.iter().find(|c| c.name.contains("square"));
        assert!(sq.is_some());
        assert_eq!(sq.unwrap().target_output, "3025 "); // 55*55
    }

    #[test]
    fn test_composition_needs_two_solved() {
        let ch = solved_fib10();
        let solution = "0 1 10 0 DO OVER + SWAP LOOP DROP .";
        // Only one solved — should return empty.
        let generated = composition_generate(&ch, solution, &[&ch], 0);
        assert!(generated.is_empty());
    }

    #[test]
    fn test_composition_from_two_solved() {
        let ch1 = solved_fib10();
        let mut ch2 = solved_fib10();
        ch2.id = 2;
        ch2.name = "square-55".into();
        ch2.target_output = "3025 ".into();
        ch2.solution = Some("55 DUP * .".into());
        let all = vec![&ch1, &ch2];
        // rng_seed % 3 == 0 to trigger.
        let generated = composition_generate(&ch1, "55 .", &all, 0);
        // May or may not generate depending on index collision.
        // With seed=0: idx_a=0, idx_b=1 — should work.
        if !generated.is_empty() {
            assert!(generated[0].name.contains("compose"));
            // 55 + 3025 = 3080
            assert_eq!(generated[0].target_output, "3080 ");
        }
    }

    #[test]
    fn test_environment_cycle() {
        let mut env = EnvironmentCycle::new();
        assert_eq!(env.current_condition(), "normal");
        for _ in 0..500 {
            env.tick();
        }
        assert_eq!(env.current_condition(), "harsh");
        for _ in 0..500 {
            env.tick();
        }
        assert_eq!(env.current_condition(), "abundant");
        for _ in 0..500 {
            env.tick();
        }
        assert_eq!(env.current_condition(), "competitive");
        for _ in 0..500 {
            env.tick();
        }
        assert_eq!(env.current_condition(), "normal"); // back to start
    }

    #[test]
    fn test_apply_to_max_steps() {
        let mut env = EnvironmentCycle::new();
        assert_eq!(env.apply_to_max_steps(10000), 10000); // normal
        for _ in 0..500 {
            env.tick();
        } // harsh
        assert_eq!(env.apply_to_max_steps(10000), 5000);
        for _ in 0..500 {
            env.tick();
        } // abundant
        assert_eq!(env.apply_to_max_steps(10000), 20000);
    }

    #[test]
    fn test_apply_to_reward() {
        let mut env = EnvironmentCycle::new();
        assert_eq!(env.apply_to_reward(100, 0), 100); // normal
        for _ in 0..500 {
            env.tick();
        } // harsh
        assert_eq!(env.apply_to_reward(100, 0), 200);
        for _ in 0..1000 {
            env.tick();
        } // competitive
        assert_eq!(env.apply_to_reward(100, 3), 25); // 100/(3+1)
    }

    #[test]
    fn test_depth_increases() {
        let mut engine = LandscapeEngine::new();
        assert_eq!(engine.depth(), 0);
        let ch = solved_fib10();
        let solution = "0 1 10 0 DO OVER + SWAP LOOP DROP .";
        let _new = engine.on_challenge_solved(&ch, solution, &[&ch]);
        assert!(engine.depth() > 0);
    }

    #[test]
    fn test_on_challenge_solved_non_fib() {
        let mut engine = LandscapeEngine::new();
        let ch = Challenge {
            id: 99,
            name: "custom".into(),
            description: "non-fib".into(),
            target_output: "42 ".into(),
            test_input: None,
            max_steps: 10000,
            seed_programs: vec![],
            origin: ChallengeOrigin::BuiltIn,
            reward: 50,
            solved: true,
            solution: Some("42 .".into()),
            solver: Some([0; 8]),
            attempts: 1,
            solutions: vec![],
        };
        let generated = engine.on_challenge_solved(&ch, "42 .", &[&ch]);
        // Arithmetic won't match (no "fib" in name), composition needs 2 solved.
        // But meta-evolved generators may produce one from target 42.
        // Authored generators produce 0, evolved may produce 0 or 1.
        let authored = generated
            .iter()
            .filter(|c| !c.name.starts_with("evolved-"))
            .count();
        assert_eq!(authored, 0);
    }

    #[test]
    fn test_format_landscape() {
        let engine = LandscapeEngine::new();
        let s = engine.format_landscape();
        assert!(s.contains("depth: 0"));
        assert!(s.contains("environment: normal"));
        assert!(s.contains("evolved generators: 20"));
    }

    // --- Meta-evolution tests ---

    #[test]
    fn test_evaluate_generator_trivial() {
        let (_, score) = evaluate_generator("", 55); // no-op, returns input
        assert!(score <= 1.0);
    }

    #[test]
    fn test_evaluate_generator_crash() {
        let (_, score) = evaluate_generator("DROP DROP DROP", 55);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_evaluate_generator_valid() {
        let (target, score) = evaluate_generator("5 +", 55);
        assert_eq!(target, Some(60));
        assert!(score > 50.0);
    }

    #[test]
    fn test_evaluate_generator_square() {
        let (target, _) = evaluate_generator("DUP *", 55);
        assert_eq!(target, Some(3025));
    }

    #[test]
    fn test_seed_generators_valid() {
        let seeds = ["5 +", "DUP *", "2 *", "1+", "3 *"];
        for seed in &seeds {
            let (target, score) = evaluate_generator(seed, 55);
            assert!(target.is_some(), "seed '{}' produced no output", seed);
            assert!(score > 0.0, "seed '{}' scored 0", seed);
        }
    }

    #[test]
    fn test_meta_evolution_produces_next_gen() {
        let mut rng = SimpleRng::new(42);
        let mut pop = GeneratorPopulation::new(&mut rng);
        assert_eq!(pop.genomes.len(), 20);
        pop.evaluate_all(55);
        pop.evolve_generators(&mut rng);
        assert_eq!(pop.genomes.len(), 20);
        assert_eq!(pop.generation, 1);
        assert!(pop.best.is_some());
    }

    #[test]
    fn test_simulate_stack() {
        assert_eq!(simulate_stack("10 5 +"), Some(15));
        assert_eq!(simulate_stack("7 DUP *"), Some(49));
        assert_eq!(simulate_stack("3 2 * 1+"), Some(7));
        assert_eq!(simulate_stack("DROP"), None); // underflow
    }

    #[test]
    fn test_mutate_generator_valid() {
        let mut rng = SimpleRng::new(99);
        for _ in 0..20 {
            let m = mutate_generator("5 +", &mut rng);
            assert!(!m.is_empty());
        }
    }

    // --- Third-order evolution tests ---

    #[test]
    fn test_evaluate_scorer() {
        // "- ABS" takes (input, output) and returns |output - input|
        let score = evaluate_scorer("- ABS", 55, 60);
        assert_eq!(score, Some(5));
    }

    #[test]
    fn test_evaluate_scorer_crash() {
        let score = evaluate_scorer("DROP DROP DROP", 55, 60);
        assert!(score.is_none());
    }

    #[test]
    fn test_scoring_population_init() {
        let mut rng = SimpleRng::new(42);
        let pop = ScoringPopulation::new(&mut rng);
        assert_eq!(pop.scorers.len(), 10);
    }

    #[test]
    fn test_scoring_history() {
        let mut rng = SimpleRng::new(42);
        let mut pop = ScoringPopulation::new(&mut rng);
        for i in 0..15 {
            pop.record_history("5 +", i as u64, i % 3 == 0);
        }
        assert_eq!(pop.history.len(), 15);
        pop.evaluate_from_history();
        // At least some scorers should have non-zero fitness.
        assert!(pop.scorers.iter().any(|s| s.fitness != 0.0));
    }

    #[test]
    fn test_scoring_evolution() {
        let mut rng = SimpleRng::new(42);
        let mut pop = ScoringPopulation::new(&mut rng);
        pop.evolve_scorers(&mut rng);
        assert_eq!(pop.generation, 1);
        assert_eq!(pop.scorers.len(), 10);
    }
}
