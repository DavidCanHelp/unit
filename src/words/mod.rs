//! Domain-split primitive words for the `unit` VM.
//!
//! Historically every `prim_*` method lived in one ~4,600-line `impl VM`
//! block in `main.rs`. They are now grouped by concern into the submodules
//! below. Each submodule contributes more `impl VM` methods; Rust applies
//! them all to the single `VM` type regardless of module.
//!
//! All methods are `pub(crate)` (this is a binary crate — no public API
//! surface) so the opcode dispatch in `vm/mod.rs` can call them across the
//! module boundary.

mod atoms;
mod mutation;
mod sexp;
mod persistence;
mod evolution;
mod distgoal;
mod immune;
mod mesh;
mod swarm;
mod consent;
mod sandbox;
mod goals;
mod io;
mod ws_bridge;
mod monitor;
mod spawn;
mod identity;
mod bench;

/// Names every submodule pulls in via `use super::prelude::*;`.
///
/// A shared prelude keeps each submodule's head short and avoids per-file
/// import drift. Re-exports are `pub(crate)` and intentionally may exceed
/// what any single submodule uses.
#[allow(unused_imports)]
mod prelude {
    pub(crate) use crate::types::*;
    pub(crate) use crate::vm; // the module path, for `vm::Fault` etc.
    pub(crate) use crate::vm::*; // VM + P_* primitive-id constants

    pub(crate) use crate::{
        challenges, distgoal, energy, evolve, goals, mesh, metrics, niche,
        persist, reproduction, snapshot, spawn, transport,
    };
    pub(crate) use crate::features::{fitness, io_words, monitor, mutation, ws_bridge};

    pub(crate) use std::collections::{HashMap, HashSet, VecDeque};
    pub(crate) use std::io::{self, BufRead, Read, Write};
    pub(crate) use std::net::SocketAddr;
    pub(crate) use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
}
