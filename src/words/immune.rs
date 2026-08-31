//! Immune-system primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Immune system primitives
    // -----------------------------------------------------------------------

    pub(crate) fn prim_challenges(&mut self) {
        let out = self.challenge_registry.format_challenges();
        self.emit_str(&out);
    }

    pub(crate) fn prim_immune_status(&mut self) {
        let total = self.challenge_registry.challenges.len();
        let solved = self
            .challenge_registry
            .challenges
            .values()
            .filter(|c| c.solved)
            .count();
        let unsolved = total - solved;
        let antibodies = self
            .dictionary
            .iter()
            .filter(|e| e.name.starts_with("SOL-"))
            .count();
        self.emit_str(&format!(
            "--- immune status ---\nchallenges: {} ({} solved, {} unsolved)\n\
             colony antibodies: {}\n",
            total, solved, unsolved, antibodies
        ));
        if let Some(active) = self.challenge_registry.active() {
            self.emit_str(&format!("active: #{} {}\n", active.id, active.name));
        }
        // List antibody words
        let sol_words: Vec<&str> = self
            .dictionary
            .iter()
            .filter(|e| e.name.starts_with("SOL-"))
            .map(|e| e.name.as_str())
            .collect();
        if !sol_words.is_empty() {
            self.emit_str(&format!("  words: {}\n", sol_words.join(" ")));
        }
    }

    pub(crate) fn prim_antibodies(&mut self) {
        let sol_words: Vec<String> = self
            .dictionary
            .iter()
            .filter(|e| e.name.starts_with("SOL-"))
            .map(|e| e.name.clone())
            .collect();
        if sol_words.is_empty() {
            self.emit_str("no antibodies yet\n");
        } else {
            self.emit_str(&format!("--- {} antibodies ---\n", sol_words.len()));
            for name in &sol_words {
                self.emit_str(&format!("  {}\n", name));
            }
        }
    }

    pub(crate) fn prim_metabolism(&mut self) {
        let out = format!(
            "--- metabolism ---\n\
             energy: {}/{}\n\
             lifetime earned: {}\n\
             lifetime spent: {}\n\
             efficiency: {:.2}\n\
             peak energy: {}\n\
             starving ticks: {}\n\
             throttled: {}\n\
             --- costs ---\n\
             \x20 spawn: {}\n\
             \x20 gp generation: {}\n\
             \x20 eval per 1000 steps: {}\n\
             \x20 mesh send: {}\n\
             --- rewards ---\n\
             \x20 task success: {}\n\
             \x20 challenge solved: {}\n\
             \x20 passive regen: {}/tick\n",
            self.energy.energy,
            self.energy.max_energy,
            self.energy.total_earned,
            self.energy.total_spent,
            self.energy.efficiency(),
            self.energy.peak_energy,
            self.energy.starving_ticks,
            if self.energy.throttled { "YES" } else { "no" },
            energy::SPAWN_COST,
            energy::GP_GENERATION_COST,
            energy::EVAL_STEP_COST_PER_1000,
            energy::MESH_SEND_COST,
            energy::TASK_REWARD,
            energy::CHALLENGE_SOLVE_REWARD,
            energy::PASSIVE_REGEN,
        );
        self.emit_str(&out);
    }

    /// Generate harder challenges from a solved one via the landscape engine.
    pub(crate) fn generate_landscape_challenges(&mut self, challenge_name: &str, solution: &str) {
        // Find the solved challenge by name.
        let solved = self
            .challenge_registry
            .challenges
            .values()
            .find(|c| c.name == challenge_name && c.solved)
            .cloned();
        let solved = match solved {
            Some(c) => c,
            None => return,
        };
        let all_solved: Vec<&challenges::Challenge> = self
            .challenge_registry
            .challenges
            .values()
            .filter(|c| c.solved)
            .collect();
        let new_challenges = self
            .landscape
            .on_challenge_solved(&solved, solution, &all_solved);
        if new_challenges.is_empty() {
            return;
        }
        let depth = self.landscape.depth();
        let my_id = self.node_id_cache.unwrap_or([0; 8]);
        // Dedupe by NAME against everything already known (solved or not):
        // generators re-derive the same rungs from related parents, and
        // re-registering them made the registry grow without bound — each
        // solve of a clone generated more clones (observed: a second
        // fib10-short9 at reward 140, a second fib15 at 170, forever).
        // Unbounded per-unit registry growth × colony size was a real slice
        // of the memory-creep open problem.
        let known: std::collections::HashSet<String> = self
            .challenge_registry
            .challenges
            .values()
            .map(|c| c.name.clone())
            .collect();
        let new_challenges: Vec<_> = new_challenges
            .into_iter()
            .filter(|ch| !known.contains(&ch.name))
            .collect();
        if new_challenges.is_empty() {
            return;
        }
        let count = new_challenges.len();
        for ch in new_challenges {
            let id = self.challenge_registry.register_discovered(
                &ch.name,
                &ch.description,
                &ch.target_output,
                ch.test_input.clone(),
                ch.seed_programs.clone(),
                my_id,
                ch.reward,
            );
            // Broadcast to mesh.
            if let Some(ref m) = self.mesh {
                if let Some(registered) = self.challenge_registry.get_challenge(id) {
                    let sexp = challenges::sexp_challenge_broadcast(registered);
                    m.send_sexp(&sexp);
                }
            }
        }
        self.emit_str(&format!(
            "[landscape] depth {}: generated {} new challenges from '{}'\n",
            depth, count, challenge_name
        ));
    }

    /// Install a solved challenge as a dictionary word (sol-{name}).
    pub(crate) fn install_solution(&mut self, challenge_name: &str, program: &str) {
        let word_name = format!("SOL-{}", challenge_name.to_uppercase());
        // Check if already installed.
        if self.find_word(&word_name).is_some() {
            return;
        }
        let def = format!(": {} {} ;", word_name, program);
        self.interpret_line(&def);
        self.energy
            .earn(energy::CHALLENGE_SOLVE_REWARD, "challenge-solved");
        self.emit_str(&format!("[immune] learned word: {}\n", word_name));
    }

    /// Absorb a dead unit's bequeathed antibodies: install each `SOL-*`
    /// word this unit lacks by evaluating its source. The trust gate —
    /// `SOL-*` names only, never overwriting an existing word, bounded
    /// name/source sizes — is enforced here as well as at the death-cry
    /// parse layer, so every caller is safe against forged messages.
    /// Returns how many words were installed.
    pub(crate) fn absorb_antibodies(&mut self, antibodies: &[(String, String)]) -> usize {
        let mut installed = 0;
        for (name, source) in antibodies
            .iter()
            .take(crate::sexp::DEATH_CRY_MAX_ANTIBODIES)
        {
            if !name.starts_with("SOL-")
                || name.len() > 64
                || source.len() > crate::sexp::DEATH_CRY_MAX_SOURCE_LEN
                || self.find_word(name).is_some()
            {
                continue;
            }
            let saved_buf = self.input_buffer.clone();
            let saved_pos = self.input_pos;
            let saved_silent = self.silent;
            self.silent = true;
            self.interpret_line(source);
            self.silent = saved_silent;
            self.input_buffer = saved_buf;
            self.input_pos = saved_pos;
            if self.find_word(name).is_some() {
                installed += 1;
            }
        }
        installed
    }

    /// Called during REPL tick to check for incoming sub-goal results and timeouts.
    pub(crate) fn tick_dist_goals(&mut self) {
        let _t_tick = metrics::Timer::new("mesh.tick");
        self.dist_engine.advance_tick();

        // Process incoming S-expression messages for sub-results.
        if let Some(ref m) = self.mesh {
            let msgs = m.recv_sexp_messages();
            for msg in &msgs {
                self.process_chatter_msg(msg);
            }
        }

        // Supervision: the gossip-death pass, then the job-timeout pass —
        // UNCONDITIONALLY, even (especially) on an empty live view: after the
        // only holding peer is evicted from the view (e.g. SIGSTOPped past
        // the peer timeout), its slots classify as dead-held with no
        // candidates and the attempt cap terminally bounds the wait. The old
        // `if !live_peers.is_empty()` skip left exactly that state
        // supervised by nothing. Abandonment self-reports flow up the tree
        // through the same result path a real reply takes.
        let live_peers: Vec<(String, u8, std::net::SocketAddr)> = match self.mesh {
            Some(ref m) => m
                .peer_resource_view()
                .into_iter()
                .map(|(id, _load, headroom, addr)| (crate::mesh::id_to_hex(&id), headroom, addr))
                .collect(),
            None => Vec::new(),
        };
        if self.mesh.is_some() {
            for (node, msg) in self.supervise(&live_peers, distgoal::recruit_timeout()) {
                self.send_to_node(&node, &msg);
            }
        }

        // Check for timed-out sub-goals and fall back to local.
        let goal_ids: Vec<u64> = self.dist_engine.goals.keys().copied().collect();
        for gid in goal_ids {
            let timed_out = self.dist_engine.timed_out_subgoals(gid);
            for (seq, expr) in timed_out {
                self.dist_engine.fallback_to_local(gid, seq);
                let result = self.execute_sandbox(&expr);
                let output = result.output.trim().to_string();
                self.dist_engine.record_result(gid, seq, &output);
                self.emit_str(&format!(
                    "(fallback: computed sub-goal {} locally — peer timeout)\n",
                    seq
                ));
                if self.dist_engine.is_complete(gid) {
                    if let Some(combined) = self.dist_engine.combine_results(gid) {
                        self.emit_str(&format!("dist-goal #{} complete: {}\n", gid, combined));
                    }
                }
            }
        }
    }

    /// Handle an inbound recruit instruction: evaluate the s-expression `instr`
    /// through the `eval_sexp` seam and build the `(recruit-result ...)` reply
    /// carrying the canonical envelope under the `:id`/`:seq`/`:from` routing
    /// wrapper. Returns the reply string (the caller sends it); kept free of the
    /// mesh send so it is testable without sockets. Unlike the legacy sub-goal
    /// path, success/error is preserved — a failed eval yields a visible
    /// `:ok 0` envelope rather than a silently-trimmed output string.
    pub(crate) fn handle_recruit<M>(
        &mut self,
        goal_id: u64,
        seq: usize,
        instr: &str,
        recruiter: &str,
        measure: &mut M,
    ) -> crate::distgoal::RecruitOutcome
    where
        M: FnMut() -> crate::resources::HostResources,
    {
        let my_id = self
            .node_id_cache
            .map(|id| crate::mesh::id_to_hex(&id))
            .unwrap_or_else(|| "local".to_string());
        let reply = |env: &crate::sexp::Sexp| {
            crate::distgoal::RecruitOutcome::Reply(distgoal::sexp_recruit_result(
                goal_id, seq, &my_id, env,
            ))
        };
        match crate::sexp::parse(instr) {
            // RECURSION: a recruited (parallel ...) re-applies the SAME
            // split-and-recruit DECISION at THIS level — it routes through
            // run_parallel, whose per-part `measure().has_headroom()` check (the
            // ceiling brake) runs again here. The reply path then splits:
            Ok(sexp) => match crate::distgoal::parallel_parts(&sexp) {
                Some(parts) => {
                    let budget_kb = crate::resources::measure_mem_budget_kb();
                    let child = self.run_parallel(&parts, measure, budget_kb);
                    let complete = self
                        .parallel_jobs
                        .get(&child)
                        .map(|j| j.is_complete())
                        .unwrap_or(true);
                    if complete {
                        // Every part ran locally (had headroom): reply NOW with
                        // the complete envelope, and drop the settled job.
                        let env = self.parallel_result(child).unwrap_or_else(|| {
                            crate::sexp::msg_result(crate::sexp::EvalOutcome::Err {
                                kind: "runtime",
                                msg: "parallel job missing",
                            })
                        });
                        self.parallel_jobs.remove(&child);
                        reply(&env)
                    } else {
                        // Recruited overflow: do NOT reply synchronously. Store
                        // who to report back to (the back-reference), keyed by
                        // this child job; completion self-reports later via the
                        // recruit-result handler.
                        self.report_targets.insert(
                            child,
                            crate::distgoal::ReportTarget {
                                recruiter_node: recruiter.to_string(),
                                goal_id,
                                seq,
                            },
                        );
                        crate::distgoal::RecruitOutcome::Deferred {
                            child_goal_id: child,
                        }
                    }
                }
                // Flat instruction: evaluate and reply synchronously, as before.
                None => {
                    let env = crate::sexp::eval_sexp(self, instr);
                    reply(&env)
                }
            },
            Err(e) => reply(&crate::sexp::msg_result(crate::sexp::EvalOutcome::Err {
                kind: "parse",
                msg: &e.0,
            })),
        }
    }

    /// Collect an inbound `(recruit-result ...)`: fill the matching parallel-job
    /// slot (and the recruit ledger, for decodable single results). If that was
    /// the job's LAST open slot, the unit is now complete: assemble its full
    /// result and, if a recruiter back-reference is stored, return the
    /// `(node, message)` self-report to send up the tree. A root job (no
    /// back-reference) surfaces locally and returns None. This is where the
    /// last-slot-fill completion check fires. Returns None when nothing to send.
    pub(crate) fn collect_recruit_result(
        &mut self,
        sexp: &crate::sexp::Sexp,
    ) -> Option<(String, String)> {
        let gid = sexp.get_key(":id")?.as_number()? as u64;
        let seq = sexp.get_key(":seq")?.as_number()? as usize;
        let env = sexp.get_key(":result")?.clone();
        // Record decodable single-result replies in the ledger (generic
        // recruits). A nested parallel-result doesn't decode into a flat
        // ResultView, but its ledger slot MUST still settle — left open, it
        // showed as pending in RECRUITS forever and the supervision passes
        // would re-recruit the already-completed subtree (every 60s via the
        // timeout pass, or on holder death).
        if let Some(rr) = crate::distgoal::read_recruit_result(sexp) {
            self.recruit_ledger.collect(rr);
        } else if crate::sexp::msg_type(&env) == Some("parallel-result") {
            let from = sexp
                .get_key(":from")
                .and_then(|s| s.as_str())
                .unwrap_or("?")
                .to_string();
            let ok = env.get_key(":ok").and_then(|s| s.as_number()) == Some(1);
            self.recruit_ledger.settle_nested(gid, seq, &from, ok);
        }
        self.fill_job_slot(gid, seq, env)
    }

    /// Fill one parallel-job slot with a result envelope and run the
    /// last-slot-fill completion check: if the job is now whole, assemble it
    /// and either return the `(node, message)` self-report for the stored
    /// recruiter back-reference, or surface locally at the root. Shared by
    /// inbound collection (`collect_recruit_result`) and fail-closed
    /// abandonment (`abandon_slot`) — abandonment unwinds a tree through
    /// exactly the same path a real reply would take.
    ///
    /// A duplicate fill for an already-filled slot (possible after a timeout
    /// re-recruit — first write won) is a silent no-op: don't re-assemble or
    /// re-announce a completion that already happened.
    fn fill_job_slot(
        &mut self,
        gid: u64,
        seq: usize,
        env: crate::sexp::Sexp,
    ) -> Option<(String, String)> {
        let full = {
            let job = self.parallel_jobs.get_mut(&gid)?;
            if !job.set(seq, env) {
                return None;
            }
            if job.is_complete() {
                Some(job.assemble())
            } else {
                None
            }
        };
        let full = full?; // not my last slot -> nothing to report yet
        // COMPLETE. Report up to the stored recruiter, or surface if root.
        match self.report_targets.remove(&gid) {
            Some(target) => {
                self.parallel_jobs.remove(&gid); // settled; result has flowed up
                let my_id = self
                    .node_id_cache
                    .map(|id| crate::mesh::id_to_hex(&id))
                    .unwrap_or_else(|| "local".to_string());
                let msg =
                    distgoal::sexp_recruit_result(target.goal_id, target.seq, &my_id, &full);
                Some((target.recruiter_node, msg))
            }
            None => {
                // Root: surface the whole answer to the original asker.
                self.emit_str(&format!("parallel #{} complete: {}\n", gid, full));
                None
            }
        }
    }

    /// Abandon one recruit slot fail-closed (attempt cap reached): settle
    /// the ledger slot with an `abandoned` error, fill the local job slot
    /// with the same error envelope, and — via the shared completion check —
    /// return the upstream self-report if that made the job whole. This is
    /// the terminal bound on every wait in a recruit tree: no fabricated
    /// success, no worker killed; a late real reply is a dropped duplicate.
    fn abandon_slot(&mut self, gid: u64, seq: usize, why: &str) -> Option<(String, String)> {
        if !self.recruit_ledger.abandon(gid, seq, why) {
            return None; // already settled (real reply won the race)
        }
        self.emit_str(&format!(
            "recruit #{} seq {}: ABANDONED — {}\n",
            gid, seq, why
        ));
        let env = crate::sexp::msg_result(crate::sexp::EvalOutcome::Err {
            kind: "abandoned",
            msg: why,
        });
        self.fill_job_slot(gid, seq, env)
    }

    /// Send an s-expression message to a specific peer by hex node id, resolving
    /// its address from the gossiped view. No-op if there is no mesh or the peer
    /// is unknown. Shared by recruit emission and result self-reporting.
    pub(crate) fn send_to_node(&self, node_hex: &str, msg: &str) {
        if let Some(ref m) = self.mesh {
            if let Some((_, _, addr_str)) = m
                .peer_details()
                .into_iter()
                .find(|(hex, _, _)| hex == node_hex)
            {
                if let Ok(addr) = addr_str.parse::<std::net::SocketAddr>() {
                    m.send_sexp_to(addr, msg);
                }
            }
        }
    }

    /// Recruiter side (emit): build a `(recruit ...)` message handing `instr`
    /// to `peer_hex`, record the outstanding request in the ledger, and send it
    /// to that peer over the mesh. Returns the message string so it is testable
    /// without a live mesh. MECHANISM ONLY — the caller (a manual trigger)
    /// supplies the target, goal_id/seq, and instruction; nothing here decides
    /// whether or when to recruit.
    pub(crate) fn send_recruit(
        &mut self,
        peer_hex: &str,
        goal_id: u64,
        seq: usize,
        instr: &str,
    ) -> String {
        let my_id = self
            .node_id_cache
            .map(|id| crate::mesh::id_to_hex(&id))
            .unwrap_or_else(|| "local".to_string());
        let bounty = self.recruit_bounty();
        let msg = distgoal::sexp_recruit(goal_id, seq, &my_id, instr, bounty);
        // Retain the instruction and the holding peer so supervision can
        // re-recruit this slot if `peer_hex` later dies.
        self.recruit_ledger.open(goal_id, seq, instr, peer_hex);
        // Send to just that peer; if it is not a known peer the request still
        // stands in the ledger and no send happens.
        self.send_to_node(peer_hex, &msg);
        msg
    }

    /// SUPERVISION (gossip-death, let-it-crash). Given the current live peer
    /// view (hex id, advertised headroom, addr), re-recruit every OPEN slot
    /// whose holding peer has left that view — its peer is presumed dead. The
    /// retained instruction is re-sent to a DIFFERENT peer chosen by the same
    /// placement logic (`choose_destination`); the dead peer isn't in the view,
    /// so it can't be re-chosen. If no peer has headroom, the slot stays
    /// open/declined (fail-closed — no fabricated completion). This handles
    /// crash/disappearance only; the alive-but-wedged case is the sibling pass
    /// `supervise_recruit_timeouts`. Supervision nests for free: a re-recruited
    /// slot may itself hold a `(parallel ...)` subtree, which the new peer
    /// re-runs normally.
    pub(crate) fn supervise_recruits(
        &mut self,
        live_peers: &[(String, u8, std::net::SocketAddr)],
        timeout: std::time::Duration,
    ) -> Vec<(String, String)> {
        let mut reports = Vec::new();
        let live: std::collections::HashSet<&str> =
            live_peers.iter().map(|(hex, _, _)| hex.as_str()).collect();
        let dead = self.recruit_ledger.open_slots_with_dead_holder(&live);
        if dead.is_empty() {
            return reports;
        }
        let candidates: Vec<crate::transport::Candidate> = live_peers
            .iter()
            .map(|(_, headroom, addr)| crate::transport::Candidate {
                headroom_pct: *headroom,
                addr: *addr,
            })
            .collect();
        for (gid, seq, instr) in dead {
            // Terminal bound first: a slot that has burned its attempt cap
            // is abandoned fail-closed, and the failure flows upstream.
            if self.recruit_ledger.attempts(gid, seq) >= distgoal::MAX_SLOT_ATTEMPTS {
                reports.extend(self.abandon_slot(
                    gid,
                    seq,
                    &format!(
                        "holder dead, no capacity after {} attempts",
                        distgoal::MAX_SLOT_ATTEMPTS
                    ),
                ));
                continue;
            }
            // Choose a new headroom peer (same placement rule). The dead peer is
            // absent from `candidates`, so it cannot be re-chosen.
            let chosen_addr =
                match crate::transport::choose_destination(&candidates, &mut self.rng) {
                    Some(c) => c.addr,
                    None => {
                        // Fail-closed: no capacity. Unlike the timeout pass,
                        // dead-holder classification fires every tick, so pace
                        // the counted reset by the slot's own deadline — one
                        // attempt per timeout window, not one per tick.
                        if self.recruit_ledger.age_expired(gid, seq, timeout) {
                            let resets = self.recruit_ledger.touch(gid, seq);
                            self.emit_str(&format!(
                                "recruit #{} seq {}: holder dead, no candidate with headroom — deadline reset ({}x)\n",
                                gid, seq, resets
                            ));
                        }
                        continue;
                    }
                };
            if let Some((hex, _, _)) = live_peers.iter().find(|(_, _, a)| *a == chosen_addr) {
                let new_peer = hex.clone();
                self.re_recruit(gid, seq, &new_peer, &instr);
            }
        }
        reports
    }

    /// SUPERVISION (job-timeout, alive-but-wedged). Re-recruit every OPEN slot
    /// whose holder is STILL in the live view but has been silent past
    /// `timeout` (production passes `distgoal::RECRUIT_TIMEOUT`; tests inject
    /// their own). The wedged holder is excluded from candidates — unlike a
    /// dead peer it is still live, so `choose_destination` could otherwise
    /// re-pick it. If no other peer has headroom, the deadline is reset and
    /// the slot keeps waiting on its current holder (fail-closed; the wedged
    /// peer may yet finish, and its late reply still routes by identity).
    /// Re-recruiting while the original holder lives means the same
    /// `(goal_id, seq)` can execute twice; first-write-wins collection
    /// (`ParallelJob::set` / `RecruitLedger::collect`) makes the duplicate
    /// harmless. Run AFTER `supervise_recruits` in the same tick: dead-held
    /// slots are either reassigned there (fresh deadline) or skipped here by
    /// the live-containment filter.
    pub(crate) fn supervise_recruit_timeouts(
        &mut self,
        live_peers: &[(String, u8, std::net::SocketAddr)],
        timeout: std::time::Duration,
    ) -> Vec<(String, String)> {
        let mut reports = Vec::new();
        let live: std::collections::HashSet<&str> =
            live_peers.iter().map(|(hex, _, _)| hex.as_str()).collect();
        let expired = self.recruit_ledger.open_slots_past_deadline(timeout, &live);
        for (gid, seq, instr, wedged) in expired {
            // Terminal bound first: attempts (reassignments + fail-closed
            // resets) are capped; at the cap the slot is abandoned and the
            // failure self-reports upstream instead of waiting forever.
            if self.recruit_ledger.attempts(gid, seq) >= distgoal::MAX_SLOT_ATTEMPTS {
                reports.extend(self.abandon_slot(
                    gid,
                    seq,
                    &format!(
                        "held by {} past deadline, attempt cap ({}) reached",
                        wedged,
                        distgoal::MAX_SLOT_ATTEMPTS
                    ),
                ));
                continue;
            }
            // Same placement rule as gossip-death, minus the wedged holder.
            let candidates: Vec<crate::transport::Candidate> = live_peers
                .iter()
                .filter(|(hex, _, _)| *hex != wedged)
                .map(|(_, headroom, addr)| crate::transport::Candidate {
                    headroom_pct: *headroom,
                    addr: *addr,
                })
                .collect();
            let chosen_addr =
                match crate::transport::choose_destination(&candidates, &mut self.rng) {
                    Some(c) => c.addr,
                    None => {
                        // Fail-closed: nowhere better to go. Keep waiting on
                        // the current holder, a full timeout from now — and
                        // SAY so: a silent fail-closed expiry is
                        // indistinguishable from the pass never firing.
                        //
                        // But don't just wait: RE-SEND the retained
                        // instruction to the still-live holder. Recruits are
                        // single UDP datagrams; if the original was lost (the
                        // Docker drill caught a just-booted worker missing
                        // one), a silent holder with no alternative candidate
                        // was previously GUARANTEED to ride every reset to
                        // abandonment. The re-send is idempotent — a worker
                        // that already computed replies again and the
                        // duplicate is dropped first-write-wins — and carries
                        // no fresh bounty (the wage was paid at first
                        // emission).
                        let resets = self.recruit_ledger.touch(gid, seq);
                        let my_id = self
                            .node_id_cache
                            .map(|id| crate::mesh::id_to_hex(&id))
                            .unwrap_or_else(|| "local".to_string());
                        let msg = distgoal::sexp_recruit(gid, seq, &my_id, &instr, 0);
                        self.send_to_node(&wedged, &msg);
                        self.emit_str(&format!(
                            "recruit #{} seq {}: timeout expired, no candidate with headroom — re-sent to holder {}, deadline reset ({}x)\n",
                            gid, seq, wedged, resets
                        ));
                        continue;
                    }
                };
            if let Some((hex, _, _)) = live_peers.iter().find(|(_, _, a)| *a == chosen_addr) {
                let new_peer = hex.clone();
                self.re_recruit(gid, seq, &new_peer, &instr);
            }
        }
        reports
    }

    /// One full supervision sweep: the gossip-death pass, then the
    /// job-timeout pass — run UNCONDITIONALLY, including on an empty live
    /// view (where every open holder classifies as dead, no candidates
    /// exist, and the attempt cap terminally bounds the wait; the old
    /// behavior of skipping supervision on an empty view left exactly the
    /// only-peer-wedged-then-evicted slot supervised by nothing). Returns
    /// the upstream self-reports produced by any abandonments, for the
    /// caller to send.
    pub(crate) fn supervise(
        &mut self,
        live_peers: &[(String, u8, std::net::SocketAddr)],
        timeout: std::time::Duration,
    ) -> Vec<(String, String)> {
        let mut reports = self.supervise_unplaced(live_peers, timeout);
        reports.extend(self.supervise_recruits(live_peers, timeout));
        reports.extend(self.supervise_recruit_timeouts(live_peers, timeout));
        reports
    }

    /// SUPERVISION (placement, declined parts). A part declined at emission
    /// time — no candidate had headroom — sits as an UNPLACED slot (empty
    /// holder). Each sweep re-attempts placement with the current view:
    /// capacity appeared → recruit it now (counts as an attempt via the
    /// reassignment); still none → a deadline-paced fail-closed reset; cap
    /// reached → abandoned, and the failure flows upstream. Before this
    /// pass, declined parts were invisible to supervision — the one wait in
    /// a recruit tree with no deadline at all.
    fn supervise_unplaced(
        &mut self,
        live_peers: &[(String, u8, std::net::SocketAddr)],
        timeout: std::time::Duration,
    ) -> Vec<(String, String)> {
        let mut reports = Vec::new();
        for (gid, seq, instr) in self.recruit_ledger.open_slots_unplaced() {
            if self.recruit_ledger.attempts(gid, seq) >= distgoal::MAX_SLOT_ATTEMPTS {
                reports.extend(self.abandon_slot(
                    gid,
                    seq,
                    &format!(
                        "never placed — no capacity after {} attempts",
                        distgoal::MAX_SLOT_ATTEMPTS
                    ),
                ));
                continue;
            }
            let candidates: Vec<crate::transport::Candidate> = live_peers
                .iter()
                .map(|(_, headroom, addr)| crate::transport::Candidate {
                    headroom_pct: *headroom,
                    addr: *addr,
                })
                .collect();
            match crate::transport::choose_destination(&candidates, &mut self.rng) {
                Some(c) => {
                    let chosen_addr = c.addr;
                    if let Some((hex, _, _)) =
                        live_peers.iter().find(|(_, _, a)| *a == chosen_addr)
                    {
                        let new_peer = hex.clone();
                        self.re_recruit(gid, seq, &new_peer, &instr);
                    }
                }
                None => {
                    // Paced fail-closed reset, same rhythm as the other passes.
                    if self.recruit_ledger.age_expired(gid, seq, timeout) {
                        let resets = self.recruit_ledger.touch(gid, seq);
                        self.emit_str(&format!(
                            "recruit #{} seq {}: unplaced, no candidate with headroom — deadline reset ({}x)\n",
                            gid, seq, resets
                        ));
                    }
                }
            }
        }
        reports
    }

    /// Re-send an existing open slot's retained instruction to `new_peer` and
    /// reassign the slot's holder. The slot keeps its identity `(goal_id, seq)`,
    /// so the eventual result still routes back to the same parallel-job slot.
    pub(crate) fn re_recruit(&mut self, goal_id: u64, seq: usize, new_peer: &str, instr: &str) {
        let my_id = self
            .node_id_cache
            .map(|id| crate::mesh::id_to_hex(&id))
            .unwrap_or_else(|| "local".to_string());
        let bounty = self.recruit_bounty();
        let msg = distgoal::sexp_recruit(goal_id, seq, &my_id, instr, bounty);
        self.recruit_ledger.reassign(goal_id, seq, new_peer);
        self.send_to_node(new_peer, &msg);
    }

    /// The recruiter's side of the wage: spend [`energy::RECRUIT_BOUNTY`]
    /// now and attach it to the outgoing recruit, if affordable. A recruiter
    /// that can't pay recruits at bounty 0 — the work may still be served;
    /// the wage is an incentive that selection can act on, not a mandate.
    fn recruit_bounty(&mut self) -> i64 {
        if self.energy.can_afford(energy::RECRUIT_BOUNTY)
            && self.energy.spend(energy::RECRUIT_BOUNTY, "recruit-bounty")
        {
            energy::RECRUIT_BOUNTY
        } else {
            0
        }
    }

    /// RECRUIT" <peer> <s-expr>" — manual recruit trigger. Parses a target peer
    /// hex id and an s-expression instruction, allocates a goal_id, and emits a
    /// recruit. The reply is collected asynchronously (see the `recruit-result`
    /// arm); view results with RECRUITS. There is NO automatic decision here —
    /// this is purely the human-fired mechanism.
    pub(crate) fn prim_recruit(&mut self) {
        let arg = self.parse_until('"');
        let arg = arg.trim();
        let (peer, instr) = match arg.split_once(char::is_whitespace) {
            Some((p, rest)) => (p.to_string(), rest.trim().to_string()),
            None => {
                self.emit_str("recruit: expected <peer> <s-expr>\n");
                return;
            }
        };
        if instr.is_empty() {
            self.emit_str("recruit: empty instruction\n");
            return;
        }
        let goal_id = self.recruit_ledger.next_id();
        let msg = self.send_recruit(&peer, goal_id, 0, &instr);
        self.emit_str(&format!("recruit #{} -> {}: {}\n", goal_id, peer, msg));
    }

    /// RECRUITS — show outstanding and collected recruit round-trips.
    /// `RECRUITS-SEXP` ( -- ) — machine-readable recruit status. Prints a
    /// `(recruit-slots :count N)` header, then one
    /// `(recruit-slot :id N :seq S :holder "…" :state … :reassigned N :resets N)`
    /// per slot. The stable assertion surface for harnesses (the Docker
    /// drill) and tooling; `RECRUITS`' prose stays free to change.
    pub(crate) fn prim_recruits_sexp(&mut self) {
        let sexps = self.recruit_ledger.status_sexps();
        self.emit_str(&format!("(recruit-slots :count {})\n", sexps.len()));
        for s in sexps {
            self.emit_str(&format!("{}\n", s));
        }
    }

    pub(crate) fn prim_recruits(&mut self) {
        let report = self.recruit_ledger.format_status();
        self.emit_str(&report);
    }

    /// `N ALLOC-MB` — load generator. Allocate and RETAIN N MiB of real process
    /// memory (every byte touched so the pages are resident), dropping
    /// MemAvailable so the NEXT `HostResources::measure()` sees higher memory
    /// utilization. This is the forcing function that drives a node over its
    /// ceiling so `run_parallel` recruits overflow — memory is the instantaneous
    /// axis (the 1-minute load average is too slow for the per-part re-measure).
    /// Allocation is fallible (`try_reserve_exact`): an over-large request is
    /// refused and pushes 0 rather than aborting. Pushes the MiB actually
    /// allocated. Freed by RECLAIM-MB. NOT general computation — a load tool.
    pub(crate) fn prim_alloc_mb(&mut self) {
        let mb = self.pop().max(0) as usize;
        // Gated like SHELL": off by default, so evolved GP code can't reach the
        // load generator. Refuse cleanly — no allocation, push 0.
        if !self.alloc_enabled {
            self.emit_str("alloc: disabled (use ALLOC-ENABLE from REPL)\n");
            self.stack.push(0);
            return;
        }
        let bytes = mb.saturating_mul(1024 * 1024);
        let mut buf: Vec<u8> = Vec::new();
        if bytes > 0 && buf.try_reserve_exact(bytes).is_ok() {
            buf.resize(bytes, 1u8); // touch every byte -> resident
            self.mem_ballast.push(buf);
            self.stack.push(mb as i64);
        } else {
            // Refused (no memory) or zero -> fail-closed, push 0.
            self.stack.push(0);
        }
    }

    /// RECLAIM-MB — free all retained ALLOC-MB memory, returning it to the OS.
    /// Pushes the number of chunks freed.
    pub(crate) fn prim_reclaim_mb(&mut self) {
        let freed = self.mem_ballast.len();
        self.mem_ballast.clear();
        self.mem_ballast.shrink_to_fit();
        self.stack.push(freed as i64);
    }

    /// Choose a peer to recruit to using the EXISTING placement logic
    /// (`transport::choose_destination`) over the gossiped peer headroom view —
    /// the same headroom-based selection placement and replication use. Returns
    /// the chosen peer's hex id, or None if there is no mesh / no suitable peer.
    pub(crate) fn choose_recruit_peer(&mut self) -> Option<String> {
        let peers = self.mesh.as_ref()?.peer_resource_view();
        if peers.is_empty() {
            return None;
        }
        let candidates: Vec<crate::transport::Candidate> = peers
            .iter()
            .map(|(_, _, headroom, addr)| crate::transport::Candidate {
                headroom_pct: *headroom,
                addr: *addr,
            })
            .collect();
        let chosen_addr = crate::transport::choose_destination(&candidates, &mut self.rng)?.addr;
        peers
            .iter()
            .find(|(_, _, _, addr)| *addr == chosen_addr)
            .map(|(id, _, _, _)| crate::mesh::id_to_hex(id))
    }

    /// Split-and-recruit a `(parallel (e1) (e2) ...)` instruction under LOCAL
    /// RESOURCE PRESSURE. For each sub-part, in order: if the unit currently has
    /// headroom under the ceiling, run it locally via the `eval_sexp` seam and
    /// re-measure on the next part; otherwise recruit it to a placement-chosen
    /// peer via `send_recruit`. Reactive and measured, NOT predictive: no
    /// part's cost is estimated; the decision reads only the current measured
    /// pressure and re-checks after each local run. If already over the ceiling
    /// when the work arrives, every part is recruited.
    ///
    /// Results are COLLECTED into a `ParallelJob` (see `parallel_result`), never
    /// combined. `measure` yields the current host reading: live callers pass
    /// `HostResources::measure`; tests pass a scripted closure. Returns the
    /// job's goal_id.
    pub(crate) fn run_parallel<M>(
        &mut self,
        parts: &[crate::sexp::Sexp],
        measure: &mut M,
        budget_kb: u64,
    ) -> u64
    where
        M: FnMut() -> crate::resources::HostResources,
    {
        let goal_id = self.recruit_ledger.next_id();
        let mut job = crate::distgoal::ParallelJob::new(goal_id, parts.len());
        // Committed-work tally: memory (kB) THIS call has decided to run locally
        // but that measure() may not yet reflect (RSS lag, swap absorption,
        // loadavg averaging). Per-call scratch — it resets here and never
        // persists across calls or ticks, so it cannot double-count against a
        // later measure() that catches up.
        let mut committed_kb: u64 = 0;
        for (seq, part) in parts.iter().enumerate() {
            let part_str = part.to_string();
            let obs = measure();
            // Add the committed tally on the same (combined RAM+swap) memory
            // axis as obs.utilization. budget_kb == 0 means the budget is
            // unknown -> accounting disabled (observed-only, the old behavior).
            // (obs.utilization is max(mem, load); adding the memory tally is
            // exact when memory is binding and mildly conservative otherwise.)
            let committed_fraction = if budget_kb > 0 {
                committed_kb as f64 / budget_kb as f64
            } else {
                0.0
            };
            let has_headroom = obs.is_available()
                && (obs.utilization + committed_fraction) < crate::resources::CEILING_UTILIZATION;
            if has_headroom {
                // Headroom (observed + committed): run locally, then ADD this
                // part's estimated cost to the tally before deciding the next.
                let envelope = crate::sexp::eval_sexp(self, &part_str);
                job.set(seq, envelope);
                committed_kb = committed_kb.saturating_add(
                    crate::distgoal::part_cost_mb(part).saturating_mul(1024),
                );
            } else if let Some(peer) = self.choose_recruit_peer() {
                // No headroom AND a peer WITH headroom exists: recruit this part.
                // The slot stays pending until its (recruit-result ...) lands.
                self.send_recruit(&peer, goal_id, seq, &part_str);
            } else {
                // No headroom and no peer with headroom — the part is DECLINED:
                // no recruit is emitted. THIS is the emergent brake: on a
                // saturated mesh fan-out stops, and the ceiling bounds the tree
                // at every level (the same check runs on every recursively-
                // recruited peer); no recursion-depth cap is needed, by design.
                //
                // But the WAIT the declined part creates must still be bounded:
                // record it as an UNPLACED ledger slot (empty holder, retained
                // instruction). The supervision placement pass re-attempts it
                // when capacity appears and terminally abandons it at the
                // attempt cap — previously a declined part was invisible to
                // supervision and could hold its job (and any Deferred
                // obligation above it) open forever.
                self.recruit_ledger.open(goal_id, seq, &part_str, "");
            }
        }
        self.parallel_jobs.insert(goal_id, job);
        goal_id
    }

    /// Assemble the collected `(parallel-result ...)` for a parallel job, or
    /// None if `goal_id` is not a known parallel job.
    pub(crate) fn parallel_result(&self, goal_id: u64) -> Option<crate::sexp::Sexp> {
        self.parallel_jobs.get(&goal_id).map(|job| job.assemble())
    }

    /// PARALLEL" (parallel (e1) (e2) ...)" — manual split-and-recruit trigger.
    /// Parses the divisible form, runs the resource-pressure decision, and
    /// prints the collected (parallel-result ...). Trigger only — there is no
    /// automatic detection of parallel work inside handle_recruit yet.
    pub(crate) fn prim_parallel(&mut self) {
        let arg = self.parse_until('"');
        let sexp = match crate::sexp::parse(arg.trim()) {
            Ok(s) => s,
            Err(e) => {
                self.emit_str(&format!("parallel: sexp error: {}\n", e));
                return;
            }
        };
        let parts = match crate::distgoal::parallel_parts(&sexp) {
            Some(p) => p,
            None => {
                self.emit_str("parallel: expected (parallel (e1) (e2) ...)\n");
                return;
            }
        };
        // run_parallel -> eval_sexp -> execute_sandbox -> interpret_line clobbers
        // the input buffer/position; save and restore so the outer line survives.
        let saved_buf = self.input_buffer.clone();
        let saved_pos = self.input_pos;
        let mut measure = crate::resources::HostResources::measure;
        let budget_kb = crate::resources::measure_mem_budget_kb();
        let goal_id = self.run_parallel(&parts, &mut measure, budget_kb);
        self.input_buffer = saved_buf;
        self.input_pos = saved_pos;
        if let Some(result) = self.parallel_result(goal_id) {
            self.emit_str(&format!("{}\n", result));
        }
    }

    /// Dispatch a single inbound chatter (S-expression) message. Extracted so
    /// the bench harness can call it directly with synthesized messages.
    pub fn process_chatter_msg(&mut self, msg: &str) {
        let _t_msg = metrics::Timer::new("chatter.process");
        if let Some(sexp) = crate::sexp::try_parse_mesh_msg(msg) {
            match crate::sexp::msg_type(&sexp) {
                        Some("death-cry") => {
                            // A peer died; inherit its immune memory. The
                            // reader trust-gates to SOL-* words and
                            // absorb_antibodies re-gates (never overwrite,
                            // bounded), so a forged cry can't install or
                            // redefine behavior.
                            if let Some(antibodies) = crate::sexp::read_death_cry(&sexp) {
                                let n = self.absorb_antibodies(&antibodies);
                                if n > 0 {
                                    self.emit_str(&format!(
                                        "[immune] inherited {} antibod{} from a dead peer\n",
                                        n,
                                        if n == 1 { "y" } else { "ies" }
                                    ));
                                }
                            }
                        }
                        Some("sub-goal") => {
                            // A peer asked us to compute something.
                            let goal_id =
                                sexp.get_key(":id").and_then(|s| s.as_number()).unwrap_or(0) as u64;
                            let seq = sexp
                                .get_key(":seq")
                                .and_then(|s| s.as_number())
                                .unwrap_or(0) as usize;
                            let _from = sexp
                                .get_key(":from")
                                .and_then(|s| s.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let expr = sexp
                                .get_key(":expr")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();
                            if !expr.is_empty() {
                                let result = self.execute_sandbox(&expr);
                                let output = result.output.trim().to_string();
                                let my_id = self
                                    .node_id_cache
                                    .map(|id| crate::mesh::id_to_hex(&id))
                                    .unwrap_or_else(|| "local".to_string());
                                if let Some(ref m2) = self.mesh {
                                    let reply =
                                        distgoal::sexp_sub_result(goal_id, seq, &my_id, &output);
                                    m2.send_sexp(&reply);
                                }
                            }
                        }
                        Some("sub-result") => {
                            // A peer sent back a result.
                            let goal_id =
                                sexp.get_key(":id").and_then(|s| s.as_number()).unwrap_or(0) as u64;
                            let seq = sexp
                                .get_key(":seq")
                                .and_then(|s| s.as_number())
                                .unwrap_or(0) as usize;
                            let result_str = sexp
                                .get_key(":result")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();
                            self.dist_engine.record_result(goal_id, seq, &result_str);

                            // Check if goal is now complete.
                            if self.dist_engine.is_complete(goal_id) {
                                if let Some(combined) = self.dist_engine.combine_results(goal_id) {
                                    self.emit_str(&format!(
                                        "dist-goal #{} complete: {}\n",
                                        goal_id, combined
                                    ));
                                }
                            }
                        }
                        Some("recruit") => {
                            // A peer recruited us to evaluate an s-expression
                            // instruction. Built on the eval_sexp seam (NOT the
                            // legacy sub-goal path): the reply carries the full
                            // canonical result envelope, so success/error is
                            // visible to the recruiter.
                            let goal_id =
                                sexp.get_key(":id").and_then(|s| s.as_number()).unwrap_or(0) as u64;
                            let seq = sexp
                                .get_key(":seq")
                                .and_then(|s| s.as_number())
                                .unwrap_or(0) as usize;
                            let from = sexp
                                .get_key(":from")
                                .and_then(|s| s.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let instr = sexp
                                .get_key(":instr")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();
                            // The wage the recruiter attached (already spent
                            // on its side). Accept-capped: recruit messages
                            // are unauthenticated, so a forged flood can
                            // mint at most BOUNTY_ACCEPT_CAP per message.
                            let bounty = sexp
                                .get_key(":bounty")
                                .and_then(|s| s.as_number())
                                .unwrap_or(0)
                                .clamp(0, energy::BOUNTY_ACCEPT_CAP);
                            if !instr.is_empty() {
                                // Live measure so a recruited (parallel ...)
                                // re-applies the ceiling decision at this level.
                                let mut measure = crate::resources::HostResources::measure;
                                match self.handle_recruit(goal_id, seq, &instr, &from, &mut measure)
                                {
                                    // Completed here: reply to the recruiter
                                    // now — and collect the wage. Work was
                                    // performed; energy moved with it.
                                    distgoal::RecruitOutcome::Reply(reply) => {
                                        self.send_to_node(&from, &reply);
                                        if bounty > 0 {
                                            self.energy.earn(bounty, "recruit-wage");
                                        }
                                    }
                                    // Recruited overflow: no reply now; this unit
                                    // self-reports up once its child job completes.
                                    // No wage at this hop — it delegated the work.
                                    distgoal::RecruitOutcome::Deferred { .. } => {}
                                }
                            }
                        }
                        Some("recruit-result") => {
                            // Recruiter side (collect): fill the matching slot.
                            // If that was the last open slot, this unit is now
                            // complete and self-reports its full result UP to
                            // whoever recruited it (fan-in). A root surfaces it
                            // locally. The completion check lives in here.
                            if let Some((target, msg)) = self.collect_recruit_result(&sexp) {
                                self.send_to_node(&target, &msg);
                            }
                        }
                        Some("mating-request") => {
                            let from_hex = sexp
                                .get_key(":from")
                                .and_then(|s| s.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let fitness = sexp
                                .get_key(":fitness")
                                .and_then(|s| s.as_number())
                                .unwrap_or(0);
                            let my_fitness = self.fitness.score;
                            // Auto-accept if requester fitness >= half of ours.
                            let accept =
                                self.mate_auto_accept && fitness >= my_fitness / 2;
                            if accept {
                                let my_id = self.node_id_cache.unwrap_or([0; 8]);
                                let words: Vec<(String, String)> = self
                                    .dictionary
                                    .iter()
                                    .filter(|e| !e.hidden && e.body.len() > 1)
                                    .take(50)
                                    .map(|e| (e.name.clone(), format!("{:?}", e.body)))
                                    .collect();
                                let resp = reproduction::MatingResponse {
                                    accepted: true,
                                    responder_id: my_id,
                                    responder_fitness: my_fitness,
                                    dictionary_words: words,
                                };
                                let reply = reproduction::sexp_mating_response(&resp);
                                if let Some(ref m2) = self.mesh {
                                    m2.send_sexp(&reply);
                                }
                                self.emit_str(&format!(
                                    "[mate] accepted mating request from {}\n",
                                    from_hex
                                ));
                            } else {
                                self.emit_str(&format!(
                                    "[mate] denied mating request from {}\n",
                                    from_hex
                                ));
                            }
                        }
                        Some("mating-response") => {
                            let accepted_str = sexp
                                .get_key(":accepted")
                                .and_then(|s| s.as_atom())
                                .unwrap_or("false");
                            let from_hex = sexp
                                .get_key(":from")
                                .and_then(|s| s.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            if accepted_str == "true" {
                                self.emit_str(&format!(
                                    "[mate] {} accepted! crossover offspring created\n",
                                    from_hex
                                ));
                                self.mating_offspring
                                    .push(("child".to_string(), from_hex));
                            } else {
                                self.emit_str(&format!(
                                    "[mate] {} denied mating request\n",
                                    from_hex
                                ));
                            }
                        }
                _ => {} // other sexp types handled elsewhere
            }
        }
    }

}
