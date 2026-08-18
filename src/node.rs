//! Multi-unit runtime: the in-process --multi-unit smoke demo and the
//! persistent resource-aware node loop (transport, placement, ticking).
use crate::*;
use crate::cli::CliArgs;
use crate::bench::{read_rss_kb, fmt_kb, fmt_wall};
use std::net::SocketAddr;

// ---------------------------------------------------------------------------
// --multi-unit smoke demo
// ---------------------------------------------------------------------------
//
// Spawns N VMs in one process, dispatches a few goals via least-busy worker
// selection, and demonstrates share_word + teach_from. Reports RSS so users
// can compare per-unit memory to the native fork model. Mirrors the WASM
// browser demo's lifecycle but with no upper bound below the configured cap.

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_multi_unit_demo(n: usize) {
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
pub(crate) const TICK_INTERVAL_MS: u64 = 1000;
/// Re-measure host resources, re-advertise headroom, and log the resource line
/// every this many ticks (so quiet ticks stay quiet; ~5s at 1s ticks).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const RESOURCE_MEASURE_EVERY_TICKS: u64 = 5;
/// Transport TCP listener sits at the mesh UDP port + this offset (the repl
/// listener already uses +1000; transport uses +2000 to avoid collision).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const TRANSPORT_PORT_OFFSET: u16 = 2000;

/// Derive a peer's transport TCP address from its gossiped mesh UDP address:
/// same IP, port + [`TRANSPORT_PORT_OFFSET`]. `None` if the port would
/// overflow (transport then can't be attempted to that peer).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn peer_transport_addr(dest: &crate::multi_unit::RemoteProcess) -> Option<String> {
    let port = dest.addr.port().checked_add(TRANSPORT_PORT_OFFSET)?;
    Some(format!("{}:{}", dest.addr.ip(), port))
}

/// Instantiate an inbound transported self as a new live unit on the host:
/// build a fresh VM (prelude loaded), restore the complete self (dictionary
/// incl. evolved antibodies, memory, fitness, code_strings), stamp a host-local
/// id, and adopt it into the host. This is how a peer's transported unit lands.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn land_transported_unit(node: &mut crate::multi_unit::MultiUnitNode, snap: persist::VmSnapshot) {
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
        starved_ticks: 0,
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
pub(crate) fn log_ts() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{:02}:{:02}:{:02}", (secs / 3600) % 24, (secs / 60) % 60, secs % 60)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_multi_unit_node(n: usize, cli: &CliArgs) {
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

        // Obituaries. A death is a real colony event and always logs: the
        // failed life strategy died, the immune memory was bequeathed.
        for d in &report.deaths {
            println!(
                "[{}] DIED gen={} fitness={} — starved (at floor {} ticks); bequeathed {} antibod{} to {} heir{} — now hosting {} units",
                log_ts(),
                d.generation,
                d.fitness,
                crate::multi_unit::STARVED_TICKS_TO_DIE,
                d.antibodies,
                if d.antibodies == 1 { "y" } else { "ies" },
                d.heirs,
                if d.heirs == 1 { "" } else { "s" },
                node.host.len()
            );
        }
        if report.scavenged_words > 0 {
            println!(
                "[{}] SCAVENGED {} antibody word(s) from a peer's death-cry",
                log_ts(),
                report.scavenged_words
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

