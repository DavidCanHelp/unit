//! Preinstalled, versioned native kernels: a unit capability, not a scheduler.
//! Pure deterministic inputs stay S-expressions; the Forth VM owns admission,
//! energy, recruitment and completion. No executable code crosses the mesh.
use crate::sexp::{EvalOutcome, Sexp};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub arrival: u64,
    pub service: u64,
    pub customers: u64,
    pub seed: u64,
}

impl Task {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=10_000).contains(&self.arrival)
            || !(1..=10_000).contains(&self.service)
            || !(1..=100_000_000).contains(&self.customers)
            || !(1..=i64::MAX as u64).contains(&self.seed)
        {
            return Err("queue-sim/v1 bounds: arrival/service 1..10000, customers 1..100000000, seed 1..i64::MAX".into());
        }
        // Worst-case total wait is at most service*n*(n-1); use u128
        // for admission so even overloaded scenarios cannot overflow results.
        if (self.service as u128) * (self.customers as u128) * ((self.customers - 1) as u128)
            > i64::MAX as u128
        {
            return Err("scenario exceeds exact i64 waiting-time range".into());
        }
        Ok(())
    }

    /// Collision-free canonical identity for the bounded integer input tuple.
    pub fn id(&self) -> String {
        format!(
            "queue-sim/v1/{}/{}/{}/{}",
            self.arrival, self.service, self.customers, self.seed
        )
    }

    pub fn expression(&self) -> String {
        format!("(native :kernel queue-sim :version 1 :arrival {} :service {} :customers {} :seed {} :task {})",
            self.arrival, self.service, self.customers, self.seed, self.id())
    }

    pub fn parse(s: &Sexp) -> Result<Self, String> {
        let items = s.as_list().ok_or("expected native expression")?;
        if items.first().and_then(Sexp::as_atom) != Some("native") || items.len() != 15 {
            return Err(
                "expected native with kernel, version, arrival, service, customers, seed, task"
                    .into(),
            );
        }
        let mut seen = std::collections::HashSet::new();
        for pair in items[1..].chunks_exact(2) {
            let key = pair[0].as_atom().ok_or("expected keyword")?;
            if ![
                ":kernel",
                ":version",
                ":arrival",
                ":service",
                ":customers",
                ":seed",
                ":task",
            ]
            .contains(&key)
                || !seen.insert(key)
            {
                return Err("unknown or duplicate native field".into());
            }
        }
        if s.get_key(":kernel")
            .and_then(|v| v.as_str().or_else(|| v.as_atom()))
            != Some("queue-sim")
            || s.get_key(":version").and_then(Sexp::as_number) != Some(1)
        {
            return Err("unsupported native kernel/version".into());
        }
        let number = |key| -> Result<u64, String> {
            s.get_key(key)
                .and_then(Sexp::as_number)
                .and_then(|n| u64::try_from(n).ok())
                .ok_or_else(|| format!("invalid {key}"))
        };
        let t = Task {
            arrival: number(":arrival")?,
            service: number(":service")?,
            customers: number(":customers")?,
            seed: number(":seed")?,
        };
        t.validate()?;
        if s.get_key(":task")
            .and_then(|v| v.as_str().or_else(|| v.as_atom()))
            != Some(t.id().as_str())
        {
            return Err("task identity does not match kernel/version/inputs".into());
        }
        Ok(t)
    }

    pub fn cost(&self) -> i64 {
        1 + (self.customers / 1_000_000) as i64
    }
}

/// A deterministic discrete-event single-server queue simulation. Interarrival
/// and service times are uniform integers 1..=2*parameter. Common random numbers
/// across parameter choices make utilization/latency sweeps reproducible. This
/// is NOT an M/M/1 model (the distributions are not exponential).
/// Returns [total waiting time, maximum wait, final departure time]. Bounds on
/// Task guarantee every accumulator fits in i64 even at worst-case overload.
pub fn execute(task: &Task) -> Result<[i64; 3], String> {
    task.validate()?;
    let mut state = task.seed;
    let mut draw = |scale: u64| {
        // Fully specified xorshift64; ordinary integer operations on all hosts.
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        1 + state % (2 * scale)
    };
    let (mut arrival, mut departure, mut total_wait, mut max_wait) = (0_u64, 0_u64, 0_u64, 0_u64);
    for _ in 0..task.customers {
        arrival += draw(task.arrival);
        let service = draw(task.service);
        let wait = departure.saturating_sub(arrival);
        total_wait += wait;
        max_wait = max_wait.max(wait);
        departure = arrival + wait + service;
    }
    Ok([total_wait as i64, max_wait as i64, departure as i64])
}

