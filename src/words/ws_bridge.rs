//! WebSocket bridge primitives — split out of the monolithic `impl VM`.
//!
//! These are `impl VM` methods carved from `main.rs` by domain. They stay
//! `pub(crate)` so the primitive dispatch in `vm/mod.rs` can reach them.

use super::prelude::*;

impl VM {
    // -----------------------------------------------------------------------
    // WebSocket bridge primitives
    // -----------------------------------------------------------------------

    pub(crate) fn prim_ws_status(&mut self) {
        if let Some(ref st) = self.ws_state {
            let s = st.lock().unwrap().format_status();
            self.emit_str(&s);
        } else {
            self.emit_str("ws-bridge: not running\n");
        }
    }

    pub(crate) fn prim_ws_clients(&mut self) {
        if let Some(ref st) = self.ws_state {
            let s = st.lock().unwrap().format_clients();
            self.emit_str(&s);
        } else {
            self.emit_str("  (ws-bridge not running)\n");
        }
    }

    pub(crate) fn prim_ws_broadcast(&mut self) {
        let msg = self.parse_until('"');
        // The broadcast happens by updating the mesh_json which gets
        // pushed to all connected browsers on the next 2s tick.
        if let Ok(mut json) = self.ws_mesh_json.lock() {
            *json = format!(
                r#"{{"type":"broadcast","message":"{}"}}"#,
                msg.replace('"', "\\\"")
            );
        }
        self.emit_str(&format!("ws broadcast: {}\n", msg));
    }

    pub(crate) fn update_ws_mesh_json(&mut self) {
        let id_hex = self
            .node_id_cache
            .map(|id| mesh::id_to_hex(&id))
            .unwrap_or_default();
        let peer_details = self
            .mesh
            .as_ref()
            .map(|m| m.peer_details())
            .unwrap_or_default();
        let goal_stats = self
            .mesh
            .as_ref()
            .map(|m| m.goal_stats())
            .unwrap_or((0, 0, 0, 0));
        let recent = self
            .mesh
            .as_ref()
            .map(|m| m.drain_recent_events())
            .unwrap_or_default();
        let children: Vec<(String, u32)> = self
            .spawn_state
            .children
            .iter()
            .map(|c| (mesh::id_to_hex(&c.node_id), self.spawn_state.generation + 1))
            .collect();
        let json = ws_bridge::build_mesh_json(
            &id_hex,
            self.fitness.score,
            self.spawn_state.generation,
            &peer_details,
            goal_stats,
            &recent,
            &children,
            self.monitor.watches.len(),
            self.monitor.alerts.len(),
        );
        if let Ok(mut j) = self.ws_mesh_json.lock() {
            *j = json;
        }
    }

    pub(crate) fn poll_ws_events(&mut self) {
        // Process incoming WS events (goal submissions from browsers).
        let events: Vec<ws_bridge::WsEvent> = self
            .ws_events
            .as_ref()
            .map(|rx| {
                let mut evts = Vec::new();
                while let Ok(e) = rx.try_recv() {
                    evts.push(e);
                }
                evts
            })
            .unwrap_or_default();

        for event in events {
            match event {
                ws_bridge::WsEvent::GoalSubmit { code, priority } => {
                    if let Some(ref m) = self.mesh {
                        let gid = m.create_goal(&code, priority, Some(code.clone()));
                        println!(
                            "[ws] goal #{} from browser: {}",
                            gid,
                            code.chars().take(40).collect::<String>()
                        );
                    }
                }
                ws_bridge::WsEvent::ClientConnected { id } => {
                    println!("[ws] browser connected: {}", id);
                }
                ws_bridge::WsEvent::ClientDisconnected { id } => {
                    println!("[ws] browser disconnected: {}", id);
                }
                ws_bridge::WsEvent::Heartbeat { .. } => {}
            }
        }
    }

}
