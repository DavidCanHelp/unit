// fuzz_tests.rs — property/fuzz tests for the untrusted-input surface.
//
// unit rewrites its own dictionary (SMART-MUTATE, genetic programming) and
// ingests data it did not author: S-expressions off the mesh, UREP packages
// from replication, snapshot blobs from disk. The invariant these tests defend
// is simple and load-bearing for a self-replicating system:
//
//     no input — however malformed, hostile, or randomly mutated — may panic.
//
// Everything here is hand-rolled: a small deterministic PRNG and grammar-aware
// generators, driven through `std::panic::catch_unwind`. Zero dependencies, and
// deterministic seeds so any failure reproduces exactly. These run under the
// normal `cargo test`, so they double as permanent regression guards.

#![cfg(test)]

use crate::vm::VM;
use crate::{persist, sexp, spawn};
use std::panic::{self, catch_unwind, AssertUnwindSafe};

// --- deterministic PRNG (xorshift64) ---------------------------------------

struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
    fn byte(&mut self) -> u8 {
        (self.next() >> 24) as u8
    }
    /// true with probability 1/n
    fn chance(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }
}

// --- generators ------------------------------------------------------------

fn rand_bytes(rng: &mut Rng, max: usize) -> Vec<u8> {
    let n = rng.below(max + 1);
    (0..n).map(|_| rng.byte()).collect()
}

/// A random (valid UTF-8) string: mostly printable ASCII, with control bytes
/// and the occasional higher code point mixed in.
fn rand_str(rng: &mut Rng, max: usize) -> String {
    let n = rng.below(max + 1);
    let mut s = String::new();
    for _ in 0..n {
        let r = rng.below(100);
        let c = if r < 78 {
            char::from((0x20 + rng.below(95)) as u8) // printable ASCII
        } else if r < 90 {
            char::from(rng.below(32) as u8) // control
        } else {
            char::from_u32(rng.below(0x2FFF) as u32).unwrap_or('?')
        };
        s.push(c);
    }
    s
}

/// Forth tokens for the loop-free fuzz target: excludes `:` `;` `DO` `LOOP`
/// `BEGIN` `UNTIL` and friends so programs terminate quickly regardless of
/// energy; keeps `IF`/`ELSE`/`THEN` (branch, no iteration) and everything
/// that stresses parsing, dispatch, the stacks, and data-space bounds.
/// Loops, definitions, and recursion get their own target below
/// (`fuzz_forth_full_vocabulary_starves_not_hangs`): since metabolic step
/// metering landed, runaway execution starves and deep recursion hits the
/// body-depth wall — both clean halts, so the full vocabulary is fuzzable.
const FORTH_TOKENS: &[&str] = &[
    "DUP", "DROP", "SWAP", "OVER", "ROT", "NIP", "TUCK", "2DUP", "2DROP", "DEPTH", "?DUP",
    "+", "-", "*", "/", "MOD", "=", "<", ">", "AND", "OR", "NOT", "INVERT",
    "NEGATE", "ABS", "MIN", "MAX", "1+", "1-", "2*", "2/", "0=", "0<",
    "@", "!", "C@", "C!", "HERE", ",", "C,", "CELLS", "ALLOT",
    ".", ".S", "EMIT", "CR", "SPACE",
    "IF", "ELSE", "THEN",
    "VARIABLE", "CONSTANT", "CREATE", "SEE", "WORDS",
    "(", ")", "\\", "[", "]", "'",
    // number literals, in-range so size arithmetic (CELLS/ALLOT) can't overflow
    "0", "1", "-1", "2", "10", "255", "256", "1024", "65535",
    // tokens that fail i64 parse -> exercised as unknown words, not values
    "3.14", "0x10", "999999999999999999999", "-", "abc",
];

fn rand_forth(rng: &mut Rng, max_tokens: usize) -> String {
    let n = rng.below(max_tokens) + 1;
    let mut s = String::new();
    for _ in 0..n {
        if rng.chance(9) {
            // in-range random integer (kept modest so ALLOT can't OOM and
            // CELLS can't overflow); still far beyond data-space bounds so the
            // `@`/`!` bounds checks are exercised.
            let v = (rng.next() % 200_001) as i64 - 100_000;
            s.push_str(&v.to_string());
        } else if rng.chance(13) {
            // a garbage atom, with `:` `;` stripped so no definition can form
            let atom: String = rand_str(rng, 6)
                .chars()
                .filter(|c| *c != ':' && *c != ';')
                .collect();
            s.push_str(&atom);
        } else {
            s.push_str(FORTH_TOKENS[rng.below(FORTH_TOKENS.len())]);
        }
        s.push(if rng.chance(8) { '\n' } else { ' ' });
    }
    s
}

