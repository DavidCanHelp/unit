//! Benchmark harness: headless scale-up + two-tier deployment timing.
use crate::vm::VM;
use crate::*;

// ---------------------------------------------------------------------------
// Bench scale-up support
// ---------------------------------------------------------------------------

/// Read this process's RSS in kilobytes via `ps`. macOS + Linux compatible.
/// Returns 0 if the call fails.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn read_rss_kb() -> u64 {
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
pub(crate) fn fmt_kb(kb: u64) -> String {
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
pub(crate) fn fmt_wall(ns: u128) -> String {
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
pub(crate) fn run_scale_bench(pop: usize, cap: usize) {
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
pub(crate) fn project_gossip_to(target_pop: usize, measured_pop: usize) {
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
pub(crate) fn bench_measure_one_fork(vm: &mut VM) -> (u64, u64) {
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
pub(crate) fn project_spawn_to(pop: usize, fork_ns: u64, pkg_ns: u64) {
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
pub(crate) fn run_two_tier_bench(configs: &[(usize, usize)], gossip_k: Option<usize>) {
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
pub(crate) fn fmt_bps(bps: f64) -> String {
    if bps >= 1_000_000.0 {
        format!("{:.2} MB/s", bps / 1_000_000.0)
    } else if bps >= 1_000.0 {
        format!("{:.2} kB/s", bps / 1_000.0)
    } else {
        format!("{:.0} B/s", bps)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn fmt_human_count(v: u64) -> String {
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
