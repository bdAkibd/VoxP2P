use serde::{Deserialize, Serialize};
use crate::audio::mixer::PeerVolumeLevel;
use crate::storage::models::ChatMessage;

pub const EVENT_VOLUME_LEVELS: &str = "voxp2p://volume-levels";
pub const EVENT_AUDIO_LEVELS: &str = "audio://levels";
pub const EVENT_LOCAL_INPUT_LEVEL: &str = "audio://local-input-level";
pub const EVENT_ROOM_STATE: &str = "voxp2p://room-state";

pub const EVENT_CHAT_MESSAGE: &str = "voxp2p://chat-message";
pub const EVENT_CHAT_MESSAGE_RECEIVED: &str = "chat://message-received";
pub const EVENT_CHAT_MESSAGE_DELIVERED: &str = "chat://message-delivered";
pub const EVENT_CALL_INCOMING_RING: &str = "call://incoming-ring";
pub const EVENT_FILE_TRANSFER: &str = "voxp2p://file-transfer";
pub const EVENT_TRANSFER_PROGRESS: &str = "transfer://progress";
pub const EVENT_HOST_CHANGED: &str = "room://host-changed";
pub const EVENT_PEERS_UPDATED: &str = "room://peers-updated";
pub const EVENT_SYNC_HOST_COMPLETED: &str = "sync://host-completed";
pub const EVENT_SYNC_IMPORT_COMPLETED: &str = "sync://import-completed";


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeLevelsPayload {
    pub levels: Vec<PeerVolumeLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalInputLevelPayload {
    pub db: f32,
    pub percent: f32,
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
pub struct MessageDeliveredPayload {
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectCallRingPayload {
    pub caller_id: String,
    pub room_ticket: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferPayload {
    pub hash: String,
    pub filename: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub is_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgressPayload {
    pub hash: String,
    pub percent: f32,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub is_complete: bool,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostChangedPayload {
    pub room_id: String,
    pub old_host_id: String,
    pub new_host_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncHostCompletedPayload {
    pub nickname: String,
    pub node_id: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncImportCompletedPayload {
    pub node_id: String,
    pub nickname: String,
    pub contacts_imported: usize,
    pub messages_imported: usize,
}
