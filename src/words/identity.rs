//! Identity primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Identity
    // -----------------------------------------------------------------------

    /// REIDENTIFY ( -- ) generate a new node ID, migrate saved state.
    pub(crate) fn prim_reidentify(&mut self) {
        let old_id = self.node_id_cache;
        // Generate a new random ID.
        let mut new_id = [0u8; 8];
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            use std::io::Read;
            let _ = f.read_exact(&mut new_id);
        }
        // Migrate state directory.
        if let Some(oid) = old_id {
            let _ = persist::rename_state(&oid, &new_id);
        }
        // Save the new ID.
        let _ = persist::save_node_id(&new_id);
        self.node_id_cache = Some(new_id);
        self.rng = mutation::SimpleRng::new(u64::from_be_bytes(new_id));
        self.emit_str(&format!(
            "reidentified: {} -> {}\n",
            old_id
                .map(|id| mesh::id_to_hex(&id))
                .unwrap_or_else(|| "none".into()),
            mesh::id_to_hex(&new_id),
        ));
    }

}
