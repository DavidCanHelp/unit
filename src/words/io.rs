//! Host I/O primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Host I/O primitives
    // -----------------------------------------------------------------------

    pub(crate) fn log_io(&mut self, msg: &str) {
        self.io_log.push_back(msg.to_string());
        if self.io_log.len() > 50 {
            self.io_log.pop_front();
        }
    }

    pub(crate) fn check_sandbox_write(&self, op: &str) -> bool {
        if self.sandbox_active {
            eprintln!("{}: blocked by sandbox", op);
            false
        } else {
            true
        }
    }

    pub(crate) fn check_shell_allowed(&self) -> bool {
        if self.sandbox_active {
            eprintln!("SHELL: blocked by sandbox");
            return false;
        }
        if !self.shell_enabled {
            eprintln!("SHELL: disabled (use SHELL-ENABLE from REPL)");
            return false;
        }
        true
    }

    /// Common handler for all immediate I/O words. Parses the string,
    /// and in compile mode stores it for runtime dispatch.
    pub(crate) fn io_immediate(&mut self, op: Cell) {
        let s = self.parse_until('"');
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(s);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Literal(op));
                def.body.push(Instruction::Primitive(P_IO_RT));
            }
        } else {
            self.execute_io(op, &s);
        }
    }

    /// Runtime dispatch for compiled I/O words.
    pub(crate) fn rt_io(&mut self) {
        let op = self.pop();
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let s = self.code_strings[idx].clone();
            self.execute_io(op, &s);
        }
    }

    pub(crate) fn execute_io(&mut self, op: Cell, s: &str) {
        match op {
            0 => self.do_file_read(s),
            1 => self.do_file_write(s),
            2 => self.do_file_exists(s),
            3 => self.do_file_list(s),
            4 => self.do_file_delete(s),
            5 => self.do_http_get(s),
            6 => self.do_http_post(s),
            7 => self.do_shell(s),
            8 => self.do_env(s),
            _ => {}
        }
    }

    pub(crate) fn do_file_read(&mut self, path: &str) {
        self.log_io(&format!("FILE-READ {}", path));
        match io_words::file_read(path) {
            Ok(data) => {
                let len = data.len().min(self.memory.len() - PAD);
                for (i, &byte) in data.iter().take(len).enumerate() {
                    self.memory[PAD + i] = byte as Cell;
                }
                self.stack.push(PAD as Cell);
                self.stack.push(len as Cell);
            }
            Err(e) => {
                if !self.silent {
                    eprintln!("FILE-READ: {}", e);
                }
                self.stack.push(0);
                self.stack.push(0);
            }
        }
    }

    pub(crate) fn do_file_write(&mut self, path: &str) {
        if !self.check_sandbox_write("FILE-WRITE") {
            return;
        }
        let n = self.pop() as usize;
        let addr = self.pop() as usize;
        let mut data = Vec::with_capacity(n);
        for i in 0..n {
            if addr + i < self.memory.len() {
                data.push(self.memory[addr + i] as u8);
            }
        }
        self.log_io(&format!("FILE-WRITE {} ({} bytes)", path, n));
        if let Err(e) = io_words::file_write(path, &data) {
            if !self.silent {
                eprintln!("FILE-WRITE: {}", e);
            }
        }
    }

    pub(crate) fn do_file_exists(&mut self, path: &str) {
        self.log_io(&format!("FILE-EXISTS {}", path));
        let flag = if io_words::file_exists(path) { -1 } else { 0 };
        self.stack.push(flag);
    }

    pub(crate) fn do_file_list(&mut self, path: &str) {
        self.log_io(&format!("FILE-LIST {}", path));
        match io_words::file_list(path) {
            Ok(names) => {
                for name in &names {
                    self.emit_str(&format!("  {}\n", name));
                }
            }
            Err(e) => {
                if !self.silent {
                    eprintln!("FILE-LIST: {}", e);
                }
            }
        }
    }

    pub(crate) fn do_file_delete(&mut self, path: &str) {
        if !self.check_sandbox_write("FILE-DELETE") {
            self.stack.push(0);
            return;
        }
        self.log_io(&format!("FILE-DELETE {}", path));
        let flag = if io_words::file_delete(path).is_ok() {
            -1
        } else {
            0
        };
        self.stack.push(flag);
    }

    pub(crate) fn do_http_get(&mut self, url: &str) {
        self.log_io(&format!("HTTP-GET {}", url));
        match io_words::http_get(url) {
            Ok((body, status)) => {
                let len = body.len().min(self.memory.len() - PAD);
                for (i, &byte) in body.iter().take(len).enumerate() {
                    self.memory[PAD + i] = byte as Cell;
                }
                self.stack.push(PAD as Cell);
                self.stack.push(len as Cell);
                self.stack.push(status as Cell);
            }
            Err(e) => {
                if !self.silent {
                    eprintln!("HTTP-GET: {}", e);
                }
                self.stack.push(0);
                self.stack.push(0);
                self.stack.push(0);
            }
        }
    }

    pub(crate) fn do_http_post(&mut self, url: &str) {
        if !self.check_sandbox_write("HTTP-POST") {
            self.stack.push(0);
            self.stack.push(0);
            self.stack.push(0);
            return;
        }
        let n = self.pop() as usize;
        let addr = self.pop() as usize;
        let mut body = Vec::with_capacity(n);
        for i in 0..n {
            if addr + i < self.memory.len() {
                body.push(self.memory[addr + i] as u8);
            }
        }
        self.log_io(&format!("HTTP-POST {} ({} bytes)", url, n));
        match io_words::http_post(url, &body) {
            Ok((resp, status)) => {
                let len = resp.len().min(self.memory.len() - PAD);
                for (i, &byte) in resp.iter().take(len).enumerate() {
                    self.memory[PAD + i] = byte as Cell;
                }
                self.stack.push(PAD as Cell);
                self.stack.push(len as Cell);
                self.stack.push(status as Cell);
            }
            Err(e) => {
                if !self.silent {
                    eprintln!("HTTP-POST: {}", e);
                }
                self.stack.push(0);
                self.stack.push(0);
                self.stack.push(0);
            }
        }
    }

    pub(crate) fn do_shell(&mut self, cmd: &str) {
        if !self.check_shell_allowed() {
            self.stack.push(0);
            self.stack.push(0);
            self.stack.push(-1);
            return;
        }
        self.log_io(&format!("SHELL {}", cmd));
        match io_words::shell_exec(cmd) {
            Ok((stdout, exit_code)) => {
                let len = stdout.len().min(self.memory.len() - PAD);
                for (i, &byte) in stdout.iter().take(len).enumerate() {
                    self.memory[PAD + i] = byte as Cell;
                }
                self.stack.push(PAD as Cell);
                self.stack.push(len as Cell);
                self.stack.push(exit_code as Cell);
            }
            Err(e) => {
                if !self.silent {
                    eprintln!("SHELL: {}", e);
                }
                self.stack.push(0);
                self.stack.push(0);
                self.stack.push(-1);
            }
        }
    }

    pub(crate) fn do_env(&mut self, name: &str) {
        self.log_io(&format!("ENV {}", name));
        if let Some(val) = io_words::env_var(name) {
            let len = val.len().min(self.memory.len() - PAD);
            for (i, byte) in val.bytes().take(len).enumerate() {
                self.memory[PAD + i] = byte as Cell;
            }
            self.stack.push(PAD as Cell);
            self.stack.push(len as Cell);
        } else {
            self.stack.push(0);
            self.stack.push(0);
        }
    }

    pub(crate) fn prim_timestamp(&mut self) {
        self.stack.push(io_words::timestamp());
    }

    pub(crate) fn prim_sleep(&mut self) {
        let ms = self.pop();
        #[cfg(target_arch = "wasm32")]
        {
            let _ = ms;
            self.emit_str("sleep not available in browser\n");
        }
        #[cfg(not(target_arch = "wasm32"))]
        if ms > 0 {
            std::thread::sleep(Duration::from_millis(ms as u64));
        }
    }

    pub(crate) fn prim_io_log(&mut self) {
        if self.io_log.is_empty() {
            self.emit_str("  (no I/O operations logged)\n");
        } else {
            self.emit_str("--- I/O log ---\n");
            let entries: Vec<String> = self.io_log.iter().cloned().collect();
            for entry in &entries {
                self.emit_str(&format!("  {}\n", entry));
            }
            self.emit_str("---\n");
        }
    }

}
