// reproduction.rs — Sexual reproduction for unit
//
// Two units combine dictionaries via crossover to produce
// a child with traits from both parents. Mating partner
// selection uses tournament selection based on fitness.

use crate::features::mutation::SimpleRng;
use crate::mesh::NodeId;

/// A mating request sent over the mesh.
#[derive(Clone, Debug)]
pub struct MatingRequest {
    pub requester_id: NodeId,
    pub requester_fitness: i64,
    pub dictionary_words: Vec<(String, String)>,
}

/// A mating response returned by a potential partner.
#[derive(Clone, Debug)]
pub struct MatingResponse {
    pub accepted: bool,
    pub responder_id: NodeId,
    pub responder_fitness: i64,
    pub dictionary_words: Vec<(String, String)>,
}

/// Combine two parent dictionaries via crossover.
///
/// - Shared words: pick from the fitter parent
/// - Unique words: include with 50% probability
/// - SOL-* words: always include from both (immune memory)
/// - Cap at 50 words to prevent genome bloat
pub fn crossover_dictionaries(
    parent_a: &[(String, String)],
    parent_b: &[(String, String)],
    fitness_a: i64,
    fitness_b: i64,
    rng: &mut SimpleRng,
) -> Vec<(String, String)> {
    use std::collections::HashMap;

    let map_a: HashMap<&str, &str> = parent_a.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let map_b: HashMap<&str, &str> = parent_b.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    let mut result: Vec<(String, String)> = Vec::new();

    // Shared words: pick from fitter parent.
    for (name, def_a) in &map_a {
        if let Some(def_b) = map_b.get(name) {
            let def = if fitness_a >= fitness_b { *def_a } else { *def_b };
            result.push((name.to_string(), def.to_string()));
        }
    }

    // Unique to A: 50% chance (but SOL-* always included).
    for (name, def) in &map_a {
        if map_b.contains_key(name) {
            continue; // already handled
        }
        if name.starts_with("SOL-") || rng.next_u64() % 2 == 0 {
            result.push((name.to_string(), def.to_string()));
        }
    }

    // Unique to B: 50% chance (but SOL-* always included).
    for (name, def) in &map_b {
        if map_a.contains_key(name) {
            continue; // already handled
        }
        if name.starts_with("SOL-") || rng.next_u64() % 2 == 0 {
            result.push((name.to_string(), def.to_string()));
        }
    }

    // Cap at 50 words.
    result.truncate(50);
    result
}

/// Tournament selection: pick 3 random peers, return the fittest.
pub fn select_mate(peers: &[(NodeId, i64)], rng: &mut SimpleRng) -> Option<NodeId> {
    if peers.is_empty() {
        return None;
    }
    let count = peers.len().min(3);
    let mut best_idx = rng.next_usize(peers.len());
    let mut best_fitness = peers[best_idx].1;
    for _ in 1..count {
        let idx = rng.next_usize(peers.len());
        if peers[idx].1 > best_fitness {
            best_idx = idx;
            best_fitness = peers[idx].1;
        }
    }
    Some(peers[best_idx].0)
}

/// Serialize a mating request as an S-expression for mesh broadcast.
pub fn sexp_mating_request(req: &MatingRequest) -> String {
    let hex = crate::mesh::id_to_hex(&req.requester_id);
    let words: Vec<String> = req
        .dictionary_words
        .iter()
        .map(|(name, def)| format!("(\"{name}\" \"{def}\")"))
        .collect();
    format!(
        "(mating-request :from \"{hex}\" :fitness {} :words ({}))",
        req.requester_fitness,
        words.join(" ")
    )
}

/// Serialize a mating response as an S-expression.
pub fn sexp_mating_response(resp: &MatingResponse) -> String {
    let hex = crate::mesh::id_to_hex(&resp.responder_id);
    let words: Vec<String> = resp
        .dictionary_words
        .iter()
        .map(|(name, def)| format!("(\"{name}\" \"{def}\")"))
        .collect();
    format!(
        "(mating-response :accepted {} :from \"{hex}\" :fitness {} :words ({}))",
        if resp.accepted { "true" } else { "false" },
        resp.responder_fitness,
        words.join(" ")
    )
}

