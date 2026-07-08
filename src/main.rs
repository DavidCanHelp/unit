// unit — a software nanobot
// Minimal Forth interpreter that is also a self-replicating networked agent.

// --- Shared types ---
pub mod types;

// --- The Forth VM ---
pub mod vm;

// --- S-expression wire format ---
pub mod sexp;

// --- JSON snapshot persistence ---
pub mod snapshot;

// --- Genetic programming engine ---
pub mod evolve;

// --- Distributed goal computation ---
pub mod distgoal;

// --- Challenge registry (immune system) ---
pub mod challenges;

// --- Problem discovery ---
pub mod discovery;

// --- Metabolic energy system ---
pub mod energy;

// --- Dynamic fitness landscape ---
pub mod landscape;

// --- Timing instrumentation ---
pub mod metrics;

// --- Single-process multi-unit host (in-process port of the WASM browser model) ---
pub mod multi_unit;

// --- Integration tests ---
#[cfg(test)]
mod integration_tests;

// --- Core nanobot ---
#[allow(dead_code)]
pub mod goals;
#[allow(dead_code)]
pub mod mesh;

// --- Sexual reproduction ---
#[allow(dead_code)]
pub mod reproduction;

// --- Niche construction ---
#[allow(dead_code)]
pub mod niche;

/// Inter-unit signaling — direct (peer inbox) and environmental layers.
pub mod signaling;

/// Host resource reader — memory, load, headroom. Drives the spawn
/// ceiling, transport admission (measured at accept time), and placement
/// (headroom-ranked destination choice).
#[allow(dead_code)]
pub mod resources;

// --- Replication & persistence ---
#[allow(dead_code)]
pub mod persist;
#[allow(dead_code)]
pub mod spawn;

/// Unit self-transport — relocate the complete self to another coordinate
/// with confirm-before-release semantics. Placement (sufficient-first,
/// headroom-ranked) and resource-gated admission live here too; the host
/// fires it via the mislocation rule each tick.
#[allow(dead_code)]
pub mod transport;

// --- Feature layers ---
pub mod features {
    #[allow(dead_code)]
    pub mod fitness;
    #[allow(dead_code)]
    pub mod io_words;
    #[allow(dead_code)]
    pub mod monitor;
    #[allow(dead_code)]
    pub mod mutation;
    #[allow(dead_code)]
    pub mod ws_bridge;
}

#[allow(dead_code)]
mod platform;

// --- HTTP bridge (localhost only, opt-in via --features http) ---
#[cfg(all(feature = "http", not(target_arch = "wasm32")))]
pub mod http;

#[cfg(target_arch = "wasm32")]
mod wasm_entry;

use std::io::{self, BufRead, Write};

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    unsafe { kill(pid, sig) }
}
use std::net::SocketAddr;
use std::time::Duration;

use features::{mutation, ws_bridge};
use vm::VM;

// ===========================================================================
// Feature primitives — extend the core VM for mesh, goals, I/O, ops, etc.
// Split by domain into the `words` module tree (see src/words/mod.rs).
// ===========================================================================
mod words;

// ---------------------------------------------------------------------------
// Bench scale-up support
// ---------------------------------------------------------------------------

/// Read this process's RSS in kilobytes via `ps`. macOS + Linux compatible.
/// Returns 0 if the call fails.
#[cfg(not(target_arch = "wasm32"))]
fn read_rss_kb() -> u64 {
    let pid = std::process::id().to_string();
    match std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
    {
        Ok(o) => String::from_utf8_lossy(&o.stdout)
            .trim()
            .parse::<u64>()
            .unwrap_or(0),
        Err(_) => 0,
    }
}

/// Format a kB value as kB/MB/GB.
#[cfg(not(target_arch = "wasm32"))]
fn fmt_kb(kb: u64) -> String {
    if kb >= 1_000_000 {
        format!("{:.2} GB", kb as f64 / 1_000_000.0)
    } else if kb >= 1_000 {
        format!("{:.2} MB", kb as f64 / 1_000.0)
    } else {
        format!("{} kB", kb)
    }
}

/// Format a wall-time duration as ms/s/min/h.
#[cfg(not(target_arch = "wasm32"))]
fn fmt_wall(ns: u128) -> String {
    let s = ns as f64 / 1e9;
    if s >= 3600.0 {
        format!("{:.2} h", s / 3600.0)
    } else if s >= 60.0 {
        format!("{:.2} min", s / 60.0)
    } else if s >= 1.0 {
        format!("{:.2} s", s)
    } else {
        format!("{:.2} ms", s * 1e3)
    }
}