pub fn envelope(task: &Task) -> Sexp {
    match execute(task) {
        Ok(value) => {
            let mut result = crate::sexp::msg_result(EvalOutcome::Ok {
                stack: &value,
                output: "",
            });
            if let Sexp::List(ref mut xs) = result {
                xs.extend([Sexp::Atom(":task".into()), Sexp::Str(task.id())]);
            }
            result
        }
        Err(e) => error(&e),
    }
}

pub fn error(message: &str) -> Sexp {
    crate::sexp::msg_result(EvalOutcome::Err {
        kind: "native",
        msg: message,
    })
}

#[cfg(not(target_arch = "wasm32"))]
pub mod executor {
    use super::*;
    use std::collections::{HashMap, VecDeque};
    use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
    use std::thread::JoinHandle;

    #[derive(Clone, Debug, Hash, PartialEq, Eq)]
    pub struct Owner {
        pub peer: String,
        pub goal: u64,
        pub seq: usize,
    }
    struct Work {
        owner: Owner,
        task: Task,
    }
    pub struct Completed {
        pub owner: Owner,
        pub task_id: String,
        pub result: Sexp,
        pub bounty: i64,
    }
    struct Pending {
        task_id: String,
        bounty: i64,
    }
    pub enum Admission {
        Accepted,
        Duplicate,
        Cached(Sexp),
        Busy,
        Conflict,
    }

    /// One active native computation and two waiting, per UNIT. Pending includes
    /// finished-but-uncollected results, so neither channel can grow unbounded.
    /// A fixed replay cache holds 64 settled requests. Eviction permits a pure
    /// task to execute again; this is bounded replay protection, not exactly-once.
    pub struct Executor {
        tx: Option<SyncSender<Work>>,
        rx: Receiver<Completed>,
        thread: Option<JoinHandle<()>>,
        pending: HashMap<Owner, Pending>,
        cache: VecDeque<(Owner, String, Sexp)>,
        pub accepted: u64,
        pub busy: u64,
        pub high_water: usize,
    }

