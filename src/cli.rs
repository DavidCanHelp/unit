//! CLI argument parsing, --help text, and the version string.

// ===========================================================================
// CLI argument parsing
// ===========================================================================

// Derived from Cargo.toml so the CLI banner can never drift from the
// released version (the prelude banner derives the same way — see
// load_prelude's {{VERSION}} substitution).
pub(crate) const VERSION: &str = concat!("unit v", env!("CARGO_PKG_VERSION"));

pub(crate) fn print_help() {
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
    println!("  --multi-unit N             Spawn N units in a single process. Alone:");
    println!("                             runs a smoke demo and exits. With --port P:");
    println!("                             launches the persistent resource-aware node");
    println!("                             (mesh peer, ticking, transport) until killed.");
    println!("  --bench-two-tier [CONFIGS] Two-tier scaling bench. CONFIGS is a comma-");
    println!("                             separated list of MxN pairs (e.g.");
    println!("                             10x10,100x100). Default:");
    println!("                             10x10,10x100,100x10,100x100,50x200.");
}

pub(crate) struct CliArgs {
    pub(crate) port: Option<u16>,
    pub(crate) peers: Option<String>,
    pub(crate) ws_port: Option<u16>,
    pub(crate) eval_code: Option<String>,
    pub(crate) file_path: Option<String>,
    pub(crate) no_mesh: bool,
    pub(crate) no_prelude: bool,
    pub(crate) swarm: bool,
    pub(crate) trust: Option<String>,
    pub(crate) quiet: bool,
    /// None = not serving, Some(p) = serve on 127.0.0.1:p.
    pub(crate) serve_port: Option<u16>,
    /// None = no bench, Some(sizes) = run bench at those populations.
    pub(crate) bench_pops: Option<Vec<usize>>,
    /// None = all-to-all (legacy); Some(k) = bounded random gossip with k peers.
    pub(crate) gossip_k: Option<usize>,
    /// None = no multi-unit run; Some(n) = spawn n units in-process and demo.
    pub(crate) multi_unit_n: Option<usize>,
    /// None = no two-tier bench; Some(configs) = run bench with these (M, N) pairs.
    pub(crate) bench_two_tier: Option<Vec<(usize, usize)>>,
}

pub(crate) fn parse_args() -> Option<CliArgs> {
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

