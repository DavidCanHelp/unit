//! Metabolic energy system for unit.
//!
//! Every unit has an energy budget that fuels computation. Energy is
//! earned from successful tasks, challenge solutions, and passive regen.
//! Energy is spent on GP generations, spawning, mesh messages, and VM steps.
//! Units that run out of energy are throttled until they recover.

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Starting energy for a newly created unit.
pub const INITIAL_ENERGY: i64 = 1000;
/// Maximum energy a unit can accumulate.
pub const MAX_ENERGY: i64 = 5000;
/// Energy gained each tick from passive regeneration.
pub const PASSIVE_REGEN: i64 = 1;
/// Energy earned for completing a task.
pub const TASK_REWARD: i64 = 50;
/// Energy earned for solving a challenge.
pub const CHALLENGE_SOLVE_REWARD: i64 = 100;
/// Energy cost to spawn a new unit.
pub const SPAWN_COST: i64 = 200;
/// Energy cost per GP generation step.
pub const GP_GENERATION_COST: i64 = 5;
/// Energy cost per 1000 VM evaluation steps.
pub const EVAL_STEP_COST_PER_1000: i64 = 1;
/// Energy cost to send a mesh message.
pub const MESH_SEND_COST: i64 = 1;
/// Energy level at or below which the unit becomes throttled.
pub const STARVATION_THRESHOLD: i64 = 0;

const HARD_FLOOR: i64 = -500;

// ---------------------------------------------------------------------------
// EnergyState
// ---------------------------------------------------------------------------

/// Tracks a unit's current energy level, lifetime totals, and throttle state.
#[derive(Clone, Debug)]
pub struct EnergyState {
    pub energy: i64,
    pub max_energy: i64,
    pub total_earned: u64,
    pub total_spent: u64,
    pub peak_energy: i64,
    pub starving_ticks: u64,
    pub throttled: bool,
}

impl Default for EnergyState {
    fn default() -> Self {
        Self::new()
    }
}

impl EnergyState {
    /// Creates a new energy state at the initial energy level.
    pub fn new() -> Self {
        EnergyState {
            energy: INITIAL_ENERGY,
            max_energy: MAX_ENERGY,
            total_earned: 0,
            total_spent: 0,
            peak_energy: INITIAL_ENERGY,
            starving_ticks: 0,
            throttled: false,
        }
    }

    /// Spend energy. Returns false if spending would push below the hard floor.
    pub fn spend(&mut self, amount: i64, _reason: &str) -> bool {
        if self.energy - amount < HARD_FLOOR {
            return false;
        }
        self.energy -= amount;
        self.total_spent += amount as u64;
        if self.energy <= STARVATION_THRESHOLD {
            self.throttled = true;
        }
        true
    }

    /// Earn energy, capped at max_energy.
    pub fn earn(&mut self, amount: i64, _reason: &str) {
        self.energy = (self.energy + amount).min(self.max_energy);
        self.total_earned += amount as u64;
        if self.energy > self.peak_energy {
            self.peak_energy = self.energy;
        }
        if self.energy > STARVATION_THRESHOLD {
            self.throttled = false;
        }
    }

    /// Called once per main loop iteration.
    pub fn tick(&mut self) {
        self.earn(PASSIVE_REGEN, "passive");
        if self.energy <= 0 {
            self.starving_ticks += 1;
        }
    }

    /// Returns true if spending `amount` would stay above the hard floor.
    pub fn can_afford(&self, amount: i64) -> bool {
        self.energy - amount >= HARD_FLOOR
    }

    /// Returns true if the unit is currently energy-throttled.
    pub fn is_throttled(&self) -> bool {
        self.throttled
    }

    /// Metabolic efficiency: total earned / total spent. Higher is better.
    pub fn efficiency(&self) -> f64 {
        self.total_earned as f64 / self.total_spent.max(1) as f64
    }

    /// Formats the energy state as a human-readable summary string.
    pub fn format(&self) -> String {
        format!(
            "energy: {}/{} (earned: {}, spent: {}, efficiency: {:.2})",
            self.energy,
            self.max_energy,
            self.total_earned,
            self.total_spent,
            self.efficiency()
        )
    }

