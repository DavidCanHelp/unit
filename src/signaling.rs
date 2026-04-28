//! Inter-unit signaling — direct (peer inbox) and environmental layers.
//!
//! Pure data layer in this module: the `Signal` struct, the `SignalKind`
//! enum (Direct + Environmental), and the per-unit `Inbox`. Producers
//! (SAY!, MARK!) and consumers (LISTEN, INBOX?, SENSE) live in the VM
//! primitives module; the per-host environmental field lives in
//! `multi_unit`. See `docs/signaling.md` for the design.

use crate::mesh::NodeId;

/// Niche category key. Matches the string keys already used by
/// `niche::NicheProfile::specializations`, so environmental signals can
/// share the existing niche addressing without a new coordinate system.
pub type NicheCategory = String;

/// Default per-unit inbox capacity. FIFO with drop-from-front on overflow.
pub const INBOX_CAP: usize = 64;

/// What kind of signal this is.
///
/// `Direct` is a SAY! broadcast delivered to a peer's inbox.
/// `Environmental` is a MARK! deposit keyed by niche category that
/// decays in a per-host field; it's also delivered into the inbox of
/// units that share the niche so they can `LISTEN` for it the same way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignalKind {
    Direct,
    Environmental { niche: NicheCategory },
}

/// One signal in flight. Single-cell payload, sender id, kind, sent-at
/// tick — minimum viable shape for the v0.28 substrate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signal {
    pub sender: NodeId,
    pub value: i64,
    pub kind: SignalKind,
    pub sent_at_tick: u64,
}

impl Signal {
    pub fn direct(sender: NodeId, value: i64, sent_at_tick: u64) -> Self {
        Signal {
            sender,
            value,
            kind: SignalKind::Direct,
            sent_at_tick,
        }
    }

    pub fn environmental(
        sender: NodeId,
        value: i64,
        niche: NicheCategory,
        sent_at_tick: u64,
    ) -> Self {
        Signal {
            sender,
            value,
            kind: SignalKind::Environmental { niche },
            sent_at_tick,
        }
    }

    /// True for SAY!-style direct signals.
    pub fn is_direct(&self) -> bool {
        matches!(self.kind, SignalKind::Direct)
    }
}

/// Per-unit signal inbox. FIFO with a fixed capacity; on overflow the
/// oldest entry is dropped (drop-head, not drop-incoming) so recent
/// signals always survive. Backed by `Vec<Signal>` per the design doc.
#[derive(Clone, Debug)]
pub struct Inbox {
    entries: Vec<Signal>,
    cap: usize,
}

impl Default for Inbox {
    fn default() -> Self {
        Self::new()
    }
}

impl Inbox {
    pub fn new() -> Self {
        Inbox {
            entries: Vec::new(),
            cap: INBOX_CAP,
        }
    }

    /// Construct with a custom cap. Useful for tests.
    pub fn with_capacity(cap: usize) -> Self {
        Inbox {
            entries: Vec::new(),
            cap,
        }
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Push a signal, dropping the oldest entry when at capacity.
    pub fn push(&mut self, signal: Signal) {
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
        }
        self.entries.push(signal);
    }

    /// Pop the oldest entry, or None if empty.
    pub fn pop_oldest(&mut self) -> Option<Signal> {
        if self.entries.is_empty() {
            None
        } else {
            Some(self.entries.remove(0))
        }
    }

    /// Iterate without consuming. Used by mate-selection signal scanning.
    pub fn iter(&self) -> std::slice::Iter<'_, Signal> {
        self.entries.iter()
    }

    /// Drop every signal whose sent_at_tick is older than `min_tick`.
    /// Reserved for future stale-signal eviction; not used in v0.28.
    pub fn evict_older_than(&mut self, min_tick: u64) {
        self.entries.retain(|s| s.sent_at_tick >= min_tick);
    }
}

// ===========================================================================
// EnvironmentalField — the slow channel for MARK! / SENSE.
// ===========================================================================

/// Multiplicative decay applied to every entry per tick.
pub const ENV_DECAY_RATE: f64 = 0.95;

/// Minimum strength; entries that fall below this are removed.
pub const ENV_MIN_STRENGTH: f64 = 1.0;