/// A random S-expression string, bounded in nesting depth, with malformed
/// variants (unbalanced parens, stray quotes) mixed in.
fn rand_sexp(rng: &mut Rng, depth: usize, out: &mut String) {
    if depth == 0 || rng.chance(2) {
        match rng.below(4) {
            0 => out.push_str(&(rng.next() as i64).to_string()),
            1 => {
                out.push('"');
                out.push_str(&rand_str(rng, 6).replace('"', "'"));
                if !rng.chance(6) {
                    out.push('"'); // sometimes leave it unterminated
                }
            }
            _ => out.push_str(["foo", "bar", "*", "+", "goal", "result", "peer"][rng.below(7)]),
        }
        return;
    }
    out.push('(');
    let items = rng.below(5);
    for _ in 0..items {
        rand_sexp(rng, depth - 1, out);
        out.push(' ');
    }
    if !rng.chance(7) {
        out.push(')'); // sometimes leave it unbalanced
    }
}

// --- the harness -----------------------------------------------------------

/// Run `body` under a suppressed panic hook; return Ok(()) or the repro seed.
fn hammer(iters: usize, base_seed: u64, mut body: impl FnMut(&mut Rng)) -> Result<(), u64> {
    let prev = panic::take_hook();
    panic::set_hook(Box::new(|_| {})); // stay quiet unless we actually fail
    let mut result = Ok(());
    for i in 0..iters {
        let seed = base_seed ^ (i as u64).wrapping_mul(0x2545_f491_4f6c_dd1d);
        let mut rng = Rng::new(seed);
        if catch_unwind(AssertUnwindSafe(|| body(&mut rng))).is_err() {
            result = Err(seed);
            break;
        }
    }
    panic::set_hook(prev);
    result
}

const ITERS: usize = 30_000;

#[test]
fn fuzz_sexp_parser_never_panics() {
    // Random bytes-as-string and grammar-aware s-exprs into the full parse path
    // (parse -> to_forth, plus the mesh-message entry point).
    let r = hammer(ITERS, 0xA5E1, |rng| {
        let input = if rng.chance(2) {
            let mut s = String::new();
            rand_sexp(rng, 8, &mut s);
            s
        } else {
            rand_str(rng, 64)
        };
        if let Ok(sx) = sexp::parse(&input) {
            let _ = sexp::to_forth(&sx);
        }
        let _ = sexp::try_parse_mesh_msg(&input);
    });
    assert!(r.is_ok(), "sexp parser panicked; reproduce with seed {:#x}", r.unwrap_err());
}

#[test]
fn fuzz_sexp_eval_never_panics() {
    // The mesh-facing eval: parse + translate + execute in a sandbox.
    let r = hammer(ITERS / 4, 0x5EE7, |rng| {
        let mut s = String::new();
        rand_sexp(rng, 6, &mut s);
        let mut vm = VM::new();
        let _ = sexp::eval_sexp(&mut vm, &s);
    });
    assert!(r.is_ok(), "sexp eval panicked; reproduce with seed {:#x}", r.unwrap_err());
}

#[test]
fn fuzz_deeply_nested_sexp_is_rejected_not_overflowed() {
    // Regression guard for the mesh DoS: a peer sending thousands of nested
    // parens must get a graceful error, never a stack overflow.
    for n in [300usize, 1_000, 50_000] {
        let input = "(".repeat(n);
        let got = catch_unwind(|| sexp::parse(&input));
        assert!(got.is_ok(), "parser aborted (stack overflow?) at depth {n}");
        assert!(got.unwrap().is_err(), "expected a parse error at depth {n}");
    }
    // A well-formed but very deep expression is also rejected, not crashed.
    let deep = format!("{}{}", "(".repeat(10_000), ")".repeat(10_000));
    let got = catch_unwind(|| sexp::parse(&deep));
    assert!(got.is_ok() && got.unwrap().is_err(), "deep balanced expr should error, not abort");
}

#[test]
fn fuzz_forth_vm_never_panics() {
    // Random (loop-free, definition-free) Forth into the interpreter.
    let r = hammer(ITERS, 0xF0_47, |rng| {
        let src = rand_forth(rng, 40);
        let mut vm = VM::new();
        let _ = vm.eval(&src);
    });
    assert!(r.is_ok(), "Forth VM panicked; reproduce with seed {:#x}", r.unwrap_err());
}

/// The previously-unfuzzable vocabulary: definitions, loops, recursion.
const FORTH_LOOP_TOKENS: &[&str] = &[
    ":", ";", "RECURSE", "BEGIN", "UNTIL", "WHILE", "REPEAT", "DO", "LOOP", "I", "J",
    "IF", "ELSE", "THEN", "DUP", "DROP", "SWAP", "OVER", "+", "-", "*", "@", "!",
    "0", "1", "-1", "2", "10", "1000", "100000", "0=", "EXIT", "ABC",
];

