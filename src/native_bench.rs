//! An ordinary unit recruits ordinary units; only the experiment driver is new.
use crate::{
    mesh::MeshNode,
    native::{execute, Task},
    sexp::Sexp,
    vm::VM,
};
use std::io::Write;
use std::net::{SocketAddr, ToSocketAddrs};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Children(Vec<Child>, std::path::PathBuf);
impl Drop for Children {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

fn vm() -> VM {
    let mut vm = VM::new();
    vm.silent = true;
    vm.load_prelude();
    vm.output_buffer = Some(String::new());
    vm
}

fn values(s: &Sexp) -> Result<Vec<Vec<i64>>, String> {
    if s.get_key(":ok").and_then(Sexp::as_number) != Some(1) {
        return Err(format!("failed result: {s}"));
    }
    s.get_key(":results")
        .and_then(Sexp::as_list)
        .ok_or("missing results")?
        .iter()
        .map(|r| match crate::sexp::read_result(r) {
            Some(crate::sexp::ResultView::Ok { value, .. }) => Ok(value),
            _ => Err(format!("bad slot: {r}")),
        })
        .collect()
}

pub fn run(peers: Option<&str>) -> Result<(), String> {
    let window = std::env::var("UNIT_BENCH_NATIVE_WINDOW")
        .unwrap_or_else(|_| "1".into())
        .parse::<usize>()
        .map_err(|_| "native window must be 1 or 2")?;
    if !(1..=2).contains(&window) {
        return Err("native window must be 1 or 2".into());
    }
    let mut root = vm();
    let seeds: Vec<SocketAddr> = if let Some(peers) = peers {
        peers
            .split(',')
            .map(|s| {
                s.to_socket_addrs()
                    .map_err(|e| e.to_string())?
                    .next()
                    .ok_or_else(|| "empty DNS resolution".to_string())
            })
            .collect::<Result<_, _>>()?
    } else {
        vec![]
    };
    let mesh = MeshNode::start(0, seeds.clone())?;
    let root_port = mesh.local_port();
    root.node_id_cache = Some(*mesh.id());
    root.mesh = Some(mesh);
    root.native_control(true);
    let state_dir = std::env::temp_dir().join(format!(
        "unit-native-{}",
        root.mesh.as_ref().unwrap().id_hex()
    ));
    std::fs::create_dir(&state_dir).map_err(|e| e.to_string())?;
    let mut children = Children(vec![], state_dir);
    if peers.is_none() {
        for i in 0..2 {
            let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
                .args([
                    "--port",
                    "0",
                    "--peers",
                    &format!("127.0.0.1:{root_port}"),
                    "--quiet",
                ])
                .env("UNIT_STATE_DIR", children.1.join(i.to_string()))
                .env_remove("UNIT_NODE_ID")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            writeln!(child.stdin.as_mut().unwrap(), "NATIVE-ON").map_err(|e| e.to_string())?;
            children.0.push(child);
        }
    }
    let count = if peers.is_some() { seeds.len() } else { 2 };
    let deadline = Instant::now() + Duration::from_secs(20);
    while root
        .native
        .peers
        .values()
        .filter(|(_, free)| *free > 0)
        .count()
        < count
    {
        root.poll_native_mesh();
        root.mesh.as_ref().unwrap().force_heartbeat();
        if Instant::now() > deadline {
            return Err(
                "native capability discovery timed out; enable NATIVE-ON on every peer".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    println!("mode,customers,tasks,repeat,elapsed_ms,remote,correct");
    // A fixed 12-point utilization/seed sweep, identical across all modes.
    for customers in [10_000, 1_000_000, 10_000_000, 100_000_000] {
        let tasks: Vec<_> = (0..12)
            .map(|i| Task {
                arrival: 100,
                service: 60 + (i % 6) * 10,
                customers,
                seed: 42 + i / 6,
            })
            .collect();
        let parts: Vec<_> = tasks
            .iter()
            .map(|t| crate::sexp::parse(&t.expression()).unwrap())
            .collect();
        let mut oracle: Option<Vec<[i64; 3]>> = None;
        for repeat in 0..3 {
            let now = Instant::now();
            let serial: Vec<_> = tasks.iter().map(execute).collect::<Result<_, _>>()?;
            let ms = now.elapsed().as_secs_f64() * 1000.0;
            if let Some(ref expected) = oracle {
                assert_eq!(&serial, expected);
            } else {
                oracle = Some(serial.clone());
            }
            println!("serial,{customers},12,{repeat},{ms:.3},0,true");
            let now = Instant::now();
            let threaded = std::thread::scope(|scope| {
                let handles: Vec<_> = tasks
                    .chunks(tasks.len().div_ceil(count + 1))
                    .map(|chunk| {
                        scope
                            .spawn(move || chunk.iter().map(execute).collect::<Result<Vec<_>, _>>())
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().unwrap())
                    .collect::<Result<Vec<_>, _>>()
            })?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            assert_eq!(threaded, serial);
            println!(
                "threads,{customers},12,{repeat},{:.3},0,true",
                now.elapsed().as_secs_f64() * 1000.0
            );
            // Each submitted benchmark job brings its own explicit fuel.
            root.energy.earn(1000, "benchmark-job-budget");
            root.poll_native_mesh();
            let before = root.recruit_ledger.len();
            let now = Instant::now();
            // Native local admission is a reserved bounded kernel, not a RAM
            // allocation. The resource closure is irrelevant for native parts.
            let mut reading = crate::resources::HostResources::measure;
            let gid = root.run_parallel_window(&parts, &mut reading, 0, true, true, window);
            while !root.parallel_jobs[&gid].is_complete() {
                root.tick_dist_goals();
                if now.elapsed() > Duration::from_secs(30) {
                    return Err(format!(
                        "mesh benchmark exceeded deadline: {}; {}",
                        root.parallel_result(gid).unwrap(),
                        root.recruit_ledger.format_status()
                    ));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            let ms = now.elapsed().as_secs_f64() * 1000.0;
            let expected: Vec<Vec<i64>> = serial
                .iter()
                .map(|r| r.iter().rev().copied().collect())
                .collect();
            assert_eq!(values(&root.parallel_result(gid).unwrap())?, expected);
            let remote = root.recruit_ledger.len() - before;
            if remote == 0 {
                return Err(
                    "no remote work; refusing to label local execution mesh throughput".into(),
                );
            }
            println!("mesh,{customers},12,{repeat},{ms:.3},{remote},true");
            root.parallel_jobs.remove(&gid);
        }
    }
    root.mesh.as_ref().unwrap().shutdown();
    Ok(())
}
