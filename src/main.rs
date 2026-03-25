// unit — a software nanobot
// Minimal Forth interpreter that is also a self-replicating networked agent.
//
// This is the seed: a complete inner interpreter with kernel primitives,
// control flow, defining words, and a REPL. Mesh primitives are stubbed
// for now — the skeleton is here, the network comes next.

#[allow(dead_code)]
mod goals;
#[allow(dead_code)]
mod mesh;

use std::io::{self, BufRead, Read, Write};
use std::net::SocketAddr;

// ---------------------------------------------------------------------------
// Cell: the fundamental unit of data
// ---------------------------------------------------------------------------

pub type Cell = i64;

// ---------------------------------------------------------------------------
// Instruction: what lives inside a compiled word body
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Instruction {
    /// Call a primitive by id.
    Primitive(usize),
    /// Push a literal value onto the data stack.
    Literal(Cell),
    /// Call a word by its dictionary index.
    Call(usize),
    /// Push a string literal (for .").
    StringLit(String),
    /// Branch unconditionally (offset from current ip).
    Branch(i64),
    /// Branch if top-of-stack is zero.
    BranchIfZero(i64),
}

// ---------------------------------------------------------------------------
// Entry: one dictionary entry
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub immediate: bool,
    pub hidden: bool,
    pub body: Vec<Instruction>,
}

// ---------------------------------------------------------------------------
// Primitive IDs — assigned in registration order
// ---------------------------------------------------------------------------

const P_DUP: usize = 0;
const P_DROP: usize = 1;
const P_SWAP: usize = 2;
const P_OVER: usize = 3;
const P_ROT: usize = 4;
const P_FETCH: usize = 5;
const P_STORE: usize = 6;
const P_ADD: usize = 7;
const P_SUB: usize = 8;
const P_MUL: usize = 9;
const P_DIV: usize = 10;
const P_MOD: usize = 11;
const P_EQ: usize = 12;
const P_LT: usize = 13;
const P_GT: usize = 14;
const P_AND: usize = 15;
const P_OR: usize = 16;
const P_NOT: usize = 17;
const P_EMIT: usize = 18;
const P_KEY: usize = 19;
const P_CR: usize = 20;
const P_DOT: usize = 21;
const P_DOT_S: usize = 22;
const P_COLON: usize = 23;
const P_SEMICOLON: usize = 24;
const P_CREATE: usize = 25;
const P_DOES: usize = 26;
const P_VARIABLE: usize = 27;
const P_CONSTANT: usize = 28;
const P_IF: usize = 29;
const P_ELSE: usize = 30;
const P_THEN: usize = 31;
const P_DO: usize = 32;
const P_LOOP: usize = 33;
const P_BEGIN: usize = 34;
const P_UNTIL: usize = 35;
const P_WHILE: usize = 36;
const P_REPEAT: usize = 37;
const P_DOT_QUOTE: usize = 38;
const P_PAREN: usize = 39;
const P_BACKSLASH: usize = 40;
const P_WORDS: usize = 41;
const P_SEE: usize = 42;
const P_QUIT: usize = 43;
const P_BYE: usize = 44;
const P_SEND: usize = 45;
const P_RECV: usize = 46;
const P_PEERS: usize = 47;
const P_REPLICATE: usize = 48;
const P_MUTATE: usize = 49;
const P_RECURSE: usize = 50;
const P_MESH_STATUS: usize = 51;
const P_PROPOSE: usize = 52;
const P_LOAD: usize = 53;
const P_CAPACITY: usize = 54;
const P_ID: usize = 55;
const P_TYPE: usize = 56;
const P_GOAL: usize = 57;
const P_GOALS: usize = 58;
const P_TASKS: usize = 59;
const P_TASK_STATUS: usize = 60;
const P_CANCEL: usize = 61;
const P_STEER: usize = 62;
const P_REPORT: usize = 63;
const P_CLAIM: usize = 64;
const P_COMPLETE: usize = 65;
// Internal runtime primitives (not directly user-visible).
const P_DO_RT: usize = 100;
const P_LOOP_RT: usize = 101;

// ---------------------------------------------------------------------------
// VM: the Forth virtual machine
// ---------------------------------------------------------------------------

/// String output buffer address (Forth PAD equivalent).
const PAD: usize = 64000;

struct VM {
    stack: Vec<Cell>,
    rstack: Vec<Cell>,
    dictionary: Vec<Entry>,
    memory: Vec<Cell>,
    /// Next free address in the memory heap (bump allocator).
    here: usize,
    primitive_names: Vec<(String, usize)>,
    compiling: bool,
    current_def: Option<Entry>,
    input_buffer: String,
    input_pos: usize,
    running: bool,
    silent: bool,
    /// Mesh networking node (None if offline).
    mesh: Option<mesh::MeshNode>,
}

impl VM {
    fn new() -> Self {
        let mut vm = VM {
            stack: Vec::with_capacity(256),
            rstack: Vec::with_capacity(256),
            dictionary: Vec::new(),
            memory: vec![0; 65536],
            here: 1, // address 0 reserved
            primitive_names: Vec::new(),
            compiling: false,
            current_def: None,
            input_buffer: String::new(),
            input_pos: 0,
            running: true,
            silent: false,
            mesh: None,
        };
        vm.register_primitives();
        vm
    }

