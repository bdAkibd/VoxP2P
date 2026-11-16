use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStatus {
    Stopped,
    Starting,
    Running,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub listen_port: u16,
    pub enable_dht: bool,
    pub enable_mdns: bool,
    pub relay_url: Option<String>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            listen_port: 0,
            enable_dht: true,
            enable_mdns: true,
            relay_url: None,
        }
    }
}

pub struct P2PNode {
    pub node_id: String,
    pub config: NodeConfig,
    pub status: NodeStatus,
}

impl P2PNode {
    pub fn new(node_id: String, config: NodeConfig) -> Self {
        Self {
            node_id,
            config,
            status: NodeStatus::Stopped,
        }
    }

    pub fn status(&self) -> &NodeStatus {
        &self.status
    }

    pub fn set_status(&mut self, status: NodeStatus) {
        self.status = status;
    }
}
