//! S-expression primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // S-expression primitives
    // -----------------------------------------------------------------------

    /// SEXP" expr" — parse S-expression and translate to Forth, then execute.
    pub(crate) fn prim_sexp_eval(&mut self) {
        let sexp_str = self.parse_until('"');
        match crate::sexp::parse(&sexp_str) {
            Ok(sexp) => {
                let forth = crate::sexp::to_forth(&sexp);
                // Save outer input state — interpret_line overwrites these.
                let saved_buf = self.input_buffer.clone();
                let saved_pos = self.input_pos;
                self.interpret_line(&forth);
                // Restore so the rest of the outer line continues.
                self.input_buffer = saved_buf;
                self.input_pos = saved_pos;
            }
            Err(e) => {
                self.emit_str(&format!("sexp error: {}\n", e));
            }
        }
    }

    /// SEXP-EVAL" expr" — evaluate an S-expression instruction through the
    /// `eval_sexp` seam and print the structured result envelope. Unlike
    /// `SEXP"` (which translates into the live VM and prints whatever the code
    /// emits), this evaluates in a sandbox and reports a `(result :ok ...)`
    /// envelope, so it never disturbs the REPL's own stack.
    pub(crate) fn prim_sexp_eval_result(&mut self) {
        let sexp_str = self.parse_until('"');
        // eval_sexp runs the code through execute_sandbox -> interpret_line,
        // which overwrites the input buffer/position; save and restore them so
        // the rest of the outer line keeps processing (same guard as
        // prim_sexp_eval).
        let saved_buf = self.input_buffer.clone();
        let saved_pos = self.input_pos;
        let envelope = crate::sexp::eval_sexp(self, &sexp_str);
        self.input_buffer = saved_buf;
        self.input_pos = saved_pos;
        self.emit_str(&format!("{}\n", envelope));
    }

    /// SEXP-SEND" expr" — broadcast an S-expression message to mesh peers.
    pub(crate) fn prim_sexp_send(&mut self) {
        let sexp_str = self.parse_until('"');
        // Validate it parses as a valid S-expression.
        match crate::sexp::parse(&sexp_str) {
            Ok(_) => {
                if let Some(ref m) = self.mesh {
                    m.send_sexp(&sexp_str);
                    self.emit_str("sexp sent\n");
                } else {
                    self.emit_str("no mesh\n");
                }
            }
            Err(e) => {
                self.emit_str(&format!("sexp error: {}\n", e));
            }
        }
    }

    /// SEXP-RECV — drain inbound S-expression messages, print them.
    pub(crate) fn prim_sexp_recv(&mut self) {
        if let Some(ref m) = self.mesh {
            let msgs = m.recv_sexp_messages();
            if msgs.is_empty() {
                self.emit_str("no sexp messages\n");
            } else {
                for msg in &msgs {
                    self.emit_str(msg);
                    self.emit_str("\n");
                }
            }
        } else {
            self.emit_str("no mesh\n");
        }
    }

}
