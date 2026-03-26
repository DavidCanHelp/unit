// unit — a software nanobot
// Minimal Forth interpreter that is also a self-replicating networked agent.
//
// This is the seed: a complete inner interpreter with kernel primitives,
// control flow, defining words, and a REPL. Mesh primitives are stubbed
// for now — the skeleton is here, the network comes next.

#[allow(dead_code)]
mod fitness;
#[allow(dead_code)]
mod goals;
#[allow(dead_code)]
mod io_words;
#[allow(dead_code)]
mod mesh;
#[allow(dead_code)]
mod mutation;

use std::collections::{HashSet, VecDeque};
use std::io::{self, BufRead, Read, Write};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

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
const P_GOAL_EXEC: usize = 66;
const P_EVAL: usize = 67;
const P_RESULT: usize = 68;
const P_AUTO_CLAIM: usize = 69;
const P_TIMEOUT: usize = 70;
const P_GOAL_RESULT: usize = 71;
// Host I/O (immediate — parse string at compile time)
const P_FILE_READ: usize = 72;
const P_FILE_WRITE: usize = 73;
const P_FILE_EXISTS: usize = 74;
const P_FILE_LIST: usize = 75;
const P_FILE_DELETE: usize = 76;
const P_HTTP_GET: usize = 77;
const P_HTTP_POST: usize = 78;
const P_SHELL: usize = 79;
const P_ENV: usize = 80;
// Host I/O (non-immediate)
const P_TIMESTAMP: usize = 81;
const P_SLEEP: usize = 82;
const P_SANDBOX_ON: usize = 83;
const P_SANDBOX_OFF: usize = 84;
const P_IO_LOG: usize = 85;
// Mutation
const P_MUTATE_RAND: usize = 86;
const P_MUTATE_WORD: usize = 87;
const P_UNDO_MUTATE: usize = 88;
const P_MUTATIONS: usize = 89;
// Fitness / Evolution
const P_FITNESS: usize = 90;
const P_LEADERBOARD: usize = 91;
const P_RATE: usize = 92;
const P_EVOLVE: usize = 93;
const P_AUTO_EVOLVE: usize = 94;
const P_BENCHMARK: usize = 95;
// Trust / Security
const P_TRUST: usize = 96;
const P_TRUST_ALL: usize = 97;
const P_TRUST_NONE: usize = 98;
const P_SHELL_ENABLE: usize = 99;
// Internal runtime primitives (not directly user-visible).
const P_DO_RT: usize = 100;
const P_LOOP_RT: usize = 101;
const P_GOAL_EXEC_RT: usize = 102;
const P_IO_RT: usize = 103;
const P_MUTATE_WORD_RT: usize = 104;
const P_BENCHMARK_RT: usize = 105;

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
    /// When set, output goes here instead of stdout (sandbox mode).
    output_buffer: Option<String>,
    /// Execution deadline for sandboxed task execution.
    deadline: Option<Instant>,
    /// Set when execution exceeds the deadline.
    timed_out: bool,
    /// Configurable execution timeout in seconds.
    execution_timeout: u64,
    /// When true, automatically claim and execute incoming tasks.
    auto_claim: bool,
    /// Stored code strings for compiled GOAL{ ... } (indexed by Literal).
    code_strings: Vec<String>,
    // --- Sandbox / Security ---
    sandbox_active: bool,
    shell_enabled: bool,
    trusted_peers: HashSet<[u8; 8]>,
    io_log: VecDeque<String>,
    // --- Mutation ---
    mutation_history: Vec<mutation::MutationRecord>,
    rng: mutation::SimpleRng,
    // --- Fitness ---
    fitness: fitness::FitnessTracker,
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
            output_buffer: None,
            deadline: None,
            timed_out: false,
            execution_timeout: 10,
            auto_claim: false,
            code_strings: Vec::new(),
            sandbox_active: false,
            shell_enabled: false,
            trusted_peers: HashSet::new(),
            io_log: VecDeque::new(),
            mutation_history: Vec::new(),
            rng: mutation::SimpleRng::new(0), // re-seeded from node ID in main()
            fitness: fitness::FitnessTracker::new(),
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
            ("GOAL{", P_GOAL_EXEC, true),
            ("EVAL\"", P_EVAL, true),
            ("RESULT", P_RESULT, false),
            ("AUTO-CLAIM", P_AUTO_CLAIM, false),
            ("TIMEOUT", P_TIMEOUT, false),
            ("GOAL-RESULT", P_GOAL_RESULT, false),
            // Host I/O
            ("FILE-READ\"", P_FILE_READ, true),
            ("FILE-WRITE\"", P_FILE_WRITE, true),
            ("FILE-EXISTS\"", P_FILE_EXISTS, true),
            ("FILE-LIST\"", P_FILE_LIST, true),
            ("FILE-DELETE\"", P_FILE_DELETE, true),
            ("HTTP-GET\"", P_HTTP_GET, true),
            ("HTTP-POST\"", P_HTTP_POST, true),
            ("SHELL\"", P_SHELL, true),
            ("ENV\"", P_ENV, true),
            ("TIMESTAMP", P_TIMESTAMP, false),
            ("SLEEP", P_SLEEP, false),
            ("SANDBOX-ON", P_SANDBOX_ON, false),
            ("SANDBOX-OFF", P_SANDBOX_OFF, false),
            ("IO-LOG", P_IO_LOG, false),
            // Mutation
            ("MUTATE", P_MUTATE_RAND, false),
            ("MUTATE-WORD\"", P_MUTATE_WORD, true),
            ("UNDO-MUTATE", P_UNDO_MUTATE, false),
            ("MUTATIONS", P_MUTATIONS, false),
            // Fitness / Evolution
            ("FITNESS", P_FITNESS, false),
            ("LEADERBOARD", P_LEADERBOARD, false),
            ("RATE", P_RATE, false),
            ("EVOLVE", P_EVOLVE, false),
            ("AUTO-EVOLVE", P_AUTO_EVOLVE, false),
            ("BENCHMARK\"", P_BENCHMARK, true),
            // Trust / Security
            ("TRUST", P_TRUST, false),
            ("TRUST-ALL", P_TRUST_ALL, false),
            ("TRUST-NONE", P_TRUST_NONE, false),
            ("SHELL-ENABLE", P_SHELL_ENABLE, false),
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
            // Check for timeout (sandbox execution).
            if self.timed_out {
                return;
            }
            if let Some(deadline) = self.deadline {
                if Instant::now() > deadline {
                    self.timed_out = true;
                    return;
                }
            }

            match &body[ip] {
                Instruction::Primitive(id) => match *id {
                    P_DO_RT => self.rt_do(),
                    P_LOOP_RT => self.rt_loop(),
                    P_GOAL_EXEC_RT => self.rt_goal_exec(),
                    P_IO_RT => self.rt_io(),
                    P_MUTATE_WORD_RT => self.rt_mutate_word(),
                    P_BENCHMARK_RT => self.rt_benchmark(),
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
                    self.emit_str(s);
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
            P_GOAL_EXEC => self.prim_goal_exec(),
            P_EVAL => self.prim_eval(),
            P_RESULT => self.prim_result(),
            P_AUTO_CLAIM => self.prim_auto_claim(),
            P_TIMEOUT => self.prim_timeout(),
            P_GOAL_RESULT => self.prim_goal_result(),
            // Host I/O
            P_FILE_READ => self.io_immediate(0),
            P_FILE_WRITE => self.io_immediate(1),
            P_FILE_EXISTS => self.io_immediate(2),
            P_FILE_LIST => self.io_immediate(3),
            P_FILE_DELETE => self.io_immediate(4),
            P_HTTP_GET => self.io_immediate(5),
            P_HTTP_POST => self.io_immediate(6),
            P_SHELL => self.io_immediate(7),
            P_ENV => self.io_immediate(8),
            P_TIMESTAMP => self.prim_timestamp(),
            P_SLEEP => self.prim_sleep(),
            P_SANDBOX_ON => { self.sandbox_active = true; self.emit_str("sandbox: ON\n"); }
            P_SANDBOX_OFF => { self.sandbox_active = false; self.emit_str("sandbox: OFF\n"); }
            P_IO_LOG => self.prim_io_log(),
            // Mutation
            P_MUTATE_RAND => self.prim_mutate_rand(),
            P_MUTATE_WORD => self.prim_mutate_word(),
            P_UNDO_MUTATE => self.prim_undo_mutate(),
            P_MUTATIONS => self.prim_mutations(),
            // Fitness
            P_FITNESS => { let s = self.fitness.score; self.stack.push(s); }
            P_LEADERBOARD => self.prim_leaderboard(),
            P_RATE => self.prim_rate(),
            P_EVOLVE => self.prim_evolve(),
            P_AUTO_EVOLVE => self.prim_auto_evolve(),
            P_BENCHMARK => self.prim_benchmark(),
            // Trust
            P_TRUST => self.prim_trust(),
            P_TRUST_ALL => { self.trusted_peers.clear(); self.emit_str("trust: ALL (cleared)\n"); }
            P_TRUST_NONE => { self.trusted_peers.clear(); self.emit_str("trust: NONE\n"); }
            P_SHELL_ENABLE => { self.shell_enabled = !self.shell_enabled;
                self.emit_str(&format!("shell: {}\n", if self.shell_enabled { "ENABLED" } else { "DISABLED" })); }
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
    // Output helpers — route output to buffer (sandbox) or stdout
    // -----------------------------------------------------------------------

    fn emit_char(&mut self, ch: char) {
        if let Some(ref mut buf) = self.output_buffer {
            buf.push(ch);
        } else {
            print!("{}", ch);
            let _ = io::stdout().flush();
        }
    }

    fn emit_str(&mut self, s: &str) {
        if let Some(ref mut buf) = self.output_buffer {
            buf.push_str(s);
        } else {
            print!("{}", s);
            let _ = io::stdout().flush();
        }
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
            self.emit_char(ch);
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
        self.emit_str("\n");
    }

    fn prim_dot(&mut self) {
        let val = self.pop();
        self.emit_str(&format!("{} ", val));
    }

    fn prim_dot_s(&mut self) {
        let s = format!("<{}> ", self.stack.len());
        self.emit_str(&s);
        let vals: Vec<String> = self.stack.iter().map(|v| format!("{} ", v)).collect();
        for v in &vals {
            self.emit_str(v);
        }
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
            self.emit_str(&s);
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
        let names: Vec<String> = self
            .dictionary
            .iter()
            .rev()
            .filter(|e| !e.hidden)
            .map(|e| e.name.clone())
            .collect();
        let mut count = 0;
        for name in &names {
            self.emit_str(&format!("{} ", name));
            count += 1;
            if count % 12 == 0 {
                self.emit_str("\n");
            }
        }
        self.emit_str("\n");
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
            let s = m.format_status();
            self.emit_str(&s);
        } else {
            self.emit_str("mesh: offline\n");
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
                self.emit_char(self.memory[a] as u8 as char);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Sandbox execution engine
    // -----------------------------------------------------------------------

    /// Parse balanced braces from the input buffer. Returns the content
    /// between the opening { (already consumed) and the closing }.
    fn parse_balanced_braces(&mut self) -> String {
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
    fn execute_sandbox(&mut self, code: &str) -> goals::TaskResult {
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

        // Set up sandbox.
        self.stack = Vec::with_capacity(256);
        self.rstack = Vec::with_capacity(256);
        self.output_buffer = Some(String::new());
        self.silent = true;
        self.sandbox_active = true; // remote code always sandboxed
        self.compiling = false;
        self.timed_out = false;
        self.deadline = Some(Instant::now() + Duration::from_secs(self.execution_timeout));

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
        let success = !self.timed_out;
        let error = if self.timed_out {
            Some(format!("execution timeout ({}s)", self.execution_timeout))
        } else {
            None
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
        self.running = true; // task execution must not kill the unit

        goals::TaskResult {
            stack_snapshot,
            output,
            success,
            error,
        }
    }

    // -----------------------------------------------------------------------
    // Goal primitives
    // -----------------------------------------------------------------------

    /// GOAL" <description>" ( priority -- goal-id ) submit a description-only goal.
    fn prim_goal(&mut self) {
        let desc = self.parse_until('"');
        let priority = self.pop();
        if let Some(ref m) = self.mesh {
            let goal_id = m.create_goal(&desc, priority, None);
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
            let s = m.format_goals();
            self.emit_str(&s);
        } else {
            self.emit_str("  (mesh offline)\n");
        }
    }

    /// TASKS ( -- ) list this unit's current task queue.
    fn prim_tasks(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_tasks();
            self.emit_str(&s);
        } else {
            self.emit_str("  (mesh offline)\n");
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
    /// CLAIM ( -- task-id ) claim and execute the next available task.
    fn prim_claim(&mut self) {
        // Extract claimed task info (releases mesh borrow).
        let claimed = self.mesh.as_ref().and_then(|m| m.claim_task());

        if let Some((task_id, goal_id, desc)) = claimed {
            println!("claimed task #{} (goal #{}): {}", task_id, goal_id, desc);
            // Check if the parent goal has executable code.
            let code = self.mesh.as_ref().and_then(|m| m.goal_code(goal_id));
            if let Some(code) = code {
                let result = self.execute_sandbox(&code);
                if !result.output.is_empty() {
                    println!("  output: {}", result.output.trim_end());
                }
                if !result.stack_snapshot.is_empty() {
                    print!("  stack: ");
                    for v in &result.stack_snapshot {
                        print!("{} ", v);
                    }
                    println!();
                }
                if !result.success {
                    println!(
                        "  FAILED: {}",
                        result.error.as_deref().unwrap_or("unknown")
                    );
                }
                if let Some(ref m) = self.mesh {
                    m.complete_task_with_result(task_id, result);
                }
            }
            self.stack.push(task_id as Cell);
        } else {
            println!("no tasks available");
            self.stack.push(0);
        }
    }

    /// COMPLETE ( task-id -- ) mark a task as done.
    fn prim_complete(&mut self) {
        let task_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            m.complete_task_with_result(task_id, goals::TaskResult {
                stack_snapshot: vec![],
                output: String::new(),
                success: true,
                error: None,
            });
            println!("task #{} completed", task_id);
        } else {
            eprintln!("COMPLETE: mesh offline");
        }
    }

    /// GOAL{ <forth code> } ( priority -- goal-id ) submit an executable goal.
    /// Immediate: parses the code at compile time. In compile mode, stores
    /// the code in a side table and compiles Literal(index) + Primitive(RT).
    fn prim_goal_exec(&mut self) {
        let code = self.parse_balanced_braces();
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(code);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(P_GOAL_EXEC_RT));
            }
        } else {
            self.create_exec_goal(&code);
        }
    }

    /// Runtime primitive for compiled GOAL{. Pops code-string index from
    /// stack, looks up the code, then creates the goal.
    fn rt_goal_exec(&mut self) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let code = self.code_strings[idx].clone();
            self.create_exec_goal(&code);
        } else {
            eprintln!("GOAL{{: invalid code index");
            self.stack.push(0);
        }
    }

    fn create_exec_goal(&mut self, code: &str) {
        let priority = self.pop();
        if let Some(ref m) = self.mesh {
            let goal_id = m.create_goal(code, priority, Some(code.to_string()));
            m.set_load(self.dictionary.len() as u32);
            self.stack.push(goal_id as Cell);
            if !self.silent {
                println!(
                    "goal #{} created [exec]: {}",
                    goal_id,
                    code.chars().take(60).collect::<String>()
                );
            }
        } else {
            eprintln!("GOAL: mesh offline");
            self.stack.push(0);
        }
    }

    /// EVAL" <forth code>" ( -- ) evaluate a string of Forth immediately.
    fn prim_eval(&mut self) {
        let code = self.parse_until('"');
        self.interpret_line(&code);
    }

    /// RESULT ( task-id -- ) display the result of a completed task.
    fn prim_result(&mut self) {
        let task_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            let s = m.format_task_result(task_id);
            self.emit_str(&s);
        } else {
            eprintln!("RESULT: mesh offline");
        }
    }

    /// AUTO-CLAIM ( -- ) toggle automatic task claiming and execution.
    fn prim_auto_claim(&mut self) {
        self.auto_claim = !self.auto_claim;
        if !self.silent {
            println!(
                "auto-claim: {}",
                if self.auto_claim { "ON" } else { "OFF" }
            );
        }
    }

    /// TIMEOUT ( seconds -- ) set execution timeout for sandboxed tasks.
    fn prim_timeout(&mut self) {
        let secs = self.pop();
        if secs > 0 {
            self.execution_timeout = secs as u64;
            if !self.silent {
                println!("execution timeout: {}s", self.execution_timeout);
            }
        } else {
            eprintln!("TIMEOUT: must be > 0");
        }
    }

    /// GOAL-RESULT ( goal-id -- ) show combined results from all tasks of a goal.
    fn prim_goal_result(&mut self) {
        let goal_id = self.pop() as u64;
        if let Some(ref m) = self.mesh {
            let s = m.format_goal_result(goal_id);
            self.emit_str(&s);
        } else {
            eprintln!("GOAL-RESULT: mesh offline");
        }
    }

    /// Check for and execute auto-claimed tasks.
    fn check_auto_claim(&mut self) {
        if !self.auto_claim {
            return;
        }
        // Extract the claimed task info while borrowing mesh immutably.
        let claimed = self
            .mesh
            .as_ref()
            .and_then(|m| m.claim_executable_task());

        if let Some((task_id, goal_id, desc, code)) = claimed {
            println!(
                "[auto] claimed task #{} (goal #{}): {}",
                task_id, goal_id, desc.chars().take(50).collect::<String>()
            );
            // Execute in sandbox with timing.
            let start = Instant::now();
            let result = self.execute_sandbox(&code);
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let success = result.success;

            // Record fitness.
            if success {
                self.fitness.record_success(elapsed_ms);
            } else {
                self.fitness.record_failure();
            }
            if !result.output.is_empty() {
                println!("[auto] output: {}", result.output.trim_end());
            }
            if !result.stack_snapshot.is_empty() {
                print!("[auto] stack: ");
                for v in &result.stack_snapshot {
                    print!("{} ", v);
                }
                println!();
            }
            if !success {
                println!(
                    "[auto] FAILED: {}",
                    result.error.as_deref().unwrap_or("unknown")
                );
            }
            // Now borrow mesh again to broadcast result.
            if let Some(ref m) = self.mesh {
                m.complete_task_with_result(task_id, result);
                m.set_fitness(self.fitness.score);
            }
            println!("[auto] task #{} done", task_id);
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
    // Host I/O primitives
    // -----------------------------------------------------------------------

    fn log_io(&mut self, msg: &str) {
        self.io_log.push_back(msg.to_string());
        if self.io_log.len() > 50 {
            self.io_log.pop_front();
        }
    }

    fn check_sandbox_write(&self, op: &str) -> bool {
        if self.sandbox_active {
            eprintln!("{}: blocked by sandbox", op);
            false
        } else {
            true
        }
    }

    fn check_shell_allowed(&self) -> bool {
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
    fn io_immediate(&mut self, op: Cell) {
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
    fn rt_io(&mut self) {
        let op = self.pop();
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let s = self.code_strings[idx].clone();
            self.execute_io(op, &s);
        }
    }

    fn execute_io(&mut self, op: Cell, s: &str) {
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

    fn do_file_read(&mut self, path: &str) {
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

    fn do_file_write(&mut self, path: &str) {
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

    fn do_file_exists(&mut self, path: &str) {
        self.log_io(&format!("FILE-EXISTS {}", path));
        let flag = if io_words::file_exists(path) { -1 } else { 0 };
        self.stack.push(flag);
    }

    fn do_file_list(&mut self, path: &str) {
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

    fn do_file_delete(&mut self, path: &str) {
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

    fn do_http_get(&mut self, url: &str) {
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

    fn do_http_post(&mut self, url: &str) {
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

    fn do_shell(&mut self, cmd: &str) {
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

    fn do_env(&mut self, name: &str) {
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

    fn prim_timestamp(&mut self) {
        self.stack.push(io_words::timestamp());
    }

    fn prim_sleep(&mut self) {
        let ms = self.pop();
        if ms > 0 {
            std::thread::sleep(Duration::from_millis(ms as u64));
        }
    }

    fn prim_io_log(&mut self) {
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

    // -----------------------------------------------------------------------
    // Mutation primitives
    // -----------------------------------------------------------------------

    fn prim_mutate_rand(&mut self) {
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
        if let Some(mut record) = mutation::mutate_entry(&mut self.dictionary[idx], &mut self.rng, dict_len) {
            record.word_index = idx;
            self.emit_str(&format!("mutated: {}\n", record.format()));
            self.mutation_history.push(record);
        } else {
            self.emit_str("mutation failed (no applicable strategy)\n");
        }
    }

    fn prim_mutate_word(&mut self) {
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

    fn rt_mutate_word(&mut self) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let name = self.code_strings[idx].clone();
            self.do_mutate_word(&name);
        }
    }

    fn do_mutate_word(&mut self, name: &str) {
        let upper = name.to_uppercase();
        if let Some(idx) = self.find_word(&upper) {
            if !mutation::is_mutable(&self.dictionary[idx]) {
                self.emit_str(&format!("{}: not mutable (kernel word)\n", upper));
                return;
            }
            let dict_len = self.dictionary.len();
            if let Some(mut record) = mutation::mutate_entry(&mut self.dictionary[idx], &mut self.rng, dict_len) {
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

    fn prim_undo_mutate(&mut self) {
        if let Some(record) = self.mutation_history.pop() {
            if record.word_index < self.dictionary.len() {
                mutation::undo_mutation(&mut self.dictionary[record.word_index], &record);
                self.emit_str(&format!("undone: {} [{}]\n", record.word_name, record.strategy.label()));
            }
        } else {
            self.emit_str("nothing to undo\n");
        }
    }

    fn prim_mutations(&mut self) {
        if self.mutation_history.is_empty() {
            self.emit_str("  (no mutations)\n");
        } else {
            let lines: Vec<String> = self.mutation_history.iter().map(|r| r.format()).collect();
            for line in &lines {
                self.emit_str(&format!("{}\n", line));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Fitness / Evolution primitives
    // -----------------------------------------------------------------------

    fn prim_leaderboard(&mut self) {
        if let Some(ref m) = self.mesh {
            let peer_fitness = m.peer_fitness_list();
            let s = fitness::format_leaderboard(&m.id_bytes(), self.fitness.score, &peer_fitness);
            self.emit_str(&s);
        } else {
            self.emit_str(&format!("  (offline) score={}\n", self.fitness.score));
        }
    }

    fn prim_rate(&mut self) {
        let score = self.pop();
        let _task_id = self.pop() as u64;
        // For now, rating adjusts local fitness (the rated peer would
        // receive the rating via gossip in a fuller implementation).
        self.fitness.record_rating(score);
        self.emit_str(&format!("rated: fitness adjusted by {}\n", score));
    }

    fn prim_evolve(&mut self) {
        self.do_evolve();
    }

    fn prim_auto_evolve(&mut self) {
        self.fitness.auto_evolve = !self.fitness.auto_evolve;
        self.emit_str(&format!(
            "auto-evolve: {}\n",
            if self.fitness.auto_evolve { "ON" } else { "OFF" }
        ));
    }

    fn prim_benchmark(&mut self) {
        let code = self.parse_until('"');
        if self.compiling {
            let idx = self.code_strings.len();
            self.code_strings.push(code);
            if let Some(ref mut def) = self.current_def {
                def.body.push(Instruction::Literal(idx as Cell));
                def.body.push(Instruction::Primitive(P_BENCHMARK_RT));
            }
        } else {
            self.fitness.benchmark_code = Some(code.clone());
            self.emit_str(&format!("benchmark set: {}\n", code.chars().take(50).collect::<String>()));
        }
    }

    fn rt_benchmark(&mut self) {
        let idx = self.pop() as usize;
        if idx < self.code_strings.len() {
            let code = self.code_strings[idx].clone();
            self.fitness.benchmark_code = Some(code.clone());
            self.emit_str(&format!("benchmark set: {}\n", code.chars().take(50).collect::<String>()));
        }
    }

    fn prim_trust(&mut self) {
        // Expect a node ID on the stack (as a number).
        let id_val = self.pop() as u64;
        let id_bytes = id_val.to_be_bytes();
        self.trusted_peers.insert(id_bytes);
        self.emit_str(&format!("trusted: {:016x}\n", id_val));
    }

    /// Run one evolution cycle.
    fn do_evolve(&mut self) {
        // Get mesh average fitness.
        let avg_fitness = self
            .mesh
            .as_ref()
            .map(|m| {
                let peers = m.peer_fitness_list();
                if peers.is_empty() {
                    self.fitness.score
                } else {
                    let total: i64 = peers.iter().map(|p| p.score).sum::<i64>() + self.fitness.score;
                    total / (peers.len() as i64 + 1)
                }
            })
            .unwrap_or(self.fitness.score);

        // Run benchmark before mutation.
        let before_score = self.run_benchmark();

        // Apply a random mutation.
        let mutable_indices: Vec<usize> = self
            .dictionary
            .iter()
            .enumerate()
            .filter(|(_, e)| mutation::is_mutable(e))
            .map(|(i, _)| i)
            .collect();
        if mutable_indices.is_empty() {
            self.emit_str("evolve: no mutable words\n");
            return;
        }
        let idx = mutable_indices[self.rng.next_usize(mutable_indices.len())];
        let dict_len = self.dictionary.len();
        if let Some(mut record) = mutation::mutate_entry(&mut self.dictionary[idx], &mut self.rng, dict_len) {
            record.word_index = idx;

            // Run benchmark after mutation.
            let after_score = self.run_benchmark();

            if after_score >= before_score {
                self.emit_str(&format!(
                    "evolve: kept mutation ({} -> {}): {}\n",
                    before_score, after_score, record.format()
                ));
                self.mutation_history.push(record);
            } else {
                mutation::undo_mutation(&mut self.dictionary[idx], &record);
                self.emit_str(&format!(
                    "evolve: reverted mutation ({} -> {})\n",
                    before_score, after_score
                ));
            }
        } else {
            self.emit_str("evolve: mutation failed\n");
        }
        self.fitness.mark_evolved();
        self.emit_str(&format!(
            "evolve: own={} avg={} evolutions={}\n",
            self.fitness.score, avg_fitness, self.fitness.evolution_count
        ));
    }

    /// Run the benchmark code and return a score (stack depth after execution).
    fn run_benchmark(&mut self) -> i64 {
        let code = match self.fitness.benchmark_code.clone() {
            Some(c) => c,
            None => return 0,
        };
        let start = Instant::now();
        let result = self.execute_sandbox(&code);
        let elapsed = start.elapsed().as_millis() as i64;
        // Score = stack depth * 10 - elapsed_ms (reward correct output, penalize slowness).
        let depth_score = result.stack_snapshot.len() as i64 * 10;
        let time_penalty = (elapsed / 100).min(50);
        if result.success {
            depth_score - time_penalty
        } else {
            -100
        }
    }

    fn check_auto_evolve(&mut self) {
        if self.fitness.should_auto_evolve() {
            self.do_evolve();
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
                        self.check_auto_claim();
                        self.check_auto_replicate();
                        self.check_auto_evolve();
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
            // Seed RNG from node ID for mutation uniqueness.
            let id = node.id_bytes();
            let seed = u64::from_be_bytes(id);
            vm.rng = mutation::SimpleRng::new(seed);
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
