//! JSON-based state persistence for unit.
//!
//! Saves and loads a unit's state as human-readable JSON so hackers can
//! inspect and hand-edit a unit's brain. Zero dependencies -- hand-written
//! JSON serializer/parser.

#[cfg(not(target_arch = "wasm32"))]
use crate::mesh::NodeId;
use crate::types::{Cell, Entry, Instruction};

// ---------------------------------------------------------------------------
// UnitSnapshot — the JSON-serializable state of a unit
// ---------------------------------------------------------------------------

/// The JSON-serializable state of a unit, capturing stack, dictionary, energy, and more.
#[derive(Clone, Debug)]
pub struct UnitSnapshot {
    pub node_id: String,
    pub timestamp: u64,
    pub stack: Vec<Cell>,
    pub fitness: i64,
    pub tasks_completed: u32,
    pub generation: u32,
    pub mutation_stats: MutStats,
    pub words: Vec<(String, String)>, // (name, decompiled source)
    pub memory_here: usize,
    pub memory: Vec<Cell>, // only up to `here`
    // Energy state
    pub energy: i64,
    pub energy_max: i64,
    pub energy_earned: u64,
    pub energy_spent: u64,
    // Landscape state
    pub landscape_depth: u32,
    pub landscape_generated: u64,
}

/// Mutation outcome statistics: counts of neutral, beneficial, harmful, and lethal mutations.
#[derive(Clone, Debug, Default)]
pub struct MutStats {
    pub total: u32,
    pub neutral: u32,
    pub beneficial: u32,
    pub harmful: u32,
    pub lethal: u32,
}

// ---------------------------------------------------------------------------
// JSON serializer (no serde)
// ---------------------------------------------------------------------------

pub(crate) fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out
}

/// Serializes a `UnitSnapshot` to a human-readable JSON string.
pub fn to_json(snap: &UnitSnapshot) -> String {
    let mut j = String::with_capacity(4096);
    j.push_str("{\n");
    j.push_str("  \"version\": 1,\n");
    j.push_str(&format!(
        "  \"node_id\": \"{}\",\n",
        escape_json_string(&snap.node_id)
    ));
    j.push_str(&format!("  \"timestamp\": {},\n", snap.timestamp));
    j.push_str(&format!("  \"fitness\": {},\n", snap.fitness));
    j.push_str(&format!(
        "  \"tasks_completed\": {},\n",
        snap.tasks_completed
    ));
    j.push_str(&format!("  \"generation\": {},\n", snap.generation));

    // Energy
    j.push_str(&format!("  \"energy\": {},\n", snap.energy));
    j.push_str(&format!("  \"energy_max\": {},\n", snap.energy_max));
    j.push_str(&format!("  \"energy_earned\": {},\n", snap.energy_earned));
    j.push_str(&format!("  \"energy_spent\": {},\n", snap.energy_spent));

    // Landscape
    j.push_str(&format!(
        "  \"landscape_depth\": {},\n",
        snap.landscape_depth
    ));
    j.push_str(&format!(
        "  \"landscape_generated\": {},\n",
        snap.landscape_generated
    ));

    // Stack
    j.push_str("  \"stack\": [");
    for (i, v) in snap.stack.iter().enumerate() {
        if i > 0 {
            j.push_str(", ");
        }
        j.push_str(&format!("{}", v));
    }
    j.push_str("],\n");

    // Mutation stats
    j.push_str("  \"mutation_stats\": {\n");
    j.push_str(&format!("    \"total\": {},\n", snap.mutation_stats.total));
    j.push_str(&format!(
        "    \"neutral\": {},\n",
        snap.mutation_stats.neutral
    ));
    j.push_str(&format!(
        "    \"beneficial\": {},\n",
        snap.mutation_stats.beneficial
    ));
    j.push_str(&format!(
        "    \"harmful\": {},\n",
        snap.mutation_stats.harmful
    ));
    j.push_str(&format!("    \"lethal\": {}\n", snap.mutation_stats.lethal));
    j.push_str("  },\n");

    // Words (user-defined)
    j.push_str("  \"words\": {\n");
    for (i, (name, source)) in snap.words.iter().enumerate() {
        j.push_str(&format!(
            "    \"{}\": \"{}\"",
            escape_json_string(name),
            escape_json_string(source)
        ));
        if i + 1 < snap.words.len() {
            j.push(',');
        }
        j.push('\n');
    }
    j.push_str("  },\n");

    // Memory
    j.push_str(&format!("  \"memory_here\": {},\n", snap.memory_here));
    j.push_str("  \"memory\": [");
    for (i, v) in snap.memory.iter().enumerate() {
        if i > 0 {
            j.push_str(", ");
        }
        j.push_str(&format!("{}", v));
    }
    j.push_str("]\n");
    j.push_str("}\n");
    j
}

