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

// --- Fuzz / property tests (untrusted-input surface) ---
#[cfg(test)]
mod fuzz_tests;

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

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}
#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    unsafe { kill(pid, sig) }
}
use std::net::SocketAddr;

use features::{mutation, ws_bridge};
use vm::VM;

// ===========================================================================
// Feature primitives — extend the core VM for mesh, goals, I/O, ops, etc.
// Split by domain into the `words` module tree (see src/words/mod.rs).
// ===========================================================================
mod words;


// bench and node are native-only concerns (fork, RSS, process transport);
// they are cfg'd out on wasm, where nothing references them.
#[cfg(not(target_arch = "wasm32"))]
mod bench;
#[cfg(not(target_arch = "wasm32"))]
mod node;
mod repl;
mod cli;

#[cfg(not(target_arch = "wasm32"))]
use bench::run_two_tier_bench;
use cli::parse_args;
#[cfg(not(target_arch = "wasm32"))]
use node::{run_multi_unit_demo, run_multi_unit_node};

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
