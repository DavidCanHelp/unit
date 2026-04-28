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
        if name.starts_with("SOL-") || rng.next_u64().is_multiple_of(2) {
            result.push((name.to_string(), def.to_string()));
        }
    }

    // Unique to B: 50% chance (but SOL-* always included).
    for (name, def) in &map_b {
        if map_a.contains_key(name) {
            continue; // already handled
        }
        if name.starts_with("SOL-") || rng.next_u64().is_multiple_of(2) {
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

/// Signal-weighted mate selection (v0.28). Same tournament-of-three as
/// `select_mate` but pulls candidate values from the unit's signal
/// inbox instead of raw peer fitness — implementing the design's
/// "mate-finding first" pressure.
///
/// Algorithm:
///   1. Scan the inbox for Direct signals from peers in the candidate
///      set; build a `(NodeId, signaled_value)` list (most recent
///      signal per sender wins).
///   2. If at least one peer has signaled, run tournament-of-three on
///      that list — the value chosen by the candidate is what gets
///      weighted, not the verified fitness.
///   3. If no peers have signaled, fall through to `select_mate` so
///      reproduction still works for units that haven't adopted COURT
///      or any other signaling word. **Additive — never breaks the
///      existing path.**
///
/// The signaled value is *whatever the candidate's dictionary chose to
/// broadcast* — `FITNESS SAY!` (the COURT prelude) is honest, but
/// nothing in the substrate enforces that. This is the load-bearing
/// asymmetry that makes signal honesty an empirical question rather
/// than a constructed one.
pub fn select_mate_signaled(
    peers: &[(NodeId, i64)],
    inbox: &crate::signaling::Inbox,
    rng: &mut SimpleRng,
) -> Option<NodeId> {
    if peers.is_empty() {
        return None;
    }
    use std::collections::HashMap;
    let peer_set: std::collections::HashSet<NodeId> = peers.iter().map(|(id, _)| *id).collect();
    // Most recent signal per sender wins (later inbox entries overwrite).
    let mut signaled: HashMap<NodeId, i64> = HashMap::new();
    for sig in inbox.iter() {
        if !sig.is_direct() {
            continue;
        }
        if peer_set.contains(&sig.sender) {
            signaled.insert(sig.sender, sig.value);
        }
    }
    if signaled.is_empty() {
        return select_mate(peers, rng);
    }
    let candidates: Vec<(NodeId, i64)> = signaled.into_iter().collect();
    let count = candidates.len().min(3);
    let mut best_idx = rng.next_usize(candidates.len());
    let mut best_value = candidates[best_idx].1;
    for _ in 1..count {
        let idx = rng.next_usize(candidates.len());
        if candidates[idx].1 > best_value {
            best_idx = idx;
            best_value = candidates[idx].1;
        }
    }
    Some(candidates[best_idx].0)
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

    // -----------------------------------------------------------------------
    // Signal-weighted mate selection (v0.28)
    // -----------------------------------------------------------------------

    #[test]
    fn test_select_mate_signaled_picks_highest_signal() {
        // Five peers — none of these fitnesses should drive the choice.
        // Three of them have signaled (with values that don't match
        // their actual fitness, which is the whole point of the
        // experiment), and one signaled value is much higher than the
        // others.
        let peers = vec![
            ([1u8; 8], 10),
            ([2u8; 8], 90), // high actual fitness, but no signal
            ([3u8; 8], 30),
            ([4u8; 8], 50),
            ([5u8; 8], 20),
        ];
        let mut inbox = crate::signaling::Inbox::new();
        inbox.push(crate::signaling::Signal::direct([1u8; 8], 200, 1)); // boasting
        inbox.push(crate::signaling::Signal::direct([3u8; 8], 50, 2));
        inbox.push(crate::signaling::Signal::direct([5u8; 8], 70, 3));
        // Over many runs, peer 1 (signaled value 200) should win most often
        // — even though peer 2 has the highest actual fitness, peer 2
        // didn't signal and so isn't a candidate in the signal-weighted
        // path.
        let mut counts = [0u32; 5];
        for seed in 0..400 {
            let mut rng = SimpleRng::new(seed);
            if let Some(id) = select_mate_signaled(&peers, &inbox, &mut rng) {
                for (i, (pid, _)) in peers.iter().enumerate() {
                    if id == *pid {
                        counts[i] += 1;
                    }
                }
            }
        }
        let max_idx = counts.iter().enumerate().max_by_key(|(_, &c)| c).unwrap().0;
        assert_eq!(
            max_idx, 0,
            "expected peer 1 (signal=200) to win most, got idx {} ({:?})",
            max_idx, counts
        );
        // Peer 2 had no signal, so should never be picked here.
        assert_eq!(counts[1], 0, "peer 2 had no signal but was picked");
    }

    #[test]
    fn test_select_mate_signaled_falls_back_when_inbox_empty() {
        let peers = vec![
            ([1u8; 8], 10),
            ([2u8; 8], 90),
            ([3u8; 8], 30),
        ];
        let inbox = crate::signaling::Inbox::new();
        let mut counts = [0u32; 3];
        for seed in 0..200 {
            let mut rng = SimpleRng::new(seed);
            if let Some(id) = select_mate_signaled(&peers, &inbox, &mut rng) {
                for (i, (pid, _)) in peers.iter().enumerate() {
                    if id == *pid {
                        counts[i] += 1;
                    }
                }
            }
        }
        // With no signals, the function falls through to select_mate's
        // peer-fitness tournament. Peer 2 (fitness=90) should win.
        let max_idx = counts.iter().enumerate().max_by_key(|(_, &c)| c).unwrap().0;
        assert_eq!(max_idx, 1, "fallback should pick fittest peer, got {}", max_idx);
    }

    #[test]
    fn test_select_mate_signaled_falls_back_when_signals_off_set() {
        // Inbox contains signals from senders not in the candidate
        // peer list — should still fall through to peer fitness.
        let peers = vec![
            ([1u8; 8], 10),
            ([2u8; 8], 90),
        ];
        let mut inbox = crate::signaling::Inbox::new();
        inbox.push(crate::signaling::Signal::direct([99u8; 8], 1000, 1));
        let mut counts = [0u32; 2];
        for seed in 0..200 {
            let mut rng = SimpleRng::new(seed);
            if let Some(id) = select_mate_signaled(&peers, &inbox, &mut rng) {
                for (i, (pid, _)) in peers.iter().enumerate() {
                    if id == *pid {
                        counts[i] += 1;
                    }
                }
            }
        }
        let max_idx = counts.iter().enumerate().max_by_key(|(_, &c)| c).unwrap().0;
        assert_eq!(
            max_idx, 1,
            "irrelevant signals should not steer selection"
        );
    }

    #[test]
    fn test_select_mate_signaled_no_peers_returns_none() {
        let peers: Vec<(NodeId, i64)> = vec![];
        let inbox = crate::signaling::Inbox::new();
        let mut rng = make_rng();
        assert!(select_mate_signaled(&peers, &inbox, &mut rng).is_none());
    }

    #[test]
    fn test_select_mate_signaled_ignores_environmental_signals() {
        let peers = vec![([1u8; 8], 10), ([2u8; 8], 90)];
        let mut inbox = crate::signaling::Inbox::new();
        inbox.push(crate::signaling::Signal::environmental(
            [1u8; 8],
            5000,
            "fib".to_string(),
            1,
        ));
        // Environmental signals don't count as mate-finding signals.
        // Falls back to peer-fitness tournament.
        let mut rng = SimpleRng::new(0);
        let pick = select_mate_signaled(&peers, &inbox, &mut rng);
        assert!(pick.is_some());
        // Most importantly, the run does not crash and the selection
        // is on the candidate set, not on the environmental sender.
    }

    #[test]
    fn test_select_mate_signaled_most_recent_signal_wins_per_sender() {
        // Same sender signals twice — newer value should be the one
        // tournament sees.
        let peers = vec![([1u8; 8], 10), ([2u8; 8], 10)];
        let mut inbox = crate::signaling::Inbox::new();
        inbox.push(crate::signaling::Signal::direct([1u8; 8], 5, 1));
        inbox.push(crate::signaling::Signal::direct([1u8; 8], 999, 2));
        inbox.push(crate::signaling::Signal::direct([2u8; 8], 100, 3));
        let mut wins = [0u32; 2];
        for seed in 0..200 {
            let mut rng = SimpleRng::new(seed);
            if let Some(id) = select_mate_signaled(&peers, &inbox, &mut rng) {
                if id == [1u8; 8] {
                    wins[0] += 1;
                } else if id == [2u8; 8] {
                    wins[1] += 1;
                }
            }
        }
        // Peer 1's *latest* signal is 999, much higher than peer 2's 100.
        assert!(
            wins[0] > wins[1],
            "peer 1 latest signal should win more than peer 2: {:?}",
            wins
        );
    }
}