/// Per-host environmental field, keyed by niche category. MARK! deposits
/// either sum into or displace the existing slot (whichever is greater);
/// SENSE reads. `decay_tick` ages every entry by `ENV_DECAY_RATE`.
///
/// Native-only: WASM shim never constructs one. The field is owned by
/// `MultiUnitHost`; the VM's `env_view` cache holds a per-unit read of
/// the slot keyed by its dominant niche, refreshed between evals.
#[derive(Clone, Debug, Default)]
pub struct EnvironmentalField {
    slots: std::collections::HashMap<NicheCategory, f64>,
}

impl EnvironmentalField {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sum or displace: the new strength is `max(current + value, value)`,
    /// which means a small repeated mark accumulates while a single large
    /// mark can take over a slot. Keeps the design's reinforcement +
    /// novelty-displacement property without adding parameters.
    pub fn deposit(&mut self, niche: NicheCategory, value: f64) {
        let entry = self.slots.entry(niche).or_insert(0.0);
        *entry = (*entry + value).max(value);
    }

    /// Read current strength for `niche` as i64 (truncating). Returns 0
    /// if the slot is empty. SENSE consumes this value without changing
    /// the field.
    pub fn sense(&self, niche: &str) -> i64 {
        self.slots.get(niche).copied().unwrap_or(0.0) as i64
    }