    // -----------------------------------------------------------------------
    // Primitive registration
    // -----------------------------------------------------------------------

    fn register_primitives(&mut self) {
        let prims: &[(&str, usize, bool)] = &[
            ("DUP", P_DUP, false),
            ("DROP", P_DROP, false),
            ("SWAP", P_SWAP, false),
            ("OVER", P_OVER, false),
            ("ROT", P_ROT, false),
            ("@", P_FETCH, false),
            ("!", P_STORE, false),
            ("+", P_ADD, false),
            ("-", P_SUB, false),
            ("*", P_MUL, false),
            ("/", P_DIV, false),
            ("MOD", P_MOD, false),
            ("=", P_EQ, false),
            ("<", P_LT, false),
            (">", P_GT, false),
            ("AND", P_AND, false),
            ("OR", P_OR, false),
            ("NOT", P_NOT, false),
            ("EMIT", P_EMIT, false),
            ("KEY", P_KEY, false),
            ("CR", P_CR, false),
            (".", P_DOT, false),
            (".S", P_DOT_S, false),
            (":", P_COLON, false),
            (";", P_SEMICOLON, true),
            ("CREATE", P_CREATE, false),
            ("DOES>", P_DOES, true),
            ("VARIABLE", P_VARIABLE, false),
            ("CONSTANT", P_CONSTANT, false),
            ("IF", P_IF, true),
            ("ELSE", P_ELSE, true),
            ("THEN", P_THEN, true),
            ("DO", P_DO, true),
            ("LOOP", P_LOOP, true),
            ("BEGIN", P_BEGIN, true),
            ("UNTIL", P_UNTIL, true),
            ("WHILE", P_WHILE, true),
            ("REPEAT", P_REPEAT, true),
            (".\"", P_DOT_QUOTE, true),
            ("(", P_PAREN, true),
            ("\\", P_BACKSLASH, true),
            ("WORDS", P_WORDS, false),
            ("SEE", P_SEE, false),
            ("QUIT", P_QUIT, false),
            ("BYE", P_BYE, false),
            ("SEND", P_SEND, false),
            ("RECV", P_RECV, false),
            ("PEERS", P_PEERS, false),
            ("REPLICATE", P_REPLICATE, false),
            ("MUTATE", P_MUTATE, false),
            ("RECURSE", P_RECURSE, true),
            ("MESH-STATUS", P_MESH_STATUS, false),
            ("PROPOSE", P_PROPOSE, false),
            ("LOAD", P_LOAD, false),
            ("CAPACITY", P_CAPACITY, false),
            ("ID", P_ID, false),
            ("TYPE", P_TYPE, false),
            ("GOAL\"", P_GOAL, true),
            ("GOALS", P_GOALS, false),
            ("TASKS", P_TASKS, false),
            ("TASK-STATUS", P_TASK_STATUS, false),
            ("CANCEL", P_CANCEL, false),
            ("STEER", P_STEER, false),
            ("REPORT", P_REPORT, false),
            ("CLAIM", P_CLAIM, false),
            ("COMPLETE", P_COMPLETE, false),
        ];

        for &(name, id, immediate) in prims {
            self.primitive_names.push((name.to_string(), id));
            self.dictionary.push(Entry {
                name: name.to_string(),
                immediate,
                hidden: false,
                body: vec![Instruction::Primitive(id)],
            });
        }
    }

    // -----------------------------------------------------------------------
    // Parser
    // -----------------------------------------------------------------------

    fn next_word(&mut self) -> Option<String> {
        let bytes = self.input_buffer.as_bytes();
        while self.input_pos < bytes.len() && (bytes[self.input_pos] as char).is_ascii_whitespace()
        {
            self.input_pos += 1;
        }
        if self.input_pos >= bytes.len() {
            return None;
        }
        let start = self.input_pos;
        while self.input_pos < bytes.len() && !(bytes[self.input_pos] as char).is_ascii_whitespace()
        {
            self.input_pos += 1;
        }
        Some(self.input_buffer[start..self.input_pos].to_string())
    }

    /// Read until a delimiter character (for comments and strings).
    fn parse_until(&mut self, delim: char) -> String {
        let bytes = self.input_buffer.as_bytes();
        // Skip one leading space if present.
        if self.input_pos < bytes.len() && bytes[self.input_pos] == b' ' {
            self.input_pos += 1;
        }
        let start = self.input_pos;
        while self.input_pos < bytes.len() && bytes[self.input_pos] as char != delim {
            self.input_pos += 1;
        }
        let result = self.input_buffer[start..self.input_pos].to_string();
        if self.input_pos < bytes.len() {
            self.input_pos += 1; // skip delimiter
        }
        result
    }

    // -----------------------------------------------------------------------
    // Dictionary lookup (search from end — most recent definition wins)
    // -----------------------------------------------------------------------

    fn find_word(&self, name: &str) -> Option<usize> {
        let upper = name.to_uppercase();
        self.dictionary
            .iter()
            .rposition(|e| !e.hidden && e.name == upper)
    }

    // -----------------------------------------------------------------------
    // Outer interpreter
    // -----------------------------------------------------------------------

