//! Swarm primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Swarm primitives
    // -----------------------------------------------------------------------

    pub(crate) fn prim_discover(&mut self) {
        if let Some(ref m) = self.mesh {
            m.send_discovery_beacon();
            self.emit_str("discovery beacon sent\n");
        }
    }

    pub(crate) fn prim_auto_discover(&mut self) {
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.auto_discover = !st.auto_discover;
            let on = st.auto_discover;
            drop(st);
            self.emit_str(&format!(
                "auto-discover: {}\n",
                if on { "ON" } else { "OFF" }
            ));
        }
    }

    pub(crate) fn prim_share_word(&mut self) {
        let name = self.parse_until('"');
        let upper = name.to_uppercase();
        // Find the word and reconstruct its source (simplified: use SEE-like decompilation).
        if let Some(idx) = self.find_word(&upper) {
            let _entry = &self.dictionary[idx];
            // Build a Forth source representation.
            let source = format!(": {} ;", upper); // simplified — real impl would decompile
            if let Some(ref m) = self.mesh {
                m.share_word(&upper, &source);
                self.emit_str(&format!("shared: {}\n", upper));
            }
        } else {
            self.emit_str(&format!("{}?\n", upper));
        }
    }

    pub(crate) fn prim_share_all(&mut self) {
        if let Some(ref m) = self.mesh {
            // Share all non-kernel words (words with more than one instruction).
            let mut count = 0;
            for entry in &self.dictionary {
                if entry.body.len() > 1 && !entry.hidden {
                    let source = format!(": {} ;", entry.name);
                    m.share_word(&entry.name, &source);
                    count += 1;
                }
            }
            self.emit_str(&format!("shared {} words\n", count));
        }
    }

    pub(crate) fn prim_auto_share(&mut self) {
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.auto_share = !st.auto_share;
            let on = st.auto_share;
            drop(st);
            self.emit_str(&format!("auto-share: {}\n", if on { "ON" } else { "OFF" }));
        }
    }

    pub(crate) fn prim_shared_words(&mut self) {
        if let Some(ref m) = self.mesh {
            let words = m.shared_words_list();
            if words.is_empty() {
                self.emit_str("  (no shared words)\n");
            } else {
                for (name, origin) in &words {
                    self.emit_str(&format!("  {} from {}\n", name, origin));
                }
            }
        }
    }

    pub(crate) fn prim_auto_spawn(&mut self) {
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.auto_spawn = !st.auto_spawn;
            let on = st.auto_spawn;
            drop(st);
            self.emit_str(&format!("auto-spawn: {}\n", if on { "ON" } else { "OFF" }));
        }
    }

    pub(crate) fn prim_auto_cull(&mut self) {
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.auto_cull = !st.auto_cull;
            let on = st.auto_cull;
            drop(st);
            self.emit_str(&format!("auto-cull: {}\n", if on { "ON" } else { "OFF" }));
        }
    }

    pub(crate) fn prim_min_units(&mut self) {
        let n = self.pop() as usize;
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.min_units = n.max(1);
            drop(st);
            self.emit_str(&format!("min-units: {}\n", n));
        }
    }

    pub(crate) fn prim_max_units(&mut self) {
        let n = self.pop() as usize;
        if let Some(ref m) = self.mesh {
            let mut st = m.state_lock();
            st.max_units = n.max(1);
            drop(st);
            self.emit_str(&format!("max-units: {}\n", n));
        }
    }

    pub(crate) fn prim_swarm_status(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_swarm_status();
            self.emit_str(&s);
        } else {
            self.emit_str("swarm: offline\n");
        }
    }

    /// Compile shared words received from peers.
    pub(crate) fn process_shared_words(&mut self) {
        let words = self
            .mesh
            .as_ref()
            .map(|m| m.recv_shared_words())
            .unwrap_or_default();
        for word in words {
            // Compile the shared word source.
            self.interpret_line(&word.body_source);
        }
    }

    /// Swarm tick — process word shares and check autonomous behaviors.
    pub(crate) fn tick_swarm(&mut self) {
        self.process_shared_words();
    }

}