// ---------------------------------------------------------------------------
// JSON parser (minimal, for known UnitSnapshot structure)
// ---------------------------------------------------------------------------

/// Parses a JSON string into a `UnitSnapshot`, returning `None` on invalid input.
pub fn from_json(input: &str) -> Option<UnitSnapshot> {
    let mut snap = UnitSnapshot {
        node_id: String::new(),
        timestamp: 0,
        stack: Vec::new(),
        fitness: 0,
        tasks_completed: 0,
        generation: 0,
        mutation_stats: MutStats::default(),
        words: Vec::new(),
        memory_here: 0,
        memory: Vec::new(),
        energy: crate::energy::INITIAL_ENERGY,
        energy_max: crate::energy::MAX_ENERGY,
        energy_earned: 0,
        energy_spent: 0,
        landscape_depth: 0,
        landscape_generated: 0,
    };

    let input = input.trim();
    if !input.starts_with('{') || !input.ends_with('}') {
        return None;
    }

    // Simple line-by-line parsing for our known JSON structure.
    let mut in_words = false;
    let mut in_mutation = false;

    for line in input.lines() {
        let line = line.trim();

        // Detect section transitions
        if line.starts_with("\"words\"") && line.contains('{') {
            in_words = true;
            continue;
        }
        if in_words && line.starts_with('}') {
            in_words = false;
            continue;
        }
        if line.starts_with("\"mutation_stats\"") && line.contains('{') {
            in_mutation = true;
            continue;
        }
        if in_mutation && line.starts_with('}') {
            in_mutation = false;
            continue;
        }

        // Parse stack array on one line
        if line.starts_with("\"stack\"") {
            if let Some(arr) = extract_array(line) {
                snap.stack = parse_i64_array(&arr);
            }
            continue;
        }

        // Parse memory array on one line
        if line.starts_with("\"memory\"") && line.contains('[') {
            if let Some(arr) = extract_array(line) {
                snap.memory = parse_i64_array(&arr);
            }
            continue;
        }

        if in_words {
            // Parse "NAME": "source"
            if let Some((key, val)) = parse_kv_string(line) {
                snap.words.push((key, val));
            }
            continue;
        }

        if in_mutation {
            if let Some((key, val)) = parse_kv_number(line) {
                match key.as_str() {
                    "total" => snap.mutation_stats.total = val as u32,
                    "neutral" => snap.mutation_stats.neutral = val as u32,
                    "beneficial" => snap.mutation_stats.beneficial = val as u32,
                    "harmful" => snap.mutation_stats.harmful = val as u32,
                    "lethal" => snap.mutation_stats.lethal = val as u32,
                    _ => {}
                }
            }
            continue;
        }

        // Top-level scalar fields
        if let Some((key, val)) = parse_kv_number(line) {
            match key.as_str() {
                "timestamp" => snap.timestamp = val as u64,
                "fitness" => snap.fitness = val,
                "tasks_completed" => snap.tasks_completed = val as u32,
                "generation" => snap.generation = val as u32,
                "memory_here" => snap.memory_here = val as usize,
                "energy" => snap.energy = val,
                "energy_max" => snap.energy_max = val,
                "energy_earned" => snap.energy_earned = val as u64,
                "energy_spent" => snap.energy_spent = val as u64,
                "landscape_depth" => snap.landscape_depth = val as u32,
                "landscape_generated" => snap.landscape_generated = val as u64,
                "version" => {} // ignore
                _ => {}
            }
        } else if let Some((key, val)) = parse_kv_string(line) {
            if key.as_str() == "node_id" {
                snap.node_id = val
            }
        }
    }

    Some(snap)
}

// Extract the contents between [ and ] from a line
fn extract_array(line: &str) -> Option<String> {
    let start = line.find('[')?;
    let end = line.rfind(']')?;
    Some(line[start + 1..end].to_string())
}

fn parse_i64_array(s: &str) -> Vec<i64> {
    s.split(',')
        .filter(|p| !p.trim().is_empty())
        .filter_map(|p| p.trim().parse().ok())
        .collect()
}

// Parse "key": "value" (possibly with trailing comma)
fn parse_kv_string(line: &str) -> Option<(String, String)> {
    let line = line.trim().trim_end_matches(',');
    let colon = line.find(':')?;
    let key = line[..colon].trim().trim_matches('"');
    let val_part = line[colon + 1..].trim();
    if val_part.starts_with('"') && val_part.ends_with('"') {
        let inner = &val_part[1..val_part.len() - 1];
        Some((key.to_string(), unescape_json_string(inner)))
    } else {
        None
    }
}

// Parse "key": number (possibly with trailing comma)
fn parse_kv_number(line: &str) -> Option<(String, i64)> {
    let line = line.trim().trim_end_matches(',');
    let colon = line.find(':')?;
    let key = line[..colon].trim().trim_matches('"');
    let val = line[colon + 1..].trim().parse::<i64>().ok()?;
    Some((key.to_string(), val))
}