#[test]
fn fuzz_forth_full_vocabulary_starves_not_hangs() {
    // Random programs WITH loops/definitions/recursion, run on a VM whose
    // energy sits just above the hard floor: any runaway loop must starve
    // (clean halt) and any recursion bomb must hit the depth wall (clean
    // error) — never a hang, never a panic, never a process abort.
    let r = hammer(ITERS / 10, 0xB10C, |rng| {
        let n = rng.below(30) + 1;
        let mut src = String::new();
        for _ in 0..n {
            src.push_str(FORTH_LOOP_TOKENS[rng.below(FORTH_LOOP_TOKENS.len())]);
            src.push(if rng.chance(10) { '\n' } else { ' ' });
        }
        let mut vm = VM::new();
        vm.energy.energy = -498; // 2 spendable energy -> starves within ~30k steps
        let _ = vm.eval(&src);
    });
    assert!(
        r.is_ok(),
        "full-vocabulary fuzz panicked; reproduce with seed {:#x}",
        r.unwrap_err()
    );
}

#[test]
fn fuzz_forth_vm_random_text_never_panics() {
    // Pure garbage text (not grammar-aware) into the interpreter.
    let r = hammer(ITERS, 0x9A_BC, |rng| {
        let src = rand_str(rng, 96);
        let mut vm = VM::new();
        let _ = vm.eval(&src);
    });
    assert!(r.is_ok(), "Forth VM panicked on random text; seed {:#x}", r.unwrap_err());
}

#[test]
fn fuzz_unpack_package_never_panics() {
    // Random bytes, plus bytes that start with the real UREP magic so the
    // length/section parsing (not just the magic check) gets exercised.
    let r = hammer(ITERS, 0x0DEC0, |rng| {
        let data = if rng.chance(2) {
            let mut v = b"UREP".to_vec();
            v.extend(rand_bytes(rng, 64));
            v
        } else {
            rand_bytes(rng, 96)
        };
        let _ = spawn::unpack_package(&data);
    });
    assert!(r.is_ok(), "unpack_package panicked; reproduce with seed {:#x}", r.unwrap_err());
}

#[test]
fn fuzz_deserialize_snapshot_never_panics() {
    let r = hammer(ITERS, 0x50A9, |rng| {
        let data = rand_bytes(rng, 128);
        let _ = persist::deserialize_snapshot(&data);
    });
    assert!(r.is_ok(), "deserialize_snapshot panicked; reproduce with seed {:#x}", r.unwrap_err());
}

#[test]
fn fuzz_genome_from_str_never_panics() {
    // The genome loader takes whatever is on disk: random text, random
    // sexps, and structurally-valid-but-lying snapshots (huge/negative
    // numbers, wrong value types in known keys).
    let r = hammer(ITERS, 0x6E03, |rng| {
        let input = match rng.below(3) {
            0 => rand_str(rng, 96),
            1 => {
                let mut s = String::new();
                rand_sexp(rng, 6, &mut s);
                s
            }
            _ => {
                // a plausible snapshot with adversarial values
                format!(
                    "(unit-snapshot :version {} :id {} :fitness {} :memory-here {} \
                     :stack ({}) :words ((\"A\" {}) ({} \"B\") (\"C\")) \
                     :mutation-stats (:total {}))",
                    (rng.next() as i64),
                    if rng.chance(2) { "\"x\"" } else { "42" },
                    (rng.next() as i64),
                    (rng.next() as i64),
                    (rng.next() as i64),
                    (rng.next() as i64),
                    (rng.next() as i64),
                    (rng.next() as i64),
                )
            }
        };
        let _ = crate::snapshot::from_str(&input);
    });
    assert!(r.is_ok(), "genome from_str panicked; reproduce with seed {:#x}", r.unwrap_err());
}

#[test]
fn fuzz_genome_roundtrip_with_mutated_words() {
    // Property: any snapshot whose words hold arbitrary (GP-mutated) content
    // must serialize -> parse back to exactly the same words.
    let r = hammer(ITERS / 10, 0x6E04, |rng| {
        let n = rng.below(6);
        let words: Vec<(String, String)> = (0..n)
            .map(|_| (rand_str(rng, 12), rand_str(rng, 40)))
            .collect();
        let snap = crate::snapshot::UnitSnapshot {
            node_id: rand_str(rng, 16),
            timestamp: rng.next(),
            stack: (0..rng.below(8)).map(|_| rng.next() as i64).collect(),
            fitness: rng.next() as i64,
            tasks_completed: rng.next() as u32,
            generation: rng.next() as u32,
            mutation_stats: Default::default(),
            words: words.clone(),
            memory_here: rng.below(64),
            memory: (0..rng.below(16)).map(|_| rng.next() as i64).collect(),
            energy: rng.next() as i64,
            energy_max: rng.next() as i64,
            energy_earned: rng.next(),
            energy_spent: rng.next(),
            landscape_depth: rng.next() as u32,
            landscape_generated: rng.next(),
        };
        let text = crate::snapshot::to_sexp(&snap);
        let back = crate::snapshot::from_sexp(&text)
            .expect("serialized snapshot must parse back");
        assert_eq!(back.words, words, "words round-trip");
        assert_eq!(back.stack, snap.stack, "stack round-trip");
    });
    assert!(r.is_ok(), "genome roundtrip failed; reproduce with seed {:#x}", r.unwrap_err());
}
