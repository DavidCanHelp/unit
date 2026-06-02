//! Host resource reader — memory, load, and derived headroom.
//!
//! A unit that may migrate between hosts needs to know how loaded its
//! current host is. This module is the measurement primitive: a zero-
//! dependency reader that samples host memory and CPU load and derives a
//! normalized utilization / headroom pair. No migration logic lives here —
//! this is just the sensor.
//!
//! Platform behavior:
//! - Linux native: read `/proc/meminfo` (MemTotal, MemAvailable),
//!   `/proc/loadavg` (1-minute figure), and the logical CPU count from
//!   `/proc/cpuinfo` (or `/proc/stat` as a fallback).
//! - Other native (e.g. macOS): `/proc` isn't present, so we return a
//!   clearly-marked unavailable reading rather than guessing.
//! - wasm32: a unit in the browser can't read host resources at all, so the
//!   measurement is cfg-shimmed out the same way MARK!/SENSE are gated in
//!   `signaling.rs` / `multi_unit.rs` — the API surface is identical but
//!   `measure()` returns the unavailable reading.
//!
//! Utilization is the BINDING CONSTRAINT: `max(memory_fraction,
//! load_per_core)`. Whichever resource is tightest sets the pressure, because
//! a host that's memory-bound and a host that's CPU-bound are both equally
//! unfit to take on another unit.

// ---------------------------------------------------------------------------
// The replication ceiling
// ---------------------------------------------------------------------------

/// The single source of truth for the replication ceiling.
///
/// Its ONLY role is refusal: a coordinate must not replicate when host
/// utilization is at or above this fraction. The colony never grows *toward*
/// this number — it is a wall, not a setpoint. Minimum-sufficient population
/// is emergent from the local replication rule plus energy metabolism; nothing
/// anywhere targets 80%.
pub const CEILING_UTILIZATION: f64 = 0.80;

// ---------------------------------------------------------------------------
// HostResources
// ---------------------------------------------------------------------------

/// A point-in-time reading of a host's resource pressure.
///
/// `valid` distinguishes a real measurement from an unavailable one (the
/// platform has no `/proc`, or we're in the browser). When `valid` is false
/// the numeric fields are all zero and should not be interpreted.
#[derive(Clone, Debug, PartialEq)]
pub struct HostResources {
    /// True if this reading reflects a real measurement. False means the
    /// host couldn't be measured (no `/proc`, wasm32, or a read error).
    pub valid: bool,
    /// Total physical memory, in kibibytes (from MemTotal).
    pub mem_total_kb: u64,
    /// Memory available for new allocations, in kibibytes (MemAvailable).
    pub mem_available_kb: u64,
    /// System 1-minute load average.
    pub load_one: f64,
    /// Number of logical CPUs, used to normalize `load_one` to a per-core
    /// figure. Zero on an unavailable reading.
    pub n_cpus: u32,
    /// The binding constraint, in `0.0..=1.0`: `max(memory_fraction,
    /// load_one / n_cpus)`. Whichever resource is tightest. `1.0 - headroom`.
    pub utilization: f64,
    /// Fraction of headroom on the binding constraint, in `0.0..=1.0`
    /// (`1.0 - utilization`).
    pub headroom: f64,
}

impl HostResources {
    /// Builds the unavailable reading: a measurement we couldn't take.
    ///
    /// All numeric fields are zero; `valid` is false. Callers should branch
    /// on [`HostResources::is_available`] before using the figures.
    pub fn unavailable() -> Self {
        HostResources {
            valid: false,
            mem_total_kb: 0,
            mem_available_kb: 0,
            load_one: 0.0,
            n_cpus: 0,
            utilization: 0.0,
            headroom: 0.0,
        }
    }