/// Parse an incoming mating request from an S-expression string.
pub fn parse_mating_request(sexp_str: &str) -> Option<MatingRequest> {
    let sexp = crate::sexp::try_parse_mesh_msg(sexp_str)?;
    if crate::sexp::msg_type(&sexp) != Some("mating-request") {
        return None;
    }
    let from_hex = sexp.get_key(":from")?.as_str()?;
    let id = parse_node_id(from_hex)?;
    let fitness = sexp.get_key(":fitness")?.as_number()?;
    let words = parse_word_pairs(&sexp, ":words");
    Some(MatingRequest {
        requester_id: id,
        requester_fitness: fitness,
        dictionary_words: words,
    })
}

/// Parse an incoming mating response from an S-expression string.
pub fn parse_mating_response(sexp_str: &str) -> Option<MatingResponse> {
    let sexp = crate::sexp::try_parse_mesh_msg(sexp_str)?;
    if crate::sexp::msg_type(&sexp) != Some("mating-response") {
        return None;
    }
    let accepted_str = sexp.get_key(":accepted")?.as_atom()?;
    let accepted = accepted_str == "true";
    let from_hex = sexp.get_key(":from")?.as_str()?;
    let id = parse_node_id(from_hex)?;
    let fitness = sexp.get_key(":fitness")?.as_number()?;
    let words = parse_word_pairs(&sexp, ":words");
    Some(MatingResponse {
        accepted,
        responder_id: id,
        responder_fitness: fitness,
        dictionary_words: words,
    })
}

