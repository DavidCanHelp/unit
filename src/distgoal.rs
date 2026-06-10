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
#[derive(Debug, Clone, PartialEq)]
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

/// Job timeout for an alive-but-wedged recruit. Gossip-death (PEER_TIMEOUT,
/// 15s) catches a crashed holder; this catches one that still heartbeats but
/// never replies. Fixed, not scaled to the instruction: a healthy worker's
/// flat eval is already wall-clock bounded on the worker side
/// (`execution_timeout`, default 10s), so the recruiter needs no cost model.
/// The one thing that legitimately stretches past this is a nested
/// `(parallel ...)` whose reply defers until its own recruits finish — a
/// deep-but-healthy subtree CAN exceed 60s of silence and be redundantly
/// re-recruited. That is correct under first-write-wins result collection
/// (see `ParallelJob::set` / `RecruitLedger::collect`), at the cost of
/// duplicating the whole subtree; each recruiter in the tree supervises its
/// own edge with this same constant, so recovery stays local to the wedged
/// edge rather than restarting from the root.
pub const RECRUIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Wall-clock for slot age. `None` on wasm32: `Instant::now()` traps there,
/// and there is no mesh on wasm so supervision never runs — a slot that
/// can't age simply never times out.
#[cfg(not(target_arch = "wasm32"))]
fn slot_now() -> Option<std::time::Instant> {
    Some(std::time::Instant::now())
}
#[cfg(target_arch = "wasm32")]
fn slot_now() -> Option<std::time::Instant> {
    None
}

/// One recruit slot. Holds the sub-instruction (so the work can be re-recruited
/// if the peer dies — mirrors confirm-before-release: don't discard the work
/// until a result is confirmed), the peer currently holding it, and the
/// collected result (`None` while pending). The instruction is released once
/// the slot settles.
#[derive(Debug, Clone)]
pub struct LedgerSlot {
    pub instr: String,
    pub peer: String,
    pub result: Option<RecruitResult>,
    /// When this slot was last (re)assigned — the job-timeout clock. Set on
    /// `open`, refreshed by `reassign` (so the timeout is per-attempt) and by
    /// `touch` (fail-closed deadline reset when no candidate exists).
    pub assigned_at: Option<std::time::Instant>,
    /// How many times this slot has been re-recruited (gossip-death or job
    /// timeout). Pure observability — surfaced by RECRUITS so re-recruit
    /// cycling between wedged peers is visible on hardware; no cap in v1.
    pub reassignments: u32,
    /// How many times the job timeout expired with NO candidate to re-recruit
    /// to (the fail-closed `touch` path). Pure observability, surfaced by
    /// RECRUITS: without it, "timeout pass firing and failing closed every
    /// 60s" and "timeout pass not firing at all" render identically as a bare
    /// pending slot — indistinguishable on hardware.
    pub deadline_resets: u32,
}

/// Recruiter-side ledger binding outstanding recruit requests to their
/// collected results, keyed by `(goal_id, seq)`. Each open slot also retains
/// the sub-instruction and the holding peer, which is the state supervision
/// reads to re-recruit a slot whose peer has died.
#[derive(Debug, Default)]
pub struct RecruitLedger {
    entries: HashMap<(GoalId, usize), LedgerSlot>,
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

    /// Open a slot, recording the sub-instruction and the peer it was sent to.
    pub fn open(&mut self, goal_id: GoalId, seq: usize, instr: &str, peer: &str) {
        self.entries.insert(
            (goal_id, seq),
            LedgerSlot {
                instr: instr.to_string(),
                peer: peer.to_string(),
                result: None,
                assigned_at: slot_now(),
                reassignments: 0,
                deadline_resets: 0,
            },
        );
    }

    /// Record a collected reply if it matches an outstanding request. Releases
    /// the retained instruction (the slot is settled — no re-recruit needed).
    /// FIRST-WRITE-WINS: a slot that already holds a result keeps it. After a
    /// timeout re-recruit the same `(goal_id, seq)` can legitimately execute
    /// twice; results are judged by identity, not sender — whichever reply
    /// lands first settles the slot, and the duplicate is dropped here.
    /// Returns true only when this reply settled the slot; false otherwise (so
    /// cross-node broadcasts that aren't ours, and duplicates, are ignored).
    pub fn collect(&mut self, rr: RecruitResult) -> bool {
        let key = (rr.goal_id, rr.seq);
        if let Some(slot) = self.entries.get_mut(&key) {
            if slot.result.is_some() {
                return false; // already settled — duplicate reply dropped
            }
            slot.result = Some(rr);
            slot.instr.clear();
            true
        } else {
            false
        }
    }