    /// True iff this is a real reading AND utilization is below the ceiling.
    ///
    /// This is the refusal gate for self-replication. It fails CLOSED: an
    /// unavailable reading (no `/proc`, a parse failure, or wasm32) returns
    /// false, because a coordinate that cannot measure its own host must not
    /// replicate onto it. The ceiling is a refusal, never a target — see
    /// [`CEILING_UTILIZATION`].
    pub fn has_headroom(&self) -> bool {
        self.valid && self.utilization < CEILING_UTILIZATION
    }

    /// Encodes this reading's headroom as a bounded `0..=100` percentage for
    /// gossip advertisement — a few bytes a peer can read to judge whether we
    /// have room. An unavailable reading advertises `0` (no room), so a
    /// coordinate that can't measure itself fails closed on the wire too.
    pub fn advertised_headroom_pct(&self) -> u8 {
        if !self.valid {
            return 0;
        }
        (self.headroom * 100.0).round().clamp(0.0, 100.0) as u8
    }

    /// Samples the current host's resources.
    ///
    /// On Linux this reads `/proc`; on any other platform (other native OSes,
    /// wasm32) it returns [`HostResources::unavailable`].
    pub fn measure() -> Self {
        #[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
        {
            Self::measure_linux()
        }
        #[cfg(not(all(not(target_arch = "wasm32"), target_os = "linux")))]
        {
            HostResources::unavailable()
        }
    }

    /// Linux measurement path: read and parse `/proc/meminfo`,
    /// `/proc/loadavg`, and the CPU count (`/proc/cpuinfo`, falling back to
    /// `/proc/stat`). Any read or parse failure yields the unavailable reading
    /// so a partial `/proc` never produces bogus figures.
    #[cfg(all(not(target_arch = "wasm32"), target_os = "linux"))]
    fn measure_linux() -> Self {
        let meminfo = match std::fs::read_to_string("/proc/meminfo") {
            Ok(s) => s,
            Err(_) => return HostResources::unavailable(),
        };
        let loadavg = match std::fs::read_to_string("/proc/loadavg") {
            Ok(s) => s,
            Err(_) => return HostResources::unavailable(),
        };
        // CPU count: prefer /proc/cpuinfo, fall back to /proc/stat.
        let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        let stat = std::fs::read_to_string("/proc/stat").unwrap_or_default();
        from_proc_text(&meminfo, &loadavg, &cpuinfo, &stat)
    }

    /// Returns true if this reading reflects a real measurement.
    pub fn is_available(&self) -> bool {
        self.valid
    }

    /// Builds a valid reading from already-parsed parts, deriving the
    /// binding-constraint utilization / headroom pair.
    ///
    /// `mem_available_kb` is clamped to `mem_total_kb`. Utilization is the
    /// larger of the memory-used fraction and the per-core load
    /// (`load_one / n_cpus`); both it and headroom are clamped to `0.0..=1.0`.
    ///
    /// `pub(crate)` so the spawn layer (and its tests) can synthesize a known
    /// reading without going through `/proc`.
    pub(crate) fn from_parts(
        mem_total_kb: u64,
        mem_available_kb: u64,
        load_one: f64,
        n_cpus: u32,
    ) -> Self {
        let available = mem_available_kb.min(mem_total_kb);
        let mem_fraction = if mem_total_kb > 0 {
            1.0 - (available as f64 / mem_total_kb as f64)
        } else {
            0.0
        };
        // Load normalized to per-core: 1.0 means ~one runnable task per core.
        let load_normalized = if n_cpus > 0 {
            load_one / n_cpus as f64
        } else {
            load_one
        };
        // The BINDING CONSTRAINT: whichever resource is tightest sets pressure.
        let utilization = mem_fraction.max(load_normalized).clamp(0.0, 1.0);
        let headroom = (1.0 - utilization).clamp(0.0, 1.0);
        HostResources {
            valid: true,
            mem_total_kb,
            mem_available_kb: available,
            load_one,
            n_cpus,
            utilization,
            headroom,
        }
    }
}

