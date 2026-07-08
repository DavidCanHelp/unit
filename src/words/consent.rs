//! Replication-consent primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Replication consent primitives
    // -----------------------------------------------------------------------

    pub(crate) fn prim_trust_all_level(&mut self) {
        if let Some(ref m) = self.mesh {
            m.set_trust_level(mesh::TrustLevel::All);
            self.emit_str("trust: ALL (auto-accept everything)\n");
        }
    }

    pub(crate) fn prim_trust_mesh(&mut self) {
        if let Some(ref m) = self.mesh {
            m.set_trust_level(mesh::TrustLevel::Mesh);
            self.emit_str("trust: MESH (auto-accept known peers)\n");
        }
    }

    pub(crate) fn prim_trust_family(&mut self) {
        if let Some(ref m) = self.mesh {
            m.set_trust_level(mesh::TrustLevel::Family);
            self.emit_str("trust: FAMILY (auto-accept parent/children)\n");
        }
    }

    pub(crate) fn prim_trust_none_level(&mut self) {
        if let Some(ref m) = self.mesh {
            m.set_trust_level(mesh::TrustLevel::None);
            self.emit_str("trust: NONE (manual approval required)\n");
        }
    }

    pub(crate) fn prim_trust_level(&mut self) {
        if let Some(ref m) = self.mesh {
            let level = m.trust_level();
            self.stack.push(level.as_val());
            self.emit_str(&format!("trust: {}\n", level.label()));
        } else {
            self.stack.push(0);
        }
    }

    pub(crate) fn prim_requests(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_requests();
            self.emit_str(&s);
        }
    }

    pub(crate) fn prim_accept_req(&mut self) {
        if let Some(ref m) = self.mesh {
            if let Some((sender, rid)) = m.accept_oldest() {
                self.emit_str(&format!(
                    "accepted request #{} from {}\n",
                    rid,
                    mesh::id_to_hex(&sender)
                ));
            } else {
                self.emit_str("no pending requests\n");
            }
        }
    }

    pub(crate) fn prim_deny_req(&mut self) {
        if let Some(ref m) = self.mesh {
            if let Some(rid) = m.deny_oldest() {
                self.emit_str(&format!("denied request #{}\n", rid));
            } else {
                self.emit_str("no pending requests\n");
            }
        }
    }

    pub(crate) fn prim_deny_all_req(&mut self) {
        if let Some(ref m) = self.mesh {
            let n = m.deny_all_requests();
            self.emit_str(&format!("denied {} request(s)\n", n));
        }
    }

    pub(crate) fn prim_replication_log(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_replication_log();
            self.emit_str(&s);
        }
    }

}