    /// Reassign an OPEN slot to a new peer (re-recruit). Keeps the instruction,
    /// restarts the job-timeout clock (the timeout is per-attempt, not
    /// per-slot-lifetime), and counts the reassignment for observability.
    pub fn reassign(&mut self, goal_id: GoalId, seq: usize, new_peer: &str) {
        if let Some(slot) = self.entries.get_mut(&(goal_id, seq)) {
            slot.peer = new_peer.to_string();
            slot.assigned_at = slot_now();
            slot.reassignments += 1;
        }
    }

    /// Restart an OPEN slot's job-timeout clock without reassigning it —
    /// the fail-closed path when a slot expired but no other peer has
    /// headroom: keep waiting on the current holder rather than fabricating
    /// progress, and check again a full timeout from now. Counts the reset
    /// (observability — see `deadline_resets`) and returns the new count so
    /// the caller can log it.
    pub fn touch(&mut self, goal_id: GoalId, seq: usize) -> u32 {
        if let Some(slot) = self.entries.get_mut(&(goal_id, seq)) {
            slot.assigned_at = slot_now();
            slot.deadline_resets += 1;
            slot.deadline_resets
        } else {
            0
        }
    }

    /// Settle a slot whose reply carries a NESTED `(parallel-result ...)`
    /// envelope, which `read_result` cannot decode into a flat `ResultView`
    /// (so the normal `collect` path never fires). Without this, a completed
    /// subtree's slot stayed OPEN forever: RECRUITS showed it pending and the
    /// supervision passes would re-recruit already-completed work — every 60s
    /// via the timeout pass, or on holder death. Same first-write-wins rule
    /// as `collect`. The stored view is a synthesized summary; the real
    /// nested envelope lives in the parallel job's slot.
    pub fn settle_nested(&mut self, goal_id: GoalId, seq: usize, from: &str, ok: bool) -> bool {
        if let Some(slot) = self.entries.get_mut(&(goal_id, seq)) {
            if slot.result.is_some() {
                return false; // already settled — duplicate reply dropped
            }
            let result = if ok {
                crate::sexp::ResultView::Ok {
                    value: vec![],
                    output: "<nested parallel-result>".to_string(),
                }
            } else {
                crate::sexp::ResultView::Err {
                    kind: "nested".to_string(),
                    msg: "nested parallel-result reported :ok 0".to_string(),
                }
            };
            slot.result = Some(RecruitResult {
                goal_id,
                seq,
                from: from.to_string(),
                result,
            });
            slot.instr.clear();
            true
        } else {
            false
        }
    }

    /// The collected result for a slot, if its reply has arrived.
    pub fn get(&self, goal_id: GoalId, seq: usize) -> Option<&RecruitResult> {
        self.entries.get(&(goal_id, seq)).and_then(|s| s.result.as_ref())
    }

    /// True if the slot was opened but its reply hasn't arrived yet.
    pub fn is_pending(&self, goal_id: GoalId, seq: usize) -> bool {
        matches!(self.entries.get(&(goal_id, seq)), Some(s) if s.result.is_none())
    }

    /// The peer currently holding a slot, if the slot exists.
    pub fn holder(&self, goal_id: GoalId, seq: usize) -> Option<&str> {
        self.entries.get(&(goal_id, seq)).map(|s| s.peer.as_str())
    }

    /// The retained sub-instruction for an OPEN slot; `None` once the slot has
    /// settled (the instruction is released after the result is confirmed).
    pub fn pending_instr(&self, goal_id: GoalId, seq: usize) -> Option<&str> {
        let slot = self.entries.get(&(goal_id, seq))?;
        if slot.result.is_none() && !slot.instr.is_empty() {
            Some(&slot.instr)
        } else {
            None
        }
    }

    /// Open slots whose holding peer has left the live set — the gossip-death
    /// signal. Returns `(goal_id, seq, instruction)` so the caller can
    /// re-recruit the retained instruction to a different peer.
    pub fn open_slots_with_dead_holder(
        &self,
        live: &std::collections::HashSet<&str>,
    ) -> Vec<(GoalId, usize, String)> {
        self.entries
            .iter()
            .filter(|(_, s)| {
                s.result.is_none() && !s.peer.is_empty() && !live.contains(s.peer.as_str())
            })
            .map(|((g, seq), s)| (*g, *seq, s.instr.clone()))
            .collect()
    }