/// True iff an advertised headroom percentage (`0..=100`) indicates a peer
/// that is under the ceiling with room — i.e. *sufficient* to accept a unit.
///
/// This mirrors [`HostResources::has_headroom`] on the wire: `has_headroom` is
/// `utilization < CEILING`, equivalently `headroom > 1 - CEILING`, so a peer
/// is sufficient exactly when its advertised headroom fraction exceeds
/// `1 - CEILING_UTILIZATION`. [`CEILING_UTILIZATION`] stays the single source
/// of truth — there is no second threshold to drift.
pub fn headroom_pct_sufficient(pct: u8) -> bool {
    // Computed in integer percent to mirror `has_headroom`'s strict
    // `utilization < CEILING` exactly at the boundary, avoiding float drift
    // (`1.0 - 0.80` is `0.199999…`, which would wrongly pass `pct == 20`).
    let min_pct = ((1.0 - CEILING_UTILIZATION) * 100.0).round() as u8;
    pct > min_pct
}

/// A second, higher headroom threshold above sufficiency, for two-tier
/// placement.
///
/// Sufficiency (≈`1 - CEILING` ≈ 20% headroom) is the bar to accept a unit at
/// all. Abundance is the bar to be *preferred* when a clearly-emptier home
/// exists. The two tiers split the difference between two failure modes:
/// pure first-sufficient concentrates load onto the first adequate peer
/// (observed in the v0.30 soak — one peer filled while another sat near-idle),
/// while pure most-headroom reintroduces a thundering herd onto whichever peer
/// looks marginally best. So placement stays first-sufficient (frugal, herd-
/// avoiding) under light load, and only chases the emptiest peer when some peer
/// is *abundantly* free — i.e. has enough slack to absorb a spread without
/// itself crowding.
pub const ABUNDANT_HEADROOM_PCT: u8 = 50;

/// True iff an advertised headroom percentage (`0..=100`) is *abundant* — at
/// or above [`ABUNDANT_HEADROOM_PCT`]. See it for the two-tier rationale.
pub fn headroom_pct_abundant(pct: u8) -> bool {
    pct >= ABUNDANT_HEADROOM_PCT
}

// ---------------------------------------------------------------------------
// Parsing — pure functions, deterministic and platform-independent so they
// can be unit-tested against fixed sample text on any machine.
// ---------------------------------------------------------------------------

/// Builds a reading from raw `/proc/meminfo`, `/proc/loadavg`, and CPU-count
/// sources (`/proc/cpuinfo` text, with `/proc/stat` text as a fallback).
///
/// Returns the unavailable reading if any source can't be parsed (missing
/// fields, zero total memory, a malformed load figure, or no CPU count).
fn from_proc_text(meminfo: &str, loadavg: &str, cpuinfo: &str, stat: &str) -> HostResources {
    let n_cpus = parse_cpu_count(cpuinfo).or_else(|| parse_cpu_count_from_stat(stat));
    match (parse_meminfo(meminfo), parse_loadavg(loadavg), n_cpus) {
        (Some((total, available)), Some(load), Some(cpus)) => {
            HostResources::from_parts(total, available, load, cpus)
        }
        _ => HostResources::unavailable(),
    }
}

/// Counts logical CPUs from `/proc/cpuinfo` — one `processor` line per core.
/// Returns `None` if no `processor` lines are present.
fn parse_cpu_count(cpuinfo: &str) -> Option<u32> {
    let n = cpuinfo
        .lines()
        .filter(|l| l.trim_start().starts_with("processor"))
        .count();
    (n > 0).then_some(n as u32)
}

/// Counts logical CPUs from `/proc/stat` — one `cpuN` line per core. The
/// aggregate `cpu ` line (no trailing digit) is excluded. Fallback for when
/// `/proc/cpuinfo` is unreadable. Returns `None` if no `cpuN` lines are found.
fn parse_cpu_count_from_stat(stat: &str) -> Option<u32> {
    let n = stat
        .lines()
        .filter(|l| {
            l.strip_prefix("cpu")
                .and_then(|rest| rest.chars().next())
                .is_some_and(|c| c.is_ascii_digit())
        })
        .count();
    (n > 0).then_some(n as u32)
}

