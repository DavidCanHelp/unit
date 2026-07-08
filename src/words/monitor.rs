//! Monitoring & ops primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Monitoring & Ops primitives
    // -----------------------------------------------------------------------

    pub(crate) fn prim_watch(&mut self, kind: i32) {
        let target = self.parse_until('"');
        let rt_prim = match kind {
            0 => P_WATCH_URL_RT,
            1 => P_WATCH_FILE_RT,
            2 => P_WATCH_PROC_RT,
            _ => P_WATCH_URL_RT,
        };
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(target);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(rt_prim));
            }
        } else {
            self.do_add_watch(kind, &target);
        }
    }

    pub(crate) fn rt_watch(&mut self, kind: i32) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let target = self.code_strings[idx].clone();
            self.do_add_watch(kind, &target);
        }
    }

    pub(crate) fn do_add_watch(&mut self, kind: i32, target: &str) {
        let interval = self.pop() as u64;
        let wk = match kind {
            0 => monitor::WatchKind::Url(target.to_string()),
            1 => monitor::WatchKind::File(target.to_string()),
            2 => monitor::WatchKind::Process(target.to_string()),
            _ => return,
        };
        let id = self.monitor.add_watch(wk, interval.max(1));
        self.stack.push(id as Cell);
        self.emit_str(&format!("watch #{} created (every {}s)\n", id, interval));
    }

    pub(crate) fn prim_on_alert(&mut self) {
        let code = self.parse_until('"');
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(code);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Primitive(P_ON_ALERT_RT));
                def.body.push(Instruction::Literal(idx as Cell));
            }
        } else {
            let watch_id = self.pop() as u32;
            self.monitor.set_alert_handler(watch_id, code);
            self.emit_str(&format!("alert handler set for watch #{}\n", watch_id));
        }
    }

    pub(crate) fn rt_on_alert(&mut self) {
        let idx = self.pop() as usize;
        let watch_id = self.pop() as u32;
        if idx < self.code_strings.len() {
            let code = self.code_strings[idx].clone();
            self.monitor.set_alert_handler(watch_id, code);
        }
    }

    pub(crate) fn prim_alert_threshold(&mut self) {
        let target = self.parse_until('"');
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(target);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(P_ALERT_THRESHOLD_RT));
            }
        } else if let Ok(watch_id) = target.trim().parse::<u32>() {
            let level = self.pop();
            self.monitor
                .set_alert_level(watch_id, monitor::AlertLevel::from_val(level));
            self.emit_str(&format!("alert threshold set for watch #{}\n", watch_id));
        }
    }

    pub(crate) fn rt_alert_threshold(&mut self) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            if let Ok(watch_id) = self.code_strings[idx].trim().parse::<u32>() {
                let level = self.pop();
                self.monitor
                    .set_alert_level(watch_id, monitor::AlertLevel::from_val(level));
            }
        }
    }

    pub(crate) fn prim_dashboard(&mut self) {
        let _t = metrics::Timer::new("dashboard.render");
        let peer_count = self.mesh.as_ref().map(|m| m.peer_count()).unwrap_or(0);
        let goal_summary = self
            .mesh
            .as_ref()
            .map(|m| m.format_goals())
            .unwrap_or_default();
        let s = self
            .monitor
            .format_dashboard(peer_count, self.fitness.score, &goal_summary);
        self.emit_str(&s);
    }

    pub(crate) fn prim_health(&mut self) {
        let peer_count = self.mesh.as_ref().map(|m| m.peer_count()).unwrap_or(0);
        let score = self.monitor.health_score(peer_count, self.fitness.score);
        self.stack.push(score);
    }

    pub(crate) fn prim_every(&mut self) {
        let interval = self.pop() as u64;
        // Consume the rest of the input line as the code to schedule.
        let remaining = self.input_buffer[self.input_pos..].trim().to_string();
        self.input_pos = self.input_buffer.len(); // consume it
        if remaining.is_empty() {
            self.emit_str("EVERY: no code to schedule\n");
            return;
        }
        let id = self
            .monitor
            .add_schedule(remaining.clone(), interval.max(1));
        self.stack.push(id as Cell);
        self.emit_str(&format!(
            "schedule #{} every {}s: {}\n",
            id,
            interval,
            remaining.chars().take(40).collect::<String>()
        ));
    }

    pub(crate) fn rt_every(&mut self) {
        // For compiled EVERY, not yet supported — would need code string storage.
        self.emit_str("EVERY only works at the REPL\n");
    }

    pub(crate) fn prim_heal(&mut self) {
        self.emit_str("--- heal cycle ---\n");
        // Check all watches.
        let due = self.monitor.due_watches();
        if due.is_empty() {
            self.emit_str("  no watches due\n");
        }
        for wid in &due {
            self.run_watch_check(*wid);
        }
        // Run handlers for active alerts.
        let handlers: Vec<(u32, String)> = self
            .monitor
            .alerts
            .iter()
            .filter(|a| !a.acknowledged)
            .filter_map(|a| {
                self.monitor
                    .watches
                    .get(&a.watch_id)
                    .and_then(|w| w.alert_handler.clone())
                    .map(|h| (a.id, h))
            })
            .collect();
        for (aid, handler) in &handlers {
            self.emit_str(&format!("  running handler for alert #{}\n", aid));
            self.interpret_line(handler);
        }
        self.emit_str("--- heal done ---\n");
    }

    /// Execute a watch check for a specific watch ID.
    pub(crate) fn run_watch_check(&mut self, watch_id: u32) {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = watch_id;
            return;
        } // watches require native I/O
        #[cfg(not(target_arch = "wasm32"))]
        {
            let kind = match self.monitor.watches.get(&watch_id) {
                Some(w) => w.kind.clone(),
                None => return,
            };
            let start = Instant::now();
            let status = match kind {
                monitor::WatchKind::Url(ref url) => match io_words::http_get(url) {
                    Ok((_, code)) => {
                        let ms = start.elapsed().as_millis() as u64;
                        if (200..400).contains(&code) {
                            monitor::WatchStatus::up(code as i32, ms, format!("{}", code))
                        } else {
                            monitor::WatchStatus::down(code as i32, format!("HTTP {}", code))
                        }
                    }
                    Err(e) => monitor::WatchStatus::down(-1, e),
                },
                monitor::WatchKind::File(ref path) => {
                    if io_words::file_exists(path) {
                        let ms = start.elapsed().as_millis() as u64;
                        match std::fs::metadata(path) {
                            Ok(m) => monitor::WatchStatus::up(0, ms, format!("{}b", m.len())),
                            Err(e) => monitor::WatchStatus::down(-1, e.to_string()),
                        }
                    } else {
                        monitor::WatchStatus::down(-1, "not found".into())
                    }
                }
                monitor::WatchKind::Process(ref name) => {
                    match io_words::shell_exec(&format!(
                        "pgrep -x {} >/dev/null 2>&1 && echo UP || echo DOWN",
                        name
                    )) {
                        Ok((stdout, _)) => {
                            let ms = start.elapsed().as_millis() as u64;
                            let out = String::from_utf8_lossy(&stdout).trim().to_string();
                            if out.contains("UP") {
                                monitor::WatchStatus::up(0, ms, "running".into())
                            } else {
                                monitor::WatchStatus::down(-1, "not running".into())
                            }
                        }
                        Err(e) => monitor::WatchStatus::down(-1, e),
                    }
                }
            };

            // Record the check result.
            if let Some(alert) = self.monitor.record_check(watch_id, status.clone()) {
                self.emit_str(&format!(
                    "ALERT [{}] watch #{}: {}\n",
                    alert.level.label(),
                    watch_id,
                    alert.message
                ));
                // Run alert handler if defined.
                let handler = self
                    .monitor
                    .watches
                    .get(&watch_id)
                    .and_then(|w| w.alert_handler.clone());
                if let Some(code) = handler {
                    self.interpret_line(&code);
                    // Fitness bonus for attempted remediation.
                    self.fitness.score += 15;
                }
            }
        } // end #[cfg(not(wasm32))]
    }

    /// Tick the monitor: check due watches and run due schedules.
    pub(crate) fn tick_monitor(&mut self) {
        // Check due watches.
        let due_watches = self.monitor.due_watches();
        for wid in due_watches {
            self.run_watch_check(wid);
        }

        // Run due schedules.
        let due_scheds = self.monitor.due_schedules();
        for (_sid, code) in due_scheds {
            self.interpret_line(&code);
        }
    }

}
