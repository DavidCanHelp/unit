//! Distributed goal computation for unit.
//!
//! A unit breaks a problem into sub-goals, distributes them as S-expressions
//! to mesh peers, collects results, and combines the answer.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Unique identifier for a distributed goal.
pub type GoalId = u64;

/// Tracks the lifecycle state of a distributed goal.
#[derive(Clone, Debug, PartialEq)]
pub enum DistStatus {
    Pending,
    Running,
    Complete,
    Failed,
}

/// A single sub-task within a distributed goal, assigned to a local or remote worker.
#[derive(Clone, Debug)]
pub struct SubGoal {
    pub seq: usize,
    pub expr: String,        // Forth code to evaluate
    pub assigned_to: String, // "local" or peer hex ID
    pub result: Option<String>,
    pub sent_at: u64, // counter-based (not time)
}

/// A distributed goal composed of multiple sub-goals with a result combiner.
#[derive(Clone, Debug)]
pub struct DistGoal {
    pub id: GoalId,
    pub parent_id: String, // node that initiated
    pub sub_goals: Vec<SubGoal>,
    pub status: DistStatus,
    pub combiner: Combiner,
    pub tick: u64, // monotonic counter for timeouts
}

/// Strategy for combining sub-goal results into a final answer.
#[derive(Clone, Debug)]
pub enum Combiner {
    List,   // collect all results as a list
    Sum,    // sum numeric results
    Concat, // concatenate output strings
}

/// Manages distributed goal creation, assignment, result collection, and timeouts.
#[derive(Clone, Debug, Default)]
pub struct DistEngine {
    pub goals: HashMap<GoalId, DistGoal>,
    next_id: u64,
    pub tick: u64,
    pub timeout_ticks: u64, // ticks before fallback (default ~50)
}

impl DistEngine {
    /// Creates a new engine with default timeout settings.
    pub fn new() -> Self {
        DistEngine {
            goals: HashMap::new(),
            next_id: 1,
            tick: 0,
            timeout_ticks: 50,
        }
    }

    /// Allocates and returns the next unique goal ID.
    pub fn next_id(&mut self) -> GoalId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Advances the internal tick counter for timeout tracking.
    pub fn advance_tick(&mut self) {
        self.tick += 1;
    }

    /// Create a new distributed goal from pipe-separated expressions.
    pub fn create_goal(
        &mut self,
        expressions: Vec<String>,
        parent_id: &str,
        peer_ids: &[String], // available peer hex IDs
    ) -> GoalId {
        let id = self.next_id();
        let mut sub_goals = Vec::new();
        let all_workers: Vec<String> = {
            let mut w = vec!["local".to_string()];
            w.extend(peer_ids.iter().cloned());
            w
        };

        for (i, expr) in expressions.iter().enumerate() {
            let worker = &all_workers[i % all_workers.len()];
            sub_goals.push(SubGoal {
                seq: i,
                expr: expr.trim().to_string(),
                assigned_to: worker.clone(),
                result: None,
                sent_at: self.tick,
            });
        }

        let goal = DistGoal {
            id,
            parent_id: parent_id.to_string(),
            sub_goals,
            status: DistStatus::Running,
            combiner: Combiner::List,
            tick: self.tick,
        };
        self.goals.insert(id, goal);
        id
    }

    /// Record a result for a sub-goal.
    pub fn record_result(&mut self, goal_id: GoalId, seq: usize, result: &str) -> bool {
        if let Some(goal) = self.goals.get_mut(&goal_id) {
            if let Some(sg) = goal.sub_goals.iter_mut().find(|sg| sg.seq == seq) {
                sg.result = Some(result.to_string());
                // Check if all done
                if goal.sub_goals.iter().all(|sg| sg.result.is_some()) {
                    goal.status = DistStatus::Complete;
                }
                return true;
            }
        }
        false
    }