/// Parses MemTotal and MemAvailable (in kB) from `/proc/meminfo` text.
///
/// Returns `None` unless both fields are present and MemTotal is non-zero.
fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total = parse_leading_kb(rest);
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available = parse_leading_kb(rest);
        }
    }
    match (total, available) {
        (Some(t), Some(a)) if t > 0 => Some((t, a)),
        _ => None,
    }
}

/// Parses the first whitespace-delimited integer from a `/proc/meminfo`
/// value (e.g. `"  16384256 kB"` → `16384256`).
fn parse_leading_kb(value: &str) -> Option<u64> {
    value.split_whitespace().next()?.parse().ok()
}

/// Parses the 1-minute load average — the first field of `/proc/loadavg`.
fn parse_loadavg(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

// ---------------------------------------------------------------------------
// S-expression constructors
// ---------------------------------------------------------------------------

/// Builds an S-expression representing the resource status for mesh broadcast.
///
/// Mirrors `sexp_energy_status` in `energy.rs`; consumed later by the
/// resource-aware migration layer.
pub fn sexp_resource_status(node_hex: &str, r: &HostResources) -> String {
    format!(
        "(resource-status :id \"{}\" :valid {} :mem-total-kb {} :mem-avail-kb {} :load {:.2} :n-cpus {} :util {:.3} :headroom {:.3})",
        node_hex,
        r.valid,
        r.mem_total_kb,
        r.mem_available_kb,
        r.load_one,
        r.n_cpus,
        r.utilization,
        r.headroom
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Fixed /proc samples so parsing is deterministic, not machine-dependent.
    const SAMPLE_MEMINFO: &str = "\
MemTotal:       16384256 kB
MemFree:         2097152 kB
MemAvailable:    8192128 kB
Buffers:          524288 kB
Cached:          4194304 kB
";
    const SAMPLE_LOADAVG: &str = "0.75 0.42 0.30 2/512 12345\n";
    // Eight logical CPUs (processor 0..=7).
    const SAMPLE_CPUINFO: &str = "\
processor	: 0
model name	: Test CPU
processor	: 1
processor	: 2
processor	: 3
processor	: 4
processor	: 5
processor	: 6
processor	: 7
";
    // /proc/stat with an aggregate line plus four cpuN lines.
    const SAMPLE_STAT: &str = "\
cpu  100 0 50 900 0 0 0 0 0 0
cpu0 25 0 12 225 0 0 0 0 0 0
cpu1 25 0 12 225 0 0 0 0 0 0
cpu2 25 0 13 225 0 0 0 0 0 0
cpu3 25 0 13 225 0 0 0 0 0 0
intr 12345
";

    #[test]
    fn test_parse_meminfo_sample() {
        let (total, available) = parse_meminfo(SAMPLE_MEMINFO).unwrap();
        assert_eq!(total, 16_384_256);
        assert_eq!(available, 8_192_128);
    }

    #[test]
    fn test_parse_loadavg_sample() {
        let load = parse_loadavg(SAMPLE_LOADAVG).unwrap();
        assert!((load - 0.75).abs() < 1e-9);
    }

    #[test]
    fn test_parse_cpu_count_sample() {
        // Eight `processor` lines → eight CPUs (the `model name` line is
        // ignored).
        assert_eq!(parse_cpu_count(SAMPLE_CPUINFO), Some(8));
        // No processor lines → None, not zero.
        assert_eq!(parse_cpu_count("model name : x\n"), None);
    }

    #[test]
    fn test_parse_cpu_count_from_stat_sample() {
        // Four cpuN lines; the aggregate `cpu ` line is excluded.
        assert_eq!(parse_cpu_count_from_stat(SAMPLE_STAT), Some(4));
        // Only the aggregate line → no per-core lines → None.
        assert_eq!(parse_cpu_count_from_stat("cpu  1 2 3\nintr 0\n"), None);
    }

    #[test]
    fn test_binding_constraint_picks_larger() {
        // Memory-bound: 90% memory used, trivial load → utilization tracks mem.
        let mem_bound = HostResources::from_parts(1000, 100, 0.1, 8);
        assert!((mem_bound.utilization - 0.9).abs() < 1e-6);

        // Load-bound: 10% memory used, load 6.0 on 8 cores (0.75 per core) →
        // utilization tracks load, the larger of the two.
        let load_bound = HostResources::from_parts(1000, 900, 6.0, 8);
        assert!((load_bound.utilization - 0.75).abs() < 1e-6);

        // The binding constraint is always the max of the two fractions.
        assert!(mem_bound.utilization > 0.1); // not the load figure
        assert!(load_bound.utilization > 0.1); // not the mem figure
    }

    #[test]
    fn test_from_proc_text_derives_headroom() {
        let r = from_proc_text(SAMPLE_MEMINFO, SAMPLE_LOADAVG, SAMPLE_CPUINFO, SAMPLE_STAT);
        assert!(r.valid);
        assert_eq!(r.mem_total_kb, 16_384_256);
        assert_eq!(r.mem_available_kb, 8_192_128);
        assert!((r.load_one - 0.75).abs() < 1e-9);
        assert_eq!(r.n_cpus, 8);
        // mem used = 0.5; load per core = 0.75/8 ≈ 0.094 → binding = 0.5.
        assert!((r.utilization - 0.5).abs() < 1e-6);
        assert!((r.headroom - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_cpu_count_falls_back_to_stat() {
        // Empty cpuinfo forces the /proc/stat fallback (4 cpuN lines).
        let r = from_proc_text(SAMPLE_MEMINFO, SAMPLE_LOADAVG, "", SAMPLE_STAT);
        assert!(r.valid);
        assert_eq!(r.n_cpus, 4);
    }

    #[test]
    fn test_utilization_and_headroom_in_range() {
        let r = from_proc_text(SAMPLE_MEMINFO, SAMPLE_LOADAVG, SAMPLE_CPUINFO, SAMPLE_STAT);
        assert!((0.0..=1.0).contains(&r.utilization));
        assert!((0.0..=1.0).contains(&r.headroom));
        // headroom is the complement of the binding constraint.
        assert!((r.utilization + r.headroom - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_available_clamped_to_total() {
        // A pathological reading where MemAvailable exceeds MemTotal; zero load
        // keeps memory the binding constraint so the clamp is observable.
        let r = HostResources::from_parts(1000, 5000, 0.0, 4);
        assert_eq!(r.mem_available_kb, 1000);
        assert!((r.headroom - 1.0).abs() < 1e-9);
        assert!((r.utilization - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_has_headroom_available_under_ceiling_is_true() {
        // 50% binding constraint, well under the 80% ceiling.
        let r = HostResources::from_parts(1000, 500, 0.0, 4);
        assert!(r.has_headroom());
    }

    #[test]
    fn test_has_headroom_available_over_ceiling_is_false() {
        // 90% memory used → over the ceiling → refuse.
        let over = HostResources::from_parts(1000, 100, 0.0, 4);
        assert!(!over.has_headroom());
        // Exactly at the ceiling is also a refusal (strictly-less-than).
        let at = HostResources::from_parts(1000, 200, 0.0, 4);
        assert!((at.utilization - CEILING_UTILIZATION).abs() < 1e-9);
        assert!(!at.has_headroom());
    }

    #[test]
    fn test_has_headroom_unavailable_fails_closed() {
        // A coordinate that cannot measure itself must not replicate.
        assert!(!HostResources::unavailable().has_headroom());
    }

    #[test]
    fn test_advertised_headroom_pct() {
        // 50% headroom → advertises 50.
        let r = HostResources::from_parts(1000, 500, 0.0, 4);
        assert_eq!(r.advertised_headroom_pct(), 50);
        // Unavailable advertises 0 — fail closed on the wire.
        assert_eq!(HostResources::unavailable().advertised_headroom_pct(), 0);
    }

    #[test]
    fn test_headroom_pct_sufficient_tracks_ceiling() {
        // Ceiling is 80% utilization → need headroom > 20% to be sufficient.
        assert!(!headroom_pct_sufficient(20)); // exactly at ceiling → not sufficient
        assert!(headroom_pct_sufficient(21));
        assert!(headroom_pct_sufficient(50));
        assert!(!headroom_pct_sufficient(0)); // unavailable advert → never sufficient
        assert!(!headroom_pct_sufficient(10));
        // Consistency: a reading that has_headroom advertises a sufficient pct.
        let healthy = HostResources::from_parts(1000, 500, 0.0, 4);
        assert!(healthy.has_headroom());
        assert!(headroom_pct_sufficient(healthy.advertised_headroom_pct()));
    }

    #[test]
    fn test_headroom_pct_abundant_is_above_sufficiency() {
        // Abundance is a strictly higher bar than sufficiency.
        assert!(!headroom_pct_abundant(49));
        assert!(headroom_pct_abundant(ABUNDANT_HEADROOM_PCT)); // 50 → abundant
        assert!(headroom_pct_abundant(80));
        // A merely-sufficient peer (just over 20%) is NOT abundant.
        assert!(headroom_pct_sufficient(30));
        assert!(!headroom_pct_abundant(30));
        // Everything abundant is also sufficient.
        for pct in ABUNDANT_HEADROOM_PCT..=100 {
            assert!(headroom_pct_sufficient(pct), "abundant {pct} must be sufficient");
        }
    }

    #[test]
    fn test_malformed_proc_is_unavailable() {
        // Missing MemAvailable.
        let bad = "MemTotal: 100 kB\nMemFree: 50 kB\n";
        let r = from_proc_text(bad, SAMPLE_LOADAVG, SAMPLE_CPUINFO, SAMPLE_STAT);
        assert!(!r.valid);
        assert!(!r.is_available());
        // No CPU count anywhere → also unavailable, even with good mem/load.
        let no_cpu = from_proc_text(SAMPLE_MEMINFO, SAMPLE_LOADAVG, "", "");
        assert!(!no_cpu.valid);
    }

    #[test]
    fn test_unavailable_reading_reports_as_such() {
        let r = HostResources::unavailable();
        assert!(!r.valid);
        assert!(!r.is_available());
        assert_eq!(r.mem_total_kb, 0);
        assert_eq!(r.mem_available_kb, 0);
        assert_eq!(r.n_cpus, 0);
        assert_eq!(r.utilization, 0.0);
        assert_eq!(r.headroom, 0.0);
    }

    #[test]
    fn test_measure_returns_valid_or_cleanly_unavailable() {
        // On Linux this should be a real reading; elsewhere (macOS native,
        // wasm32) it must be a cleanly-marked unavailable one. Either way the
        // invariants must hold.
        let r = HostResources::measure();
        if r.is_available() {
            assert!(r.mem_total_kb > 0);
            assert!(r.mem_available_kb <= r.mem_total_kb);
            assert!(r.n_cpus > 0);
            assert!((0.0..=1.0).contains(&r.utilization));
            assert!((0.0..=1.0).contains(&r.headroom));
        } else {
            assert_eq!(r, HostResources::unavailable());
        }
    }

    #[test]
    fn test_sexp_resource_status() {
        let r = from_proc_text(SAMPLE_MEMINFO, SAMPLE_LOADAVG, SAMPLE_CPUINFO, SAMPLE_STAT);
        let s = sexp_resource_status("aabbccdd", &r);
        assert!(s.contains("resource-status"));
        assert!(s.contains(":id \"aabbccdd\""));
        assert!(s.contains(":valid true"));
        assert!(s.contains(":mem-total-kb 16384256"));
        assert!(s.contains(":n-cpus 8"));
        assert!(s.contains(":headroom 0.500"));
    }
}
