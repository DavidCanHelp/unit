// niche.rs — Niche construction for unit
//
// Units modify their own selection pressures based on their
// strengths. A unit that solves arithmetic challenges well
// increases arithmetic challenge frequency in its environment.
// This creates ecological specialization across the colony.

use std::collections::HashMap;

/// A unit's ecological niche profile, tracking specialization
/// across challenge categories.
pub struct NicheProfile {
    /// Category -> strength (0.0 to 1.0).
    pub specializations: HashMap<String, f64>,
    /// Recent challenge outcomes: (category, solved?). Last 50.
    pub challenge_history: Vec<(String, bool)>,
    /// Category -> frequency multiplier for landscape generation.
    pub niche_modifier: HashMap<String, f64>,
    /// Tick when niche was last updated.
    pub constructed_at: u64,
}

impl Default for NicheProfile {
    fn default() -> Self {
        Self::new()
    }
}

impl NicheProfile {
    pub fn new() -> Self {
        NicheProfile {
            specializations: HashMap::new(),
            challenge_history: Vec::new(),
            niche_modifier: HashMap::new(),
            constructed_at: 0,
        }
    }
}

/// Categorize a challenge by its name pattern.
pub fn categorize_challenge(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.contains("compose") {
        "composition".to_string()
    } else if lower.contains("fib") {
        "fibonacci".to_string()
    } else if lower.contains("square") || lower.contains("cube") {
        "polynomial".to_string()
    } else if lower.contains("evolved") {
        "evolved".to_string()
    } else if lower.contains("short") || lower.contains("parsimony") {
        "parsimony".to_string()
    } else {
        "general".to_string()
    }
}

/// Recalculate specializations and niche modifiers from history.
pub fn update_niche(profile: &mut NicheProfile) {
    // Cap history at 50.
    while profile.challenge_history.len() > 50 {
        profile.challenge_history.remove(0);
    }

    // Count solved/total per category.
    let mut totals: HashMap<String, (u32, u32)> = HashMap::new(); // (solved, total)
    for (cat, solved) in &profile.challenge_history {
        let entry = totals.entry(cat.clone()).or_insert((0, 0));
        entry.1 += 1;
        if *solved {
            entry.0 += 1;
        }
    }

    // Update specializations.
    profile.specializations.clear();
    for (cat, (solved, total)) in &totals {
        if *total > 0 {
            profile
                .specializations
                .insert(cat.clone(), *solved as f64 / *total as f64);
        }
    }

    // Update niche modifiers.
    profile.niche_modifier.clear();
    for (cat, strength) in &profile.specializations {
        if *strength > 0.6 {
            profile.niche_modifier.insert(cat.clone(), 2.0);
        } else if *strength < 0.2 {
            profile.niche_modifier.insert(cat.clone(), 0.5);
        } else {
            profile.niche_modifier.insert(cat.clone(), 1.0);
        }
    }
}

/// Apply niche modifiers to a landscape engine's challenge generation.
/// Returns the modifier for a given challenge category.
pub fn niche_modifier_for(profile: &NicheProfile, category: &str) -> f64 {
    *profile.niche_modifier.get(category).unwrap_or(&1.0)
}

/// Format a human-readable niche profile summary.
pub fn format_niche(profile: &NicheProfile) -> String {
    let mut out = String::from("--- niche profile ---\n");

    if profile.specializations.is_empty() {
        out.push_str("no specializations yet\n");
        return out;
    }

    // Sort by strength descending.
    let mut specs: Vec<(&String, &f64)> = profile.specializations.iter().collect();
    specs.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (cat, strength) in &specs {
        let modifier = profile.niche_modifier.get(*cat).unwrap_or(&1.0);
        out.push_str(&format!(
            "  {}: {:.0}% (modifier: {:.1}x)\n",
            cat,
            *strength * 100.0,
            modifier
        ));
    }

    if let Some((name, strength)) = dominant_niche(profile) {
        out.push_str(&format!("dominant niche: {} ({:.0}%)\n", name, strength * 100.0));
    }

    out
}