/// Build a synthetic peer-table HashMap of size `n` and time the operations
/// `send_sexp` performs on it (collect-addrs, gossip-sample). Also synthesize
/// an inbox of size `n` and time the drain. Reads RSS before/after.
///
/// If `pop > cap`, only `cap` entries are actually built — caller is
/// responsible for projecting from the measured cost.
#[cfg(not(target_arch = "wasm32"))]
fn run_scale_bench(pop: usize, cap: usize) {
    use std::collections::{HashMap, VecDeque};
    use std::net::SocketAddr;

    let actual = pop.min(cap);
    let projected = pop > cap;
    let label = if projected {
        format!("scale (measured at {}, projected to {})", actual, pop)
    } else {
        format!("scale (measured at {})", actual)
    };

    let rss_before = read_rss_kb();

    // PeerInfo is private to mesh; build a same-shape stub for memory accounting.
    // Real PeerInfo size is reported via mesh::peer_info_size_bytes().
    #[allow(dead_code)]
    struct PeerStub {
        addr: SocketAddr,
        id: [u8; 8],
        load: u32,
        capacity: u32,
        peer_count: u16,
        fitness: i64,
        last_seen: std::time::Instant,
    }

    let populate_start = std::time::Instant::now();
    let mut peer_table: HashMap<[u8; 8], PeerStub> = HashMap::with_capacity(actual);
    for i in 0..actual {
        let mut id = [0u8; 8];
        id.copy_from_slice(&(i as u64).to_le_bytes());
        peer_table.insert(
            id,
            PeerStub {
                addr: SocketAddr::from(([127, 0, 0, 1], 1024 + (i % 60000) as u16)),
                id,
                load: 0,
                capacity: 100,
                peer_count: 0,
                fitness: 0,
                last_seen: std::time::Instant::now(),
            },
        );
    }
    let populate_elapsed = populate_start.elapsed();

    // The legacy collect-addrs step (full O(N) Vec materialization).
    let collect_iters = if actual >= 100_000 { 10 } else { 100 };
    let collect_start = std::time::Instant::now();
    for _ in 0..collect_iters {
        let _v: Vec<SocketAddr> = peer_table.values().map(|p| p.addr).collect();
    }
    let collect_per_call = collect_start.elapsed() / collect_iters as u32;

    // Reservoir sampling (Vitter R) on the HashMap iterator. O(N) iteration
    // with O(k) allocation. This is what send_sexp used to do.
    let reservoir_iters = collect_iters;
    let reservoir_start = std::time::Instant::now();
    let mut rng_state: u64 = 0xdeadbeefcafebabe;
    for _ in 0..reservoir_iters {
        let mut reservoir: Vec<SocketAddr> = Vec::with_capacity(8);
        for (i, p) in peer_table.values().enumerate() {
            if i < 8 {
                reservoir.push(p.addr);
            } else {
                rng_state ^= rng_state << 13;
                rng_state ^= rng_state >> 7;
                rng_state ^= rng_state << 17;
                if rng_state == 0 {
                    rng_state = 0xdeadbeefcafebabe;
                }
                let j = (rng_state as usize) % (i + 1);
                if j < 8 {
                    reservoir[j] = p.addr;
                }
            }
        }
        std::hint::black_box(reservoir);
    }
    let reservoir_per_call = reservoir_start.elapsed() / reservoir_iters as u32;

    // Indexable Vec sampling: rejection-sample k random indices in 0..len and
    // read those slots directly. True O(k) — no iteration over N. This is
    // what send_sexp does now on the gossip path.
    let indexed_addrs: Vec<SocketAddr> = peer_table.values().map(|p| p.addr).collect();
    let indexed_iters = 100_000usize;
    let indexed_start = std::time::Instant::now();
    let mut idx_rng: u64 = 0xfeedface12345678;
    for _ in 0..indexed_iters {
        let n = indexed_addrs.len();
        let mut out: Vec<SocketAddr> = Vec::with_capacity(8);
        let mut chosen: Vec<usize> = Vec::with_capacity(8);
        while out.len() < 8 {
            idx_rng ^= idx_rng << 13;
            idx_rng ^= idx_rng >> 7;
            idx_rng ^= idx_rng << 17;
            if idx_rng == 0 {
                idx_rng = 0xdeadbeefcafebabe;
            }
            let i = (idx_rng as usize) % n;
            if !chosen.contains(&i) {
                chosen.push(i);
                out.push(indexed_addrs[i]);
            }
        }
        std::hint::black_box(out);
    }
    let indexed_per_call = indexed_start.elapsed() / indexed_iters as u32;

    // Inbox drain: with gossip k=8, expected inbox per tick is ~k = 8 (constant
    // independent of N). But model the worst case: a flood of N messages.
    let inbox_n = actual.min(50_000);
    let mut inbox: VecDeque<String> = (0..inbox_n)
        .map(|i| format!("(peer-hello :id \"peer{}\" :gen 0)", i))
        .collect();
    let drain_start = std::time::Instant::now();
    let _drained: Vec<String> = inbox.drain(..).collect();
    let drain_elapsed = drain_start.elapsed();

    let rss_after = read_rss_kb();

    println!("--- {} ---", label);
    println!(
        "  peer-table populate ({} inserts):       {}",
        actual,
        fmt_wall(populate_elapsed.as_nanos())
    );
    println!(
        "  legacy collect_peers (per call):        {}  [O(N) iter + O(N) alloc]",
        fmt_wall(collect_per_call.as_nanos())
    );
    println!(
        "  reservoir k=8 (per call):               {}  [O(N) iter + O(k) alloc]",
        fmt_wall(reservoir_per_call.as_nanos())
    );
    println!(
        "  indexed Vec sample k=8 (per call):      {}  [O(k) — current send_sexp]",
        fmt_wall(indexed_per_call.as_nanos())
    );
    let res_speedup = if reservoir_per_call.as_nanos() > 0 {
        collect_per_call.as_nanos() as f64 / reservoir_per_call.as_nanos() as f64
    } else {
        0.0
    };
    let idx_speedup = if indexed_per_call.as_nanos() > 0 {
        collect_per_call.as_nanos() as f64 / indexed_per_call.as_nanos() as f64
    } else {
        0.0
    };
    println!(
        "  reservoir vs legacy: {:.2}x   indexed vs legacy: {:.2}x   indexed vs reservoir: {:.2}x",
        res_speedup,
        idx_speedup,
        if indexed_per_call.as_nanos() > 0 {
            reservoir_per_call.as_nanos() as f64 / indexed_per_call.as_nanos() as f64
        } else {
            0.0
        }
    );
    println!(
        "  inbox drain ({} msgs):                   {}",
        inbox_n,
        fmt_wall(drain_elapsed.as_nanos())
    );
    println!(
        "  RSS delta (peer table only):            {} → {} (Δ {})",
        fmt_kb(rss_before),
        fmt_kb(rss_after),
        fmt_kb(rss_after.saturating_sub(rss_before))
    );

    // Per-unit projection: at full N, each unit holds an N-entry peer table
    // (assuming epidemic discovery converges to full mesh).
    let per_entry_bytes = mesh::peer_info_size_bytes() + 24; // + ~24B HashMap overhead
    let per_unit_peer_mem = (pop as u128) * (per_entry_bytes as u128);
    let total_peer_mem = (pop as u128) * per_unit_peer_mem;
    println!(
        "  projected per-unit peer table at N={}: {}",
        pop,
        fmt_kb((per_unit_peer_mem / 1024) as u64)
    );
    println!(
        "  projected aggregate peer-table memory:  {}  [O(N²) WALL]",
        fmt_kb((total_peer_mem / 1024) as u64)
    );

    // Projections at full N. Legacy/reservoir scale linearly in N (both
    // iterate the table). Indexed sampling is O(k) — N-independent per call.
    let scale = if actual > 0 {
        pop as f64 / actual as f64
    } else {
        1.0
    };
    let legacy_at_full_ns = (collect_per_call.as_nanos() as f64) * scale;
    let reservoir_at_full_ns = (reservoir_per_call.as_nanos() as f64) * scale;
    // Indexed sampling per-call is independent of N — same measurement holds.
    let indexed_at_full_ns = indexed_per_call.as_nanos() as f64;
    let legacy_per_tick_ns = legacy_at_full_ns * pop as f64;
    let reservoir_per_tick_ns = reservoir_at_full_ns * pop as f64;
    let indexed_per_tick_ns = indexed_at_full_ns * pop as f64;
    println!(
        "  projected legacy collect at full N:     {}  per call",
        fmt_wall(legacy_at_full_ns as u128)
    );
    println!(
        "  projected reservoir at full N:          {}  per call",
        fmt_wall(reservoir_at_full_ns as u128)
    );
    println!(
        "  projected indexed at full N:            {}  per call  [N-independent]",
        fmt_wall(indexed_at_full_ns as u128)
    );
    println!(
        "  projected per-tick (legacy):     {}",
        fmt_wall(legacy_per_tick_ns as u128)
    );
    println!(
        "  projected per-tick (reservoir):  {}",
        fmt_wall(reservoir_per_tick_ns as u128)
    );
    println!(
        "  projected per-tick (indexed):    {}",
        fmt_wall(indexed_per_tick_ns as u128)
    );
}

/// Project chatter dispatch cost from the measured population to the requested
/// one. Both grow as N (gossip is O(N·k) per tick). Per-call latency is constant.
#[cfg(not(target_arch = "wasm32"))]
fn project_gossip_to(target_pop: usize, measured_pop: usize) {
    let mean_proc_ns = metrics::duration_mean_ns("chatter.process");
    let dispatches_per_tick = (target_pop as u128).saturating_mul(8);
    let projected_ns = dispatches_per_tick.saturating_mul(mean_proc_ns as u128);
    println!(
        "projected per-tick chatter dispatch (gossip k=8) at N={} (from N={}):",
        target_pop, measured_pop
    );
    println!(
        "  {} dispatches × {}ns ≈ {}",
        fmt_human_count(dispatches_per_tick.min(u64::MAX as u128) as u64),
        mean_proc_ns,
        fmt_wall(projected_ns)
    );
}

/// Measure one real fork via spawn_local_with_energy and clean up the child
/// and its on-disk artifacts. Returns (fork_ns, pkg_build_ns).
#[cfg(not(target_arch = "wasm32"))]
fn bench_measure_one_fork(vm: &mut VM) -> (u64, u64) {
    let pkg_start = std::time::Instant::now();
    let state = vm.build_state_for_spawn();
    let package = match spawn::build_package(&state) {
        Ok(p) => p,
        Err(_) => return (0, 0),
    };
    let pkg_ns = pkg_start.elapsed().as_nanos().min(u64::MAX as u128) as u64;

    let fork_start = std::time::Instant::now();
    let res = spawn::spawn_local_with_energy(&package, 0, 1, Some(1000));
    let fork_ns = fork_start.elapsed().as_nanos().min(u64::MAX as u128) as u64;

    match res {
        Ok((pid, _port, child_id)) => {
            // Kill child immediately.
            #[cfg(unix)]
            unsafe {
                libc_kill(pid as i32, 9);
            }
            #[cfg(not(unix))]
            let _ = pid;
            // Clean up the spawn artifacts so repeated benches don't bloat ~/.unit.
            let hex: String = child_id.iter().map(|b| format!("{:02x}", b)).collect();
            if let Ok(home) = std::env::var("HOME") {
                let _ = std::fs::remove_dir_all(format!("{}/.unit/spawn/{}", home, hex));
                let _ = std::fs::remove_dir_all(format!("{}/.unit/{}", home, hex));
            }
            println!(
                "single fork measurement: pkg-build {} + fork+exec {} = {} per unit (cleaned up)",
                fmt_wall(pkg_ns as u128),
                fmt_wall(fork_ns as u128),
                fmt_wall((pkg_ns + fork_ns) as u128)
            );
        }
        Err(e) => {
            println!("single fork measurement: SKIPPED ({})", e);
        }
    }
    (fork_ns, pkg_ns)
}

