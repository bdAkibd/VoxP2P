use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalProfile {
    pub node_id: String,
    pub secret_key_bytes: Vec<u8>,
    pub nickname: String,
    pub avatar_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub node_id: String,
    pub nickname: String,
    pub avatar_path: Option<String>,
    pub custom_volume: f64,
    pub last_seen: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub chat_id: String,
    pub sender_id: String,
    pub content: String,
    pub attachment_hash: Option<String>,
    pub attachment_name: Option<String>,
    pub timestamp: i64,
    pub delivered: i32,
    #[serde(default)]
    pub is_edited: i32,
}

pub type ChatMessage = Message;