/// Serialize niche profile as S-expression for mesh broadcast.
pub fn sexp_niche_broadcast(node_hex: &str, profile: &NicheProfile) -> String {
    let specs: Vec<String> = profile
        .specializations
        .iter()
        .map(|(cat, strength)| format!("(\"{cat}\" {:.2})", strength))
        .collect();
    format!(
        "(niche-profile :from \"{}\" :specializations ({}))",
        node_hex,
        specs.join(" ")
    )
}

/// Return the category with the highest specialization, if any exceeds 0.4.
pub fn dominant_niche(profile: &NicheProfile) -> Option<(String, f64)> {
    profile
        .specializations
        .iter()
        .filter(|(_, &v)| v > 0.4)
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(k, v)| (k.clone(), *v))
}

// =========================================================================
// Tests
// =========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_categorize_fib() {
        assert_eq!(categorize_challenge("fib15"), "fibonacci");
        assert_eq!(categorize_challenge("fib10-short9"), "fibonacci"); // fib takes priority over short
    }

    #[test]
    fn test_categorize_square() {
        assert_eq!(categorize_challenge("square-55"), "polynomial");
        assert_eq!(categorize_challenge("cube-27"), "polynomial");
    }

    #[test]
    fn test_categorize_compose() {
        assert_eq!(
            categorize_challenge("compose-fib10+square-55"),
            "composition"
        );
    }

    #[test]
    fn test_categorize_evolved() {
        assert_eq!(categorize_challenge("evolved-abc12345"), "evolved");
    }

    #[test]
    fn test_categorize_unknown() {
        assert_eq!(categorize_challenge("custom-xyz"), "general");
    }

    #[test]
    fn test_update_niche_specialization() {
        let mut profile = NicheProfile::new();
        // 10 solved fibonacci challenges.
        for _ in 0..10 {
            profile
                .challenge_history
                .push(("fibonacci".to_string(), true));
        }
        // 2 failed polynomial.
        for _ in 0..2 {
            profile
                .challenge_history
                .push(("polynomial".to_string(), false));
        }
        update_niche(&mut profile);
        let fib_spec = profile.specializations.get("fibonacci").unwrap();
        assert!(*fib_spec > 0.8, "fibonacci spec={}, expected > 0.8", fib_spec);
    }

    #[test]
    fn test_update_niche_modifier() {
        let mut profile = NicheProfile::new();
        // Strong in fibonacci.
        for _ in 0..8 {
            profile
                .challenge_history
                .push(("fibonacci".to_string(), true));
        }
        // Weak in polynomial.
        for _ in 0..10 {
            profile
                .challenge_history
                .push(("polynomial".to_string(), false));
        }
        update_niche(&mut profile);
        let fib_mod = profile.niche_modifier.get("fibonacci").unwrap();
        assert_eq!(*fib_mod, 2.0);
        let poly_mod = profile.niche_modifier.get("polynomial").unwrap();
        assert_eq!(*poly_mod, 0.5);
    }

    #[test]
    fn test_niche_history_cap() {
        let mut profile = NicheProfile::new();
        for i in 0..60 {
            profile
                .challenge_history
                .push((format!("cat-{}", i), true));
        }
        update_niche(&mut profile);
        assert!(
            profile.challenge_history.len() <= 50,
            "history len={}, expected <= 50",
            profile.challenge_history.len()
        );
    }

    #[test]
    fn test_dominant_niche() {
        let mut profile = NicheProfile::new();
        profile.specializations.insert("fibonacci".to_string(), 0.9);
        profile
            .specializations
            .insert("polynomial".to_string(), 0.3);
        let dom = dominant_niche(&profile);
        assert!(dom.is_some());
        let (name, strength) = dom.unwrap();
        assert_eq!(name, "fibonacci");
        assert!((strength - 0.9).abs() < 0.01);
    }

    #[test]
    fn test_format_niche() {
        let mut profile = NicheProfile::new();
        for _ in 0..8 {
            profile
                .challenge_history
                .push(("fibonacci".to_string(), true));
        }
        update_niche(&mut profile);
        let output = format_niche(&profile);
        assert!(output.contains("niche profile"));
        assert!(output.contains("fibonacci"));
        assert!(output.contains("dominant niche"));
    }
}