    /// Formats a compact one-line energy status including node hex ID.
    pub fn format_line(&self, id: &[u8; 8]) -> String {
        format!(
            "  {} energy={}/{} eff={:.2}{}",
            crate::mesh::id_to_hex(id),
            self.energy,
            self.max_energy,
            self.efficiency(),
            if self.throttled { " [THROTTLED]" } else { "" }
        )
    }
}

// ---------------------------------------------------------------------------
// EnergyEvent (for optional logging)
// ---------------------------------------------------------------------------

/// A loggable energy event for auditing energy changes.
#[derive(Clone, Debug)]
pub enum EnergyEvent {
    Earned { amount: i64, reason: String },
    Spent { amount: i64, reason: String },
    Throttled,
    Recovered,
}

// ---------------------------------------------------------------------------
// S-expression constructors
// ---------------------------------------------------------------------------

/// Builds an S-expression representing the energy status for mesh broadcast.
pub fn sexp_energy_status(node_hex: &str, state: &EnergyState) -> String {
    format!(
        "(energy-status :id \"{}\" :energy {} :max {} :efficiency {:.2})",
        node_hex,
        state.energy,
        state.max_energy,
        state.efficiency()
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_starts_at_initial() {
        let e = EnergyState::new();
        assert_eq!(e.energy, INITIAL_ENERGY);
        assert_eq!(e.max_energy, MAX_ENERGY);
        assert!(!e.throttled);
    }

    #[test]
    fn test_spend_deducts() {
        let mut e = EnergyState::new();
        assert!(e.spend(100, "test"));
        assert_eq!(e.energy, INITIAL_ENERGY - 100);
        assert_eq!(e.total_spent, 100);
    }

    #[test]
    fn test_spend_hard_floor() {
        let mut e = EnergyState::new();
        // Spend down to near floor
        assert!(e.spend(1400, "drain")); // 1000 - 1400 = -400, above -500
        assert_eq!(e.energy, -400);
        // This would push to -600, below -500 floor
        assert!(!e.spend(200, "too much"));
        assert_eq!(e.energy, -400); // unchanged
    }

    #[test]
    fn test_earn_caps_at_max() {
        let mut e = EnergyState::new();
        e.earn(10000, "bonanza");
        assert_eq!(e.energy, MAX_ENERGY);
        assert_eq!(e.total_earned, 10000);
        assert_eq!(e.peak_energy, MAX_ENERGY);
    }

    #[test]
    fn test_throttled_at_threshold() {
        let mut e = EnergyState::new();
        e.spend(1000, "drain"); // energy = 0
        assert!(e.throttled);
        assert!(e.is_throttled());
    }

    #[test]
    fn test_recovery_clears_throttle() {
        let mut e = EnergyState::new();
        e.spend(1000, "drain");
        assert!(e.throttled);
        e.earn(50, "reward");
        assert!(!e.throttled);
        assert!(!e.is_throttled());
    }

    #[test]
    fn test_efficiency() {
        let mut e = EnergyState::new();
        e.earn(100, "work");
        e.spend(50, "cost");
        // earned=100, spent=50
        assert!((e.efficiency() - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_tick_adds_passive() {
        let mut e = EnergyState::new();
        let before = e.energy;
        e.tick();
        assert_eq!(e.energy, (before + PASSIVE_REGEN).min(MAX_ENERGY));
    }

    #[test]
    fn test_starving_ticks() {
        let mut e = EnergyState::new();
        e.spend(1100, "drain"); // energy = -100
        assert_eq!(e.starving_ticks, 0);
        e.tick(); // energy = -99, still <= 0
        assert_eq!(e.starving_ticks, 1);
        e.tick();
        assert_eq!(e.starving_ticks, 2);
    }

    #[test]
    fn test_can_afford() {
        let e = EnergyState::new();
        assert!(e.can_afford(1000));
        assert!(e.can_afford(1500)); // 1000 - 1500 = -500 = HARD_FLOOR, ok
        assert!(!e.can_afford(1501)); // would be -501 < -500
    }

    #[test]
    fn test_format() {
        let e = EnergyState::new();
        let s = e.format();
        assert!(s.contains("energy: 1000/5000"));
    }

    #[test]
    fn test_sexp_energy_status() {
        let e = EnergyState::new();
        let s = sexp_energy_status("aabbccdd", &e);
        assert!(s.contains("energy-status"));
        assert!(s.contains(":energy 1000"));
        assert!(s.contains(":max 5000"));
    }
}
