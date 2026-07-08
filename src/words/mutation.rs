//! Mutation primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Smart mutation
    // -----------------------------------------------------------------------

    pub(crate) fn snapshot_word(&mut self, idx: usize) -> u64 {
        let body = self.dictionary[idx].body.clone();
        let mut combined = String::new();
        // Save the outer output buffer so callers don't lose accumulated output.
        let saved_outer_buffer = self.output_buffer.take();
        for test_stack in &[vec![], vec![1i64], vec![1, 2, 3]] {
            let saved = std::mem::take(&mut self.stack);
            self.stack = test_stack.clone();
            self.output_buffer = Some(String::new());
            self.timed_out = false;
            #[cfg(not(target_arch = "wasm32"))]
            {
                self.deadline = Some(Instant::now() + Duration::from_millis(100));
            }
            self.execute_body(&body);
            combined.push_str(&self.output_buffer.take().unwrap_or_default());
            combined.push_str(&format!("{:?}", self.stack));
            self.stack = saved;
            self.deadline = None;
            self.timed_out = false;
        }
        // Restore the outer output buffer.
        self.output_buffer = saved_outer_buffer;
        mutation::hash_output(&combined)
    }

    pub(crate) fn prim_smart_mutate(&mut self) {
        let mutable_indices: Vec<usize> = self
            .dictionary
            .iter()
            .enumerate()
            .filter(|(_, e)| mutation::is_mutable(e))
            .map(|(i, _)| i)
            .collect();
        if mutable_indices.is_empty() {
            self.emit_str("no mutable words\n");
            self.stack.push(0);
            return;
        }
        let idx = mutable_indices[self.rng.next_usize(mutable_indices.len())];
        let word_name = self.dictionary[idx].name.clone();
        let before_hash = self.snapshot_word(idx);

        let dict_len = self.dictionary.len();
        let record =
            match mutation::mutate_entry(&mut self.dictionary[idx], &mut self.rng, dict_len) {
                Some(mut r) => {
                    r.word_index = idx;
                    r
                }
                None => {
                    self.stack.push(0);
                    return;
                }
            };

        let after_hash = self.snapshot_word(idx);
        let class = if after_hash == before_hash {
            mutation::MutationClass::Neutral
        } else {
            let score = self.run_benchmark();
            if score >= 0 {
                mutation::MutationClass::Beneficial
            } else {
                mutation::MutationClass::Harmful
            }
        };

        let kept = matches!(
            class,
            mutation::MutationClass::Neutral | mutation::MutationClass::Beneficial
        );
        if kept {
            self.mutation_history.push(record.clone());
        } else {
            mutation::undo_mutation(&mut self.dictionary[idx], &record);
        }

        self.mutation_stats.record(&class);
        self.last_mutation_result = Some(mutation::SmartMutationResult {
            word_name,
            strategy: record.strategy.clone(),
            class,
            before_hash,
            after_hash,
            kept,
            description: record.description,
        });
        self.stack.push(if kept { -1 } else { 0 });
    }

    pub(crate) fn prim_mutation_report(&mut self) {
        if let Some(ref r) = self.last_mutation_result {
            self.emit_str(&format!(
                "last: {} [{}] {} {}\n",
                r.word_name,
                r.strategy.label(),
                r.class.label(),
                if r.kept { "(kept)" } else { "(reverted)" }
            ));
        } else {
            self.emit_str("no mutations yet\n");
        }
    }

    // -----------------------------------------------------------------------
    // Mutation primitives
    // -----------------------------------------------------------------------

    pub(crate) fn prim_mutate_rand(&mut self) {
        // Pick a random mutable word.
        let mutable_indices: Vec<usize> = self
            .dictionary
            .iter()
            .enumerate()
            .filter(|(_, e)| mutation::is_mutable(e))
            .map(|(i, _)| i)
            .collect();
        if mutable_indices.is_empty() {
            self.emit_str("no mutable words\n");
            return;
        }
        let idx = mutable_indices[self.rng.next_usize(mutable_indices.len())];
        let dict_len = self.dictionary.len();
        if let Some(mut record) =
            mutation::mutate_entry(&mut self.dictionary[idx], &mut self.rng, dict_len)
        {
            record.word_index = idx;
            self.emit_str(&format!("mutated: {}\n", record.format()));
            self.mutation_history.push(record);
        } else {
            self.emit_str("mutation failed (no applicable strategy)\n");
        }
    }

    pub(crate) fn prim_mutate_word(&mut self) {
        let name = self.parse_until('"');
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(name);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(P_MUTATE_WORD_RT));
            }
        } else {
            self.do_mutate_word(&name);
        }
    }

    pub(crate) fn rt_mutate_word(&mut self) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let name = self.code_strings[idx].clone();
            self.do_mutate_word(&name);
        }
    }

    pub(crate) fn do_mutate_word(&mut self, name: &str) {
        let upper = name.to_uppercase();
        if let Some(idx) = self.find_word(&upper) {
            if !mutation::is_mutable(&self.dictionary[idx]) {
                self.emit_str(&format!("{}: not mutable (kernel word)\n", upper));
                return;
            }
            let dict_len = self.dictionary.len();
            if let Some(mut record) =
                mutation::mutate_entry(&mut self.dictionary[idx], &mut self.rng, dict_len)
            {
                record.word_index = idx;
                self.emit_str(&format!("mutated: {}\n", record.format()));
                self.mutation_history.push(record);
            } else {
                self.emit_str("mutation failed\n");
            }
        } else {
            self.emit_str(&format!("{}?\n", upper));
        }
    }

    pub(crate) fn prim_undo_mutate(&mut self) {
        if let Some(record) = self.mutation_history.pop() {
            if record.word_index < self.dictionary.len() {
                mutation::undo_mutation(&mut self.dictionary[record.word_index], &record);
                self.emit_str(&format!(
                    "undone: {} [{}]\n",
                    record.word_name,
                    record.strategy.label()
                ));
            }
        } else {
            self.emit_str("nothing to undo\n");
        }
    }

    pub(crate) fn prim_mutations(&mut self) {
        if self.mutation_history.is_empty() {
            self.emit_str("  (no mutations)\n");
        } else {
            let lines: Vec<String> = self.mutation_history.iter().map(|r| r.format()).collect();
            for line in &lines {
                self.emit_str(&format!("{}\n", line));
            }
        }
    }

}