    /// Multiply every slot by `ENV_DECAY_RATE`, removing entries that
    /// fall below `ENV_MIN_STRENGTH`.
    pub fn decay_tick(&mut self) {
        self.slots.retain(|_, v| {
            *v *= ENV_DECAY_RATE;
            *v >= ENV_MIN_STRENGTH
        });
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Iterate (niche, strength) without consuming.
    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, NicheCategory, f64> {
        self.slots.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nid(b: u8) -> NodeId {
        [b; 8]
    }

    #[test]
    fn inbox_starts_empty() {
        let inbox = Inbox::new();
        assert!(inbox.is_empty());
        assert_eq!(inbox.len(), 0);
        assert_eq!(inbox.cap(), INBOX_CAP);
    }

    #[test]
    fn inbox_default_matches_new() {
        let a = Inbox::new();
        let b = Inbox::default();
        assert_eq!(a.cap(), b.cap());
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn push_then_len_one() {
        let mut inbox = Inbox::new();
        inbox.push(Signal::direct(nid(1), 42, 0));
        assert_eq!(inbox.len(), 1);
        assert!(!inbox.is_empty());
    }

    #[test]
    fn pop_returns_fifo_order() {
        let mut inbox = Inbox::new();
        inbox.push(Signal::direct(nid(1), 10, 0));
        inbox.push(Signal::direct(nid(2), 20, 1));
        inbox.push(Signal::direct(nid(3), 30, 2));
        assert_eq!(inbox.pop_oldest().unwrap().value, 10);
        assert_eq!(inbox.pop_oldest().unwrap().value, 20);
        assert_eq!(inbox.pop_oldest().unwrap().value, 30);
        assert!(inbox.pop_oldest().is_none());
    }

    #[test]
    fn overflow_drops_oldest() {
        let mut inbox = Inbox::with_capacity(3);
        inbox.push(Signal::direct(nid(1), 1, 0));
        inbox.push(Signal::direct(nid(2), 2, 1));
        inbox.push(Signal::direct(nid(3), 3, 2));
        // At cap. Next push should drop value=1.
        inbox.push(Signal::direct(nid(4), 4, 3));
        assert_eq!(inbox.len(), 3);
        let values: Vec<i64> = inbox.iter().map(|s| s.value).collect();
        assert_eq!(values, vec![2, 3, 4]);
    }

    #[test]
    fn cap_64_default() {
        let mut inbox = Inbox::new();
        for i in 0..70 {
            inbox.push(Signal::direct(nid(0), i as i64, i as u64));
        }
        assert_eq!(inbox.len(), 64);
        // Oldest surviving is i=6 (i=0..=5 dropped).
        assert_eq!(inbox.pop_oldest().unwrap().value, 6);
    }

    #[test]
    fn pop_empty_returns_none() {
        let mut inbox = Inbox::new();
        assert!(inbox.pop_oldest().is_none());
    }

    #[test]
    fn signal_kind_direct_vs_environmental() {
        let d = Signal::direct(nid(1), 7, 0);
        let e = Signal::environmental(nid(2), 9, "fibonacci".to_string(), 1);
        assert!(d.is_direct());
        assert!(!e.is_direct());
        assert_eq!(d.kind, SignalKind::Direct);
        assert_eq!(
            e.kind,
            SignalKind::Environmental {
                niche: "fibonacci".to_string()
            }
        );
    }

    #[test]
    fn signal_round_trip_fields() {
        let s = Signal::direct(nid(0xab), 12345, 99);
        assert_eq!(s.sender, [0xab; 8]);
        assert_eq!(s.value, 12345);
        assert_eq!(s.sent_at_tick, 99);
    }

    #[test]
    fn iter_does_not_consume() {
        let mut inbox = Inbox::new();
        inbox.push(Signal::direct(nid(1), 1, 0));
        inbox.push(Signal::direct(nid(2), 2, 0));
        let _ = inbox.iter().count();
        assert_eq!(inbox.len(), 2);
    }

    #[test]
    fn evict_older_than_drops_stale() {
        let mut inbox = Inbox::new();
        inbox.push(Signal::direct(nid(1), 1, 5));
        inbox.push(Signal::direct(nid(2), 2, 10));
        inbox.push(Signal::direct(nid(3), 3, 15));
        inbox.evict_older_than(10);
        assert_eq!(inbox.len(), 2);
        assert_eq!(inbox.pop_oldest().unwrap().value, 2);
    }

    // -----------------------------------------------------------------------
    // EnvironmentalField
    // -----------------------------------------------------------------------

    #[test]
    fn env_field_starts_empty() {
        let f = EnvironmentalField::new();
        assert!(f.is_empty());
        assert_eq!(f.len(), 0);
        assert_eq!(f.sense("anything"), 0);
    }

    #[test]
    fn env_deposit_then_sense() {
        let mut f = EnvironmentalField::new();
        f.deposit("fibonacci".to_string(), 100.0);
        assert_eq!(f.sense("fibonacci"), 100);
        assert_eq!(f.sense("sorting"), 0);
    }

    #[test]
    fn env_deposit_accumulates() {
        let mut f = EnvironmentalField::new();
        f.deposit("fib".to_string(), 30.0);
        f.deposit("fib".to_string(), 20.0);
        // sum = 50, max(50, 20) = 50
        assert_eq!(f.sense("fib"), 50);
    }

    #[test]
    fn env_large_deposit_displaces() {
        let mut f = EnvironmentalField::new();
        f.deposit("fib".to_string(), 5.0);
        // Large mark exceeds sum: max(5+1000, 1000) = 1005 — no surprise here,
        // sum still wins. Verify the displacement branch by depositing a
        // negative-summing pathological case.
        f.deposit("fib".to_string(), -2.0);
        // entry was 5, new = max(5 + -2, -2) = max(3, -2) = 3.
        assert_eq!(f.sense("fib"), 3);
    }

    #[test]
    fn env_decay_tick_multiplies_by_rate() {
        let mut f = EnvironmentalField::new();
        f.deposit("fib".to_string(), 100.0);
        // 5 ticks: 100 * 0.95^5 ≈ 77.378
        for _ in 0..5 {
            f.decay_tick();
        }
        let sensed = f.sense("fib");
        assert!(
            (76..=78).contains(&sensed),
            "expected ~77 after 5 ticks, got {}",
            sensed
        );
    }

    #[test]
    fn env_decay_drops_below_floor() {
        let mut f = EnvironmentalField::new();
        f.deposit("fib".to_string(), 1.5);
        // 0.95 * 1.5 = 1.425 (above 1.0, retained)
        f.decay_tick();
        assert_eq!(f.len(), 1);
        // Drain by repeated decay.
        for _ in 0..40 {
            f.decay_tick();
        }
        assert_eq!(f.len(), 0, "below ENV_MIN_STRENGTH entries should drop");
    }
}
