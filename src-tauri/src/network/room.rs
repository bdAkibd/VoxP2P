use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerInfo {
    pub node_id: String,
    pub nickname: String,
    pub join_timestamp: u64,
    pub is_host: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferHostMessage {
    pub target_node_id: String,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomState {
    pub room_id: String,
    pub host_node_id: String,
    pub peers: Vec<PeerInfo>,
    pub heartbeat_timeout_ms: u64,
}

impl RoomState {
    pub fn new(room_id: String, host_node_id: String) -> Self {
        Self {
            room_id,
            host_node_id,
            peers: Vec::new(),
            heartbeat_timeout_ms: 2000,
        }
    }

    pub fn add_or_update_peer(&mut self, peer: PeerInfo) {
        if let Some(existing) = self.peers.iter_mut().find(|p| p.node_id == peer.node_id) {
            *existing = peer;
        } else {
            self.peers.push(peer);
        }
    }

    pub fn remove_peer(&mut self, node_id: &str) {
        self.peers.retain(|p| p.node_id != node_id);
    }

    /// Deterministic Host Migration:
    /// Peers are sorted by (join_timestamp, node_id).
    /// If the current host leaves or times out, the first eligible peer in order is promoted.
    pub fn compute_next_host(&self) -> Option<String> {
        let mut sorted_peers = self.peers.clone();
        sorted_peers.sort_by(|a, b| {
            a.join_timestamp
                .cmp(&b.join_timestamp)
                .then_with(|| a.node_id.cmp(&b.node_id))
        });

        sorted_peers.first().map(|p| p.node_id.clone())
    }

    pub fn set_host(&mut self, new_host_id: String) {
        self.host_node_id = new_host_id.clone();
        for peer in self.peers.iter_mut() {
            peer.is_host = peer.node_id == new_host_id;
        }
    }
}
