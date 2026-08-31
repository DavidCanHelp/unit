//! Sandbox execution engine — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Sandbox execution engine
    // -----------------------------------------------------------------------

    /// Parse balanced braces from the input buffer. Returns the content
    /// between the opening { (already consumed) and the closing }.
    pub(crate) fn parse_balanced_braces(&mut self) -> String {
        let bytes = self.input_buffer.as_bytes();
        if self.input_pos < bytes.len() && bytes[self.input_pos] == b' ' {
            self.input_pos += 1;
        }
        let start = self.input_pos;
        let mut depth = 1i32;
        while self.input_pos < bytes.len() && depth > 0 {
            match bytes[self.input_pos] as char {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        let result = self.input_buffer[start..self.input_pos].to_string();
                        self.input_pos += 1;
                        return result;
                    }
                }
                _ => {}
            }
            self.input_pos += 1;
        }
        self.input_buffer[start..self.input_pos].to_string()
    }

    /// Execute Forth code in a sandbox. Saves/restores VM state. Returns
    /// a TaskResult with the captured stack, output, and success status.
    pub(crate) fn execute_sandbox(&mut self, code: &str) -> goals::TaskResult {
        // Save state.
        let saved_stack = std::mem::take(&mut self.stack);
        let saved_rstack = std::mem::take(&mut self.rstack);
        let saved_silent = self.silent;
        let saved_compiling = self.compiling;
        let saved_current_def = self.current_def.take();
        let saved_output_buffer = self.output_buffer.take();
        let saved_deadline = self.deadline.take();
        let saved_timed_out = self.timed_out;
        let saved_sandbox = self.sandbox_active;
        // `take` both stashes any outer fault and resets to None for this run,
        // so a fault from a prior evaluation cannot leak into this result.
        let saved_fault = self.fault.take();
        let saved_step_budget = self.step_budget;

        // Set up sandbox.
        self.stack = Vec::with_capacity(256);
        self.rstack = Vec::with_capacity(256);
        self.output_buffer = Some(String::new());
        self.silent = true;
        self.sandbox_active = true; // remote code always sandboxed
        self.compiling = false;
        self.timed_out = false;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.deadline = Some(Instant::now() + Duration::from_secs(self.execution_timeout));
        }

        // Execute.
        for line in code.lines() {
            self.interpret_line(line);
            if self.timed_out || !self.running {
                break;
            }
        }

        // Capture results.
        let stack_snapshot = self.stack.clone();
        let output = self.output_buffer.take().unwrap_or_default();
        let success = !self.timed_out && self.fault.is_none();
        let error = if self.timed_out {
            Some(format!("execution timeout ({}s)", self.execution_timeout))
        } else {
            self.fault.map(|f| f.message().to_string())
        };

        // Restore state.
        self.stack = saved_stack;
        self.rstack = saved_rstack;
        self.silent = saved_silent;
        self.compiling = saved_compiling;
        self.current_def = saved_current_def;
        self.output_buffer = saved_output_buffer;
        self.deadline = saved_deadline;
        self.timed_out = saved_timed_out;
        self.sandbox_active = saved_sandbox;
        self.fault = saved_fault;
        self.step_budget = saved_step_budget;
        self.running = true; // task execution must not kill the unit

        goals::TaskResult {
            stack_snapshot,
            output,
            success,
            error,
        }
    }

}