    /// Open slots whose holder is STILL LIVE but has been silent past `age` —
    /// the alive-but-wedged signal. Dead holders are excluded: they belong to
    /// `open_slots_with_dead_holder` (the gossip-death pass runs first and
    /// must not race this one on the same slot). Returns
    /// `(goal_id, seq, instruction, wedged_peer)` — the holder is included so
    /// the caller can exclude it from re-recruit candidates (unlike a dead
    /// peer, a wedged one is still in the live view and could be re-chosen).
    pub fn open_slots_past_deadline(
        &self,
        age: std::time::Duration,
        live: &std::collections::HashSet<&str>,
    ) -> Vec<(GoalId, usize, String, String)> {
        self.entries
            .iter()
            .filter(|(_, s)| {
                s.result.is_none()
                    && !s.peer.is_empty()
                    && live.contains(s.peer.as_str())
                    && s.assigned_at.is_some_and(|t| t.elapsed() >= age)
            })
            .map(|((g, seq), s)| (*g, *seq, s.instr.clone(), s.peer.clone()))
            .collect()
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
            let slot = &self.entries[&key];
            // Supervision history — visible here so re-recruit cycling and
            // fail-closed timeout expiries both show up on hardware. Without
            // the reset count, a fail-closed pass firing every 60s and a pass
            // not firing at all rendered identically.
            let mut re = String::new();
            if slot.reassignments > 0 {
                re.push_str(&format!(" (re-recruited {}x)", slot.reassignments));
            }
            if slot.deadline_resets > 0 {
                re.push_str(&format!(" (deadline reset {}x)", slot.deadline_resets));
            }
            match &slot.result {
                None => out.push_str(&format!(
                    "recruit #{} seq {} -> {}: pending{}\n",
                    g, s, slot.peer, re
                )),
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
                        "recruit #{} seq {} from {}: {}{}\n",
                        g, s, rr.from, body, re
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

/// Estimated committed memory cost of a parallel sub-part, in MiB, for
/// run_parallel's committed-work accounting. Known only for the `(alloc-mb N)`
/// load generator (cost = N). Every other part — arithmetic, nested parallels,
/// anything without a cost model — contributes 0: we do not invent a number, so
/// such parts fall back to observed-measure-only admission as before.
pub fn part_cost_mb(part: &crate::sexp::Sexp) -> u64 {
    if let Some(items) = part.as_list() {
        if items.len() == 2 {
            if let (Some("alloc-mb"), Some(n)) = (items[0].as_atom(), items[1].as_number()) {
                return n.max(0) as u64;
            }
        }
    }
    0
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
    /// range). FIRST-WRITE-WINS: a filled slot keeps its envelope — after a
    /// timeout re-recruit, both the wedged original and its replacement may
    /// reply for the same seq; whichever lands first is the result, and the
    /// duplicate is dropped here. Returns true only if this call filled the
    /// slot, so the reply path can treat duplicates as silent no-ops instead
    /// of re-running completion.
    pub fn set(&mut self, seq: usize, envelope: crate::sexp::Sexp) -> bool {
        if let Some(slot) = self.slots.get_mut(seq) {
            if slot.is_none() {
                *slot = Some(envelope);
                return true;
            }
        }
        false
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

    // --- RecruitLedger job-timeout state ---

    fn live<'a>(peers: &[&'a str]) -> std::collections::HashSet<&'a str> {
        peers.iter().copied().collect()
    }

    fn ok_reply(goal_id: GoalId, seq: usize, from: &str, value: i64) -> RecruitResult {
        let envelope = crate::sexp::msg_result(crate::sexp::EvalOutcome::Ok {
            stack: &[value],
            output: "",
        });
        let wrapped = sexp_recruit_result(goal_id, seq, from, &envelope);
        read_recruit_result(&crate::sexp::parse(&wrapped).unwrap()).unwrap()
    }

    #[test]
    fn test_deadline_query_fires_only_on_live_stale_holders() {
        let mut ledger = RecruitLedger::new();
        ledger.open(1, 0, "(+ 1 1)", "W"); // live + (with age=0) stale -> fires
        ledger.open(1, 1, "(+ 2 2)", "D"); // holder dead -> the death pass's job
        ledger.open(1, 2, "(+ 3 3)", "S"); // will be settled -> never fires
        assert!(ledger.collect(ok_reply(1, 2, "S", 6)));

        let hits = ledger.open_slots_past_deadline(std::time::Duration::ZERO, &live(&["W", "S"]));
        assert_eq!(hits.len(), 1, "only the live stale slot fires: {hits:?}");
        let (gid, seq, instr, wedged) = &hits[0];
        assert_eq!((*gid, *seq), (1, 0));
        assert_eq!(instr, "(+ 1 1)");
        assert_eq!(wedged, "W");

        // A generous deadline fires on nothing — the slots are fresh.
        assert!(ledger
            .open_slots_past_deadline(RECRUIT_TIMEOUT, &live(&["W", "S"]))
            .is_empty());
    }

    #[test]
    fn test_reassign_restarts_deadline_and_counts() {
        let mut ledger = RecruitLedger::new();
        ledger.open(2, 0, "(* 6 7)", "W");
        assert_eq!(ledger.entries[&(2, 0)].reassignments, 0);

        ledger.reassign(2, 0, "Q");
        // Counter is observability for re-recruit cycling (no cap in v1).
        assert_eq!(ledger.entries[&(2, 0)].reassignments, 1);
        assert_eq!(ledger.holder(2, 0), Some("Q"));
        // The clock restarted: a fresh assignment is within any real deadline.
        assert!(ledger
            .open_slots_past_deadline(RECRUIT_TIMEOUT, &live(&["Q"]))
            .is_empty());
        // The instruction survives reassignment (it may be re-sent again).
        assert_eq!(ledger.pending_instr(2, 0), Some("(* 6 7)"));
    }

    #[test]
    fn test_touch_restarts_deadline_without_counting() {
        let mut ledger = RecruitLedger::new();
        ledger.open(3, 0, "(+ 1 2)", "W");
        ledger.touch(3, 0);
        assert_eq!(ledger.entries[&(3, 0)].reassignments, 0);
        assert_eq!(ledger.holder(3, 0), Some("W"), "touch never reassigns");
    }

    #[test]
    fn test_touch_counts_deadline_resets_and_status_shows_them() {
        // The fail-closed path must be distinguishable from "pass never
        // fired": each touch counts, and RECRUITS renders the count.
        let mut ledger = RecruitLedger::new();
        ledger.open(7, 0, "(+ 1 2)", "W");
        assert!(!ledger.format_status().contains("deadline reset"));
        assert_eq!(ledger.touch(7, 0), 1);
        assert_eq!(ledger.touch(7, 0), 2);
        assert_eq!(ledger.entries[&(7, 0)].deadline_resets, 2);
        let status = ledger.format_status();
        assert!(
            status.contains("deadline reset 2x"),
            "RECRUITS must show fail-closed expiries: {status}"
        );
        assert!(
            !status.contains("re-recruited"),
            "touches are not reassignments: {status}"
        );
    }

    #[test]
    fn test_settle_nested_settles_slot_first_write_wins() {
        // A nested parallel-result reply can't decode into a flat ResultView;
        // settle_nested must still close the ledger slot so supervision stops
        // watching completed work.
        let mut ledger = RecruitLedger::new();
        ledger.open(8, 0, "(parallel (+ 1 1) (+ 2 2))", "W");
        assert!(ledger.settle_nested(8, 0, "W", true));
        assert!(!ledger.is_pending(8, 0), "slot settled");
        assert_eq!(ledger.pending_instr(8, 0), None, "instruction released");
        // First-write-wins: duplicates (re-recruit race) are dropped.
        assert!(!ledger.settle_nested(8, 0, "Q", true));
        assert!(!ledger.collect(ok_reply(8, 0, "Q", 999)));
        assert_eq!(ledger.get(8, 0).unwrap().from, "W");
        // And it never fires either supervision query again.
        assert!(ledger
            .open_slots_past_deadline(std::time::Duration::ZERO, &live(&["W"]))
            .is_empty());
        assert!(ledger
            .open_slots_with_dead_holder(&live(&["Q"]))
            .is_empty());
    }

    #[test]
    fn test_collect_is_first_write_wins() {
        let mut ledger = RecruitLedger::new();
        ledger.open(4, 0, "(+ 2 2)", "W");
        // After a timeout re-recruit both W and its replacement may reply for
        // the same (goal_id, seq). Whichever lands first settles the slot.
        assert!(ledger.collect(ok_reply(4, 0, "W", 4)));
        assert!(
            !ledger.collect(ok_reply(4, 0, "Q", 999)),
            "duplicate reply must be dropped, not collected"
        );
        let kept = ledger.get(4, 0).unwrap();
        assert_eq!(kept.from, "W", "first reply wins regardless of sender");
    }

    #[test]
    fn test_parallel_job_set_is_first_write_wins() {
        let mut job = ParallelJob::new(5, 1);
        let first = crate::sexp::msg_result(crate::sexp::EvalOutcome::Ok {
            stack: &[4],
            output: "",
        });
        let second = crate::sexp::msg_result(crate::sexp::EvalOutcome::Ok {
            stack: &[999],
            output: "",
        });
        job.set(0, first);
        job.set(0, second);
        let assembled = job.assemble();
        let results = assembled.get_key(":results").and_then(|s| s.as_list()).unwrap();
        let v = results[0]
            .get_key(":value")
            .and_then(|s| s.as_list())
            .and_then(|l| l[0].as_number());
        assert_eq!(v, Some(4), "the first envelope is kept");
    }

    #[test]
    fn test_format_status_surfaces_reassignments() {
        let mut ledger = RecruitLedger::new();
        ledger.open(6, 0, "(+ 1 1)", "W");
        assert!(!ledger.format_status().contains("re-recruited"));
        ledger.reassign(6, 0, "Q");
        ledger.reassign(6, 0, "R");
        let status = ledger.format_status();
        assert!(
            status.contains("re-recruited 2x"),
            "RECRUITS must show the counter: {status}"
        );
    }

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
