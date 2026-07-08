//! Mesh & cross-machine primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // Cross-machine mesh primitives
    // -----------------------------------------------------------------------

    pub(crate) fn prim_my_addr(&mut self) {
        if let Some(ref m) = self.mesh {
            self.emit_str(&format!("{}\n", m.my_addr()));
        } else {
            self.emit_str("mesh offline\n");
        }
    }

    pub(crate) fn prim_peer_table(&mut self) {
        if let Some(ref m) = self.mesh {
            let table = m.peer_table();
            if table.is_empty() {
                self.emit_str("no peers\n");
            } else {
                self.emit_str("--- peer table ---\n");
                for (id, addr, fitness, age) in &table {
                    self.emit_str(&format!(
                        "  {} @ {} fitness={} seen={}s ago\n",
                        id, addr, fitness, age
                    ));
                }
            }
        } else {
            self.emit_str("mesh offline\n");
        }
    }

    pub(crate) fn prim_mesh_key(&mut self) {
        if let Some(ref m) = self.mesh {
            if m.mesh_key.is_some() {
                self.emit_str("mesh-key: enabled\n");
            } else {
                self.emit_str("mesh-key: disabled (open mesh)\n");
            }
        } else {
            self.emit_str("mesh offline\n");
        }
    }

    pub(crate) fn prim_connect(&mut self) {
        let addr_str = self.parse_until('"');
        let addr: SocketAddr = match addr_str.trim().parse().or_else(|_| {
            use std::net::ToSocketAddrs;
            addr_str
                .trim()
                .to_socket_addrs()
                .map_err(|e| e.to_string())
                .and_then(|mut a| a.next().ok_or_else(|| "no address".into()))
        }) {
            Ok(a) => a,
            Err(e) => {
                self.emit_str(&format!("connect: {}\n", e));
                return;
            }
        };
        if let Some(ref m) = self.mesh {
            m.connect_peer(addr);
            self.emit_str(&format!("connected to {}\n", addr));
        } else {
            self.emit_str("mesh offline\n");
        }
    }

    pub(crate) fn prim_disconnect(&mut self) {
        let hex_id = self.parse_until('"');
        if let Some(ref m) = self.mesh {
            if m.disconnect_peer(hex_id.trim()) {
                self.emit_str(&format!("disconnected {}\n", hex_id.trim()));
            } else {
                self.emit_str(&format!("peer {} not found\n", hex_id.trim()));
            }
        } else {
            self.emit_str("mesh offline\n");
        }
    }

    pub(crate) fn prim_mesh_stats(&mut self) {
        if let Some(ref m) = self.mesh {
            let (peers, port) = m.mesh_stats();
            self.emit_str(&format!(
                "--- mesh stats ---\nport: {}\npeers: {}\naddress: {}\nkey: {}\n",
                port,
                peers,
                m.my_addr(),
                if m.mesh_key.is_some() {
                    "enabled"
                } else {
                    "disabled"
                }
            ));
        } else {
            self.emit_str("mesh offline\n");
        }
    }

    // -----------------------------------------------------------------------
    // Mesh primitives
    // -----------------------------------------------------------------------

    /// SEND ( addr n peer -- ) send n bytes from memory to all peers.
    /// The peer argument is reserved for future use (ignored, broadcast).
    pub(crate) fn prim_send(&mut self) {
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
    pub(crate) fn prim_recv(&mut self) {
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
    pub(crate) fn prim_peers(&mut self) {
        let count = self.mesh.as_ref().map_or(0, |m| m.peer_count());
        self.stack.push(count as Cell);
    }

    /// REPLICATE ( -- ) serialize this unit's state and broadcast to peers.
    pub(crate) fn prim_replicate(&mut self) {
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
    pub(crate) fn prim_mutate(&mut self) {
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
    pub(crate) fn prim_mesh_status(&mut self) {
        if let Some(ref m) = self.mesh {
            let s = m.format_status();
            self.emit_str(&s);
        } else {
            self.emit_str("mesh: offline\n");
        }
    }

    /// PROPOSE ( -- ) trigger a replication proposal via consensus.
    pub(crate) fn prim_propose(&mut self) {
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
    pub(crate) fn prim_mesh_load(&mut self) {
        let load = self.mesh.as_ref().map_or(0, |m| m.load());
        self.stack.push(load as Cell);
    }

    /// CAPACITY ( -- n ) push capacity threshold.
    pub(crate) fn prim_mesh_capacity(&mut self) {
        let cap = self.mesh.as_ref().map_or(0, |m| m.capacity());
        self.stack.push(cap as Cell);
    }

    /// ID ( -- addr n ) push this unit's ID string to PAD and return addr+len.
    pub(crate) fn prim_id(&mut self) {
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
    pub(crate) fn prim_type(&mut self) {
        let n = self.pop() as usize;
        let addr = self.pop() as usize;
        for i in 0..n {
            let a = addr + i;
            if a < self.memory.len() {
                self.emit_char(self.memory[a] as u8 as char);
            }
        }
    }

}