/// Project total wall time to bring N units online from a single measured
/// fork+pkg-build cost. Linear projection — an upper bound that ignores any
/// per-process overhead growth (file-descriptor pressure, fork rate limits).
#[cfg(not(target_arch = "wasm32"))]
fn project_spawn_to(pop: usize, fork_ns: u64, pkg_ns: u64) {
    let per_unit_ns = (fork_ns as u128) + (pkg_ns as u128);
    let serial_ns = (pop as u128).saturating_mul(per_unit_ns);
    // 8-way parallel disk + exec; optimistic floor.
    let parallel_ns = serial_ns / 8;
    println!(
        "spawn projection at N={}: per-unit {} (pkg {} + fork {})",
        pop,
        fmt_wall(per_unit_ns),
        fmt_wall(pkg_ns as u128),
        fmt_wall(fork_ns as u128)
    );
    println!("  serial bring-up:    {}", fmt_wall(serial_ns));
    println!(
        "  8-way parallel:     {}  (optimistic; disk I/O may dominate)",
        fmt_wall(parallel_ns)
    );
}

// ---------------------------------------------------------------------------
// --multi-unit smoke demo
// ---------------------------------------------------------------------------
//
// Spawns N VMs in one process, dispatches a few goals via least-busy worker
// selection, and demonstrates share_word + teach_from. Reports RSS so users
// can compare per-unit memory to the native fork model. Mirrors the WASM
// browser demo's lifecycle but with no upper bound below the configured cap.

#[cfg(not(target_arch = "wasm32"))]
fn run_multi_unit_demo(n: usize) {
    use crate::multi_unit::MultiUnitHost;

    let cap = n.max(1);
    println!("=== unit --multi-unit {} ===", n);
    println!("(single process, no fork, no UDP; cap = {})", cap);

    let rss_before = read_rss_kb();
    println!("RSS before spawn: {}", fmt_kb(rss_before));

    let spawn_start = std::time::Instant::now();
    let mut host = MultiUnitHost::new(cap);
    let spawned = host.spawn_n(n);
    let spawn_elapsed = spawn_start.elapsed();

    let rss_after = read_rss_kb();
    let rss_delta = rss_after.saturating_sub(rss_before);
    let per_unit_kb = if spawned > 0 {
        rss_delta / spawned as u64
    } else {
        0
    };

    println!(
        "spawned {} units in {} ({} per unit, total Δ {})",
        spawned,
        fmt_wall(spawn_elapsed.as_nanos()),
        fmt_kb(per_unit_kb),
        fmt_kb(rss_delta)
    );
    println!("RSS after spawn:  {}", fmt_kb(rss_after));

    if spawned == 0 {
        return;
    }

    // Demo 1: dispatch a few goals. Verify least-busy picker spreads work.
    let goals = ["2 3 + .", "10 4 - .", "6 7 * .", "100 5 / ."];
    println!("\n--- goal dispatch (least-busy worker) ---");
    for code in &goals {
        if let Some(r) = host.execute_goal(code) {
            println!(
                "  unit #{:<4} `{}` → {}",
                r.unit_index,
                code,
                r.output.trim().replace('\n', " ")
            );
        }
    }

    // Demo 2: share a word across every unit (zero-copy &str).
    println!("\n--- share_word ---");
    host.share_word(": DOUBLE 2 * ;");
    let probe_idx = spawned.saturating_sub(1);
    let out = host.units[probe_idx].vm.eval("21 DOUBLE .");
    println!(
        "  defined DOUBLE on every unit; unit #{} evaluates `21 DOUBLE .` → {}",
        probe_idx,
        out.trim()
    );

    // Demo 3: teach_from — define a word on one unit only, then teach it.
    println!("\n--- teach_from ---");
    host.define_on(0, ": TRIPLE 3 * ;");
    let taught = host.teach_from(0, &["TRIPLE"]);
    println!("  unit #0 taught: {:?}", taught);
    if spawned > 1 {
        let out = host.units[1].vm.eval("7 TRIPLE .");
        println!(
            "  unit #1 evaluates `7 TRIPLE .` → {}",
            out.trim()
        );
    }

    println!("\nfinal RSS: {}", fmt_kb(read_rss_kb()));
}

// ---------------------------------------------------------------------------
// --multi-unit + --port: persistent resource-aware node (the live v0.29 run)
// ---------------------------------------------------------------------------
//
// Replaces the old 5-second discovery demo. `unit --multi-unit N --port P
// --peers ...` launches a living node that stays up and ticks the full v0.29
// resource-aware self-replication path until killed. Observability is the
// point: every meaningful event logs one timestamped line so a live tail on a
// real box shows whether transport actually happens.

/// Steady tick interval for the persistent run loop.
#[cfg(not(target_arch = "wasm32"))]
const TICK_INTERVAL_MS: u64 = 1000;
/// Re-measure host resources, re-advertise headroom, and log the resource line
/// every this many ticks (so quiet ticks stay quiet; ~5s at 1s ticks).
#[cfg(not(target_arch = "wasm32"))]
const RESOURCE_MEASURE_EVERY_TICKS: u64 = 5;
/// Transport TCP listener sits at the mesh UDP port + this offset (the repl
/// listener already uses +1000; transport uses +2000 to avoid collision).
#[cfg(not(target_arch = "wasm32"))]
const TRANSPORT_PORT_OFFSET: u16 = 2000;

/// Derive a peer's transport TCP address from its gossiped mesh UDP address:
/// same IP, port + [`TRANSPORT_PORT_OFFSET`]. `None` if the port would
/// overflow (transport then can't be attempted to that peer).
#[cfg(not(target_arch = "wasm32"))]
fn peer_transport_addr(dest: &crate::multi_unit::RemoteProcess) -> Option<String> {
    let port = dest.addr.port().checked_add(TRANSPORT_PORT_OFFSET)?;
    Some(format!("{}:{}", dest.addr.ip(), port))
}

/// Instantiate an inbound transported self as a new live unit on the host:
/// build a fresh VM (prelude loaded), restore the complete self (dictionary
/// incl. evolved antibodies, memory, fitness, code_strings), stamp a host-local
/// id, and adopt it into the host. This is how a peer's transported unit lands.
#[cfg(not(target_arch = "wasm32"))]
fn land_transported_unit(node: &mut crate::multi_unit::MultiUnitNode, snap: persist::VmSnapshot) {
    let mut vm = crate::vm::VM::new();
    vm.silent = true;
    vm.output_buffer = Some(String::new());
    vm.load_prelude();
    vm.output_buffer = None;
    vm.silent = false;
    let idx = node.host.units.len();
    vm.node_id_cache = Some([0xC0, 0xFE, 0, 0, 0, 0, 0, idx as u8]);
    vm.restore_snapshot(snap);
    node.host.units.push(crate::multi_unit::UnitSlot {
        vm,
        busy: false,
        tasks_completed: 0,
        user_words: Vec::new(),
    });
}

/// Shutdown signalling for the run loop. SIGINT/SIGTERM set a flag the loop
/// polls each tick, so the node logs a clean shutdown rather than dying mid-
/// tick. Uses a raw `signal(2)` FFI binding against the already-linked libc —
/// no new dependency.
#[cfg(all(not(target_arch = "wasm32"), unix))]
mod run_signals {
    use std::sync::atomic::{AtomicBool, Ordering};

    static SHUTDOWN: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_signal(_sig: i32) {
        // Atomic store is async-signal-safe.
        SHUTDOWN.store(true, Ordering::SeqCst);
    }

    extern "C" {
        fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
    }

    /// Install handlers for SIGINT (2) and SIGTERM (15).
    pub fn install() {
        unsafe {
            signal(2, on_signal);
            signal(15, on_signal);
        }
    }

    pub fn requested() -> bool {
        SHUTDOWN.load(Ordering::SeqCst)
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(unix)))]
mod run_signals {
    pub fn install() {}
    pub fn requested() -> bool {
        false
    }
}

/// UTC `HH:MM:SS` timestamp for log lines — zero-dependency, comparable across
/// machines under NTP, and readable in a live tail.
#[cfg(not(target_arch = "wasm32"))]
fn log_ts() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{:02}:{:02}:{:02}", (secs / 3600) % 24, (secs / 60) % 60, secs % 60)
}

