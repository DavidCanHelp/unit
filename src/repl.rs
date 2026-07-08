//! The interactive REPL loop and its idle-tick duties.
use crate::vm::VM;
use std::io::{self, BufRead, Write};
use std::time::Duration;

// ===========================================================================
// REPL
// ===========================================================================

/// Idle-tick cadence for the REPL loop: how long the main loop waits for a
/// line of input before running the periodic duties anyway. Bounds the
/// latency of recruit evaluation, supervision, replication acceptance, and
/// every other duty in `repl_tick` on a node nobody is typing at.
pub(crate) const REPL_TICK: Duration = Duration::from_millis(250);

impl VM {
    /// The REPL's periodic duties — everything a node must do whether or not
    /// a human is typing. Runs after every input line AND on a timer while
    /// the REPL is idle. These were originally gated on stdin input, which
    /// meant an idle interactive node never evaluated recruited work, never
    /// ran its supervision passes, never accepted inbound replications, and
    /// had frozen metabolism — heartbeats kept flowing (mesh thread) while
    /// all VM-side duties waited for a keypress.
    fn repl_tick(&mut self) {
        self.check_auto_claim();
        self.check_auto_replicate();
        self.check_auto_evolve();
        self.check_incoming_replications();
        self.energy.tick();
        self.landscape.tick();
        self.tick_monitor();
        self.tick_swarm();
        self.check_auto_snapshot();
        self.tick_dist_goals();
        self.poll_ws_events();
        self.update_ws_mesh_json();
    }

    pub(crate)     fn repl(&mut self) {
        let mut stdout = io::stdout();

        let _ = write!(stdout, "> ");
        let _ = stdout.flush();

        // The blocking stdin read lives on its own thread so the main loop
        // can tick on a timer while idle. Only line Strings cross the
        // channel — the VM (and the recruit ledger with it) stays owned by
        // this thread, exactly as before. EOF or a read error drops the
        // sender, which ends the loop below: piped input (`echo ... | unit`)
        // keeps its run-then-exit semantics.
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let stdin = io::stdin();
            for line in stdin.lock().lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break; // REPL ended (BYE) — stop reading
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        loop {
            match rx.recv_timeout(REPL_TICK) {
                Ok(line) => {
                    self.interpret_line(&line);
                    if !self.running {
                        break;
                    }
                    if !self.compiling {
                        self.repl_tick();
                    }
                    if self.compiling {
                        let _ = write!(stdout, "  ");
                    } else {
                        let _ = write!(stdout, " ok\n> ");
                    }
                    let _ = stdout.flush();
                    self.needs_prompt_redraw = false; // prompt just printed
                }
                // Idle: no input this interval — run the duties anyway.
                // (Skipped mid-definition, same as the per-line path: a
                // half-compiled word must not be observed by spawn/snapshot.)
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if !self.compiling {
                        self.repl_tick();
                        // Tick-driven output (e.g. "parallel #N complete:")
                        // leaves the cursor mid-line with no prompt, which
                        // reads as a hang. Redraw it. (Characters typed but
                        // not yet submitted stay in the terminal's line
                        // buffer and still apply — we can't re-echo them
                        // without raw mode, so they appear above the fresh
                        // prompt but are not lost.)
                        if self.needs_prompt_redraw {
                            let _ = write!(stdout, "> ");
                            let _ = stdout.flush();
                            self.needs_prompt_redraw = false;
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        println!();
    }
}