    fn interpret_line(&mut self, line: &str) {
        self.input_buffer = line.to_string();
        self.input_pos = 0;

        while let Some(word) = self.next_word() {
            if !self.running {
                return;
            }
            let upper = word.to_uppercase();

            if self.compiling {
                self.compile_word(&upper);
            } else {
                self.interpret_word(&upper);
            }
        }
    }

    fn interpret_word(&mut self, word: &str) {
        if let Some(idx) = self.find_word(word) {
            self.execute_word(idx);
            return;
        }
        if let Ok(n) = word.parse::<Cell>() {
            self.stack.push(n);
            return;
        }
        if !self.silent {
            eprintln!("{}?", word);
        }
    }

    fn compile_word(&mut self, word: &str) {
        if let Some(idx) = self.find_word(word) {
            if self.dictionary[idx].immediate {
                self.execute_word(idx);
            } else {
                if let Some(ref mut def) = self.current_def {
                    def.body.push(Instruction::Call(idx));
                }
            }
            return;
        }
        if let Ok(n) = word.parse::<Cell>() {
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(n));
            }
            return;
        }
        eprintln!("{}?", word);
        self.compiling = false;
        self.current_def = None;
    }

    // -----------------------------------------------------------------------
    // Execution engine
    // -----------------------------------------------------------------------

    fn execute_word(&mut self, dict_idx: usize) {
        let body = self.dictionary[dict_idx].body.clone();
        self.execute_body(&body);
    }

    fn execute_body(&mut self, body: &[Instruction]) {
        let mut ip: usize = 0;
        while ip < body.len() {
            match &body[ip] {
                Instruction::Primitive(id) => match *id {
                    P_DO_RT => self.rt_do(),
                    P_LOOP_RT => self.rt_loop(),
                    _ => self.execute_primitive(*id),
                },
                Instruction::Literal(val) => {
                    self.stack.push(*val);
                }
                Instruction::Call(idx) => {
                    let callee = self.dictionary[*idx].body.clone();
                    self.execute_body(&callee);
                }
                Instruction::StringLit(s) => {
                    print!("{}", s);
                    let _ = io::stdout().flush();
                }
                Instruction::Branch(offset) => {
                    ip = (ip as i64 + offset) as usize;
                    continue;
                }
                Instruction::BranchIfZero(offset) => {
                    let flag = self.pop();
                    if flag == 0 {
                        ip = (ip as i64 + offset) as usize;
                        continue;
                    }
                }
            }
            ip += 1;
        }
    }

    // -----------------------------------------------------------------------
    // Primitive dispatch
    // -----------------------------------------------------------------------

    fn execute_primitive(&mut self, id: usize) {
        match id {
            P_DUP => self.prim_dup(),
            P_DROP => self.prim_drop(),
            P_SWAP => self.prim_swap(),
            P_OVER => self.prim_over(),
            P_ROT => self.prim_rot(),
            P_FETCH => self.prim_fetch(),
            P_STORE => self.prim_store(),
            P_ADD => self.prim_add(),
            P_SUB => self.prim_sub(),
            P_MUL => self.prim_mul(),
            P_DIV => self.prim_div(),
            P_MOD => self.prim_mod(),
            P_EQ => self.prim_eq(),
            P_LT => self.prim_lt(),
            P_GT => self.prim_gt(),
            P_AND => self.prim_and(),
            P_OR => self.prim_or(),
            P_NOT => self.prim_not(),
            P_EMIT => self.prim_emit(),
            P_KEY => self.prim_key(),
            P_CR => self.prim_cr(),
            P_DOT => self.prim_dot(),
            P_DOT_S => self.prim_dot_s(),
            P_COLON => self.prim_colon(),
            P_SEMICOLON => self.prim_semicolon(),
            P_CREATE => self.prim_create(),
            P_DOES => self.prim_does(),
            P_VARIABLE => self.prim_variable(),
            P_CONSTANT => self.prim_constant(),
            P_IF => self.prim_if(),
            P_ELSE => self.prim_else(),
            P_THEN => self.prim_then(),
            P_DO => self.prim_do(),
            P_LOOP => self.prim_loop(),
            P_BEGIN => self.prim_begin(),
            P_UNTIL => self.prim_until(),
            P_WHILE => self.prim_while(),
            P_REPEAT => self.prim_repeat(),
            P_DOT_QUOTE => self.prim_dot_quote(),
            P_PAREN => self.prim_paren(),
            P_BACKSLASH => self.prim_backslash(),
            P_WORDS => self.prim_words(),
            P_SEE => self.prim_see(),
            P_QUIT => self.prim_quit(),
            P_BYE => self.prim_bye(),
            P_SEND => self.prim_send(),
            P_RECV => self.prim_recv(),
            P_PEERS => self.prim_peers(),
            P_REPLICATE => self.prim_replicate(),
            P_MUTATE => self.prim_mutate(),
            P_RECURSE => self.prim_recurse(),
            P_MESH_STATUS => self.prim_mesh_status(),
            P_PROPOSE => self.prim_propose(),
            P_LOAD => self.prim_mesh_load(),
            P_CAPACITY => self.prim_mesh_capacity(),
            P_ID => self.prim_id(),
            P_TYPE => self.prim_type(),
            P_GOAL => self.prim_goal(),
            P_GOALS => self.prim_goals(),
            P_TASKS => self.prim_tasks(),
            P_TASK_STATUS => self.prim_task_status(),
            P_CANCEL => self.prim_cancel(),
            P_STEER => self.prim_steer(),
            P_REPORT => self.prim_report(),
            P_CLAIM => self.prim_claim(),
            P_COMPLETE => self.prim_complete(),
            _ => eprintln!("unknown primitive {}", id),
        }
    }

    // -----------------------------------------------------------------------
    // Stack helpers
    // -----------------------------------------------------------------------

    fn pop(&mut self) -> Cell {
        self.stack.pop().unwrap_or_else(|| {
            eprintln!("stack underflow");
            0
        })
    }

    fn rpop(&mut self) -> Cell {
        self.rstack.pop().unwrap_or_else(|| {
            eprintln!("return stack underflow");
            0
        })
    }

    // -----------------------------------------------------------------------
    // Stack primitives
    // -----------------------------------------------------------------------

    fn prim_dup(&mut self) {
        if let Some(&val) = self.stack.last() {
            self.stack.push(val);
        } else {
            eprintln!("stack underflow");
        }
    }

    fn prim_drop(&mut self) {
        self.pop();
    }

    fn prim_swap(&mut self) {
        let len = self.stack.len();
        if len < 2 {
            eprintln!("stack underflow");
            return;
        }
        self.stack.swap(len - 1, len - 2);
    }

    fn prim_over(&mut self) {
        let len = self.stack.len();
        if len < 2 {
            eprintln!("stack underflow");
            return;
        }
        self.stack.push(self.stack[len - 2]);
    }

    fn prim_rot(&mut self) {
        let len = self.stack.len();
        if len < 3 {
            eprintln!("stack underflow");
            return;
        }
        let val = self.stack.remove(len - 3);
        self.stack.push(val);
    }

    // -----------------------------------------------------------------------
    // Memory
    // -----------------------------------------------------------------------

    fn prim_fetch(&mut self) {
        let addr = self.pop() as usize;
        if addr < self.memory.len() {
            self.stack.push(self.memory[addr]);
        } else {
            eprintln!("invalid address: {}", addr);
            self.stack.push(0);
        }
    }

    fn prim_store(&mut self) {
        let addr = self.pop() as usize;
        let val = self.pop();
        if addr < self.memory.len() {
            self.memory[addr] = val;
        } else {
            eprintln!("invalid address: {}", addr);
        }
    }

    // -----------------------------------------------------------------------
    // Arithmetic
    // -----------------------------------------------------------------------

    fn prim_add(&mut self) {
        let b = self.pop();
        let a = self.pop();
        self.stack.push(a.wrapping_add(b));
    }

    fn prim_sub(&mut self) {
        let b = self.pop();
        let a = self.pop();
        self.stack.push(a.wrapping_sub(b));
    }

    fn prim_mul(&mut self) {
        let b = self.pop();
        let a = self.pop();
        self.stack.push(a.wrapping_mul(b));
    }

    fn prim_div(&mut self) {
        let b = self.pop();
        let a = self.pop();
        if b == 0 {
            eprintln!("division by zero");
            self.stack.push(0);
        } else {
            self.stack.push(a / b);
        }
    }

    fn prim_mod(&mut self) {
        let b = self.pop();
        let a = self.pop();
        if b == 0 {
            eprintln!("division by zero");
            self.stack.push(0);
        } else {
            self.stack.push(a % b);
        }
    }

    // -----------------------------------------------------------------------
    // Comparison (Forth convention: -1 = true, 0 = false)
    // -----------------------------------------------------------------------

    fn prim_eq(&mut self) {
        let b = self.pop();
        let a = self.pop();
        self.stack.push(if a == b { -1 } else { 0 });
    }

    fn prim_lt(&mut self) {
        let b = self.pop();
        let a = self.pop();
        self.stack.push(if a < b { -1 } else { 0 });
    }

    fn prim_gt(&mut self) {
        let b = self.pop();
        let a = self.pop();
        self.stack.push(if a > b { -1 } else { 0 });
    }

    // -----------------------------------------------------------------------
    // Logic
    // -----------------------------------------------------------------------

    fn prim_and(&mut self) {
        let b = self.pop();
        let a = self.pop();
        self.stack.push(a & b);
    }

    fn prim_or(&mut self) {
        let b = self.pop();
        let a = self.pop();
        self.stack.push(a | b);
    }

    fn prim_not(&mut self) {
        let a = self.pop();
        self.stack.push(if a == 0 { -1 } else { 0 });
    }

    // -----------------------------------------------------------------------
    // I/O
    // -----------------------------------------------------------------------

    fn prim_emit(&mut self) {
        let code = self.pop();
        if let Some(ch) = char::from_u32(code as u32) {
            print!("{}", ch);
            let _ = io::stdout().flush();
        }
    }

    fn prim_key(&mut self) {
        let stdin = io::stdin();
        let mut buf = [0u8; 1];
        if stdin.lock().read_exact(&mut buf).is_ok() {
            self.stack.push(buf[0] as Cell);
        } else {
            self.stack.push(-1);
        }
    }

    fn prim_cr(&mut self) {
        println!();
    }

    fn prim_dot(&mut self) {
        let val = self.pop();
        print!("{} ", val);
        let _ = io::stdout().flush();
    }

    fn prim_dot_s(&mut self) {
        print!("<{}> ", self.stack.len());
        for val in &self.stack {
            print!("{} ", val);
        }
        let _ = io::stdout().flush();
    }

    // -----------------------------------------------------------------------
    // Defining words
    // -----------------------------------------------------------------------

    fn prim_colon(&mut self) {
        if let Some(name) = self.next_word() {
            self.compiling = true;
            self.current_def = Some(Entry {
                name: name.to_uppercase(),
                immediate: false,
                hidden: false,
                body: Vec::new(),
            });
        } else {
            eprintln!("expected word name after :");
        }
    }

    fn prim_semicolon(&mut self) {
        if let Some(def) = self.current_def.take() {
            self.dictionary.push(def);
            self.compiling = false;
        } else {
            eprintln!("; without matching :");
        }
    }

    fn prim_create(&mut self) {
        if let Some(name) = self.next_word() {
            // CREATE'd word pushes the address of its data field.
            let addr = self.here as Cell;
            self.dictionary.push(Entry {
                name: name.to_uppercase(),
                immediate: false,
                hidden: false,
                body: vec![Instruction::Literal(addr)],
            });
        } else {
            eprintln!("expected word name after CREATE");
        }
    }

    fn prim_does(&mut self) {
        // Simplified DOES>: when encountered during compilation, everything
        // after DOES> in the current definition becomes the runtime behavior
        // appended to the most recently CREATE'd word. This is a seed-level
        // approximation — good enough for basic use.
    }

    fn prim_variable(&mut self) {
        if let Some(name) = self.next_word() {
            let addr = self.here;
            self.here += 1;
            self.dictionary.push(Entry {
                name: name.to_uppercase(),
                immediate: false,
                hidden: false,
                body: vec![Instruction::Literal(addr as Cell)],
            });
        } else {
            eprintln!("expected word name after VARIABLE");
        }
    }

    fn prim_constant(&mut self) {
        if let Some(name) = self.next_word() {
            let val = self.pop();
            self.dictionary.push(Entry {
                name: name.to_uppercase(),
                immediate: false,
                hidden: false,
                body: vec![Instruction::Literal(val)],
            });
        } else {
            eprintln!("expected word name after CONSTANT");
        }
    }

    // -----------------------------------------------------------------------
    // Control flow (immediate — compile branch instructions)
    // -----------------------------------------------------------------------

    fn prim_if(&mut self) {
        if let Some(ref mut def) = self.current_def {
            let fixup = def.body.len() as Cell;
            self.rstack.push(fixup);
            def.body.push(Instruction::BranchIfZero(0));
        }
    }

    fn prim_else(&mut self) {
        let if_fixup = self.rstack.pop().unwrap_or(0) as usize;
        if let Some(ref mut def) = self.current_def {
            let here = def.body.len();
            let offset = (here as i64 + 1) - if_fixup as i64;
            def.body[if_fixup] = Instruction::BranchIfZero(offset);
            def.body.push(Instruction::Branch(0));
            self.rstack.push(here as Cell);
        }
    }

    fn prim_then(&mut self) {
        let fixup = self.rstack.pop().unwrap_or(0) as usize;
        if let Some(ref mut def) = self.current_def {
            let here = def.body.len();
            let offset = here as i64 - fixup as i64;
            match def.body[fixup] {
                Instruction::BranchIfZero(_) => {
                    def.body[fixup] = Instruction::BranchIfZero(offset);
                }
                Instruction::Branch(_) => {
                    def.body[fixup] = Instruction::Branch(offset);
                }
                _ => {}
            }
        }
    }

    fn prim_do(&mut self) {
        if let Some(ref mut def) = self.current_def {
            def.body.push(Instruction::Primitive(P_DO_RT));
            let loop_start = def.body.len() as Cell;
            self.rstack.push(loop_start);
        }
    }

    fn prim_loop(&mut self) {
        let loop_start = self.rstack.pop().unwrap_or(0);
        if let Some(ref mut def) = self.current_def {
            def.body.push(Instruction::Primitive(P_LOOP_RT));
            let here = def.body.len();
            let offset = loop_start - here as i64;
            def.body.push(Instruction::BranchIfZero(offset));
        }
    }

    fn prim_begin(&mut self) {
        if let Some(ref def) = self.current_def {
            let here = def.body.len() as Cell;
            self.rstack.push(here);
        }
    }

    fn prim_until(&mut self) {
        let begin_addr = self.rstack.pop().unwrap_or(0);
        if let Some(ref mut def) = self.current_def {
            let here = def.body.len();
            let offset = begin_addr - here as i64;
            def.body.push(Instruction::BranchIfZero(offset));
        }
    }

    fn prim_while(&mut self) {
        if let Some(ref mut def) = self.current_def {
            let fixup = def.body.len() as Cell;
            self.rstack.push(fixup);
            def.body.push(Instruction::BranchIfZero(0));
        }
    }

    fn prim_repeat(&mut self) {
        let while_fixup = self.rstack.pop().unwrap_or(0) as usize;
        let begin_addr = self.rstack.pop().unwrap_or(0);
        if let Some(ref mut def) = self.current_def {
            let here = def.body.len();
            let offset = begin_addr - here as i64;
            def.body.push(Instruction::Branch(offset));
            let after = def.body.len();
            let while_offset = after as i64 - while_fixup as i64;
            def.body[while_fixup] = Instruction::BranchIfZero(while_offset);
        }
    }

    // -----------------------------------------------------------------------
    // DO...LOOP runtime helpers
    // -----------------------------------------------------------------------

    fn rt_do(&mut self) {
        let index = self.pop();
        let limit = self.pop();
        self.rstack.push(limit);
        self.rstack.push(index);
    }

    fn rt_loop(&mut self) {
        let index = self.rpop() + 1;
        let limit = *self.rstack.last().unwrap_or(&0);
        if index >= limit {
            self.rpop(); // remove limit
            self.stack.push(-1); // done — don't branch back
        } else {
            self.rstack.push(index);
            self.stack.push(0); // not done — branch back
        }
    }

    // -----------------------------------------------------------------------
    // Strings and comments
    // -----------------------------------------------------------------------

    fn prim_dot_quote(&mut self) {
        let s = self.parse_until('"');
        if self.compiling {
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::StringLit(s));
            }
        } else {
            print!("{}", s);
            let _ = io::stdout().flush();
        }
    }

    fn prim_paren(&mut self) {
        self.parse_until(')');
    }

    fn prim_backslash(&mut self) {
        self.input_pos = self.input_buffer.len();
    }

    // -----------------------------------------------------------------------
    // Introspection
    // -----------------------------------------------------------------------

    fn prim_words(&mut self) {
        let mut count = 0;
        for entry in self.dictionary.iter().rev() {
            if !entry.hidden {
                print!("{} ", entry.name);
                count += 1;
                if count % 12 == 0 {
                    println!();
                }
            }
        }
        println!();
    }

    fn prim_see(&mut self) {
        if let Some(name) = self.next_word() {
            let upper = name.to_uppercase();
            if let Some(idx) = self.find_word(&upper) {
                let entry = &self.dictionary[idx];
                print!(": {} ", entry.name);
                for instr in &entry.body {
                    match instr {
                        Instruction::Primitive(id) => {
                            let pname = self
                                .primitive_names
                                .iter()
                                .find(|(_, pid)| pid == id)
                                .map(|(n, _)| n.as_str())
                                .unwrap_or("?PRIM");
                            print!("{} ", pname);
                        }
                        Instruction::Literal(val) => print!("LIT({}) ", val),
                        Instruction::Call(cidx) => {
                            if *cidx < self.dictionary.len() {
                                print!("{} ", self.dictionary[*cidx].name);
                            } else {
                                print!("CALL({}) ", cidx);
                            }
                        }
                        Instruction::StringLit(s) => print!(".\" {}\" ", s),
                        Instruction::Branch(off) => print!("BRANCH({}) ", off),
                        Instruction::BranchIfZero(off) => print!("0BRANCH({}) ", off),
                    }
                }
                println!(";");
            } else {
                eprintln!("{}?", upper);
            }
        } else {
            eprintln!("expected word name after SEE");
        }
    }

    // -----------------------------------------------------------------------
    // REPL control
    // -----------------------------------------------------------------------

    fn prim_recurse(&mut self) {
        // Immediate: compile a self-call to the word currently being defined.
        if let Some(ref mut def) = self.current_def {
            let target = self.dictionary.len(); // where this def will land
            def.body.push(Instruction::Call(target));
        }
    }

    fn prim_quit(&mut self) {
        self.running = false;
    }

    fn prim_bye(&mut self) {
        self.running = false;
    }

    // -----------------------------------------------------------------------
    // Mesh primitives
    // -----------------------------------------------------------------------

    /// SEND ( addr n peer -- ) send n bytes from memory to all peers.
    /// The peer argument is reserved for future use (ignored, broadcast).
    fn prim_send(&mut self) {
        let _peer = self.pop(); // reserved
        let n = self.pop() as usize;
        let addr = self.pop() as usize;

        // Read n cells from memory, convert each to a byte.
        let mut data = Vec::with_capacity(n);
        for i in 0..n {
            let a = addr + i;
            if a < self.memory.len() {
                data.push(self.memory[a] as u8);
            }
        }

        if let Some(ref m) = self.mesh {
            m.send_data(&data);
        } else {
            eprintln!("SEND: mesh offline");
        }
    }

    /// RECV ( -- addr n peer ) receive next message.
    /// Copies data to PAD buffer. peer is the sender (0 = none).
    fn prim_recv(&mut self) {
        if let Some(ref m) = self.mesh {
            if let Some(msg) = m.recv_data() {
                // Copy data to PAD area in memory.
                let len = msg.data.len().min(self.memory.len() - PAD);
                for (i, &byte) in msg.data.iter().take(len).enumerate() {
                    self.memory[PAD + i] = byte as Cell;
                }
                self.stack.push(PAD as Cell);
                self.stack.push(len as Cell);
                // Push a nonzero "peer" value to indicate a message was received.
                self.stack.push(-1);
                return;
            }
        }
        // No message or mesh offline.
        self.stack.push(0);
        self.stack.push(0);
        self.stack.push(0);
    }

    /// PEERS ( -- n ) number of known peers.
    fn prim_peers(&mut self) {
        let count = self.mesh.as_ref().map_or(0, |m| m.peer_count());
        self.stack.push(count as Cell);
    }

    /// REPLICATE ( -- ) serialize this unit's state and broadcast to peers.
    fn prim_replicate(&mut self) {
        if let Some(ref m) = self.mesh {
            // Update load metric before serializing.
            let user_words = self.dictionary.len();
            m.set_load(user_words as u32);

            let goals = m.clone_goals();
            let state_bytes =
                mesh::serialize_state(&self.dictionary, &self.memory, self.here, Some(&goals));
            println!(
                "REPLICATE: serialized {} bytes ({} dictionary entries, {} memory cells)",
                state_bytes.len(),
                self.dictionary.len(),
                self.here
            );
            m.send_data(&state_bytes);
        } else {
            eprintln!("REPLICATE: mesh offline");
        }
    }

    /// MUTATE ( xt -- ) replace a word's definition at runtime.
    /// Stub: prints info about what would happen.
    fn prim_mutate(&mut self) {
        let xt = self.pop() as usize;
        if xt < self.dictionary.len() {
            let name = &self.dictionary[xt].name;
            eprintln!(
                "MUTATE: would replace definition of {} (xt={}). Not yet implemented.",
                name, xt
            );
        } else {
            eprintln!("MUTATE: invalid xt {}", xt);
        }
    }

    /// MESH-STATUS ( -- ) print mesh state.
    fn prim_mesh_status(&mut self) {
        if let Some(ref m) = self.mesh {
            m.status();
        } else {
            println!("mesh: offline");
        }
    }

    /// PROPOSE ( -- ) trigger a replication proposal via consensus.
    fn prim_propose(&mut self) {
        if let Some(ref m) = self.mesh {
            // Update load metric.
            let user_words = self.dictionary.len();
            m.set_load(user_words as u32);

            // Serialize state for the proposal.
            let goals = m.clone_goals();
            let state_bytes =
                mesh::serialize_state(&self.dictionary, &self.memory, self.here, Some(&goals));
            let reason = format!("load={} dict_size={}", user_words, self.dictionary.len());

            match m.propose_replicate(&reason, state_bytes) {
                Ok(()) => println!("PROPOSE: proposal submitted to mesh"),
                Err(e) => eprintln!("PROPOSE: {}", e),
            }
        } else {
            eprintln!("PROPOSE: mesh offline");
        }
    }

    /// LOAD ( -- n ) push current load metric.
    fn prim_mesh_load(&mut self) {
        let load = self.mesh.as_ref().map_or(0, |m| m.load());
        self.stack.push(load as Cell);
    }

    /// CAPACITY ( -- n ) push capacity threshold.
    fn prim_mesh_capacity(&mut self) {
        let cap = self.mesh.as_ref().map_or(0, |m| m.capacity());
        self.stack.push(cap as Cell);
    }

    /// ID ( -- addr n ) push this unit's ID string to PAD and return addr+len.
    fn prim_id(&mut self) {
        let id_str = self
            .mesh
            .as_ref()
            .map_or_else(|| "offline".to_string(), |m| m.id_hex().to_string());

        // Write to PAD area.
        let len = id_str.len().min(self.memory.len() - PAD);
        for (i, byte) in id_str.bytes().take(len).enumerate() {
            self.memory[PAD + i] = byte as Cell;
        }
        self.stack.push(PAD as Cell);
        self.stack.push(len as Cell);
    }

    /// TYPE ( addr n -- ) print n characters from memory starting at addr.
    fn prim_type(&mut self) {
        let n = self.pop() as usize;
        let addr = self.pop() as usize;
        for i in 0..n {
            let a = addr + i;
            if a < self.memory.len() {
                let ch = self.memory[a] as u8 as char;
                print!("{}", ch);
            }
        }
        let _ = io::stdout().flush();
    }

    // -----------------------------------------------------------------------
    // Goal primitives
    // -----------------------------------------------------------------------

    /// GOAL" <description>" ( priority -- goal-id ) submit a goal to the mesh.
    /// Immediate: parses the description string like .", then acts.
    /// Pushes the new goal ID onto the stack.
    fn prim_goal(&mut self) {
        let desc = self.parse_until('"');
        let priority = self.pop();
        if let Some(ref m) = self.mesh {
            let goal_id = m.create_goal(&desc, priority);
            m.set_load(self.dictionary.len() as u32);
            self.stack.push(goal_id as Cell);
            if !self.silent {
                println!("goal #{} created", goal_id);
            }
        } else {
            eprintln!("GOAL: mesh offline");
            self.stack.push(0);
        }
    }

    /// GOALS ( -- ) list all known goals.
    fn prim_goals(&mut self) {
        if let Some(ref m) = self.mesh {
            print!("{}", m.format_goals());
            let _ = io::stdout().flush();
        } else {
            println!("  (mesh offline)");
        }
    }

    /// TASKS ( -- ) list this unit's current task queue.
    fn prim_tasks(&mut self) {
        if let Some(ref m) = self.mesh {
            print!("{}", m.format_tasks());
            let _ = io::stdout().flush();
        } else {
            println!("  (mesh offline)");
        }
    }

    /// TASK-STATUS ( goal-id -- ) show task breakdown for a specific goal.
    fn prim_task_status(&mut self) {
        let goal_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            print!("{}", m.format_goal_tasks(goal_id));
            let _ = io::stdout().flush();
        } else {
            eprintln!("TASK-STATUS: mesh offline");
        }
    }

    /// CANCEL ( goal-id -- ) cancel a goal and all its tasks.
    fn prim_cancel(&mut self) {
        let goal_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            if m.cancel_goal(goal_id) {
                println!("goal #{} cancelled", goal_id);
            } else {
                eprintln!("goal #{} not found", goal_id);
            }
        } else {
            eprintln!("CANCEL: mesh offline");
        }
    }

    /// STEER ( goal-id priority -- ) change priority of a goal.
    fn prim_steer(&mut self) {
        let priority = self.pop();
        let goal_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            if m.steer_goal(goal_id, priority) {
                println!("goal #{} priority -> {}", goal_id, priority);
            } else {
                eprintln!("goal #{} not found", goal_id);
            }
        } else {
            eprintln!("STEER: mesh offline");
        }
    }

    /// REPORT ( -- ) mesh-wide progress summary.
    fn prim_report(&mut self) {
        if let Some(ref m) = self.mesh {
            print!("{}", m.format_report());
            let _ = io::stdout().flush();
        } else {
            println!("  (mesh offline)");
        }
    }

    /// CLAIM ( -- task-id ) claim the next available task, or 0 if none.
    fn prim_claim(&mut self) {
        if let Some(ref m) = self.mesh {
            if let Some((task_id, goal_id, desc)) = m.claim_task() {
                println!("claimed task #{} (goal #{}): {}", task_id, goal_id, desc);
                self.stack.push(task_id as Cell);
            } else {
                println!("no tasks available");
                self.stack.push(0);
            }
        } else {
            eprintln!("CLAIM: mesh offline");
            self.stack.push(0);
        }
    }

    /// COMPLETE ( task-id -- ) mark a task as done.
    fn prim_complete(&mut self) {
        let task_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            m.complete_task(task_id);
            println!("task #{} completed", task_id);
        } else {
            eprintln!("COMPLETE: mesh offline");
        }
    }

    /// Check if auto-replication should be triggered by goal load.
    fn check_auto_replicate(&mut self) {
        let should = self
            .mesh
            .as_ref()
            .map_or(false, |m| m.should_auto_replicate());
        if should {
            if let Some(ref m) = self.mesh {
                m.clear_auto_replicate();
                m.set_load(self.dictionary.len() as u32);
                let goals = m.clone_goals();
                let state_bytes = mesh::serialize_state(
                    &self.dictionary,
                    &self.memory,
                    self.here,
                    Some(&goals),
                );
                let reason = format!(
                    "auto: goal_load dict={}",
                    self.dictionary.len()
                );
                match m.propose_replicate(&reason, state_bytes) {
                    Ok(()) => println!("auto-replication proposed"),
                    Err(e) => {
                        if !self.silent {
                            eprintln!("auto-replicate: {}", e);
                        }
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Load prelude
    // -----------------------------------------------------------------------

    fn load_prelude(&mut self) {
        let prelude = include_str!("prelude.fs");
        self.silent = true;
        for line in prelude.lines() {
            self.interpret_line(line);
            if !self.running {
                break;
            }
        }
        self.silent = false;
    }

    // -----------------------------------------------------------------------
    // REPL
    // -----------------------------------------------------------------------

    fn repl(&mut self) {
        let stdin = io::stdin();
        let mut stdout = io::stdout();

        let _ = write!(stdout, "> ");
        let _ = stdout.flush();

        for line in stdin.lock().lines() {
            match line {
                Ok(line) => {
                    self.interpret_line(&line);
                    if !self.running {
                        break;
                    }
                    if !self.compiling {
                        self.check_auto_replicate();
                    }
                    if self.compiling {
                        let _ = write!(stdout, "  ");
                    } else {
                        let _ = write!(stdout, " ok\n> ");
                    }
                    let _ = stdout.flush();
                }
                Err(_) => break,
            }
        }
        println!();
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let mut vm = VM::new();

    // Parse mesh configuration from environment.
    let port: u16 = std::env::var("UNIT_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let seed_peers: Vec<SocketAddr> = std::env::var("UNIT_PEERS")
        .unwrap_or_default()
        .split(',')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    // Start the mesh networking layer.
    match mesh::MeshNode::start(port, seed_peers) {
        Ok(node) => {
            vm.mesh = Some(node);
        }
        Err(e) => {
            eprintln!("mesh: failed to start: {}", e);
        }
    }

    // Update the load metric (dictionary size) now that mesh is running.
    if let Some(ref m) = vm.mesh {
        m.set_load(vm.dictionary.len() as u32);
    }

    vm.load_prelude();
    vm.repl();
}
