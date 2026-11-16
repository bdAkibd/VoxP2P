use serde::{Deserialize, Serialize};
use crate::audio::mixer::PeerVolumeLevel;
use crate::storage::models::ChatMessage;

pub const EVENT_VOLUME_LEVELS: &str = "voxp2p://volume-levels";
pub const EVENT_ROOM_STATE: &str = "voxp2p://room-state";
pub const EVENT_CHAT_MESSAGE: &str = "voxp2p://chat-message";
pub const EVENT_FILE_TRANSFER: &str = "voxp2p://file-transfer";
pub const EVENT_HOST_CHANGED: &str = "room://host-changed";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeLevelsPayload {
    pub levels: Vec<PeerVolumeLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomStatePayload {
    pub room_id: String,
    pub host_node_id: String,
    pub peer_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessagePayload {
    pub message: ChatMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferPayload {
    pub hash: String,
    pub filename: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub is_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostChangedPayload {
    pub room_id: String,
    pub old_host_id: String,
    pub new_host_id: String,
}