fn parse_node_id(hex: &str) -> Option<NodeId> {
    if hex.len() != 16 {
        return None;
    }
    let mut id = [0u8; 8];
    for i in 0..8 {
        id[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(id)
}

fn parse_word_pairs(sexp: &crate::sexp::Sexp, key: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    if let Some(words_sexp) = sexp.get_key(key) {
        if let Some(items) = words_sexp.as_list() {
            for item in items {
                if let Some(pair) = item.as_list() {
                    if pair.len() >= 2 {
                        if let (Some(name), Some(def)) = (pair[0].as_str(), pair[1].as_str()) {
                            result.push((name.to_string(), def.to_string()));
                        }
                    }
                }
            }
        }
    }
    result
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rng() -> SimpleRng {
        SimpleRng::new(42)
    }

    fn test_node_a() -> NodeId {
        [0xAA, 0xBB, 0xCC, 0xDD, 0x11, 0x22, 0x33, 0x44]
    }

    fn test_node_b() -> NodeId {
        [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]
    }

    #[test]
    fn test_crossover_shared_words_picks_fitter() {
        let mut rng = make_rng();
        let a = vec![("SQUARE".into(), ": SQUARE DUP * ;".into())];
        let b = vec![("SQUARE".into(), ": SQUARE DUP DUP * * ;".into())];

        // A is fitter, so A's definition should be picked.
        let result = crossover_dictionaries(&a, &b, 100, 50, &mut rng);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1, ": SQUARE DUP * ;");

        // B is fitter, so B's definition should be picked.
        let result2 = crossover_dictionaries(&a, &b, 30, 80, &mut rng);
        assert_eq!(result2.len(), 1);
        assert_eq!(result2[0].1, ": SQUARE DUP DUP * * ;");
    }

    #[test]
    fn test_crossover_unique_words_probabilistic() {
        // Run many trials: unique words should appear roughly 50% of the time.
        let a: Vec<(String, String)> = (0..20)
            .map(|i| (format!("A-WORD-{}", i), format!(": A-WORD-{} {} ;", i, i)))
            .collect();
        let b: Vec<(String, String)> = Vec::new();
        let mut included = 0;
        let trials = 100;
        for seed in 0..trials {
            let mut rng = SimpleRng::new(seed);
            let result = crossover_dictionaries(&a, &b, 50, 50, &mut rng);
            included += result.len();
        }
        let avg = included as f64 / trials as f64;
        // Should be roughly 10 (50% of 20), allow wide tolerance.
        assert!(avg > 5.0, "avg={}, expected ~10", avg);
        assert!(avg < 16.0, "avg={}, expected ~10", avg);
    }

    #[test]
    fn test_crossover_sol_words_always_included() {
        let a = vec![
            ("SOL-FIB10".into(), ": SOL-FIB10 0 1 10 0 DO OVER + SWAP LOOP DROP . ;".into()),
            ("MAYBE".into(), ": MAYBE 1 ;".into()),
        ];
        let b = vec![
            ("SOL-SQUARE".into(), ": SOL-SQUARE DUP * . ;".into()),
            ("OTHER".into(), ": OTHER 2 ;".into()),
        ];

        // Run multiple times — SOL-* should always be included.
        for seed in 0..20 {
            let mut rng = SimpleRng::new(seed);
            let result = crossover_dictionaries(&a, &b, 50, 50, &mut rng);
            let names: Vec<&str> = result.iter().map(|(n, _)| n.as_str()).collect();
            assert!(names.contains(&"SOL-FIB10"), "seed={}: missing SOL-FIB10", seed);
            assert!(names.contains(&"SOL-SQUARE"), "seed={}: missing SOL-SQUARE", seed);
        }
    }

    #[test]
    fn test_crossover_cap_at_50() {
        let mut rng = SimpleRng::new(0);
        let a: Vec<(String, String)> = (0..40)
            .map(|i| (format!("A-{}", i), format!("def-a-{}", i)))
            .collect();
        let b: Vec<(String, String)> = (0..40)
            .map(|i| (format!("B-{}", i), format!("def-b-{}", i)))
            .collect();
        let result = crossover_dictionaries(&a, &b, 50, 50, &mut rng);
        assert!(result.len() <= 50, "got {} words, expected <= 50", result.len());
    }

    #[test]
    fn test_select_mate_tournament() {
        let peers = vec![
            ([1u8; 8], 10),
            ([2u8; 8], 50),
            ([3u8; 8], 30),
            ([4u8; 8], 80),
            ([5u8; 8], 20),
        ];
        // Over many runs, the highest-fitness peer should be selected most often.
        let mut counts = [0u32; 5];
        for seed in 0..200 {
            let mut rng = SimpleRng::new(seed);
            if let Some(id) = select_mate(&peers, &mut rng) {
                for (i, (pid, _)) in peers.iter().enumerate() {
                    if id == *pid {
                        counts[i] += 1;
                    }
                }
            }
        }
        // Peer 4 (fitness=80) should be selected most often.
        let max_idx = counts.iter().enumerate().max_by_key(|(_, &c)| c).unwrap().0;
        assert_eq!(max_idx, 3, "expected peer 4 (idx 3) to win most, got idx {}", max_idx);
    }

    #[test]
    fn test_select_mate_no_peers() {
        let mut rng = make_rng();
        let peers: Vec<(NodeId, i64)> = vec![];
        assert!(select_mate(&peers, &mut rng).is_none());
    }

    #[test]
    fn test_sexp_roundtrip_request() {
        let req = MatingRequest {
            requester_id: test_node_a(),
            requester_fitness: 42,
            dictionary_words: vec![
                ("SQUARE".into(), ": SQUARE DUP * ;".into()),
                ("CUBE".into(), ": CUBE DUP DUP * * ;".into()),
            ],
        };
        let serialized = sexp_mating_request(&req);
        assert!(serialized.contains("mating-request"));
        assert!(serialized.contains(":fitness 42"));

        let parsed = parse_mating_request(&serialized);
        assert!(parsed.is_some(), "failed to parse: {}", serialized);
        let parsed = parsed.unwrap();
        assert_eq!(parsed.requester_id, test_node_a());
        assert_eq!(parsed.requester_fitness, 42);
        assert_eq!(parsed.dictionary_words.len(), 2);
        assert_eq!(parsed.dictionary_words[0].0, "SQUARE");
    }

    #[test]
    fn test_sexp_roundtrip_response() {
        let resp = MatingResponse {
            accepted: true,
            responder_id: test_node_b(),
            responder_fitness: 88,
            dictionary_words: vec![("SOL-FIB10".into(), ": SOL-FIB10 55 . ;".into())],
        };
        let serialized = sexp_mating_response(&resp);
        assert!(serialized.contains("mating-response"));
        assert!(serialized.contains(":accepted true"));

        let parsed = parse_mating_response(&serialized);
        assert!(parsed.is_some(), "failed to parse: {}", serialized);
        let parsed = parsed.unwrap();
        assert!(parsed.accepted);
        assert_eq!(parsed.responder_id, test_node_b());
        assert_eq!(parsed.responder_fitness, 88);
        assert_eq!(parsed.dictionary_words.len(), 1);
    }
}
