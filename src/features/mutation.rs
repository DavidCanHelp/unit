// mutation.rs — Self-mutation engine for unit
//
// Provides random mutations of Forth word definitions. Mutations allow
// the mesh to explore variations of code and, combined with the fitness
// system, evolve toward better-performing implementations.
//
// Mutation strategies:
//   - Constant tweak: adjust a literal ±1–10%
//   - Word swap: replace a Call with a different Call
//   - Instruction deletion: remove one instruction
//   - Instruction duplication: duplicate one instruction

use std::time::{SystemTime, UNIX_EPOCH};

use crate::types::{Entry, Instruction};

// ---------------------------------------------------------------------------
// Simple RNG (LCG — Knuth's constants)
// ---------------------------------------------------------------------------

pub struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    pub fn new(seed: u64) -> Self {
        SimpleRng {
            state: seed.wrapping_add(1),
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    pub fn next_usize(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        (self.next_u64() as usize) % max
    }

    pub fn next_range(&mut self, min: i64, max: i64) -> i64 {
        if min >= max {
            return min;
        }
        let range = (max - min) as u64;
        min + (self.next_u64() % range) as i64
    }
}

// ---------------------------------------------------------------------------
// Mutation record
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum MutationStrategy {
    ConstantTweak,
    WordSwap,
    InstructionDelete,
    InstructionDup,
}

impl MutationStrategy {
    pub fn label(&self) -> &str {
        match self {
            MutationStrategy::ConstantTweak => "constant-tweak",
            MutationStrategy::WordSwap => "word-swap",
            MutationStrategy::InstructionDelete => "instruction-delete",
            MutationStrategy::InstructionDup => "instruction-dup",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MutationRecord {
    pub word_name: String,
    pub word_index: usize,
    pub strategy: MutationStrategy,
    pub original_body: Vec<Instruction>,
    pub description: String,
    pub timestamp: u64,
}

impl MutationRecord {
    pub fn format(&self) -> String {
        format!(
            "  {} [{}]: {}",
            self.word_name,
            self.strategy.label(),
            self.description
        )
    }
}

/// Classification of a mutation's effect.
#[derive(Clone, Debug, PartialEq)]
pub enum MutationClass {
    Neutral,    // output unchanged — possible dead code
    Beneficial, // output changed, benchmark improved or held
    Harmful,    // output changed, benchmark worsened
    Lethal,     // word crashed or errored
}

impl MutationClass {
    pub fn label(&self) -> &str {
        match self {
            MutationClass::Neutral => "neutral",
            MutationClass::Beneficial => "beneficial",
            MutationClass::Harmful => "harmful",
            MutationClass::Lethal => "lethal",
        }
    }
}

/// Cumulative mutation statistics.
#[derive(Clone, Debug, Default)]
pub struct MutationStats {
    pub total: u32,
    pub neutral: u32,
    pub beneficial: u32,
    pub harmful: u32,
    pub lethal: u32,
}

impl MutationStats {
    pub fn record(&mut self, class: &MutationClass) {
        self.total += 1;
        match class {
            MutationClass::Neutral => self.neutral += 1,
            MutationClass::Beneficial => self.beneficial += 1,
            MutationClass::Harmful => self.harmful += 1,
            MutationClass::Lethal => self.lethal += 1,
        }
    }

    pub fn format(&self) -> String {
        format!(
            "mutations: {} total ({} neutral, {} beneficial, {} harmful, {} lethal)",
            self.total, self.neutral, self.beneficial, self.harmful, self.lethal
        )
    }
}

/// Result of a smart mutation with classification.
#[derive(Clone, Debug)]
pub struct SmartMutationResult {
    pub word_name: String,
    pub strategy: MutationStrategy,
    pub class: MutationClass,
    pub before_hash: u64,
    pub after_hash: u64,
    pub kept: bool,
    pub description: String,
}

/// Simple hash of a string (for comparing outputs).
pub fn hash_output(s: &str) -> u64 {
    let mut h: u64 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u64);
    }
    h
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// Mutation engine
// ---------------------------------------------------------------------------

/// Check if a dictionary entry is a Forth-defined (non-kernel) word
/// suitable for mutation.
pub fn is_mutable(entry: &Entry) -> bool {
    if entry.hidden || entry.body.is_empty() {
        return false;
    }
    // Kernel words have a single Primitive instruction.
    if entry.body.len() == 1 {
        if let Instruction::Primitive(_) = &entry.body[0] {
            return false;
        }
    }
    true
}

/// Apply a random mutation to a dictionary entry. Returns a record
/// describing the mutation (for undo) or None if mutation failed.
pub fn mutate_entry(
    entry: &mut Entry,
    rng: &mut SimpleRng,
    dictionary_len: usize,
) -> Option<MutationRecord> {
    if entry.body.is_empty() {
        return None;
    }

    let original_body = entry.body.clone();

    // Pick a strategy. Try up to 4 times if the chosen strategy can't apply.
    for _ in 0..4 {
        let strategy_idx = rng.next_usize(4);
        let result = match strategy_idx {
            0 => try_constant_tweak(entry, rng),
            1 => try_word_swap(entry, rng, dictionary_len),
            2 => try_instruction_delete(entry, rng),
            3 => try_instruction_dup(entry, rng),
            _ => None,
        };
        if let Some((strategy, description)) = result {
            return Some(MutationRecord {
                word_name: entry.name.clone(),
                word_index: 0, // filled by caller
                strategy,
                original_body,
                description,
                timestamp: now_millis(),
            });
        }
        // Strategy didn't apply, restore and retry.
        entry.body = original_body.clone();
    }
    // All attempts failed.
    entry.body = original_body;
    None
}

/// Revert a mutation by restoring the original body.
pub fn undo_mutation(entry: &mut Entry, record: &MutationRecord) {
    entry.body = record.original_body.clone();
}

// ---------------------------------------------------------------------------
// Mutation strategies
// ---------------------------------------------------------------------------

fn try_constant_tweak(
    entry: &mut Entry,
    rng: &mut SimpleRng,
) -> Option<(MutationStrategy, String)> {
    // Find all Literal instructions.
    let literals: Vec<usize> = entry
        .body
        .iter()
        .enumerate()
        .filter_map(|(i, instr)| match instr {
            Instruction::Literal(_) => Some(i),
            _ => None,
        })
        .collect();

    if literals.is_empty() {
        return None;
    }

    let idx = literals[rng.next_usize(literals.len())];
    if let Instruction::Literal(val) = &mut entry.body[idx] {
        let old = *val;
        // Adjust by ±1–10% (minimum ±1).
        let magnitude = (old.abs() / 10).max(1);
        let delta = rng.next_range(-magnitude, magnitude + 1);
        *val = old.wrapping_add(delta);
        Some((
            MutationStrategy::ConstantTweak,
            format!("literal {} -> {} at pos {}", old, *val, idx),
        ))
    } else {
        None
    }
}

fn try_word_swap(
    entry: &mut Entry,
    rng: &mut SimpleRng,
    dictionary_len: usize,
) -> Option<(MutationStrategy, String)> {
    if dictionary_len < 2 {
        return None;
    }
    // Find all Call instructions.
    let calls: Vec<usize> = entry
        .body
        .iter()
        .enumerate()
        .filter_map(|(i, instr)| match instr {
            Instruction::Call(_) => Some(i),
            _ => None,
        })
        .collect();

    if calls.is_empty() {
        return None;
    }

    let idx = calls[rng.next_usize(calls.len())];
    if let Instruction::Call(old_target) = &entry.body[idx] {
        let old_target = *old_target;
        // Pick a random different target.
        let new_target = loop {
            let t = rng.next_usize(dictionary_len);
            if t != old_target {
                break t;
            }
        };
        entry.body[idx] = Instruction::Call(new_target);
        Some((
            MutationStrategy::WordSwap,
            format!("call {} -> {} at pos {}", old_target, new_target, idx),
        ))
    } else {
        None
    }
}

fn try_instruction_delete(
    entry: &mut Entry,
    rng: &mut SimpleRng,
) -> Option<(MutationStrategy, String)> {
    if entry.body.len() <= 1 {
        return None;
    }
    let idx = rng.next_usize(entry.body.len());
    let removed = format!("{:?}", entry.body[idx]);
    entry.body.remove(idx);
    Some((
        MutationStrategy::InstructionDelete,
        format!("removed {:?} at pos {}", removed, idx),
    ))
}

fn try_instruction_dup(
    entry: &mut Entry,
    rng: &mut SimpleRng,
) -> Option<(MutationStrategy, String)> {
    if entry.body.is_empty() {
        return None;
    }
    let idx = rng.next_usize(entry.body.len());
    let instr = entry.body[idx].clone();
    let desc = format!("duplicated {:?} at pos {}", instr, idx);
    entry.body.insert(idx + 1, instr);
    Some((MutationStrategy::InstructionDup, desc))
}
