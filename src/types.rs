//! Shared types used across all unit modules.
//!
//! These are the fundamental building blocks of the Forth VM.
//! Every module imports from here rather than from main.rs.

/// The fundamental data type — a signed 64-bit integer.
pub type Cell = i64;

/// PAD buffer address in memory, used for string operations.
pub const PAD: usize = 60000;

/// A single Forth instruction in a compiled word body.
#[derive(Clone, Debug)]
pub enum Instruction {
    /// Call a kernel primitive by ID.
    Primitive(usize),
    /// Push a literal value.
    Literal(Cell),
    /// Call a compiled word by dictionary index.
    Call(usize),
    /// Print a string literal (compiled from .").
    StringLit(String),
    /// Unconditional branch (relative offset).
    Branch(Cell),
    /// Branch if top of stack is zero (relative offset).
    BranchIfZero(Cell),
}

/// A dictionary entry — a named, compiled Forth word.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub immediate: bool,
    pub hidden: bool,
    pub body: Vec<Instruction>,
}