fn unescape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Decompile a word to Forth source
// ---------------------------------------------------------------------------

/// Decompiles a dictionary entry back into readable Forth source code.
pub fn decompile_word(
    entry: &Entry,
    dictionary: &[Entry],
    primitive_names: &[(String, usize)],
) -> String {
    let mut out = format!(": {} ", entry.name);
    for instr in &entry.body {
        match instr {
            Instruction::Primitive(id) => {
                let pname = primitive_names
                    .iter()
                    .find(|(_, pid)| pid == id)
                    .map(|(n, _)| n.as_str())
                    .unwrap_or("?PRIM");
                out.push_str(pname);
                out.push(' ');
            }
            Instruction::Literal(val) => {
                out.push_str(&format!("{} ", val));
            }
            Instruction::Call(idx) => {
                if *idx < dictionary.len() {
                    out.push_str(&dictionary[*idx].name);
                    out.push(' ');
                }
            }
            Instruction::StringLit(s) => {
                out.push_str(&format!(".\" {}\" ", s));
            }
            Instruction::Branch(off) => {
                out.push_str(&format!("BRANCH({}) ", off));
            }
            Instruction::BranchIfZero(off) => {
                out.push_str(&format!("0BRANCH({}) ", off));
            }
        }
    }
    out.push(';');
    out
}

// ---------------------------------------------------------------------------
// File I/O helpers
// ---------------------------------------------------------------------------

/// Returns the directory path for storing snapshot files.
#[cfg(not(target_arch = "wasm32"))]
pub fn snapshot_dir(_node_id: &NodeId) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    format!("{}/.unit/snapshots", home)
}

/// Returns the full file path for a node's snapshot JSON file.
#[cfg(not(target_arch = "wasm32"))]
pub fn snapshot_path(node_id: &NodeId) -> String {
    let id_hex: String = node_id.iter().map(|b| format!("{:02x}", b)).collect();
    format!("{}/{}.json", snapshot_dir(node_id), id_hex)
}

/// Writes a JSON snapshot to disk, creating directories as needed.
#[cfg(not(target_arch = "wasm32"))]
pub fn save_json_snapshot(node_id: &NodeId, json: &str) -> Result<String, String> {
    let dir = snapshot_dir(node_id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {}", e))?;
    let path = snapshot_path(node_id);
    std::fs::write(&path, json).map_err(|e| format!("write: {}", e))?;
    Ok(path)
}

/// Loads a JSON snapshot from disk, returning `None` if missing.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_json_snapshot(node_id: &NodeId) -> Option<String> {
    let path = snapshot_path(node_id);
    std::fs::read_to_string(&path).ok()
}

/// Lists all snapshot node IDs found in the snapshot directory.
#[cfg(not(target_arch = "wasm32"))]
pub fn list_json_snapshots() -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = format!("{}/.unit/snapshots", home);
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") {
                names.push(name.trim_end_matches(".json").to_string());
            }
        }
    }
    names.sort();
    names
}

// ---------------------------------------------------------------------------
// WASM: in-memory snapshot storage (no filesystem)
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
mod wasm_store {
    use std::cell::RefCell;

    thread_local! {
        static SNAPSHOT: RefCell<Option<String>> = RefCell::new(None);
    }

    pub fn save(json: &str) {
        SNAPSHOT.with(|s| *s.borrow_mut() = Some(json.to_string()));
    }

    pub fn load() -> Option<String> {
        SNAPSHOT.with(|s| s.borrow().clone())
    }

    pub fn has_snapshot() -> bool {
        SNAPSHOT.with(|s| s.borrow().is_some())
    }
}

#[cfg(target_arch = "wasm32")]
pub fn snapshot_path(_node_id: &[u8; 8]) -> String {
    "(in-memory)".to_string()
}