    impl Default for Executor {
        fn default() -> Self {
            Self::new()
        }
    }
    impl Executor {
        pub fn new() -> Self {
            let (tx, work_rx) = mpsc::sync_channel::<Work>(3);
            let (result_tx, rx) = mpsc::channel();
            let thread = std::thread::spawn(move || {
                while let Ok(work) = work_rx.recv() {
                    let result = envelope(&work.task);
                    if result_tx
                        .send(Completed {
                            owner: work.owner,
                            task_id: work.task.id(),
                            result,
                            bounty: 0,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
            Self {
                tx: Some(tx),
                rx,
                thread: Some(thread),
                pending: HashMap::new(),
                cache: VecDeque::new(),
                accepted: 0,
                busy: 0,
                high_water: 0,
            }
        }
        pub fn outstanding(&self) -> usize {
            self.pending.len()
        }
        #[cfg(test)]
        pub fn replay_len(&self) -> usize {
            self.cache.len()
        }
        pub fn existing(&self, owner: &Owner, task: &Task) -> Option<Admission> {
            let id = task.id();
            if let Some(old) = self.pending.get(owner) {
                return Some(if old.task_id == id {
                    Admission::Duplicate
                } else {
                    Admission::Conflict
                });
            }
            self.cache
                .iter()
                .find(|(o, _, _)| o == owner)
                .map(|(_, old_id, result)| {
                    if *old_id == id {
                        Admission::Cached(result.clone())
                    } else {
                        Admission::Conflict
                    }
                })
        }
        pub fn admit(&mut self, owner: Owner, task: Task, bounty: i64) -> Admission {
            if let Some(a) = self.existing(&owner, &task) {
                return a;
            }
            let id = task.id();
            if self.pending.len() >= 3 {
                self.busy += 1;
                return Admission::Busy;
            }
            match self.tx.as_ref().unwrap().try_send(Work {
                owner: owner.clone(),
                task,
            }) {
                Ok(()) => {
                    self.pending.insert(
                        owner,
                        Pending {
                            task_id: id,
                            bounty,
                        },
                    );
                    self.accepted += 1;
                    self.high_water = self.high_water.max(self.pending.len());
                    Admission::Accepted
                }
                Err(TrySendError::Full(_)) => {
                    self.busy += 1;
                    Admission::Busy
                }
                Err(TrySendError::Disconnected(_)) => Admission::Conflict,
            }
        }
        pub fn collect(&mut self) -> Vec<Completed> {
            let mut out = Vec::new();
            while let Ok(mut done) = self.rx.try_recv() {
                if let Some(p) = self.pending.remove(&done.owner) {
                    done.bounty = p.bounty;
                }
                self.cache.push_back((
                    done.owner.clone(),
                    done.task_id.clone(),
                    done.result.clone(),
                ));
                if self.cache.len() > 64 {
                    self.cache.pop_front();
                }
                out.push(done);
            }
            out
        }
    }
    impl Drop for Executor {
        fn drop(&mut self) {
            self.tx.take();
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub mod runtime {
    use super::executor::{Admission, Executor, Owner};
    use super::*;
    use std::collections::HashMap;
    use std::time::{Duration, Instant};

    #[derive(Default)]
    pub struct State {
        pub enabled: bool,
        pub executor: Option<Executor>,
        pub peers: HashMap<String, (Instant, usize)>,
        retry: HashMap<(u64, usize), (String, Instant)>,
        advertised: Option<Instant>,
        announce: bool,
        pub refusals: u64,
    }

    impl crate::vm::VM {
        pub(crate) fn print_native_status(&mut self) {
            let (pending, accepted, high_water) = self
                .native
                .executor
                .as_ref()
                .map(|e| (e.outstanding(), e.accepted, e.high_water))
                .unwrap_or((0, 0, 0));
            self.emit_str(&format!("(native-status :enabled {} :kernel queue-sim :version 1 :pending {pending} :capacity 3 :accepted {accepted} :high-water {high_water} :busy {})\n",
                u8::from(self.native.enabled), self.native.refusals));
        }

        pub(crate) fn native_control(&mut self, enabled: bool) {
            if self.sandbox_active {
                self.fault.get_or_insert(crate::vm::Fault::NativeExecution);
                return;
            }
            self.native.enabled = enabled;
            self.native.advertised = None;
            self.native.announce = true;
            // Already accepted work finishes even after NATIVE-OFF.
            self.poll_native();
        }

        pub(crate) fn local_native(&mut self, task: &Task) -> Sexp {
            if let Err(e) = task.validate() {
                return error(&e);
            }
            if let Some(budget) = self.step_budget.as_mut() {
                if *budget < task.customers {
                    return error("native task exceeds VM step budget");
                }
                *budget -= task.customers;
            }
            if !self.energy.spend(task.cost(), "native-compute") {
                return error("insufficient energy");
            }
            envelope(task)
        }

        pub(crate) fn native_capable(&self, part: &Sexp, peer: &str) -> bool {
            if crate::sexp::msg_type(part) != Some("native") {
                return true;
            }
            self.native
                .peers
                .get(peer)
                .is_some_and(|(at, free)| at.elapsed() < Duration::from_secs(5) && *free > 0)
        }

        pub(crate) fn accept_native(
            &mut self,
            goal: u64,
            seq: usize,
            instr: &str,
            peer: &str,
            bounty: i64,
        ) {
            let own = self
                .node_id_cache
                .map(|id| crate::mesh::id_to_hex(&id))
                .unwrap_or_else(|| "local".into());
            let send_result = |vm: &Self, env: &Sexp| {
                vm.send_to_node(
                    peer,
                    &crate::distgoal::sexp_recruit_result(goal, seq, &own, env),
                )
            };
            let task = match crate::sexp::parse(instr)
                .map_err(|e| e.0)
                .and_then(|s| Task::parse(&s))
            {
                Ok(t) => t,
                Err(e) => {
                    send_result(self, &error(&e));
                    return;
                }
            };
            let owner = Owner {
                peer: peer.into(),
                goal,
                seq,
            };
            if let Some(a) = self
                .native
                .executor
                .as_ref()
                .and_then(|e| e.existing(&owner, &task))
            {
                match a {
                    Admission::Cached(env) => send_result(self, &env),
                    Admission::Conflict => send_result(
                        self,
                        &error("recruit identity reused for different native input"),
                    ),
                    _ => {}
                }
                return;
            }
            if !self.native.enabled {
                send_result(self, &error("native admission disabled"));
                return;
            }
            // Energy participates in the same admission decision as the hard
            // per-unit queue bound. No income is earned on duplicate replay.
            if !self.energy.can_afford(task.cost()) {
                self.send_native_busy(peer, goal, seq);
                return;
            }
            let cost = task.cost();
            let executor = self.native.executor.get_or_insert_with(Executor::new);
            match executor.admit(owner, task, bounty) {
                Admission::Accepted => {
                    self.energy.spend(cost, "native-compute");
                }
                Admission::Duplicate => {}
                Admission::Cached(env) => send_result(self, &env),
                Admission::Busy => self.send_native_busy(peer, goal, seq),
                Admission::Conflict => send_result(
                    self,
                    &error("recruit identity reused for different native input"),
                ),
            }
        }

        fn send_native_busy(&mut self, peer: &str, goal: u64, seq: usize) {
            self.native.refusals += 1;
            let own = self
                .node_id_cache
                .map(|id| crate::mesh::id_to_hex(&id))
                .unwrap_or_else(|| "local".into());
            self.send_to_node(
                peer,
                &format!("(recruit-busy :id {goal} :seq {seq} :from \"{own}\" :retry-ms 50)"),
            );
        }

        pub(crate) fn native_message(&mut self, s: &Sexp) {
            let from = s.get_key(":from").and_then(Sexp::as_str).unwrap_or("");
            match crate::sexp::msg_type(s) {
                Some("native-cap") => {
                    if s.get_key(":kernel")
                        .and_then(|v| v.as_str().or_else(|| v.as_atom()))
                        != Some("queue-sim")
                        || s.get_key(":version").and_then(Sexp::as_number) != Some(1)
                    {
                        return;
                    }
                    let known = self
                        .mesh
                        .as_ref()
                        .is_some_and(|m| m.peer_details().iter().any(|(id, _, _)| id == from));
                    if known {
                        let free = s
                            .get_key(":free")
                            .and_then(Sexp::as_number)
                            .unwrap_or(0)
                            .clamp(0, 3) as usize;
                        self.native
                            .peers
                            .insert(from.into(), (Instant::now(), free));
                    }
                }
                Some("recruit-busy") => {
                    let (Some(g), Some(q)) = (
                        s.get_key(":id").and_then(Sexp::as_number),
                        s.get_key(":seq").and_then(Sexp::as_number),
                    ) else {
                        return;
                    };
                    if g < 0 || q < 0 {
                        return;
                    }
                    if self
                        .recruit_ledger
                        .pending_instruction(g as u64, q as usize, from)
                        .is_some()
                    {
                        self.native
                            .retry
                            .entry((g as u64, q as usize))
                            .or_insert_with(|| {
                                (from.into(), Instant::now() + Duration::from_millis(50))
                            });
                    }
                }
                _ => {}
            }
        }

        pub(crate) fn poll_native(&mut self) {
            let own = self
                .node_id_cache
                .map(|id| crate::mesh::id_to_hex(&id))
                .unwrap_or_else(|| "local".into());
            if let Some(e) = self.native.executor.as_mut() {
                let completed = e.collect();
                for done in completed {
                    self.send_to_node(
                        &done.owner.peer,
                        &crate::distgoal::sexp_recruit_result(
                            done.owner.goal,
                            done.owner.seq,
                            &own,
                            &done.result,
                        ),
                    );
                    if done.bounty > 0 {
                        self.energy.earn(done.bounty, "recruit-wage");
                    }
                }
            }
            // Busy is not success or an execution attempt. Retry without
            // resetting the original supervision deadline: a permanently full
            // worker still reaches the existing terminal attempt bound.
            let now = Instant::now();
            let due: Vec<_> = self
                .native
                .retry
                .iter()
                .filter(|(_, (_, at))| *at <= now)
                .map(|(key, (peer, _))| (*key, peer.clone()))
                .collect();
            for ((g, seq), peer) in due {
                self.native.retry.remove(&(g, seq));
                if let Some(instr) = self.recruit_ledger.pending_instruction(g, seq, &peer) {
                    let msg = crate::distgoal::sexp_recruit(g, seq, &own, &instr, 0);
                    self.send_to_node(&peer, &msg);
                }
            }
            self.native
                .peers
                .retain(|_, (at, _)| at.elapsed() < Duration::from_secs(5));
            if (self.native.enabled || self.native.announce)
                && self
                    .native
                    .advertised
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
            {
                let outstanding = self
                    .native
                    .executor
                    .as_ref()
                    .map_or(0, Executor::outstanding);
                let free = if self.native.enabled && self.energy.can_afford(1) {
                    3 - outstanding
                } else {
                    0
                };
                if let Some(m) = self.mesh.as_ref() {
                    m.send_sexp(&format!("(native-cap :from \"{own}\" :kernel \"queue-sim\" :version 1 :free {free})"));
                }
                self.native.advertised = Some(now);
                self.native.announce = false;
            }
        }

        pub(crate) fn poll_native_mesh(&mut self) {
            let messages = self
                .mesh
                .as_ref()
                .map(|m| m.recv_native_messages())
                .unwrap_or_default();
            for msg in messages {
                self.process_chatter_msg(&msg);
            }
            self.poll_native();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task() -> Task {
        Task {
            arrival: 100,
            service: 80,
            customers: 1000,
            seed: 42,
        }
    }

    #[test]
    fn identity_roundtrip_and_strict_contract() {
        let t = task();
        assert_eq!(
            Task::parse(&crate::sexp::parse(&t.expression()).unwrap()).unwrap(),
            t
        );
        for bad in [
            t.expression().replace(":version 1", ":version 2"),
            t.expression().replace(":arrival 100", ":arrival 101"),
            t.expression().replace(":seed 42", ":service 42"),
            t.expression().replace(":customers 1000", ":customers -1"),
        ] {
            assert!(Task::parse(&crate::sexp::parse(&bad).unwrap()).is_err());
        }
    }

    #[test]
    fn simulation_matches_independent_event_list() {
        let t = task();
        let mut state = t.seed;
        let mut random = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut clock = 0;
        let events: Vec<_> = (0..t.customers)
            .map(|_| {
                clock += 1 + random() % (2 * t.arrival);
                (clock, 1 + random() % (2 * t.service))
            })
            .collect();
        let mut departures = vec![0_u64];
        let mut waits = Vec::new();
        for (at, service) in events {
            let begin = at.max(*departures.last().unwrap());
            waits.push(begin - at);
            departures.push(begin + service);
        }
        assert_eq!(execute(&t).unwrap(), [138346, 763, 98717]);
        assert_eq!(
            execute(&t).unwrap(),
            [
                waits.iter().sum::<u64>() as i64,
                *waits.iter().max().unwrap() as i64,
                *departures.last().unwrap() as i64
            ]
        );
    }

    #[test]
    fn forth_keeps_kernel_composable_and_bounded() {
        let mut vm = crate::vm::VM::new();
        vm.silent = true;
        vm.load_prelude();
        vm.eval(": SCENARIO 100 80 1000 42 QUEUE-SIM ;");
        let r = vm.execute_sandbox("SCENARIO");
        assert!(r.success);
        assert_eq!(r.stack_snapshot, execute(&task()).unwrap());
        vm.step_budget = Some(10);
        assert!(!vm.execute_sandbox("SCENARIO").success);
        vm.step_budget = None;
        assert!(!vm.execute_sandbox("NATIVE-ON").success);
        #[cfg(not(target_arch = "wasm32"))]
        assert!(!vm.native.enabled);
    }

    #[test]
    fn result_identity_must_match_the_retained_instruction() {
        let mut vm = crate::vm::VM::new();
        vm.silent = true;
        let t = task();
        vm.recruit_ledger.open(1, 0, &t.expression(), "worker");
        vm.parallel_jobs
            .insert(1, crate::distgoal::ParallelJob::new(1, 1));
        let mut wrong = t.clone();
        wrong.seed += 1;
        let reply = crate::distgoal::sexp_recruit_result(1, 0, "worker", &envelope(&wrong));
        vm.process_chatter_msg(&reply);
        assert_eq!(vm.recruit_ledger.pending_on("worker"), 1);
        let reply = crate::distgoal::sexp_recruit_result(1, 0, "worker", &envelope(&t));
        vm.process_chatter_msg(&reply);
        assert_eq!(vm.recruit_ledger.pending_on("worker"), 0);
        assert!(vm.parallel_jobs[&1].is_complete());
    }

    #[test]
    fn numerical_admission_proves_accumulators_fit() {
        let mut t = task();
        t.customers = 100_000_000;
        t.service = 10_000;
        assert!(t.validate().is_err());
        t.service = 80;
        assert!(t.validate().is_ok());
        t.customers = 0;
        assert!(t.validate().is_err());
    }

    #[test]
    fn old_genome_gets_missing_capabilities_without_replacing_policy() {
        let mut vm = crate::vm::VM::new();
        vm.silent = true;
        vm.dictionary.retain(|e| {
            !["QUEUE-SIM", "NATIVE-ON", "NATIVE-OFF", "NATIVE-STATUS"].contains(&e.name.as_str())
        });
        vm.eval(": INHERITED 6 7 * ; : NATIVE-ON 123 ;");
        let old_len = vm.dictionary.len();
        let old_word = vm.find_word("INHERITED").unwrap();
        vm.install_native_primitives();
        assert_eq!(vm.dictionary.len(), old_len + 3);
        assert_eq!(vm.find_word("INHERITED"), Some(old_word));
        assert_eq!(
            vm.execute_sandbox("INHERITED NATIVE-ON").stack_snapshot,
            vec![42, 123]
        );
        assert!(vm.find_word("QUEUE-SIM").is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn admission_replay_and_retention_are_bounded() {
        use executor::*;
        let mut e = Executor::new();
        let owner = |seq| Owner {
            peer: "peer".into(),
            goal: 1,
            seq,
        };
        for i in 0..3 {
            assert!(matches!(e.admit(owner(i), task(), 1), Admission::Accepted));
        }
        assert!(matches!(e.admit(owner(3), task(), 1), Admission::Busy));
        assert_eq!(e.outstanding(), 3);
        assert!(matches!(e.admit(owner(0), task(), 1), Admission::Duplicate));
        let mut different = task();
        different.seed = 43;
        assert!(matches!(
            e.admit(owner(0), different, 1),
            Admission::Conflict
        ));
        let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut results = Vec::new();
        while results.len() < 3 {
            results.extend(e.collect());
            assert!(std::time::Instant::now() < until);
            std::thread::yield_now();
        }
        assert_eq!(e.outstanding(), 0);
        assert!(matches!(e.admit(owner(0), task(), 1), Admission::Cached(_)));
        assert_eq!(e.accepted, 3);
        assert_eq!(e.high_water, 3);
        for i in 3..70 {
            let mut tiny = task();
            tiny.customers = 1;
            assert!(matches!(e.admit(owner(i), tiny, 0), Admission::Accepted));
            let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
            while e.collect().is_empty() {
                assert!(std::time::Instant::now() < end);
                std::thread::yield_now();
            }
        }
        assert_eq!(e.replay_len(), 64);
    }
}