#[cfg(not(target_arch = "wasm32"))]
fn run_multi_unit_node(n: usize, cli: &CliArgs) {
    use crate::multi_unit::MultiUnitNode;
    use std::time::{Duration, Instant};

    let port = cli.port.unwrap_or(0);
    // Reuse the same --peers parsing as the normal mesh path.
    let peers_str = cli.peers.clone().unwrap_or_default();
    let seed_peers: Vec<SocketAddr> = peers_str
        .split(',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            let s = s.trim();
            s.parse().ok().or_else(|| {
                use std::net::ToSocketAddrs;
                s.to_socket_addrs().ok().and_then(|mut a| a.next())
            })
        })
        .collect();

    // ----- startup phase: banner, host id, spawn, brief discovery window -----
    println!("=== unit --multi-unit {} --port {} ===", n, port);
    println!("(persistent resource-aware node; seeds = {:?})", seed_peers);

    let mut node = match MultiUnitNode::new(n.max(1), Some(port), seed_peers) {
        Ok(node) => node,
        Err(e) => {
            eprintln!("multi-unit: failed to start mesh: {}", e);
            std::process::exit(1);
        }
    };
    // Honor --gossip-k bounded fan-out, as the normal mesh path does.
    if let Some(k) = cli.gossip_k {
        if let Some(ref m) = node.mesh {
            m.set_gossip_fanout(Some(k));
        }
    }
    let spawned = node.spawn_n(n);
    let host_hex = node.host_id_hex().unwrap_or_default();
    let mesh_port = node.mesh_port().unwrap_or(0);
    println!(
        "host id: {}  port: {}  units: {}",
        host_hex, mesh_port, spawned
    );

    // Brief discovery window so peers are visible before the loop's first rule
    // evaluation (kept from the old demo's startup behavior).
    println!("listening for peers (5s)...");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        for ev in node.drain_and_dispatch() {
            println!(
                "[{}] RECV from {} → unit #{}: {}",
                log_ts(),
                ev.from_host_hex,
                ev.unit_index,
                ev.output.trim().replace('\n', " ")
            );
        }
        if let Some(ref m) = node.mesh {
            m.force_heartbeat();
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let remotes = node.remote_processes();
    println!(
        "[{}] discovered {} remote process(es)",
        log_ts(),
        remotes.len()
    );
    for r in &remotes {
        println!(
            "[{}]   peer {} @ {}  units={} headroom={}%",
            log_ts(),
            r.host_id_hex,
            r.addr,
            r.units_hosted,
            r.advertised_headroom
        );
    }

    // ----- persistent run loop -----
    // Bind the transport listener so inbound transports can land. It sits at
    // mesh port + TRANSPORT_PORT_OFFSET on 0.0.0.0 and runs its own accept
    // thread; we drain its channel each tick to instantiate received units.
    let transport_port = mesh_port.wrapping_add(TRANSPORT_PORT_OFFSET);
    let transport_rx = match crate::transport::start_transport_listener(transport_port) {
        Ok(rx) => {
            println!(
                "[{}] transport listener up on 0.0.0.0:{} (mesh port + {})",
                log_ts(),
                transport_port,
                TRANSPORT_PORT_OFFSET
            );
            Some(rx)
        }
        Err(e) => {
            println!(
                "[{}] WARN transport listener failed to bind port {}: {} — inbound transport disabled",
                log_ts(),
                transport_port,
                e
            );
            None
        }
    };

    run_signals::install();
    println!(
        "[{}] node up — ticking every {}ms (Ctrl-C / SIGTERM to stop)",
        log_ts(),
        TICK_INTERVAL_MS
    );

    let mut tick_n: u64 = 0;
    // Last peer-headroom view, so we log the gossiped view only when it changes.
    let mut last_peer_view: Vec<(String, u8)> = Vec::new();
    // Whether we were mislocated last tick, so the "MISLOCATED" line logs on
    // crossing the ceiling rather than every tick while stuck over it.
    let mut prev_mislocated = false;
    loop {
        if run_signals::requested() {
            println!(
                "[{}] shutdown requested — {} units still hosted, exiting cleanly",
                log_ts(),
                node.host.len()
            );
            break;
        }

        // Always heartbeat so peers keep seeing us.
        if let Some(ref m) = node.mesh {
            m.force_heartbeat();
        }

        // Service the transport listener: land any inbound transported units.
        if let Some(ref rx) = transport_rx {
            while let Ok(snap) = rx.try_recv() {
                land_transported_unit(&mut node, snap);
                println!(
                    "[{}] TRANSPORT inbound: landed a unit — now hosting {} units",
                    log_ts(),
                    node.host.len()
                );
            }
        }

        // Measure this host's resources each tick for the local rule (a cheap
        // /proc read). The real transport sends to a peer's TCP listener.
        let res = crate::resources::HostResources::measure();
        let report = node.tick(&res, |dest, payload| match peer_transport_addr(dest) {
            Some(addr) => crate::transport::send_transport(&addr, payload),
            None => Err(crate::transport::TransportError::Connect("port overflow".into())),
        });

        for ev in &report.dispatched {
            println!(
                "[{}] RECV from {} → unit #{}: {}",
                log_ts(),
                ev.from_host_hex,
                ev.unit_index,
                ev.output.trim().replace('\n', " ")
            );
        }

        // Placement logging — the local rule's outcome this tick. A transport
        // attempt is a real event and always logs; bare mislocation (no
        // destination) logs only on crossing the ceiling so a stuck-over-ceiling
        // host doesn't spam every tick (the periodic RES line shows the status).
        let util_str = if res.is_available() {
            format!("util={:.1}% over ceiling", res.utilization * 100.0)
        } else {
            "unmeasurable (fail-closed)".to_string()
        };
        match &report.transport {
            // Bare mislocation logs only on crossing the ceiling (not every
            // tick); when prev_mislocated this falls through to the no-op arm.
            Some(crate::multi_unit::TickTransport::NoDestination) if !prev_mislocated => {
                println!(
                    "[{}] MISLOCATED {} — no peer with sufficient headroom, staying put",
                    log_ts(),
                    util_str
                );
            }
            Some(crate::multi_unit::TickTransport::Attempted {
                target_hex,
                target_headroom,
                outcome,
            }) => {
                println!(
                    "[{}] MISLOCATED {} → transport target {} (headroom {}%)",
                    log_ts(),
                    util_str,
                    target_hex,
                    target_headroom
                );
                match outcome {
                    Ok(_) => println!(
                        "[{}] TRANSPORT accepted, origin released — now hosting {} units",
                        log_ts(),
                        node.host.len()
                    ),
                    Err(e) => println!(
                        "[{}] TRANSPORT refused/failed, staying put ({})",
                        log_ts(),
                        e
                    ),
                }
            }
            // No transport this tick, or mislocation already logged on entry.
            _ => {}
        }
        prev_mislocated = report.mislocated;

        // Periodically re-advertise current headroom and log the resource /
        // evolve / peer lines. Quiet ticks stay silent.
        if tick_n.is_multiple_of(RESOURCE_MEASURE_EVERY_TICKS) {
            // Re-advertise current headroom so peers see real, current capacity.
            if let Some(ref m) = node.mesh {
                m.set_headroom(res.advertised_headroom_pct());
            }
            if res.is_available() {
                let mem_pct = if res.mem_total_kb > 0 {
                    (1.0 - res.mem_available_kb as f64 / res.mem_total_kb as f64) * 100.0
                } else {
                    0.0
                };
                let load_per_cpu = if res.n_cpus > 0 {
                    res.load_one / res.n_cpus as f64
                } else {
                    res.load_one
                };
                println!(
                    "[{}] RES util={:.1}% (mem={:.1}% load/cpu={:.2} cpus={}) headroom={:.1}% {} units={} rss={}",
                    log_ts(),
                    res.utilization * 100.0,
                    mem_pct,
                    load_per_cpu,
                    res.n_cpus,
                    res.headroom * 100.0,
                    if res.has_headroom() {
                        "UNDER-ceiling"
                    } else {
                        "OVER-CEILING"
                    },
                    node.host.len(),
                    fmt_kb(read_rss_kb()),
                );
            } else {
                println!(
                    "[{}] RES unavailable (fail-closed: will not replicate or accept) units={} rss={}",
                    log_ts(),
                    node.host.len(),
                    fmt_kb(read_rss_kb()),
                );
            }

            if report.evolved_units > 0 {
                println!(
                    "[{}] EVOLVE {} unit(s) evolving, best fitness {}",
                    log_ts(),
                    report.evolved_units,
                    report.best_fitness
                );
            }

            // Log the gossiped peer headroom view, but only when it changes.
            let mut view: Vec<(String, u8)> = node
                .remote_processes()
                .iter()
                .map(|r| (r.host_id_hex.clone(), r.advertised_headroom))
                .collect();
            view.sort();
            if view != last_peer_view {
                println!("[{}] PEERS {} visible", log_ts(), view.len());
                for (hex, hr) in &view {
                    println!("[{}]   peer {} headroom={}%", log_ts(), hex, hr);
                }
                last_peer_view = view;
            }
        }

        tick_n = tick_n.wrapping_add(1);
        std::thread::sleep(Duration::from_millis(TICK_INTERVAL_MS));
    }

    println!("[{}] final RSS: {}", log_ts(), fmt_kb(read_rss_kb()));
}