#[cfg(target_arch = "wasm32")]
pub fn save_json_snapshot(_node_id: &[u8; 8], json: &str) -> Result<String, String> {
    wasm_store::save(json);
    Ok("(in-memory)".to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn load_json_snapshot(_node_id: &[u8; 8]) -> Option<String> {
    wasm_store::load()
}

#[cfg(target_arch = "wasm32")]
pub fn list_json_snapshots() -> Vec<String> {
    if wasm_store::has_snapshot() {
        vec!["(in-memory)".to_string()]
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_snapshot() -> UnitSnapshot {
        UnitSnapshot {
            node_id: "abcdef0123456789".to_string(),
            timestamp: 1711900800,
            stack: vec![42, -7, 100],
            fitness: 55,
            tasks_completed: 12,
            generation: 3,
            mutation_stats: MutStats {
                total: 20,
                neutral: 10,
                beneficial: 5,
                harmful: 3,
                lethal: 2,
            },
            words: vec![
                ("SQUARE".to_string(), ": SQUARE DUP * ;".to_string()),
                ("CUBE".to_string(), ": CUBE DUP SQUARE * ;".to_string()),
            ],
            memory_here: 3,
            memory: vec![0, 42, -1],
            energy: 800,
            energy_max: 5000,
            energy_earned: 500,
            energy_spent: 700,
            landscape_depth: 2,
            landscape_generated: 5,
        }
    }

    #[test]
    fn test_roundtrip() {
        let snap = make_test_snapshot();
        let json = to_json(&snap);
        let restored = from_json(&json).unwrap();
        assert_eq!(snap.node_id, restored.node_id);
        assert_eq!(snap.timestamp, restored.timestamp);
        assert_eq!(snap.stack, restored.stack);
        assert_eq!(snap.fitness, restored.fitness);
        assert_eq!(snap.tasks_completed, restored.tasks_completed);
        assert_eq!(snap.generation, restored.generation);
        assert_eq!(snap.mutation_stats.total, restored.mutation_stats.total);
        assert_eq!(
            snap.mutation_stats.beneficial,
            restored.mutation_stats.beneficial
        );
        assert_eq!(snap.words, restored.words);
        assert_eq!(snap.memory_here, restored.memory_here);
        assert_eq!(snap.memory, restored.memory);
    }

    #[test]
    fn test_empty_snapshot() {
        let snap = UnitSnapshot {
            node_id: "0000000000000000".to_string(),
            timestamp: 0,
            stack: vec![],
            fitness: 0,
            tasks_completed: 0,
            generation: 0,
            mutation_stats: MutStats::default(),
            words: vec![],
            memory_here: 0,
            memory: vec![],
            energy: 1000,
            energy_max: 5000,
            energy_earned: 0,
            energy_spent: 0,
            landscape_depth: 0,
            landscape_generated: 0,
        };
        let json = to_json(&snap);
        let restored = from_json(&json).unwrap();
        assert_eq!(restored.stack.len(), 0);
        assert_eq!(restored.words.len(), 0);
        assert_eq!(restored.memory.len(), 0);
    }

    #[test]
    fn test_escape_roundtrip() {
        let snap = UnitSnapshot {
            node_id: "test".to_string(),
            timestamp: 0,
            stack: vec![],
            fitness: 0,
            tasks_completed: 0,
            generation: 0,
            mutation_stats: MutStats::default(),
            words: vec![(
                "HELLO".to_string(),
                ": HELLO .\" hello\\nworld\" ;".to_string(),
            )],
            memory_here: 0,
            memory: vec![],
            energy: 1000,
            energy_max: 5000,
            energy_earned: 0,
            energy_spent: 0,
            landscape_depth: 0,
            landscape_generated: 0,
        };
        let json = to_json(&snap);
        let restored = from_json(&json).unwrap();
        assert_eq!(snap.words[0].1, restored.words[0].1);
    }

    #[test]
    fn test_corrupt_json() {
        assert!(from_json("not json").is_none());
        assert!(from_json("").is_none());
        // Minimal valid JSON
        assert!(from_json("{}").is_some());
    }

    #[test]
    fn test_json_is_human_readable() {
        let snap = make_test_snapshot();
        let json = to_json(&snap);
        assert!(json.contains("\"node_id\""));
        assert!(json.contains("SQUARE"));
        assert!(json.contains("CUBE"));
        assert!(json.contains("\"fitness\": 55"));
    }

    #[test]
    fn test_decompile_word() {
        use crate::types::Instruction;
        let entry = Entry {
            name: "TEST".to_string(),
            immediate: false,
            hidden: false,
            body: vec![
                Instruction::Literal(42),
                Instruction::Primitive(7), // P_ADD
            ],
        };
        let prims = vec![("+".to_string(), 7usize)];
        let dict = vec![entry.clone()];
        let source = decompile_word(&entry, &dict, &prims);
        assert_eq!(source, ": TEST 42 + ;");
    }

    #[test]
    fn test_msg_snapshot_sexp() {
        let sexp = crate::sexp::parse("(snapshot :id \"abc\" :fitness 42 :gen 0)").unwrap();
        assert_eq!(crate::sexp::msg_type(&sexp), Some("snapshot"));
        assert_eq!(sexp.get_key(":fitness").unwrap().as_number(), Some(42));
    }

    #[test]
    fn test_msg_resurrect_sexp() {
        let sexp = crate::sexp::parse(
            "(resurrect :id \"abc\" :fitness 42 :gen 0 :saved-at \"1711900800\")",
        )
        .unwrap();
        assert_eq!(crate::sexp::msg_type(&sexp), Some("resurrect"));
        assert_eq!(
            sexp.get_key(":saved-at").unwrap().as_str(),
            Some("1711900800")
        );
    }
}