    /// Get sub-goals that need to be sent to remote peers.
    pub fn pending_remote_subgoals(&self, goal_id: GoalId) -> Vec<(usize, String, String)> {
        // Returns (seq, expr, peer_id) for sub-goals assigned to non-local peers
        if let Some(goal) = self.goals.get(&goal_id) {
            goal.sub_goals
                .iter()
                .filter(|sg| sg.assigned_to != "local" && sg.result.is_none())
                .map(|sg| (sg.seq, sg.expr.clone(), sg.assigned_to.clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get sub-goals assigned to "local" that haven't been computed yet.
    pub fn pending_local_subgoals(&self, goal_id: GoalId) -> Vec<(usize, String)> {
        if let Some(goal) = self.goals.get(&goal_id) {
            goal.sub_goals
                .iter()
                .filter(|sg| sg.assigned_to == "local" && sg.result.is_none())
                .map(|sg| (sg.seq, sg.expr.clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Get sub-goals that have timed out (assigned to remote, no result after timeout_ticks).
    pub fn timed_out_subgoals(&self, goal_id: GoalId) -> Vec<(usize, String)> {
        if let Some(goal) = self.goals.get(&goal_id) {
            goal.sub_goals
                .iter()
                .filter(|sg| {
                    sg.assigned_to != "local"
                        && sg.result.is_none()
                        && self.tick - sg.sent_at > self.timeout_ticks
                })
                .map(|sg| (sg.seq, sg.expr.clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Reassign a timed-out sub-goal to local.
    pub fn fallback_to_local(&mut self, goal_id: GoalId, seq: usize) {
        if let Some(goal) = self.goals.get_mut(&goal_id) {
            if let Some(sg) = goal.sub_goals.iter_mut().find(|sg| sg.seq == seq) {
                sg.assigned_to = "local".to_string();
                sg.sent_at = self.tick;
            }
        }
    }

    /// Is the goal complete?
    pub fn is_complete(&self, goal_id: GoalId) -> bool {
        self.goals
            .get(&goal_id)
            .is_some_and(|g| g.status == DistStatus::Complete)
    }

    /// Combine results into final output.
    pub fn combine_results(&self, goal_id: GoalId) -> Option<String> {
        let goal = self.goals.get(&goal_id)?;
        let results: Vec<String> = goal
            .sub_goals
            .iter()
            .filter_map(|sg| sg.result.clone())
            .collect();
        if results.len() != goal.sub_goals.len() {
            return None; // not all results in
        }
        Some(match goal.combiner {
            Combiner::List => results.join(" "),
            Combiner::Sum => {
                let total: i64 = results
                    .iter()
                    .filter_map(|r| r.trim().parse::<i64>().ok())
                    .sum();
                format!("{}", total)
            }
            Combiner::Concat => results.join(""),
        })
    }

    /// Format status for display.
    pub fn format_status(&self) -> String {
        if self.goals.is_empty() {
            return "no distributed goals\n".to_string();
        }
        let mut out = String::new();
        for (id, goal) in &self.goals {
            let done = goal
                .sub_goals
                .iter()
                .filter(|sg| sg.result.is_some())
                .count();
            let total = goal.sub_goals.len();
            out.push_str(&format!(
                "goal #{}: {:?} ({}/{} complete)\n",
                id, goal.status, done, total
            ));
            for sg in &goal.sub_goals {
                let status = if sg.result.is_some() {
                    "done"
                } else {
                    "pending"
                };
                out.push_str(&format!(
                    "  [{}] {} -> {} ({})\n",
                    sg.seq,
                    if sg.expr.len() > 30 {
                        format!("{}...", &sg.expr[..30])
                    } else {
                        sg.expr.clone()
                    },
                    sg.assigned_to,
                    status
                ));
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Parse pipe-separated expressions from DIST-GOAL{ ... }
// ---------------------------------------------------------------------------

/// Splits a pipe-separated input string into individual Forth expressions.
pub fn parse_pipe_expressions(input: &str) -> Vec<String> {
    input
        .split('|')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// S-expression message constructors
// ---------------------------------------------------------------------------

/// Builds an S-expression to dispatch a sub-goal to a remote peer.
pub fn sexp_sub_goal(goal_id: GoalId, seq: usize, from: &str, expr: &str) -> String {
    format!(
        "(sub-goal :id {} :seq {} :from \"{}\" :expr \"{}\")",
        goal_id,
        seq,
        from,
        expr.replace('"', "\\\"")
    )
}

/// Builds an S-expression to return a sub-goal result to the originator.
pub fn sexp_sub_result(goal_id: GoalId, seq: usize, from: &str, result: &str) -> String {
    format!(
        "(sub-result :id {} :seq {} :from \"{}\" :result \"{}\")",
        goal_id,
        seq,
        from,
        result.replace('"', "\\\"")
    )
}

/// Builds an S-expression announcing that a distributed goal is complete.
pub fn sexp_dist_complete(goal_id: GoalId, results: &str, peers: usize) -> String {
    format!(
        "(dist-complete :id {} :results \"{}\" :peers {})",
        goal_id,
        results.replace('"', "\\\""),
        peers
    )
}

// ---------------------------------------------------------------------------
// Recruit message pair (step-2 recruit pattern, built on the eval_sexp seam)
//
// These are additive and independent of the sub-goal/sub-result path above:
// the recruit reply carries the canonical (result ...) envelope from
// eval_sexp, preserving success/error, rather than a bare output string.
// ---------------------------------------------------------------------------

/// Builds an outgoing `(recruit ...)` message handing an s-expression
/// instruction to a peer. The routing fields (`:id`, `:seq`, `:from`) let the
/// recruiter match the eventual `(recruit-result ...)` back to this slot;
/// `:instr` carries the s-expression instruction text the worker evaluates
/// through the eval_sexp seam.
pub fn sexp_recruit(goal_id: GoalId, seq: usize, from: &str, instr: &str) -> String {
    format!(
        "(recruit :id {} :seq {} :from \"{}\" :instr \"{}\")",
        goal_id,
        seq,
        from,
        instr.replace('"', "\\\"")
    )
}

/// Builds a `(recruit-result ...)` reply. The routing fields live on the outer
/// wrapper; the full canonical result envelope (from `eval_sexp`) rides nested
/// as `:result`'s value — not flattened to a bare string — so the recruiter
/// reads back ok/value/output/error structurally via [`read_recruit_result`].
pub fn sexp_recruit_result(
    goal_id: GoalId,
    seq: usize,
    from: &str,
    envelope: &crate::sexp::Sexp,
) -> String {
    use crate::sexp::Sexp;
    Sexp::List(vec![
        Sexp::Atom("recruit-result".into()),
        Sexp::Atom(":id".into()),
        Sexp::Number(goal_id as i64),
        Sexp::Atom(":seq".into()),
        Sexp::Number(seq as i64),
        Sexp::Atom(":from".into()),
        Sexp::Str(from.into()),
        Sexp::Atom(":result".into()),
        envelope.clone(),
    ])
    .to_string()
}

/// A `(recruit-result ...)` read back: the routing fields plus the decoded
/// result envelope.
#[derive(Debug, PartialEq)]
pub struct RecruitResult {
    pub goal_id: GoalId,
    pub seq: usize,
    pub from: String,
    pub result: crate::sexp::ResultView,
}

/// Parses a `(recruit-result :id :seq :from :result <envelope>)` message,
/// extracting the routing fields and `read_result`-ing the nested envelope.
/// Returns `None` if it is not a well-formed recruit-result.
pub fn read_recruit_result(sexp: &crate::sexp::Sexp) -> Option<RecruitResult> {
    if crate::sexp::msg_type(sexp)? != "recruit-result" {
        return None;
    }
    let goal_id = sexp.get_key(":id")?.as_number()? as GoalId;
    let seq = sexp.get_key(":seq")?.as_number()? as usize;
    let from = sexp.get_key(":from")?.as_str()?.to_string();
    let result = crate::sexp::read_result(sexp.get_key(":result")?)?;
    Some(RecruitResult {
        goal_id,
        seq,
        from,
        result,
    })
}

/// Recruiter-side ledger binding outstanding recruit requests to their
/// collected results, keyed by `(goal_id, seq)`. Mechanism only — it records
/// what was recruited and what came back; it holds no policy about *when* to
/// recruit. `open` on emit, `collect` on reply (matched by key).
#[derive(Debug, Default)]
pub struct RecruitLedger {
    entries: HashMap<(GoalId, usize), Option<RecruitResult>>,
    next_id: GoalId,
}

impl RecruitLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate a fresh goal_id for a manually-triggered recruit. Independent
    /// of `DistEngine`'s id space so the recruit path stays decoupled.
    pub fn next_id(&mut self) -> GoalId {
        self.next_id += 1;
        self.next_id
    }

    /// Record an outstanding request whose reply has not yet arrived.
    pub fn open(&mut self, goal_id: GoalId, seq: usize) {
        self.entries.insert((goal_id, seq), None);
    }

    /// Record a collected reply if it matches an outstanding request. Returns
    /// true if it matched a known `(goal_id, seq)` and was recorded; false if
    /// the reply is for a request this node did not open (so cross-node
    /// broadcasts that aren't ours are ignored).
    pub fn collect(&mut self, rr: RecruitResult) -> bool {
        let key = (rr.goal_id, rr.seq);
        if let Some(slot) = self.entries.get_mut(&key) {
            *slot = Some(rr);
            true
        } else {
            false
        }
    }

    /// The collected result for a slot, if its reply has arrived.
    pub fn get(&self, goal_id: GoalId, seq: usize) -> Option<&RecruitResult> {
        self.entries.get(&(goal_id, seq)).and_then(|o| o.as_ref())
    }

    /// True if the slot was opened but its reply hasn't arrived yet.
    pub fn is_pending(&self, goal_id: GoalId, seq: usize) -> bool {
        matches!(self.entries.get(&(goal_id, seq)), Some(None))
    }

    /// Number of recruit requests opened (outstanding + collected). Lets a
    /// caller observe recruit emission — e.g. that a saturated mesh emits none.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if no recruit has been opened.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Human-readable dump for the RECRUITS REPL word (sorted for determinism).
    pub fn format_status(&self) -> String {
        if self.entries.is_empty() {
            return "no recruits\n".to_string();
        }
        let mut keys: Vec<(GoalId, usize)> = self.entries.keys().copied().collect();
        keys.sort();
        let mut out = String::new();
        for key in keys {
            let (g, s) = key;
            match &self.entries[&key] {
                None => out.push_str(&format!("recruit #{} seq {}: pending\n", g, s)),
                Some(rr) => {
                    let body = match &rr.result {
                        crate::sexp::ResultView::Ok { value, output } => {
                            format!("ok value={:?} output={:?}", value, output)
                        }
                        crate::sexp::ResultView::Err { kind, msg } => {
                            format!("ERR [{}] {}", kind, msg)
                        }
                    };
                    out.push_str(&format!(
                        "recruit #{} seq {} from {}: {}\n",
                        g, s, rr.from, body
                    ));
                }
            }
        }
        out
    }
}

/// Where a unit reports its completed result back to. Stored by a unit that
/// recruited overflow (so it can't complete synchronously), keyed by its own
/// child job's goal_id; consulted when that child job becomes whole. No
/// coordinator — each unit holds only its own back-references.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportTarget {
    /// Hex node id of whoever recruited this unit — who to report back to.
    pub recruiter_node: String,
    /// The recruiter's goal_id for this work (routes the reply to its slot).
    pub goal_id: GoalId,
    /// The recruiter's slot index for this work.
    pub seq: usize,
}

/// Outcome of handling an inbound recruit. `Reply` carries a recruit-result to
/// send back synchronously — the work completed here. `Deferred` means this
/// unit recruited overflow and will self-report once its child job completes;
/// `child_goal_id` is that job's id (exposed for correlation and testing).
#[derive(Debug)]
pub enum RecruitOutcome {
    Reply(String),
    Deferred { child_goal_id: GoalId },
}

// ---------------------------------------------------------------------------
// (parallel ...) split-and-recruit job
//
// The divisible form is (parallel (e1) (e2) ...): each element is an
// independent s-expr sub-instruction that runs correctly from an empty stack.
// A ParallelJob holds one ordered slot per sub-part; local parts fill
// immediately, recruited parts fill when their (recruit-result ...) arrives.
// Results are COLLECTED, never combined — combination is a later step.
// ---------------------------------------------------------------------------

/// If `sexp` is a `(parallel (e1) (e2) ...)` form, returns its sub-parts in
/// order. Returns `None` for any other shape. Only this form is divisible:
/// the sandbox resets the stack per eval, so an arbitrary mid-stack Forth
/// split would underflow — each sub-part must stand alone.
pub fn parallel_parts(sexp: &crate::sexp::Sexp) -> Option<Vec<crate::sexp::Sexp>> {
    let items = sexp.as_list()?;
    if items.first()?.as_atom()? != "parallel" {
        return None;
    }
    Some(items[1..].to_vec())
}

/// A parallel job: one ordered result slot per sub-part. A slot holds its
/// sub-part's canonical result envelope once available (local parts fill
/// immediately; recruited parts fill when their reply arrives).
#[derive(Debug)]
pub struct ParallelJob {
    pub goal_id: GoalId,
    slots: Vec<Option<crate::sexp::Sexp>>,
}

impl ParallelJob {
    pub fn new(goal_id: GoalId, parts: usize) -> Self {
        ParallelJob {
            goal_id,
            slots: vec![None; parts],
        }
    }

    /// Record a sub-part's result envelope at its slot index (no-op if out of
    /// range).
    pub fn set(&mut self, seq: usize, envelope: crate::sexp::Sexp) {
        if let Some(slot) = self.slots.get_mut(seq) {
            *slot = Some(envelope);
        }
    }

    /// True once every slot has a result.
    pub fn is_complete(&self) -> bool {
        self.slots.iter().all(|s| s.is_some())
    }

    /// Assemble the collected result:
    ///   (parallel-result :ok <1|0> :results ( <env0> <env1> ... ))
    /// preserving sub-part order. Results are collected, NOT combined. Overall
    /// `:ok` is 1 only if every slot is present and every envelope is `:ok 1`;
    /// a still-pending slot renders as a pending error envelope and forces 0.
    pub fn assemble(&self) -> crate::sexp::Sexp {
        use crate::sexp::Sexp;
        let mut all_ok = true;
        let mut results = Vec::with_capacity(self.slots.len());
        for slot in &self.slots {
            match slot {
                Some(env) => {
                    // A slot envelope is either a (result ...) or, for a
                    // recursively-recruited part, a nested (parallel-result ...).
                    // Both carry :ok; treat :ok 1 as success. Nested
                    // parallel-results are collected as-is, never flattened.
                    if env.get_key(":ok").and_then(|s| s.as_number()) != Some(1) {
                        all_ok = false;
                    }
                    results.push(env.clone());
                }
                None => {
                    all_ok = false;
                    results.push(crate::sexp::msg_result(crate::sexp::EvalOutcome::Err {
                        kind: "pending",
                        msg: "pending",
                    }));
                }
            }
        }
        Sexp::List(vec![
            Sexp::Atom("parallel-result".into()),
            Sexp::Atom(":ok".into()),
            Sexp::Number(if all_ok { 1 } else { 0 }),
            Sexp::Atom(":results".into()),
            Sexp::List(results),
        ])
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pipe_expressions() {
        let exprs = parse_pipe_expressions("10 10 * | 20 20 * | 30 30 *");
        assert_eq!(exprs.len(), 3);
        assert_eq!(exprs[0], "10 10 *");
        assert_eq!(exprs[1], "20 20 *");
        assert_eq!(exprs[2], "30 30 *");
    }

    // --- Recruit message pair (pure; no VM) ---

    #[test]
    fn test_sexp_recruit_shape() {
        let s = sexp_recruit(7, 2, "abc", "(+ 2 3)");
        assert_eq!(s, "(recruit :id 7 :seq 2 :from \"abc\" :instr \"(+ 2 3)\")");
    }

    #[test]
    fn test_recruit_result_round_trip() {
        // Build a canonical envelope by hand, wrap it, parse it back.
        let envelope = crate::sexp::msg_result(crate::sexp::EvalOutcome::Ok {
            stack: &[5],
            output: "",
        });
        let wire = sexp_recruit_result(7, 2, "node-xyz", &envelope);
        // The envelope rides nested as :result's value, not as a quoted string.
        assert!(
            wire.contains(":result (result :ok 1 :value (5) :output \"\")"),
            "wire was: {}",
            wire
        );
        let parsed = crate::sexp::parse(&wire).unwrap();
        let rr = read_recruit_result(&parsed).unwrap();
        assert_eq!(rr.goal_id, 7);
        assert_eq!(rr.seq, 2);
        assert_eq!(rr.from, "node-xyz");
        assert_eq!(
            rr.result,
            crate::sexp::ResultView::Ok {
                value: vec![5],
                output: String::new()
            }
        );
    }

    #[test]
    fn test_read_recruit_result_rejects_other() {
        // A legacy sub-result must not parse as a recruit-result.
        let other =
            crate::sexp::parse("(sub-result :id 1 :seq 0 :from \"x\" :result \"5\")").unwrap();
        assert!(read_recruit_result(&other).is_none());
    }

    // --- (parallel ...) shape + job (pure; no VM) ---

    #[test]
    fn test_parallel_parts_extracts_in_order() {
        let s = crate::sexp::parse("(parallel (+ 1 1) (* 2 3))").unwrap();
        let parts = parallel_parts(&s).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].to_string(), "(+ 1 1)");
        assert_eq!(parts[1].to_string(), "(* 2 3)");
    }

    #[test]
    fn test_parallel_parts_rejects_non_parallel() {
        let s = crate::sexp::parse("(+ 2 3)").unwrap();
        assert!(parallel_parts(&s).is_none());
    }

    #[test]
    fn test_parallel_job_assemble_pending_then_complete() {
        let env0 = crate::sexp::msg_result(crate::sexp::EvalOutcome::Ok {
            stack: &[5],
            output: "",
        });
        let mut job = ParallelJob::new(7, 2);
        job.set(0, env0);
        // Slot 1 still pending -> not complete, overall :ok 0.
        assert!(!job.is_complete());
        let asm = job.assemble();
        assert_eq!(asm.get_key(":ok").and_then(|s| s.as_number()), Some(0));

        let env1 = crate::sexp::msg_result(crate::sexp::EvalOutcome::Ok {
            stack: &[6],
            output: "",
        });
        job.set(1, env1);
        assert!(job.is_complete());
        let asm2 = job.assemble();
        assert_eq!(asm2.get_key(":ok").and_then(|s| s.as_number()), Some(1));
        let results = asm2.get_key(":results").and_then(|s| s.as_list()).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_parallel_job_ok_zero_if_any_part_failed() {
        let ok = crate::sexp::msg_result(crate::sexp::EvalOutcome::Ok {
            stack: &[5],
            output: "",
        });
        let err = crate::sexp::msg_result(crate::sexp::EvalOutcome::Err {
            kind: "runtime",
            msg: "stack underflow",
        });
        let mut job = ParallelJob::new(1, 2);
        job.set(0, ok);
        job.set(1, err);
        assert!(job.is_complete());
        assert_eq!(
            job.assemble().get_key(":ok").and_then(|s| s.as_number()),
            Some(0)
        );
    }

    #[test]
    fn test_local_fallback() {
        let mut eng = DistEngine::new();
        let id = eng.create_goal(
            vec!["1 2 +".into(), "3 4 +".into()],
            "self",
            &[], // no peers
        );
        // All should be local
        let local = eng.pending_local_subgoals(id);
        assert_eq!(local.len(), 2);
        assert!(eng.pending_remote_subgoals(id).is_empty());
    }

    #[test]
    fn test_round_robin() {
        let mut eng = DistEngine::new();
        let peers = vec!["aaa".into(), "bbb".into()];
        let id = eng.create_goal(
            vec![
                "a".into(),
                "b".into(),
                "c".into(),
                "d".into(),
                "e".into(),
                "f".into(),
            ],
            "self",
            &peers,
        );
        let goal = &eng.goals[&id];
        // 3 workers: local, aaa, bbb → round robin
        assert_eq!(goal.sub_goals[0].assigned_to, "local");
        assert_eq!(goal.sub_goals[1].assigned_to, "aaa");
        assert_eq!(goal.sub_goals[2].assigned_to, "bbb");
        assert_eq!(goal.sub_goals[3].assigned_to, "local");
        assert_eq!(goal.sub_goals[4].assigned_to, "aaa");
        assert_eq!(goal.sub_goals[5].assigned_to, "bbb");
    }

    #[test]
    fn test_result_collection() {
        let mut eng = DistEngine::new();
        let id = eng.create_goal(vec!["a".into(), "b".into()], "self", &[]);
        assert!(!eng.is_complete(id));
        eng.record_result(id, 0, "100");
        assert!(!eng.is_complete(id));
        eng.record_result(id, 1, "200");
        assert!(eng.is_complete(id));
    }

    #[test]
    fn test_combine_list() {
        let mut eng = DistEngine::new();
        let id = eng.create_goal(vec!["a".into(), "b".into(), "c".into()], "self", &[]);
        eng.record_result(id, 0, "100");
        eng.record_result(id, 1, "200");
        eng.record_result(id, 2, "300");
        assert_eq!(eng.combine_results(id), Some("100 200 300".into()));
    }

    #[test]
    fn test_combine_sum() {
        let mut eng = DistEngine::new();
        let id = eng.create_goal(vec!["a".into(), "b".into()], "self", &[]);
        eng.goals.get_mut(&id).unwrap().combiner = Combiner::Sum;
        eng.record_result(id, 0, "100");
        eng.record_result(id, 1, "200");
        assert_eq!(eng.combine_results(id), Some("300".into()));
    }

    #[test]
    fn test_timeout_detection() {
        let mut eng = DistEngine::new();
        eng.timeout_ticks = 5;
        let peers = vec!["aaa".into()];
        let id = eng.create_goal(vec!["a".into(), "b".into()], "self", &peers);
        // Advance ticks past timeout
        for _ in 0..10 {
            eng.advance_tick();
        }
        let timed_out = eng.timed_out_subgoals(id);
        // Only the remote one (assigned to "aaa") should time out
        assert_eq!(timed_out.len(), 1);
        assert_eq!(timed_out[0].0, 1); // seq 1 was assigned to aaa
    }

    #[test]
    fn test_fallback_to_local() {
        let mut eng = DistEngine::new();
        eng.timeout_ticks = 5;
        let peers = vec!["aaa".into()];
        let id = eng.create_goal(vec!["x".into(), "y".into()], "self", &peers);
        for _ in 0..10 {
            eng.advance_tick();
        }
        eng.fallback_to_local(id, 1);
        let goal = &eng.goals[&id];
        assert_eq!(goal.sub_goals[1].assigned_to, "local");
    }

    #[test]
    fn test_sexp_messages() {
        let sg = sexp_sub_goal(42, 0, "aaa", "10 10 *");
        assert!(sg.contains("sub-goal"));
        assert!(sg.contains(":id 42"));
        assert!(sg.contains(":expr \"10 10 *\""));

        let sr = sexp_sub_result(42, 0, "bbb", "100");
        assert!(sr.contains("sub-result"));
        assert!(sr.contains(":result \"100\""));
    }

    #[test]
    fn test_single_subgoal() {
        let mut eng = DistEngine::new();
        let id = eng.create_goal(vec!["42 .".into()], "self", &[]);
        eng.record_result(id, 0, "42");
        assert!(eng.is_complete(id));
        assert_eq!(eng.combine_results(id), Some("42".into()));
    }

    #[test]
    fn test_empty_expressions() {
        let exprs = parse_pipe_expressions("");
        assert!(exprs.is_empty());
    }
}