// ---------------------------------------------------------------------------
// --bench-two-tier: characterize the bridged MultiUnitNode deployment
// ---------------------------------------------------------------------------
//
// Spins M MultiUnitNodes in one process, each with N in-process units, all
// peered into one mesh on loopback. Reports peer-table size, gossip
// bandwidth, cross-process send_to_process latency p50/p95, aggregate
// spawn time, and any non-linear scaling. The single-process model is
// chosen for simplicity; per-MultiUnitNode metrics are inferred by
// dividing process-wide counters by M.

#[cfg(not(target_arch = "wasm32"))]
fn run_two_tier_bench(configs: &[(usize, usize)], gossip_k: Option<usize>) {
    use crate::multi_unit::MultiUnitNode;
    use std::net::SocketAddr;
    use std::time::{Duration, Instant};

    let mode_label = match gossip_k {
        Some(k) => format!("gossip k={}", k),
        None => "all-to-all".to_string(),
    };
    println!("=== two-tier scaling bench [{}] ===", mode_label);
    println!(
        "(M MultiUnitNodes × N units, all in one process; M peer tables on loopback)\n"
    );
    println!("sizeof(PeerInfo) = {} bytes", mesh::peer_info_size_bytes());
    println!("self RSS at start: {}\n", fmt_kb(read_rss_kb()));

    for &(m, n) in configs {
        println!("###########################################################");
        println!("# M={} processes × N={} units = {} aggregate", m, n, m * n);
        println!("###########################################################");

        let rss_before = read_rss_kb();
        metrics::reset();

        // ---- (4) aggregate spawn time ----
        let spawn_start = Instant::now();
        let mut nodes: Vec<MultiUnitNode> = Vec::with_capacity(m);
        for i in 0..m {
            // Seed each new node with up to 4 of the already-running peers,
            // so transitive gossip can fill in the rest. This keeps the
            // bootstrap O(M·k) instead of O(M²).
            let seeds: Vec<SocketAddr> = nodes
                .iter()
                .rev()
                .take(4)
                .filter_map(|nd| nd.mesh_port())
                .map(|p| format!("127.0.0.1:{}", p).parse().unwrap())
                .collect();
            let mut node = MultiUnitNode::new(n, Some(0), seeds)
                .expect("MultiUnitNode start failed");
            // Apply bounded-k gossip to BOTH send_sexp and send_heartbeat.
            if let Some(k) = gossip_k {
                if let Some(ref mesh_node) = node.mesh {
                    mesh_node.set_gossip_fanout(Some(k));
                }
            }
            node.spawn_n(n);
            nodes.push(node);
            // Periodic heartbeat during ramp so newcomers learn quickly.
            if i % 10 == 9 {
                for nd in &nodes {
                    if let Some(ref mesh_node) = nd.mesh {
                        mesh_node.force_heartbeat();
                    }
                }
            }
        }
        let spawn_elapsed = spawn_start.elapsed();
        let _rss_after_spawn = read_rss_kb();

        // ---- discovery convergence ----
        let conv_start = Instant::now();
        for _ in 0..8 {
            for nd in &nodes {
                if let Some(ref mesh_node) = nd.mesh {
                    mesh_node.force_heartbeat();
                }
            }
            std::thread::sleep(Duration::from_millis(60));
        }
        let conv_elapsed = conv_start.elapsed();

        // Drain stale envelopes so they don't pollute later measurements.
        for nd in nodes.iter_mut() {
            let _ = nd.drain_and_dispatch();
        }

        // ---- (1) peer-table size + memory ----
        let peer_counts: Vec<usize> = nodes.iter().map(|nd| nd.remote_processes().len()).collect();
        let peer_min = *peer_counts.iter().min().unwrap_or(&0);
        let peer_max = *peer_counts.iter().max().unwrap_or(&0);
        let peer_sum: usize = peer_counts.iter().sum();
        let peer_mean = if !peer_counts.is_empty() {
            peer_sum as f64 / peer_counts.len() as f64
        } else {
            0.0
        };
        let per_entry_bytes = mesh::peer_info_size_bytes() + 24 + 16; // Vec slot + HashMap overhead
        let per_proc_table_bytes = peer_max as u128 * per_entry_bytes as u128;
        let total_peer_mem_bytes = (peer_sum as u128) * per_entry_bytes as u128;

        // ---- (2) gossip bandwidth: sample over a steady-state window ----
        // Rely on the network thread's natural HEARTBEAT_INTERVAL (2s); also
        // force-tick to pump traffic for a denser sample. Reset metrics first.
        let bw_window = Duration::from_secs(3);
        let bytes_sent_before = metrics::value_total("mesh.bytes_sent");
        let msgs_sent_before = metrics::value_count("mesh.bytes_sent");
        let bytes_recv_before = metrics::value_total("mesh.bytes_recv");
        let msgs_recv_before = metrics::value_count("mesh.bytes_recv");
        let bw_start = Instant::now();
        while bw_start.elapsed() < bw_window {
            for nd in &nodes {
                if let Some(ref mesh_node) = nd.mesh {
                    mesh_node.force_heartbeat();
                }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let bw_elapsed = bw_start.elapsed().as_secs_f64();
        let bytes_sent = metrics::value_total("mesh.bytes_sent") - bytes_sent_before;
        let msgs_sent = metrics::value_count("mesh.bytes_sent") - msgs_sent_before;
        let bytes_recv = metrics::value_total("mesh.bytes_recv") - bytes_recv_before;
        let msgs_recv = metrics::value_count("mesh.bytes_recv") - msgs_recv_before;
        // Process-wide totals; per-process = total / M.
        let per_proc_send_bps = (bytes_sent as f64) / bw_elapsed / m as f64;
        let per_proc_send_mps = (msgs_sent as f64) / bw_elapsed / m as f64;
        let per_proc_recv_bps = (bytes_recv as f64) / bw_elapsed / m as f64;
        let per_proc_recv_mps = (msgs_recv as f64) / bw_elapsed / m as f64;

        // ---- (3) cross-process send_to_process latency ----
        // From node 0, send probes to a sweep of targets one-at-a-time and
        // poll the target's inbox until the probe arrives, recording elapsed
        // wall time. This measures actual end-to-end delivery (send →
        // network thread recv → sexp_inbox enqueue → drain), not the time
        // until our outer loop bothers to look.
        let epoch = Instant::now();
        let probe_targets = (1..m.min(11)).collect::<Vec<_>>(); // up to 10 targets
        let probes_per_target = 50usize;
        let poll_timeout = Duration::from_millis(50);
        for &target_i in &probe_targets {
            if target_i >= nodes.len() {
                continue;
            }
            let target_id = nodes[target_i].host_id().expect("target host id");
            for seq in 0..probes_per_target {
                let send_ns = epoch.elapsed().as_nanos();
                let payload = format!("__BENCH_PROBE_{}_{}", seq, send_ns);
                nodes[0].send_to_process(&target_id, &payload);
                // Poll-drain target until we see THIS probe (or timeout).
                let probe_deadline = Instant::now() + poll_timeout;
                let target_mesh = nodes[target_i].mesh.as_ref().expect("target mesh");
                let mut found = false;
                while !found && Instant::now() < probe_deadline {
                    let raw_msgs = target_mesh.recv_sexp_messages();
                    for raw in raw_msgs {
                        let parsed = match crate::sexp::try_parse_mesh_msg(&raw) {
                            Some(s) => s,
                            None => continue,
                        };
                        let p = match parsed.get_key(":payload").and_then(|s| s.as_str()) {
                            Some(p) => p.to_string(),
                            None => continue,
                        };
                        if let Some(rest) = p.strip_prefix("__BENCH_PROBE_") {
                            if let Some((_seq_str, send_ns_str)) = rest.split_once('_') {
                                if let Ok(this_send_ns) = send_ns_str.parse::<u128>() {
                                    let recv_ns = epoch.elapsed().as_nanos();
                                    let lat = (recv_ns - this_send_ns) as u64;
                                    metrics::record("send_to_process.latency", lat);
                                    if this_send_ns == send_ns {
                                        found = true;
                                    }
                                }
                            }
                        }
                    }
                    if !found {
                        std::thread::sleep(Duration::from_micros(200));
                    }
                }
            }
        }
        let lat_count = metrics::value_count("dummy"); // unused, just to keep API in scope
        let _ = lat_count;
        let lat_p50 = metrics::histogram_percentile_ns("send_to_process.latency", 0.50);
        let lat_p95 = metrics::histogram_percentile_ns("send_to_process.latency", 0.95);
        let lat_max = metrics::histogram_max_ns("send_to_process.latency");
        let lat_n = metrics::histogram_count("send_to_process.latency");

        let rss_after_bw = read_rss_kb();
        let rss_delta = rss_after_bw.saturating_sub(rss_before);
        let per_unit_kb = if m * n > 0 {
            rss_delta / (m * n) as u64
        } else {
            0
        };

        // ---- print report ----
        println!("spawn:");
        println!(
            "  aggregate spawn:          {} ({} per node, {} per unit)",
            fmt_wall(spawn_elapsed.as_nanos()),
            fmt_wall(spawn_elapsed.as_nanos() / m.max(1) as u128),
            fmt_wall(spawn_elapsed.as_nanos() / (m * n).max(1) as u128)
        );
        println!(
            "  discovery convergence:    {} ({} forced-heartbeat rounds)",
            fmt_wall(conv_elapsed.as_nanos()),
            8
        );
        println!("memory:");
        println!(
            "  RSS delta:                {}  ({} per unit)",
            fmt_kb(rss_delta),
            fmt_kb(per_unit_kb)
        );
        println!(
            "  per-process peer table:   {} (max peers = {})",
            fmt_kb((per_proc_table_bytes / 1024) as u64),
            peer_max
        );
        println!(
            "  aggregate peer-table:     {}",
            fmt_kb((total_peer_mem_bytes / 1024) as u64)
        );
        println!("peer table:");
        println!(
            "  observed peers / process: min {} mean {:.1} max {} (target M-1 = {})",
            peer_min,
            peer_mean,
            peer_max,
            m.saturating_sub(1)
        );
        println!("gossip bandwidth (steady-state, per process):");
        println!(
            "  send: {:.0} msg/s, {} (over {:.2}s window)",
            per_proc_send_mps,
            fmt_bps(per_proc_send_bps),
            bw_elapsed
        );
        println!(
            "  recv: {:.0} msg/s, {}",
            per_proc_recv_mps,
            fmt_bps(per_proc_recv_bps)
        );
        println!("cross-process latency ({} samples):", lat_n);
        println!(
            "  p50: {}   p95: {}   max: {}",
            fmt_wall(lat_p50 as u128),
            fmt_wall(lat_p95 as u128),
            fmt_wall(lat_max as u128)
        );

        // ---- (5) non-linear flags ----
        let expected_peers = m.saturating_sub(1);
        if peer_max < expected_peers {
            println!(
                "  ⚠ discovery did not fully converge: max peers {} < expected {} (need more rounds or larger seed fanout)",
                peer_max, expected_peers
            );
        }
        if lat_n == 0 {
            println!("  ⚠ no latency samples — check probe parsing or send_to_process");
        }

        println!();
        // Drop nodes between configs to free sockets/threads.
        drop(nodes);
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Format bytes-per-second as B/s, kB/s, MB/s.
#[cfg(not(target_arch = "wasm32"))]
fn fmt_bps(bps: f64) -> String {
    if bps >= 1_000_000.0 {
        format!("{:.2} MB/s", bps / 1_000_000.0)
    } else if bps >= 1_000.0 {
        format!("{:.2} kB/s", bps / 1_000.0)
    } else {
        format!("{:.0} B/s", bps)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn fmt_human_count(v: u64) -> String {
    if v >= 1_000_000_000 {
        format!("{:.2}G", v as f64 / 1e9)
    } else if v >= 1_000_000 {
        format!("{:.2}M", v as f64 / 1e6)
    } else if v >= 1_000 {
        format!("{:.2}k", v as f64 / 1e3)
    } else {
        format!("{}", v)
    }
}

// ===========================================================================
// REPL
// ===========================================================================

/// Idle-tick cadence for the REPL loop: how long the main loop waits for a
/// line of input before running the periodic duties anyway. Bounds the
/// latency of recruit evaluation, supervision, replication acceptance, and
/// every other duty in `repl_tick` on a node nobody is typing at.
const REPL_TICK: Duration = Duration::from_millis(250);

impl VM {
    /// The REPL's periodic duties — everything a node must do whether or not
    /// a human is typing. Runs after every input line AND on a timer while
    /// the REPL is idle. These were originally gated on stdin input, which
    /// meant an idle interactive node never evaluated recruited work, never
    /// ran its supervision passes, never accepted inbound replications, and
    /// had frozen metabolism — heartbeats kept flowing (mesh thread) while
    /// all VM-side duties waited for a keypress.
    fn repl_tick(&mut self) {
        self.check_auto_claim();
        self.check_auto_replicate();
        self.check_auto_evolve();
        self.check_incoming_replications();
        self.energy.tick();
        self.landscape.tick();
        self.tick_monitor();
        self.tick_swarm();
        self.check_auto_snapshot();
        self.tick_dist_goals();
        self.poll_ws_events();
        self.update_ws_mesh_json();
    }

    fn repl(&mut self) {
        let mut stdout = io::stdout();

        let _ = write!(stdout, "> ");
        let _ = stdout.flush();

        // The blocking stdin read lives on its own thread so the main loop
        // can tick on a timer while idle. Only line Strings cross the
        // channel — the VM (and the recruit ledger with it) stays owned by
        // this thread, exactly as before. EOF or a read error drops the
        // sender, which ends the loop below: piped input (`echo ... | unit`)
        // keeps its run-then-exit semantics.
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let stdin = io::stdin();
            for line in stdin.lock().lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break; // REPL ended (BYE) — stop reading
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        loop {
            match rx.recv_timeout(REPL_TICK) {
                Ok(line) => {
                    self.interpret_line(&line);
                    if !self.running {
                        break;
                    }
                    if !self.compiling {
                        self.repl_tick();
                    }
                    if self.compiling {
                        let _ = write!(stdout, "  ");
                    } else {
                        let _ = write!(stdout, " ok\n> ");
                    }
                    let _ = stdout.flush();
                    self.needs_prompt_redraw = false; // prompt just printed
                }
                // Idle: no input this interval — run the duties anyway.
                // (Skipped mid-definition, same as the per-line path: a
                // half-compiled word must not be observed by spawn/snapshot.)
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if !self.compiling {
                        self.repl_tick();
                        // Tick-driven output (e.g. "parallel #N complete:")
                        // leaves the cursor mid-line with no prompt, which
                        // reads as a hang. Redraw it. (Characters typed but
                        // not yet submitted stay in the terminal's line
                        // buffer and still apply — we can't re-echo them
                        // without raw mode, so they appear above the fresh
                        // prompt but are not lost.)
                        if self.needs_prompt_redraw {
                            let _ = write!(stdout, "> ");
                            let _ = stdout.flush();
                            self.needs_prompt_redraw = false;
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        println!();
    }
}

// ===========================================================================
// CLI argument parsing
// ===========================================================================

const VERSION: &str = "unit v0.33.0";

fn print_help() {
    println!("{}", VERSION);
    println!("A self-replicating software nanobot.\n");
    println!("USAGE:");
    println!("  unit                        Start interactive REPL");
    println!("  unit --eval \"2 3 + .\"       Evaluate and print result");
    println!("  unit --port 4201 --swarm    Start swarm node on port 4201");
    println!("  unit --file script.fs       Load a Forth script\n");
    println!("OPTIONS:");
    println!("  -h, --help                  Show this help");
    println!("  -v, --version               Print version and exit");
    println!("  -q, --quiet                 Suppress boot banner");
    println!("  --port PORT                 Set mesh UDP port (or UNIT_PORT env)");
    println!("  --peers HOST:PORT,...       Set seed peers (or UNIT_PEERS env)");
    println!("  --ws-port PORT             Set WebSocket bridge port");
    println!("  --eval \"FORTH CODE\"         Evaluate code, print output, exit");
    println!("  --file PATH                Load a .fs file, then start REPL");
    println!("  --no-mesh                  Start without mesh networking");
    println!("  --no-prelude               Start without loading prelude.fs");
    println!("  --swarm                    Start with SWARM-ON");
    println!("  --trust LEVEL              Set trust: all, mesh, family, none");
    println!("  --serve [PORT]             Start HTTP bridge on 127.0.0.1 (default :9898)");
    println!("                             (requires: cargo build --features http)");
    println!("  --bench [SIZES]            Run headless timing bench at the given");
    println!("                             populations (comma-separated, default");
    println!("                             10,100,1000,10000) and exit. Reports both");
    println!("                             all-to-all and bounded-k gossip modes.");
    println!("  --gossip-k K               Use bounded random gossip with fan-out K");
    println!("                             on the live mesh (default: all-to-all).");
    println!("  --multi-unit N             Spawn N units in a single process (no fork,");
    println!("                             no UDP). Combine with --port and --peers to");
    println!("                             also participate in the mesh as one peer");
    println!("                             process advertising N units. Runs a smoke");
    println!("                             demo and exits.");
    println!("  --bench-two-tier [CONFIGS] Two-tier scaling bench. CONFIGS is a comma-");
    println!("                             separated list of MxN pairs (e.g.");
    println!("                             10x10,100x100). Default:");
    println!("                             10x10,10x100,100x10,100x100,50x200.");
}

struct CliArgs {
    port: Option<u16>,
    peers: Option<String>,
    ws_port: Option<u16>,
    eval_code: Option<String>,
    file_path: Option<String>,
    no_mesh: bool,
    no_prelude: bool,
    swarm: bool,
    trust: Option<String>,
    quiet: bool,
    /// None = not serving, Some(p) = serve on 127.0.0.1:p.
    serve_port: Option<u16>,
    /// None = no bench, Some(sizes) = run bench at those populations.
    bench_pops: Option<Vec<usize>>,
    /// None = all-to-all (legacy); Some(k) = bounded random gossip with k peers.
    gossip_k: Option<usize>,
    /// None = no multi-unit run; Some(n) = spawn n units in-process and demo.
    multi_unit_n: Option<usize>,
    /// None = no two-tier bench; Some(configs) = run bench with these (M, N) pairs.
    bench_two_tier: Option<Vec<(usize, usize)>>,
}

fn parse_args() -> Option<CliArgs> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cli = CliArgs {
        port: None,
        peers: None,
        ws_port: None,
        eval_code: None,
        file_path: None,
        no_mesh: false,
        no_prelude: false,
        swarm: false,
        trust: None,
        quiet: false,
        serve_port: None,
        bench_pops: None,
        gossip_k: None,
        multi_unit_n: None,
        bench_two_tier: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "-v" | "--version" => {
                println!("{}", VERSION);
                std::process::exit(0);
            }
            "-q" | "--quiet" => cli.quiet = true,
            "--port" => {
                i += 1;
                cli.port = args.get(i).and_then(|s| s.parse().ok());
            }
            "--peers" => {
                i += 1;
                cli.peers = args.get(i).cloned();
            }
            "--ws-port" => {
                i += 1;
                cli.ws_port = args.get(i).and_then(|s| s.parse().ok());
            }
            "--eval" => {
                i += 1;
                cli.eval_code = args.get(i).cloned();
            }
            "--file" => {
                i += 1;
                cli.file_path = args.get(i).cloned();
            }
            "--no-mesh" => cli.no_mesh = true,
            "--no-prelude" => cli.no_prelude = true,
            "--swarm" => cli.swarm = true,
            "--trust" => {
                i += 1;
                cli.trust = args.get(i).cloned();
            }
            "--serve" => {
                // Optional PORT: consume next arg only if it parses as u16.
                let port = match args.get(i + 1).and_then(|s| s.parse::<u16>().ok()) {
                    Some(p) => {
                        i += 1;
                        p
                    }
                    None => 9898,
                };
                cli.serve_port = Some(port);
            }
            "--bench" => {
                // Optional SIZES (comma-separated). If next arg parses as a
                // comma-separated list of usize, consume it; otherwise default.
                let pops: Vec<usize> = match args.get(i + 1) {
                    Some(s) if s.split(',').all(|p| p.parse::<usize>().is_ok()) => {
                        i += 1;
                        s.split(',').filter_map(|p| p.parse().ok()).collect()
                    }
                    _ => vec![10, 100, 1000, 10000],
                };
                cli.bench_pops = Some(pops);
            }
            "--gossip-k" => {
                i += 1;
                cli.gossip_k = args.get(i).and_then(|s| s.parse().ok());
            }
            "--multi-unit" => {
                i += 1;
                cli.multi_unit_n = args.get(i).and_then(|s| s.parse().ok());
                if cli.multi_unit_n.is_none() {
                    eprintln!("--multi-unit requires a positive integer N");
                    std::process::exit(1);
                }
            }
            "--bench-two-tier" => {
                // Optional CONFIGS — comma-separated MxN pairs. If next arg
                // parses as such, consume it; otherwise default.
                let parsed: Option<Vec<(usize, usize)>> = args.get(i + 1).and_then(|s| {
                    let pairs: Option<Vec<(usize, usize)>> = s
                        .split(',')
                        .map(|tok| {
                            tok.split_once('x').and_then(|(a, b)| {
                                Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                            })
                        })
                        .collect();
                    pairs
                });
                cli.bench_two_tier = match parsed {
                    Some(p) if !p.is_empty() => {
                        i += 1;
                        Some(p)
                    }
                    _ => Some(vec![
                        (10, 10),
                        (10, 100),
                        (100, 10),
                        (100, 100),
                        (50, 200),
                        (200, 50),
                    ]),
                };
            }
            other => {
                eprintln!("unknown option: {}", other);
                std::process::exit(1);
            }
        }
        i += 1;
    }
    Some(cli)
}

// ===========================================================================
// Entry point
// ===========================================================================

fn main() {
    let cli = parse_args().unwrap();

    // --bench: headless timing run, no mesh, no REPL. Native only — the
    // metrics module is a no-op on wasm32 (no Instant), so bench would
    // produce all zeros.
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(ref pops) = cli.bench_pops {
            let mut vm = VM::new();
            vm.silent = true;
            vm.load_prelude();
            vm.silent = false;
            vm.run_bench(pops);
            return;
        }
        if let Some(n) = cli.multi_unit_n {
            // If --port is also set, run the bridged demo (in-process units +
            // mesh peer). Otherwise run the strictly intra-process demo.
            if cli.port.is_some() {
                run_multi_unit_node(n, &cli);
            } else {
                run_multi_unit_demo(n);
            }
            return;
        }
        if let Some(ref cfgs) = cli.bench_two_tier {
            run_two_tier_bench(cfgs, cli.gossip_k);
            return;
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = cli.bench_pops;
        let _ = cli.multi_unit_n;
        let _ = cli.bench_two_tier;
    }

    let mut vm = VM::new();
    vm.silent = cli.quiet;

    // Port: CLI flag > env var > default 0.
    let port: u16 = cli
        .port
        .or_else(|| std::env::var("UNIT_PORT").ok().and_then(|s| s.parse().ok()))
        .unwrap_or(0);

    let peers_str = cli
        .peers
        .or_else(|| std::env::var("UNIT_PEERS").ok())
        .or_else(|| std::env::var("UNIT_SEEDS").ok())
        .unwrap_or_default();
    let seed_peers: Vec<SocketAddr> = peers_str
        .split(',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            let s = s.trim();
            // Try direct parse first, then DNS resolution.
            s.parse().ok().or_else(|| {
                use std::net::ToSocketAddrs;
                match s.to_socket_addrs() {
                    Ok(mut addrs) => addrs.next(),
                    Err(e) => {
                        eprintln!("resolve {}: {}", s, e);
                        None
                    }
                }
            })
        })
        .collect();

    // Start mesh unless --no-mesh.
    if !cli.no_mesh {
        let env_node_id: Option<[u8; 8]> = std::env::var("UNIT_NODE_ID").ok().and_then(|hex| {
            if hex.len() != 16 {
                return None;
            }
            let mut id = [0u8; 8];
            for i in 0..8 {
                id[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
            }
            Some(id)
        });

        let persisted_id = env_node_id.or_else(persist::load_node_id);
        let resumed = persisted_id.is_some() && env_node_id.is_none();

        match mesh::MeshNode::start_with_id(persisted_id, port, seed_peers) {
            Ok(node) => {
                let id = node.id_bytes();
                let seed = u64::from_be_bytes(id);
                vm.rng = mutation::SimpleRng::new(seed);
                vm.node_id_cache = Some(id);
                vm.challenge_registry = challenges::ChallengeRegistry::new(&id);
                // Register fib10 as a built-in challenge.
                let fib = challenges::fib10_as_challenge();
                vm.challenge_registry.register_builtin(
                    &fib.name,
                    &fib.target_output,
                    fib.seed_programs,
                );
                let _ = persist::save_node_id(&id);
                if resumed && !cli.quiet {
                    eprintln!("resumed identity {}", mesh::id_to_hex(&id));
                }
                vm.mesh = Some(node);

                // Set external address for NAT traversal.
                if let Ok(ext) = std::env::var("UNIT_EXTERNAL_ADDR") {
                    if let Ok(addr) = ext.parse::<SocketAddr>() {
                        if let Some(ref mut m) = vm.mesh {
                            m.external_addr = Some(addr);
                        }
                        if !cli.quiet {
                            eprintln!("external address: {}", addr);
                        }
                    }
                }

                // Set mesh authentication key.
                if let Ok(key) = std::env::var("UNIT_MESH_KEY") {
                    if !key.is_empty() {
                        if let Some(ref mut m) = vm.mesh {
                            m.mesh_key = Some(key);
                        }
                        if !cli.quiet {
                            eprintln!("mesh-key: enabled");
                        }
                    }
                }

                // Apply --gossip-k bounded fan-out (applies to both send_sexp
                // and send_heartbeat).
                if let Some(k) = cli.gossip_k {
                    if let Some(ref m) = vm.mesh {
                        m.set_gossip_fanout(Some(k));
                    }
                    if !cli.quiet {
                        eprintln!("gossip: bounded fan-out k={}", k);
                    }
                }

                let ws_port: u16 = cli
                    .ws_port
                    .or_else(|| {
                        std::env::var("UNIT_WS_PORT")
                            .ok()
                            .and_then(|s| s.parse().ok())
                    })
                    .unwrap_or_else(|| if port > 0 { port + 2000 } else { 0 });
                if ws_port > 0 {
                    match ws_bridge::start_ws_bridge(ws_port, vm.ws_mesh_json.clone()) {
                        Ok((ws_st, ws_rx)) => {
                            vm.ws_state = Some(ws_st);
                            vm.ws_events = Some(ws_rx);
                            if !cli.quiet {
                                eprintln!("ws-bridge: listening on port {}", ws_port);
                            }
                        }
                        Err(e) => {
                            if !cli.quiet {
                                eprintln!("ws-bridge: {}", e);
                            }
                        }
                    }
                }

                if let Ok(gen_str) = std::env::var("UNIT_GENERATION") {
                    if let Ok(gen) = gen_str.parse::<u32>() {
                        vm.spawn_state.generation = gen;
                    }
                }
                if let Ok(parent_hex) = std::env::var("UNIT_PARENT_ID") {
                    if parent_hex.len() == 16 {
                        let mut pid = [0u8; 8];
                        let mut ok = true;
                        for i in 0..8 {
                            match u8::from_str_radix(&parent_hex[i * 2..i * 2 + 2], 16) {
                                Ok(b) => pid[i] = b,
                                Err(_) => {
                                    ok = false;
                                    break;
                                }
                            }
                        }
                        if ok {
                            vm.spawn_state.parent_id = Some(pid);
                        }
                    }
                }
                if let Ok(energy_str) = std::env::var("UNIT_CHILD_ENERGY") {
                    if let Ok(energy) = energy_str.parse::<i64>() {
                        vm.energy.energy = energy;
                    }
                }
            }
            Err(e) => {
                if !cli.quiet {
                    eprintln!("mesh: failed to start: {}", e);
                }
            }
        }
    }

    if let Some(ref m) = vm.mesh {
        m.set_load(vm.dictionary.len() as u32);
    }

    // Restore state or load prelude.
    let mut restored = false;
    if let Some(id) = vm.node_id_cache {
        if let Some(data) = persist::load_state(&id) {
            if let Some(snap) = persist::deserialize_snapshot(&data) {
                vm.dictionary = snap.dictionary;
                vm.memory = snap.memory;
                vm.here = snap.here;
                vm.fitness = snap.fitness;
                vm.code_strings = snap.code_strings;
                if let Some(ref m) = vm.mesh {
                    let mut st = m.state_lock();
                    st.goals = snap.goals;
                }
                restored = true;
                if !cli.quiet {
                    eprintln!("restored from {}/state.bin", persist::state_dir(&id));
                }
            }
        }
    }

    if !restored && !cli.no_prelude {
        // Suppress prelude output for --eval and --quiet modes.
        let suppress = cli.eval_code.is_some() || cli.quiet;
        if suppress {
            vm.output_buffer = Some(String::new());
        }
        vm.load_prelude();
        if suppress {
            vm.output_buffer = None;
        }
    }
    // Record kernel+prelude dictionary size so snapshots only save user words.
    vm.kernel_word_count = vm.dictionary.len();
    vm.silent = false;

    // Try JSON resurrection (only if not already restored from binary state).
    if !restored && vm.try_resurrect() {
        if !cli.quiet {
            eprintln!("resurrected from snapshot");
        }
        // Broadcast resurrection to mesh.
        if let Some(id) = vm.node_id_cache {
            if let Some(json) = snapshot::load_json_snapshot(&id) {
                if let Some(snap) = snapshot::from_json(&json) {
                    if let Some(ref m) = vm.mesh {
                        let sexp =
                            sexp::msg_resurrect(&id, snap.fitness, snap.generation, snap.timestamp);
                        m.send_sexp(&sexp.to_string());
                    }
                }
            }
        }
    }

    // Apply --trust.
    if let Some(ref level) = cli.trust {
        match level.as_str() {
            "all" => vm.interpret_line("TRUST-ALL"),
            "mesh" => vm.interpret_line("TRUST-MESH"),
            "family" => vm.interpret_line("TRUST-FAMILY"),
            "none" => vm.interpret_line("TRUST-NONE"),
            _ => eprintln!("unknown trust level: {}", level),
        }
    }

    // Apply --swarm.
    if cli.swarm {
        vm.interpret_line("SWARM-ON");
    }

    // --file: load a Forth script.
    if let Some(ref path) = cli.file_path {
        match std::fs::read_to_string(path) {
            Ok(source) => {
                for line in source.lines() {
                    vm.interpret_line(line);
                }
            }
            Err(e) => {
                eprintln!("error: cannot read {}: {}", path, e);
                std::process::exit(1);
            }
        }
    }

    // --eval: evaluate and exit.
    if let Some(ref code) = cli.eval_code {
        let output = vm.eval(code);
        if !output.is_empty() {
            print!("{}", output);
        }
        return;
    }

    // --serve: run as HTTP bridge instead of starting the REPL.
    if let Some(port) = cli.serve_port {
        #[cfg(feature = "http")]
        {
            http::serve(vm, port);
        }
        #[cfg(not(feature = "http"))]
        {
            let _ = port;
            eprintln!("--serve requires building with --features http");
            std::process::exit(1);
        }
    }

    vm.repl();
}
