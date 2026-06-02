//! Host resource reader — memory, load, and derived headroom.
//!
//! A unit that may migrate between hosts needs to know how loaded its
//! current host is. This module is the measurement primitive: a zero-
//! dependency reader that samples host memory and CPU load and derives a
//! normalized utilization / headroom pair. No migration logic lives here —
//! this is just the sensor.
//!
//! Platform behavior:
//! - Linux native: read `/proc/meminfo` (MemTotal, MemAvailable) and
//!   `/proc/loadavg` (1-minute figure).
//! - Other native (e.g. macOS): `/proc` isn't present, so we return a
//!   clearly-marked unavailable reading rather than guessing.
//! - wasm32: a unit in the browser can't read host resources at all, so the
//!   measurement is cfg-shimmed out the same way MARK!/SENSE are gated in
//!   `signaling.rs` / `multi_unit.rs` — the API surface is identical but
//!   `measure()` returns the unavailable reading.

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
    /// Fraction of memory in use, in `0.0..=1.0`. `1.0 - headroom`.
    pub utilization: f64,
    /// Fraction of memory still free, in `0.0..=1.0` (MemAvailable / MemTotal).
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
            utilization: 0.0,
            headroom: 0.0,
        }
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

    /// Linux measurement path: read and parse `/proc/meminfo` and
    /// `/proc/loadavg`. Any read or parse failure yields the unavailable
    /// reading so a partial `/proc` never produces bogus figures.
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
        from_proc_text(&meminfo, &loadavg)
    }

    /// Returns true if this reading reflects a real measurement.
    pub fn is_available(&self) -> bool {
        self.valid
    }

    /// Builds a valid reading from already-parsed parts, deriving the
    /// normalized utilization / headroom pair. `mem_available_kb` is clamped
    /// to `mem_total_kb`, and both derived figures are clamped to `0.0..=1.0`.
    fn from_parts(mem_total_kb: u64, mem_available_kb: u64, load_one: f64) -> Self {
        let available = mem_available_kb.min(mem_total_kb);
        let headroom = if mem_total_kb > 0 {
            (available as f64 / mem_total_kb as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        HostResources {
            valid: true,
            mem_total_kb,
            mem_available_kb: available,
            load_one,
            utilization: 1.0 - headroom,
            headroom,
        }
    }
}

// ---------------------------------------------------------------------------
// Parsing — pure functions, deterministic and platform-independent so they
// can be unit-tested against fixed sample text on any machine.
// ---------------------------------------------------------------------------

/// Builds a reading from raw `/proc/meminfo` + `/proc/loadavg` text.
///
/// Returns the unavailable reading if either source can't be parsed (missing
/// fields, zero total memory, or a malformed load figure).
fn from_proc_text(meminfo: &str, loadavg: &str) -> HostResources {
    match (parse_meminfo(meminfo), parse_loadavg(loadavg)) {
        (Some((total, available)), Some(load)) => {
            HostResources::from_parts(total, available, load)
        }
        _ => HostResources::unavailable(),
    }
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
        "(resource-status :id \"{}\" :valid {} :mem-total-kb {} :mem-avail-kb {} :load {:.2} :util {:.3} :headroom {:.3})",
        node_hex,
        r.valid,
        r.mem_total_kb,
        r.mem_available_kb,
        r.load_one,
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

    // A fixed /proc sample so parsing is deterministic, not machine-dependent.
    const SAMPLE_MEMINFO: &str = "\
MemTotal:       16384256 kB
MemFree:         2097152 kB
MemAvailable:    8192128 kB
Buffers:          524288 kB
Cached:          4194304 kB
";
    const SAMPLE_LOADAVG: &str = "0.75 0.42 0.30 2/512 12345\n";

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
    fn test_from_proc_text_derives_headroom() {
        let r = from_proc_text(SAMPLE_MEMINFO, SAMPLE_LOADAVG);
        assert!(r.valid);
        assert_eq!(r.mem_total_kb, 16_384_256);
        assert_eq!(r.mem_available_kb, 8_192_128);
        assert!((r.load_one - 0.75).abs() < 1e-9);
        // 8192128 / 16384256 == 0.5 exactly here.
        assert!((r.headroom - 0.5).abs() < 1e-6);
        assert!((r.utilization - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_utilization_and_headroom_in_range() {
        let r = from_proc_text(SAMPLE_MEMINFO, SAMPLE_LOADAVG);
        assert!((0.0..=1.0).contains(&r.utilization));
        assert!((0.0..=1.0).contains(&r.headroom));
        // They are complementary fractions of the same resource.
        assert!((r.utilization + r.headroom - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_available_clamped_to_total() {
        // A pathological reading where MemAvailable exceeds MemTotal.
        let r = HostResources::from_parts(1000, 5000, 0.1);
        assert_eq!(r.mem_available_kb, 1000);
        assert!((r.headroom - 1.0).abs() < 1e-9);
        assert!((r.utilization - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_malformed_proc_is_unavailable() {
        // Missing MemAvailable.
        let bad = "MemTotal: 100 kB\nMemFree: 50 kB\n";
        let r = from_proc_text(bad, SAMPLE_LOADAVG);
        assert!(!r.valid);
        assert!(!r.is_available());
    }

    #[test]
    fn test_unavailable_reading_reports_as_such() {
        let r = HostResources::unavailable();
        assert!(!r.valid);
        assert!(!r.is_available());
        assert_eq!(r.mem_total_kb, 0);
        assert_eq!(r.mem_available_kb, 0);
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
            assert!((0.0..=1.0).contains(&r.utilization));
            assert!((0.0..=1.0).contains(&r.headroom));
        } else {
            assert_eq!(r, HostResources::unavailable());
        }
    }

    #[test]
    fn test_sexp_resource_status() {
        let r = from_proc_text(SAMPLE_MEMINFO, SAMPLE_LOADAVG);
        let s = sexp_resource_status("aabbccdd", &r);
        assert!(s.contains("resource-status"));
        assert!(s.contains(":id \"aabbccdd\""));
        assert!(s.contains(":valid true"));
        assert!(s.contains(":mem-total-kb 16384256"));
        assert!(s.contains(":headroom 0.500"));
    }
}
