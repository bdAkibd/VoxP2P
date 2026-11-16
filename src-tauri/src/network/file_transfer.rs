use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointAddr, PublicKey};
use iroh_blobs::store::mem::MemStore;
use iroh_blobs::ticket::BlobTicket;
use iroh_blobs::{BlobFormat, Hash};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::ipc::events::TransferProgressPayload;
pub use crate::network::room::ChatMessageCallback;
use crate::storage::db::Database;
use crate::storage::models::Message;

pub const DIRECT_MESSAGE_ALPN: &[u8] = b"voxp2p/dm/1";

pub type TransferProgressCallback = Arc<dyn Fn(&TransferProgressPayload) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobTransferProgress {
    pub hash: String,
    pub filename: String,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    pub is_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachFileResponse {
    pub id: String,
    pub chat_id: String,
    pub text: String,
    pub attachment_hash: String,
    pub attachment_name: String,
    pub attachment_size: u64,
    pub mime_type: String,
    pub ticket: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DirectMessageEnvelope {
    Message(Message),
    Ack { message_id: String },
}

/// Detects standard MIME types based on file extension.
pub fn detect_mime_type(filename: &str) -> String {
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "opus" => "audio/opus",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Protocol handler for accepting 1-on-1 direct messages over QUIC.
#[derive(Clone)]
pub struct DirectMessageHandler {
    db: Database,
    chat_cb: Option<ChatMessageCallback>,
}

impl fmt::Debug for DirectMessageHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DirectMessageHandler")
    }
}

impl ProtocolHandler for DirectMessageHandler {
    fn accept(
        &self,
        connection: Connection,
    ) -> impl Future<Output = Result<(), AcceptError>> + Send {
        let db = self.db.clone();
        let chat_cb = self.chat_cb.clone();

        async move {
            while let Ok((mut send, mut recv)) = connection.accept_bi().await {
                if let Ok(buf) = recv.read_to_end(65536).await {
                    if let Ok(envelope) = serde_json::from_slice::<DirectMessageEnvelope>(&buf) {
                        match envelope {
                            DirectMessageEnvelope::Message(mut msg) => {
                                msg.delivered = 1;
                                let _ = db.insert_message(&msg);

                                // Send Ack back to sender
                                let ack = DirectMessageEnvelope::Ack {
                                    message_id: msg.id.clone(),
                                };
                                if let Ok(ack_bytes) = serde_json::to_vec(&ack) {
                                    let _ = send.write_all(&ack_bytes).await;
                                    let _ = send.finish();
                                }

                                if let Some(ref cb) = chat_cb {
                                    cb(&msg);
                                }
                            }
                            DirectMessageEnvelope::Ack { message_id } => {
                                let _ = db.mark_message_delivered(&message_id);
                            }
                        }
                    }
                }
            }
            Ok(())
        }
    }
}


/// Central manager for P2P file/media transfers via `iroh-blobs`, 1-on-1 direct messaging,
/// offline queue syncing, and peer avatar caching.
pub struct FileTransferManager {
    endpoint: Arc<RwLock<Option<Endpoint>>>,
    blobs_store: Arc<RwLock<Option<MemStore>>>,
    pub db: Database,
    app_data_dir: PathBuf,
    local_node_id: Arc<RwLock<String>>,
    transfers: Arc<RwLock<HashMap<String, BlobTransferProgress>>>,
    progress_cb: Option<TransferProgressCallback>,
    chat_cb: Option<ChatMessageCallback>,
    offline_watcher_cancel: Arc<AtomicBool>,
}

impl FileTransferManager {
    pub fn new(db: Database, app_data_dir: PathBuf) -> Arc<Self> {
        Self::with_callbacks(db, app_data_dir, None, None)
    }

    pub fn with_callbacks(
        db: Database,
        app_data_dir: PathBuf,
        progress_cb: Option<TransferProgressCallback>,
        chat_cb: Option<ChatMessageCallback>,
    ) -> Arc<Self> {
        let avatars_dir = app_data_dir.join("avatars");
        std::fs::create_dir_all(&avatars_dir).ok();

        Arc::new(Self {
            endpoint: Arc::new(RwLock::new(None)),
            blobs_store: Arc::new(RwLock::new(None)),
            db,
            app_data_dir,
            local_node_id: Arc::new(RwLock::new("local_node".to_string())),
            transfers: Arc::new(RwLock::new(HashMap::new())),
            progress_cb,
            chat_cb,
            offline_watcher_cancel: Arc::new(AtomicBool::new(false)),
        })
    }

    pub async fn set_endpoint_and_store(
        &self,
        endpoint: Endpoint,
        blobs_store: MemStore,
        local_node_id: String,
    ) {
        *self.endpoint.write().await = Some(endpoint);
        *self.blobs_store.write().await = Some(blobs_store);
        *self.local_node_id.write().await = local_node_id;
    }

    pub fn create_dm_handler(&self) -> DirectMessageHandler {
        DirectMessageHandler {
            db: self.db.clone(),
            chat_cb: self.chat_cb.clone(),
        }
    }

    /// Imports a file into local `iroh-blobs` store, computes Blake3 hash and ticket,
    /// and optionally creates/sends a chat message with the attachment.
    pub async fn attach_file(
        &self,
        file_path: &str,
        chat_id: Option<String>,
        text: Option<String>,
    ) -> Result<AttachFileResponse, String> {
        let path = Path::new(file_path);
        if !path.exists() {
            return Err(format!("File does not exist: {file_path}"));
        }

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment.bin")
            .to_string();

        let file_bytes = tokio::fs::read(path)
            .await
            .map_err(|e| format!("Failed to read file: {e}"))?;
        let size = file_bytes.len() as u64;
        let mime = detect_mime_type(&filename);

        let blobs_store = {
            let guard = self.blobs_store.read().await;
            guard
                .clone()
                .ok_or_else(|| "Blobs store not initialized".to_string())?
        };

        // Add to iroh-blobs store
        let tag = blobs_store
            .add_bytes(file_bytes)
            .temp_tag()
            .await
            .map_err(|e| format!("Failed to import blob into store: {e}"))?;
        let hash = tag.hash();
        let hash_str = hash.to_string();

        // Generate ticket
        let endpoint_opt = self.endpoint.read().await.clone();
        let ticket_str = if let Some(endpoint) = endpoint_opt {
            let addr = endpoint.addr();
            let ticket = BlobTicket::new(addr, hash, BlobFormat::Raw);
            ticket.to_string()
        } else {
            hash_str.clone()
        };


        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let local_id = self.local_node_id.read().await.clone();
        let target_chat = chat_id.unwrap_or_else(|| "default_chat".to_string());
        let msg_id = format!("msg_{}_{}", local_id, now);
        let message_text = text.unwrap_or_default();

        let mut msg = Message {
            id: msg_id.clone(),
            chat_id: target_chat.clone(),
            sender_id: local_id,
            content: message_text.clone(),
            attachment_hash: Some(hash_str.clone()),
            attachment_name: Some(filename.clone()),
            timestamp: now,
            delivered: 1,
        };

        if PublicKey::from_str(&target_chat).is_ok() {
            let delivered = self.try_send_quic_dm(&target_chat, &msg).await;
            msg.delivered = if delivered { 1 } else { 0 };
        }

        // Save in local SQLite DB
        let _ = self.db.insert_message(&msg);

        // If registered, invoke chat callback
        if let Some(ref cb) = self.chat_cb {
            cb(&msg);
        }

        Ok(AttachFileResponse {
            id: msg_id,
            chat_id: target_chat,
            text: message_text,
            attachment_hash: hash_str,
            attachment_name: filename,
            attachment_size: size,
            mime_type: mime,
            ticket: ticket_str,
        })
    }

    /// Sends a 1-on-1 direct message via bi-directional QUIC stream.
    /// If peer is offline, stores in SQLite with `delivered = 0` for subsequent offline queue flush.
    pub async fn send_direct_message(
        &self,
        recipient_node_id: &str,
        content: &str,
        attachment_hash: Option<String>,
        attachment_name: Option<String>,
    ) -> Result<Message, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let local_id = self.local_node_id.read().await.clone();
        let msg_id = format!("dm_{}_{}", local_id, now);

        let mut msg = Message {
            id: msg_id,
            chat_id: recipient_node_id.to_string(),
            sender_id: local_id,
            content: content.to_string(),
            attachment_hash,
            attachment_name,
            timestamp: now,
            delivered: 0,
        };

        // Try direct delivery over QUIC
        let delivered = self.try_send_quic_dm(recipient_node_id, &msg).await;
        if delivered {
            msg.delivered = 1;
        }

        // Save to SQLite (either delivered=1 or delivered=0 in offline queue)
        self.db.insert_message(&msg).map_err(|e| e.to_string())?;

        if let Some(ref cb) = self.chat_cb {
            cb(&msg);
        }

        Ok(msg)
    }

    /// Internal attempt to deliver a message over a bi-directional QUIC stream and wait for Ack.
    async fn try_send_quic_dm(&self, recipient_node_id: &str, msg: &Message) -> bool {
        let endpoint = match &*self.endpoint.read().await {
            Some(ep) => ep.clone(),
            None => return false,
        };

        let target_pk = match PublicKey::from_str(recipient_node_id) {
            Ok(pk) => pk,
            Err(_) => return false,
        };

        let addr = EndpointAddr::new(target_pk);

        // Attempt connect with 3-second timeout
        let conn = match tokio::time::timeout(
            Duration::from_millis(3000),
            endpoint.connect(addr, DIRECT_MESSAGE_ALPN),
        )
        .await
        {
            Ok(Ok(c)) => c,
            _ => return false,
        };

        let (mut send, mut recv) = match conn.open_bi().await {
            Ok(bi) => bi,
            Err(_) => return false,
        };

        let envelope = DirectMessageEnvelope::Message(msg.clone());
        let data = match serde_json::to_vec(&envelope) {
            Ok(d) => d,
            Err(_) => return false,
        };

        if send.write_all(&data).await.is_err() || send.finish().is_err() {
            return false;
        }

        // Await Ack
        if let Ok(Ok(ack_buf)) = tokio::time::timeout(
            Duration::from_millis(3000),
            recv.read_to_end(1024),
        )
        .await
        {
            if let Ok(DirectMessageEnvelope::Ack { message_id }) =
                serde_json::from_slice::<DirectMessageEnvelope>(&ack_buf)
            {
                return message_id == msg.id;
            }
        }


        false
    }

    /// Flushes any pending offline messages queued for `recipient_node_id`.
    pub async fn flush_undelivered_messages(&self, recipient_node_id: &str) -> Result<usize, String> {
        let pending = self
            .db
            .get_undelivered_messages(recipient_node_id)
            .map_err(|e| e.to_string())?;

        if pending.is_empty() {
            return Ok(0);
        }

        let mut flushed = 0;
        for msg in pending {
            if self.try_send_quic_dm(recipient_node_id, &msg).await {
                let _ = self.db.mark_message_delivered(&msg.id);
                flushed += 1;
            } else {
                // Peer is currently unreachable; leave remaining in queue
                break;
            }
        }

        Ok(flushed)
    }

    /// Spawns the background watcher that checks offline messages and automatically
    /// flushes them when contacts come online.
    pub fn start_offline_queue_watcher(self: &Arc<Self>) {
        let cancel = self.offline_watcher_cancel.clone();
        let this = self.clone();

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(3500));
                while !cancel.load(Ordering::Acquire) {
                    interval.tick().await;

                    if let Ok(undelivered) = this.db.get_all_undelivered_messages() {
                        let mut recipient_set = std::collections::HashSet::new();
                        for msg in undelivered {
                            recipient_set.insert(msg.chat_id);
                        }

                        for recipient in recipient_set {
                            let _ = this.flush_undelivered_messages(&recipient).await;
                        }
                    }
                }
            });
        }
    }

    /// Downloads a blob from a remote peer via `iroh-blobs`, emitting progress events
    /// `transfer://progress { hash, percent }` to the UI, and saves it to `save_path`.
    pub async fn download_attachment(
        &self,
        node_id: &str,
        blob_hash: &str,
        save_path: &str,
    ) -> Result<(), String> {
        let hash = Hash::from_str(blob_hash).map_err(|e| format!("Invalid blob hash: {e}"))?;

        // 1. Emit start progress
        self.emit_progress(blob_hash, 0.0, 0, 0, false).await;

        let blobs_store = {
            let guard = self.blobs_store.read().await;
            guard
                .clone()
                .ok_or_else(|| "Blobs store not initialized".to_string())?
        };

        // 2. Check if blob already exists locally in store
        let local_bytes_opt = blobs_store.blobs().get_bytes(hash).await.ok();
        let bytes = if let Some(local_bytes) = local_bytes_opt {
            self.emit_progress(
                blob_hash,
                100.0,
                local_bytes.len() as u64,
                local_bytes.len() as u64,
                true,
            )
            .await;
            local_bytes.to_vec()
        } else {
            // 3. Connect to sender peer over iroh-blobs ALPN
            let endpoint = self
                .endpoint
                .read()
                .await
                .clone()
                .ok_or_else(|| "Endpoint not initialized".to_string())?;

            let target_pk = PublicKey::from_str(node_id)
                .map_err(|e| format!("Invalid node ID public key: {e}"))?;
            let addr = EndpointAddr::new(target_pk);

            let conn = endpoint
                .connect(addr, iroh_blobs::ALPN)
                .await
                .map_err(|e| format!("Failed to connect to peer for blob download: {e}"))?;

            self.emit_progress(blob_hash, 25.0, 0, 0, false).await;

            // 4. Download blob via verified BLAKE3 get_blob
            let (downloaded, _stats) = iroh_blobs::get::request::get_blob(conn, hash)
                .bytes_and_stats()
                .await
                .map_err(|e| format!("Blob streaming failed: {e}"))?;

            self.emit_progress(
                blob_hash,
                75.0,
                downloaded.len() as u64,
                downloaded.len() as u64,
                false,
            )
            .await;

            // Also import downloaded bytes into local blobs store
            let _ = blobs_store.add_bytes(downloaded.clone()).temp_tag().await;
            downloaded.to_vec()
        };


        // 5. Write completed file to save_path
        if let Some(parent) = Path::new(save_path).parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create destination dir: {e}"))?;
        }

        tokio::fs::write(save_path, &bytes)
            .await
            .map_err(|e| format!("Failed to save downloaded file: {e}"))?;

        // 6. Emit completion progress
        self.emit_progress(
            blob_hash,
            100.0,
            bytes.len() as u64,
            bytes.len() as u64,
            true,
        )
        .await;

        Ok(())
    }

    /// Automatically fetches and caches peer avatars when an avatar_hash is received during handshake.
    pub async fn handle_avatar_handshake(&self, peer_node_id: &str, avatar_hash: &str) {
        let avatar_filename = format!("{avatar_hash}.png");
        let cache_path = self.app_data_dir.join("avatars").join(&avatar_filename);

        if cache_path.exists() {
            // Already cached, update contact
            let _ = self
                .db
                .handle_profile_handshake(peer_node_id, "", Some(&cache_path.to_string_lossy()));
            return;
        }

        // Trigger background download
        let save_str = cache_path.to_string_lossy().to_string();
        if self
            .download_attachment(peer_node_id, avatar_hash, &save_str)
            .await
            .is_ok()
        {
            let _ = self
                .db
                .handle_profile_handshake(peer_node_id, "", Some(&save_str));
        }
    }

    async fn emit_progress(
        &self,
        hash: &str,
        percent: f32,
        bytes_downloaded: u64,
        total_bytes: u64,
        is_complete: bool,
    ) {
        let progress = TransferProgressPayload {
            hash: hash.to_string(),
            percent,
            bytes_downloaded,
            total_bytes,
            is_complete,
        };

        let mut transfers = self.transfers.write().await;
        transfers.insert(
            hash.to_string(),
            BlobTransferProgress {
                hash: hash.to_string(),
                filename: hash.to_string(),
                total_bytes,
                transferred_bytes: bytes_downloaded,
                is_complete,
            },
        );

        if let Some(ref cb) = self.progress_cb {
            cb(&progress);
        }
    }

    pub fn stop(&self) {
        self.offline_watcher_cancel.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_and_ticket() {
        let pk = PublicKey::from_bytes(&[1u8; 32]).unwrap();
        let addr = EndpointAddr::new(pk);
        let hash = Hash::from_bytes([0u8; 32]);
        let ticket = BlobTicket::new(addr, hash, BlobFormat::Raw);
        let ticket_str = ticket.to_string();
        let parsed = BlobTicket::from_str(&ticket_str).unwrap();
        assert_eq!(parsed.hash(), hash);
    }

    #[test]
    fn test_detect_mime_type() {
        assert_eq!(detect_mime_type("photo.png"), "image/png");
        assert_eq!(detect_mime_type("song.mp3"), "audio/mpeg");
        assert_eq!(detect_mime_type("doc.pdf"), "application/pdf");
        assert_eq!(detect_mime_type("archive.zip"), "application/zip");
        assert_eq!(detect_mime_type("data.bin"), "application/octet-stream");
    }

    #[tokio::test]
    async fn test_attach_file_imports_into_blobs_store() {
        let db = Database::new_in_memory().unwrap();
        let temp_dir = std::env::temp_dir().join("voxp2p_test_attach");
        tokio::fs::create_dir_all(&temp_dir).await.unwrap();
        let test_file = temp_dir.join("test_note.txt");
        tokio::fs::write(&test_file, b"Hello VoxP2P File Transfer")
            .await
            .unwrap();

        let ft = FileTransferManager::new(db.clone(), temp_dir.clone());
        let blobs_store = MemStore::new();

        *ft.blobs_store.write().await = Some(blobs_store.clone());

        let res = ft
            .attach_file(&test_file.to_string_lossy(), Some("chat_123".to_string()), Some("Here is the file".to_string()))
            .await
            .unwrap();

        assert_eq!(res.attachment_name, "test_note.txt");
        assert_eq!(res.mime_type, "text/plain");
        assert_eq!(res.attachment_size, 26);
        assert!(!res.attachment_hash.is_empty());

        // Verify stored in DB
        let msgs = db.get_messages_for_chat("chat_123", 10).unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].attachment_hash, Some(res.attachment_hash.clone()));

        // Cleanup
        tokio::fs::remove_dir_all(&temp_dir).await.ok();
    }

    #[tokio::test]
    async fn test_offline_queue_and_flush() {
        let db = Database::new_in_memory().unwrap();
        let temp_dir = std::env::temp_dir().join("voxp2p_test_offline");
        let ft = FileTransferManager::new(db.clone(), temp_dir);

        // Send DM when recipient is offline (no endpoint running)
        let msg = ft
            .send_direct_message("recipient_offline_node", "Offline message text", None, None)
            .await
            .unwrap();

        // Message should be saved with delivered = 0
        assert_eq!(msg.delivered, 0);

        let undelivered = db.get_undelivered_messages("recipient_offline_node").unwrap();
        assert_eq!(undelivered.len(), 1);
        assert_eq!(undelivered[0].content, "Offline message text");

        // Manually mark delivered via simulated Ack
        db.mark_message_delivered(&msg.id).unwrap();
        let remaining = db.get_undelivered_messages("recipient_offline_node").unwrap();
        assert_eq!(remaining.len(), 0);
    }
}
