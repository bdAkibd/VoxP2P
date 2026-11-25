use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
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

pub const DM_ALPN: &[u8] = b"/p2pchat/dm/1";
pub const DIRECT_MESSAGE_ALPN: &[u8] = DM_ALPN;

static MSG_ID_SEQ: AtomicU64 = AtomicU64::new(0);

/// Returns a monotonically increasing sequence number to keep burst message IDs collision-free.
pub fn next_message_seq() -> u64 {
    MSG_ID_SEQ.fetch_add(1, Ordering::Relaxed)
}

/// Sanitizes an avatar hash into a bare hex/hash filename stem.
pub fn sanitize_avatar_hash(avatar_hash: &str) -> String {
    let clean = avatar_hash.replace('\\', "/");
    clean
        .split('/')
        .last()
        .unwrap_or(avatar_hash)
        .trim_end_matches(".png")
        .to_string()
}

/// Returns the canonical avatar path `<app_data_dir>/avatars/<hash>.png`.
pub fn avatar_file_path(app_data_dir: &Path, avatar_hash: &str) -> PathBuf {
    let hash_part = sanitize_avatar_hash(avatar_hash);
    app_data_dir.join("avatars").join(format!("{hash_part}.png"))
}

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
pub struct DownloadAttachmentResult {
    pub saved_path: String,
    pub data_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncTicket {
    pub ticket: String,
    pub blob_hash: String,
    pub contacts_count: usize,
    pub messages_count: usize,
    pub exported_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncPackage {
    pub source_node_id: String,
    pub contacts: Vec<crate::storage::models::Contact>,
    pub messages: Vec<crate::storage::models::Message>,
    pub exported_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSyncResult {
    pub contacts_imported: usize,
    pub messages_imported: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DmPayload {
    #[serde(alias = "Message")]
    DirectMessage {
        #[serde(flatten)]
        message: Message,
    },
    #[serde(alias = "Ack")]
    DeliveryAck {
        message_id: String,
    },
    ProfileHandshake {
        #[serde(default)]
        node_id: Option<String>,
        nickname: String,
        avatar_hash: Option<String>,
    },
    DirectCallRing {
        caller_id: String,
        room_ticket: String,
    },
    MessageEdited {
        message_id: String,
        new_content: String,
    },
    MessageDeleted {
        message_id: String,
    },
}

pub type DirectMessageEnvelope = DmPayload;

/// Writes a 4-byte big-endian length prefix followed by the JSON-encoded payload.
pub async fn write_framed_payload<T: Serialize>(
    send: &mut iroh::endpoint::SendStream,
    payload: &T,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = serde_json::to_vec(payload)?;
    let len = (data.len() as u32).to_be_bytes();
    send.write_all(&len).await?;
    send.write_all(&data).await?;
    let _ = send.finish();
    Ok(())
}

/// Reads a 4-byte big-endian length prefix followed by the framed JSON payload.
/// Falls back to raw JSON stream reading if raw JSON (starting with '{') is received.
pub async fn read_framed_payload(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<DmPayload, Box<dyn std::error::Error + Send + Sync>> {
    let mut prefix = [0u8; 4];
    recv.read_exact(&mut prefix).await?;
    if prefix[0] == b'{' {
        let rest = recv.read_to_end(65536).await?;
        let mut full = prefix.to_vec();
        full.extend_from_slice(&rest);
        let payload = serde_json::from_slice::<DmPayload>(&full)?;
        return Ok(payload);
    }
    let len = u32::from_be_bytes(prefix) as usize;
    if len > 10 * 1024 * 1024 {
        return Err("Payload too large".into());
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;
    let payload = serde_json::from_slice::<DmPayload>(&buf)?;
    Ok(payload)
}

/// Sanitizes an attachment filename to a safe single-segment basename.
pub fn sanitize_attachment_filename(name: &str) -> String {
    let clean = name.replace('\\', "/");
    let candidate = clean
        .split('/')
        .next_back()
        .unwrap_or("")
        .trim();
    if candidate.is_empty() || candidate == "." || candidate == ".." {
        "attachment.bin".to_string()
    } else {
        candidate.to_string()
    }
}

/// Encodes image/attachment bytes into a Base64 Data URL, sniffing magic bytes first
/// and falling back to the filename extension MIME type.
pub fn bytes_to_data_url(bytes: &[u8], filename: Option<&str>) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mime = if bytes.starts_with(b"\x89PNG") {
        "image/png".to_string()
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        "image/webp".to_string()
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        "image/jpeg".to_string()
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif".to_string()
    } else if let Some(name) = filename {
        detect_mime_type(name)
    } else {
        "image/png".to_string()
    };
    format!("data:{mime};base64,{b64}")
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

/// Helper to reliably download an avatar blob from a peer with up to 4 retries and backoff.
pub async fn download_avatar_blob(
    endpoint: Arc<RwLock<Option<Endpoint>>>,
    blobs_store: Arc<RwLock<Option<MemStore>>>,
    db: Database,
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    app_data_dir: PathBuf,
    peer_id: String,
    avatar_hash: String,
) {
    let hash_part = sanitize_avatar_hash(&avatar_hash);
    if hash_part.is_empty() {
        return;
    }

    let avatar_file = avatar_file_path(&app_data_dir, &hash_part);
    let avatars_dir = app_data_dir.join("avatars");

    // If file already exists locally, ensure it is in blobs_store and DB is updated
    if avatar_file.exists() {
        if let Ok(bytes) = tokio::fs::read(&avatar_file).await {
            if let Some(store) = blobs_store.read().await.clone() {
                let _ = store.add_bytes(bytes).temp_tag().await;
            }
        }
        let _ = db.handle_profile_handshake(&peer_id, "", Some(&hash_part));
        if let Some(ref app) = *app_handle.read().await {
            use tauri::Emitter;
            let _ = app.emit("contacts://updated", ());
            let _ = app.emit("avatar://downloaded", &hash_part);
        }
        return;
    }

    let Ok(target_pk) = PublicKey::from_str(&peer_id) else { return };
    let Ok(hash_obj) = Hash::from_str(&hash_part) else { return };
    let addr = EndpointAddr::new(target_pk);

    for attempt in 1..=4 {
        let ep_opt = endpoint.read().await.clone();
        let store_opt = blobs_store.read().await.clone();
        if let (Some(ep), Some(store)) = (ep_opt, store_opt) {
            match ep.connect(addr.clone(), iroh_blobs::ALPN).await {
                Ok(conn) => {
                    match iroh_blobs::get::request::get_blob(conn, hash_obj).bytes_and_stats().await {
                        Ok((downloaded, _)) => {
                            let _ = store.add_bytes(downloaded.clone()).temp_tag().await;
                            let _ = tokio::fs::create_dir_all(&avatars_dir).await;
                            let _ = tokio::fs::write(&avatar_file, &downloaded).await;
                            let _ = db.handle_profile_handshake(&peer_id, "", Some(&hash_part));
                            if let Some(ref app) = *app_handle.read().await {
                                use tauri::Emitter;
                                let _ = app.emit("contacts://updated", ());
                                let _ = app.emit("avatar://downloaded", &hash_part);
                            }
                            return;
                        }
                        Err(e) => {
                            eprintln!("[Avatar] Download get_blob attempt {attempt} failed for {hash_part}: {e}");
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[Avatar] Connect attempt {attempt} failed for {hash_part}: {e}");
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(800 * attempt)).await;
    }
}

const DM_CONNECT_TIMEOUT: Duration = Duration::from_millis(12_000);
const DM_ACK_TIMEOUT: Duration = Duration::from_millis(4_000);

/// Per-peer exponential backoff state with a monotonic epoch counter.
/// The epoch prevents an in-flight failed connection attempt from overwriting
/// a newer backoff reset (e.g. from `send_direct_message` or an incoming connection).
#[derive(Debug, Clone, Default)]
pub struct PeerBackoffState {
    pub next_try: Option<Instant>,
    pub failures: u32,
    pub epoch: u64,
}

async fn reset_peer_backoff(
    peer_backoff: &RwLock<HashMap<String, PeerBackoffState>>,
    peer_id: &str,
) {
    let mut guard = peer_backoff.write().await;
    let entry = guard.entry(peer_id.to_string()).or_default();
    entry.next_try = None;
    entry.failures = 0;
    entry.epoch = entry.epoch.wrapping_add(1);
}

async fn get_peer_backoff_epoch(
    peer_backoff: &RwLock<HashMap<String, PeerBackoffState>>,
    peer_id: &str,
) -> u64 {
    peer_backoff
        .read()
        .await
        .get(peer_id)
        .map(|s| s.epoch)
        .unwrap_or(0)
}

async fn record_peer_backoff_failure(
    peer_backoff: &RwLock<HashMap<String, PeerBackoffState>>,
    peer_id: &str,
    attempt_epoch: u64,
) -> Option<(u32, u64)> {
    let mut guard = peer_backoff.write().await;
    let entry = guard.entry(peer_id.to_string()).or_default();
    if entry.epoch != attempt_epoch {
        return None;
    }
    entry.failures = entry.failures.saturating_add(1);
    let delay_secs = match entry.failures {
        1 => 5,
        2 => 10,
        3 => 20,
        4 => 45,
        5 => 90,
        _ => 180,
    };
    entry.next_try = Some(Instant::now() + Duration::from_secs(delay_secs));
    Some((entry.failures, delay_secs))
}

/// Protocol handler for accepting 1-on-1 direct messages over QUIC.
#[derive(Clone)]
pub struct DirectMessageHandler {
    db: Database,
    chat_cb: Option<ChatMessageCallback>,
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    app_data_dir: PathBuf,
    endpoint: Arc<RwLock<Option<Endpoint>>>,
    blobs_store: Arc<RwLock<Option<MemStore>>>,
    outbox_notify: Arc<tokio::sync::Notify>,
    peer_backoff: Arc<RwLock<HashMap<String, PeerBackoffState>>>,
}

impl fmt::Debug for DirectMessageHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DirectMessageHandler")
    }
}

impl DirectMessageHandler {
    /// Processes a single decoded DM payload using the TLS-authenticated `peer_id`
    /// (`connection.remote_id()`) and returns an optional response payload for the same stream.
    pub(crate) async fn handle_incoming_payload(
        &self,
        peer_id: &str,
        payload: DmPayload,
    ) -> Option<DmPayload> {
        match payload {
            DmPayload::DirectMessage { mut message } => {
                // Normalize sender_id and chat_id to the cryptographically authenticated peer_id
                message.sender_id = peer_id.to_string();
                message.chat_id = peer_id.to_string();
                message.delivered = 1;

                // Ensure sender exists in contacts table and notify UI when newly created
                let contact_created = match self.db.get_contact(peer_id) {
                    Ok(None) => self.db.handle_profile_handshake(peer_id, "", None).is_ok(),
                    _ => false,
                };
                if contact_created {
                    if let Some(ref app) = *self.app_handle.read().await {
                        use tauri::Emitter;
                        let _ = app.emit("contacts://updated", ());
                    }
                }

                // Use INSERT OR IGNORE so duplicate deliveries never overwrite local flags (e.g. is_edited)
                let inserted = self.db.insert_message_if_absent(&message).unwrap_or(false);

                // Peer is confirmed online: reset backoff and wake outbox flush immediately
                reset_peer_backoff(&self.peer_backoff, peer_id).await;
                self.outbox_notify.notify_one();

                if inserted {
                    if let Some(ref cb) = self.chat_cb {
                        cb(&message);
                    } else if let Some(ref app) = *self.app_handle.read().await {
                        use tauri::Emitter;
                        let _ = app.emit(
                            crate::ipc::events::EVENT_CHAT_MESSAGE,
                            crate::ipc::events::ChatMessagePayload {
                                message: message.clone(),
                            },
                        );
                    }
                }

                Some(DmPayload::DeliveryAck {
                    message_id: message.id,
                })
            }
            DmPayload::DeliveryAck { message_id } => {
                if let Err(e) = self.db.mark_message_delivered(&message_id) {
                    eprintln!("[DM] Failed to mark message {message_id} delivered: {e}");
                }
                self.outbox_notify.notify_one();
                if let Some(ref app) = *self.app_handle.read().await {
                    use tauri::Emitter;
                    let _ = app.emit(
                        crate::ipc::events::EVENT_CHAT_MESSAGE_DELIVERED,
                        crate::ipc::events::MessageDeliveredPayload {
                            message_id: message_id.clone(),
                        },
                    );
                }
                None
            }
            DmPayload::ProfileHandshake {
                node_id,
                nickname,
                avatar_hash,
            } => {
                if let Some(ref claimed) = node_id {
                    if !claimed.is_empty() && claimed != peer_id {
                        eprintln!(
                            "[DM] Ignoring ProfileHandshake with mismatched node_id: claimed={claimed}, remote_id={peer_id}"
                        );
                        return None;
                    }
                }

                // Peer is active: reset backoff and trigger instant outbox flush
                reset_peer_backoff(&self.peer_backoff, peer_id).await;
                self.outbox_notify.notify_one();

                let _ = self.db.handle_profile_handshake(
                    peer_id,
                    &nickname,
                    avatar_hash.as_deref(),
                );

                // Reply with our own ProfileHandshake on the same bi-stream
                let reply = if let Ok(Some(local_profile)) = self.db.get_local_profile() {
                    Some(DmPayload::ProfileHandshake {
                        node_id: Some(local_profile.node_id),
                        nickname: local_profile.nickname,
                        avatar_hash: local_profile.avatar_hash,
                    })
                } else {
                    None
                };

                if let Some(ref hash) = avatar_hash {
                    let ep = self.endpoint.clone();
                    let bs = self.blobs_store.clone();
                    let d = self.db.clone();
                    let ah = self.app_handle.clone();
                    let ad = self.app_data_dir.clone();
                    let pid = peer_id.to_string();
                    let h = hash.clone();
                    if let Ok(handle) = tokio::runtime::Handle::try_current() {
                        handle.spawn(async move {
                            download_avatar_blob(ep, bs, d, ah, ad, pid, h).await;
                        });
                    } else {
                        tauri::async_runtime::spawn(async move {
                            download_avatar_blob(ep, bs, d, ah, ad, pid, h).await;
                        });
                    }
                }
                if let Some(ref app) = *self.app_handle.read().await {
                    use tauri::Emitter;
                    let _ = app.emit("contacts://updated", ());
                }
                reply
            }
            DmPayload::DirectCallRing {
                caller_id,
                room_ticket,
            } => {
                if !caller_id.is_empty() && caller_id != peer_id {
                    eprintln!(
                        "[DM] Ignoring DirectCallRing with mismatched caller_id: claimed={caller_id}, remote_id={peer_id}"
                    );
                    return None;
                }

                reset_peer_backoff(&self.peer_backoff, peer_id).await;
                self.outbox_notify.notify_one();
                if let Some(ref app) = *self.app_handle.read().await {
                    use tauri::Emitter;
                    let _ = app.emit(
                        crate::ipc::events::EVENT_CALL_INCOMING_RING,
                        crate::ipc::events::DirectCallRingPayload {
                            caller_id: peer_id.to_string(),
                            room_ticket,
                        },
                    );
                }
                Some(DmPayload::DeliveryAck {
                    message_id: "ring".to_string(),
                })
            }
            DmPayload::MessageEdited {
                message_id,
                new_content,
            } => {
                if let Ok(Some(msg)) = self.db.get_message(&message_id) {
                    if msg.sender_id == peer_id {
                        if let Err(e) = self.db.edit_message(&message_id, &new_content) {
                            eprintln!("[DM] Failed to apply MessageEdited for {message_id}: {e}");
                        }
                        if let Some(ref app) = *self.app_handle.read().await {
                            use tauri::Emitter;
                            let _ = app.emit(
                                crate::ipc::events::EVENT_CHAT_MESSAGE_EDITED,
                                crate::ipc::events::MessageEditedPayload {
                                    message_id: message_id.clone(),
                                    new_content,
                                },
                            );
                        }
                    } else {
                        eprintln!(
                            "[DM] Rejected MessageEdited for {message_id}: sender_id={} != remote_id={peer_id}",
                            msg.sender_id
                        );
                    }
                }
                Some(DmPayload::DeliveryAck { message_id })
            }
            DmPayload::MessageDeleted { message_id } => {
                if let Ok(Some(msg)) = self.db.get_message(&message_id) {
                    if msg.sender_id == peer_id {
                        if let Err(e) = self.db.delete_message(&message_id) {
                            eprintln!("[DM] Failed to apply MessageDeleted for {message_id}: {e}");
                        }
                        if let Some(ref app) = *self.app_handle.read().await {
                            use tauri::Emitter;
                            let _ = app.emit(
                                crate::ipc::events::EVENT_CHAT_MESSAGE_DELETED,
                                crate::ipc::events::MessageDeletedPayload {
                                    message_id: message_id.clone(),
                                },
                            );
                        }
                    } else {
                        eprintln!(
                            "[DM] Rejected MessageDeleted for {message_id}: sender_id={} != remote_id={peer_id}",
                            msg.sender_id
                        );
                    }
                }
                Some(DmPayload::DeliveryAck { message_id })
            }
        }
    }
}

impl ProtocolHandler for DirectMessageHandler {
    fn accept(
        &self,
        connection: Connection,
    ) -> impl Future<Output = Result<(), AcceptError>> + Send {
        let handler = self.clone();

        async move {
            let peer_id = connection.remote_id().to_string();
            eprintln!("[DM] Accepted incoming DM_ALPN connection from {peer_id}");
            reset_peer_backoff(&handler.peer_backoff, &peer_id).await;
            handler.outbox_notify.notify_one();

            loop {
                match connection.accept_bi().await {
                    Ok((mut send, mut recv)) => {
                        let h = handler.clone();
                        let pid = peer_id.clone();
                        let task = async move {
                            match read_framed_payload(&mut recv).await {
                                Ok(payload) => {
                                    if let Some(reply) = h.handle_incoming_payload(&pid, payload).await {
                                        if let Err(e) = write_framed_payload(&mut send, &reply).await {
                                            eprintln!("[DM] Failed to write reply to {pid}: {e}");
                                        }
                                    } else {
                                        let _ = send.finish();
                                    }
                                }
                                Err(e) => {
                                    eprintln!("[DM] Failed to read framed payload from {pid}: {e}");
                                }
                            }
                        };

                        if let Ok(rt) = tokio::runtime::Handle::try_current() {
                            rt.spawn(task);
                        } else {
                            tauri::async_runtime::spawn(task);
                        }
                    }
                    Err(e) => {
                        eprintln!("[DM] Connection accept_bi ended for {peer_id}: {e}");
                        break;
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
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    offline_watcher_cancel: Arc<AtomicBool>,
    outbox_notify: Arc<tokio::sync::Notify>,
    peer_backoff: Arc<RwLock<HashMap<String, PeerBackoffState>>>,
    peer_locks: Arc<RwLock<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    dm_connections: Arc<RwLock<HashMap<String, Connection>>>,
    memory_lookup: Arc<RwLock<Option<iroh::address_lookup::memory::MemoryLookup>>>,
}

impl FileTransferManager {
    pub fn new(db: Database, app_data_dir: PathBuf) -> Arc<Self> {
        Self::with_callbacks(db, app_data_dir, None, None, None)
    }

    pub fn with_callbacks(
        db: Database,
        app_data_dir: PathBuf,
        progress_cb: Option<TransferProgressCallback>,
        chat_cb: Option<ChatMessageCallback>,
        app_handle: Option<tauri::AppHandle>,
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
            app_handle: Arc::new(RwLock::new(app_handle)),
            offline_watcher_cancel: Arc::new(AtomicBool::new(false)),
            outbox_notify: Arc::new(tokio::sync::Notify::new()),
            peer_backoff: Arc::new(RwLock::new(HashMap::new())),
            peer_locks: Arc::new(RwLock::new(HashMap::new())),
            dm_connections: Arc::new(RwLock::new(HashMap::new())),
            memory_lookup: Arc::new(RwLock::new(None)),
        })
    }

    pub async fn set_memory_lookup(&self, lookup: iroh::address_lookup::memory::MemoryLookup) {
        *self.memory_lookup.write().await = Some(lookup);
    }

    async fn record_peer_addr(&self, ep: &Endpoint, peer_id: PublicKey) {
        if let Some(ml) = self.memory_lookup.read().await.clone() {
            crate::network::node::record_connected_peer(ep, &ml, peer_id).await;
        }
    }

    pub async fn set_app_handle(&self, app_handle: tauri::AppHandle) {
        *self.app_handle.write().await = Some(app_handle);
    }

    pub fn set_local_node_id_sync(&self, local_node_id: &str) {
        if let Ok(mut guard) = self.local_node_id.try_write() {
            *guard = local_node_id.to_string();
        }
    }

    pub async fn set_endpoint_and_store(
        &self,
        endpoint: Endpoint,
        blobs_store: MemStore,
        local_node_id: String,
    ) {
        // Pre-populate blobs_store with local avatar files so peers can fetch them
        let avatars_dir = self.app_data_dir.join("avatars");
        if let Ok(mut entries) = tokio::fs::read_dir(&avatars_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(bytes) = tokio::fs::read(&path).await {
                        let _ = blobs_store.add_bytes(bytes).temp_tag().await;
                    }
                }
            }
        }

        *self.endpoint.write().await = Some(endpoint);
        *self.blobs_store.write().await = Some(blobs_store);
        *self.local_node_id.write().await = local_node_id;
    }

    pub fn create_dm_handler(&self) -> DirectMessageHandler {
        DirectMessageHandler {
            db: self.db.clone(),
            chat_cb: self.chat_cb.clone(),
            app_handle: self.app_handle.clone(),
            app_data_dir: self.app_data_dir.clone(),
            endpoint: self.endpoint.clone(),
            blobs_store: self.blobs_store.clone(),
            outbox_notify: self.outbox_notify.clone(),
            peer_backoff: self.peer_backoff.clone(),
        }
    }

    /// Imports a file from disk into local `iroh-blobs` store, computes Blake3 hash and ticket,
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

        self.attach_file_bytes(&filename, file_bytes, chat_id, text)
            .await
    }

    /// Imports raw bytes into local `iroh-blobs` store, computes Blake3 hash and ticket,
    /// and creates/sends a chat message with the attachment (used on Android/mobile WebView
    /// where `<input type="file">` has no disk path).
    pub async fn attach_file_bytes(
        &self,
        filename: &str,
        file_bytes: Vec<u8>,
        chat_id: Option<String>,
        text: Option<String>,
    ) -> Result<AttachFileResponse, String> {
        let filename = sanitize_attachment_filename(filename);
        let size = file_bytes.len() as u64;
        let mime = detect_mime_type(&filename);
        eprintln!("[VoxP2P] Attaching file '{filename}' ({size} bytes, mime={mime})");

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
        eprintln!("[VoxP2P] Imported attachment '{filename}' into iroh-blobs (hash={hash_str})");

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
        let msg_id = format!("msg_{}_{}_{}", local_id, now, next_message_seq());
        let message_text = text.unwrap_or_default();

        let msg = Message {
            id: msg_id.clone(),
            chat_id: target_chat.clone(),
            sender_id: local_id,
            content: message_text.clone(),
            attachment_hash: Some(hash_str.clone()),
            attachment_name: Some(filename.clone()),
            timestamp: now,
            delivered: 0,
            is_edited: 0,
        };

        // Save in local SQLite DB
        let _ = self.db.insert_message(&msg);

        // If registered, invoke chat callback
        if let Some(ref cb) = self.chat_cb {
            cb(&msg);
        }

        // Trigger immediate outbox sync if direct recipient
        if PublicKey::from_str(&target_chat).is_ok() {
            reset_peer_backoff(&self.peer_backoff, &target_chat).await;
            self.outbox_notify.notify_one();
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

    /// Sends a 1-on-1 direct message via Briar-style outbox.
    /// Inserts into SQLite with `delivered = 0` (NEVER set `delivered = 1` before receiving a network `DeliveryAck`!)
    /// and immediately wakes up the outbox sync worker.
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
        let msg_id = format!("dm_{}_{}_{}", local_id, now, next_message_seq());

        let msg = Message {
            id: msg_id,
            chat_id: recipient_node_id.to_string(),
            sender_id: local_id,
            content: content.to_string(),
            attachment_hash,
            attachment_name,
            timestamp: now,
            delivered: 0,
            is_edited: 0,
        };

        // Insert into SQLite with delivered = 0
        self.db.insert_message(&msg).map_err(|e| e.to_string())?;

        if let Some(ref cb) = self.chat_cb {
            cb(&msg);
        }

        // Reset backoff for this contact since user actively sent a new message
        reset_peer_backoff(&self.peer_backoff, recipient_node_id).await;

        // Trigger the outbox sync worker immediately
        self.outbox_notify.notify_one();

        Ok(msg)
    }

    async fn get_peer_lock(&self, recipient_node_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        if let Some(lock) = self.peer_locks.read().await.get(recipient_node_id).cloned() {
            return lock;
        }
        let mut guard = self.peer_locks.write().await;
        guard
            .entry(recipient_node_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    async fn invalidate_dm_connection(&self, recipient_node_id: &str) {
        self.dm_connections.write().await.remove(recipient_node_id);
    }

    /// Returns a pooled `DM_ALPN` connection for `recipient_node_id` or establishes a new one.
    /// The boolean in the returned tuple is `true` if a new connection was created.
    async fn get_or_connect_dm(&self, recipient_node_id: &str) -> Result<(Connection, bool), String> {
        if let Some(conn) = self.dm_connections.read().await.get(recipient_node_id).cloned() {
            if conn.close_reason().is_none() {
                return Ok((conn, false));
            }
            self.dm_connections.write().await.remove(recipient_node_id);
        }

        let endpoint = match &*self.endpoint.read().await {
            Some(ep) => ep.clone(),
            None => return Err("Endpoint not ready".into()),
        };

        let target_pk = PublicKey::from_str(recipient_node_id).map_err(|e| e.to_string())?;
        let addr = EndpointAddr::new(target_pk);

        let conn = tokio::time::timeout(DM_CONNECT_TIMEOUT, endpoint.connect(addr, DM_ALPN))
            .await
            .map_err(|_| {
                format!(
                    "Connection timed out after {}ms for {recipient_node_id}",
                    DM_CONNECT_TIMEOUT.as_millis()
                )
            })?
            .map_err(|e| format!("Failed to connect to {recipient_node_id}: {e}"))?;

        self.dm_connections
            .write()
            .await
            .insert(recipient_node_id.to_string(), conn.clone());
        reset_peer_backoff(&self.peer_backoff, recipient_node_id).await;
        self.record_peer_addr(&endpoint, target_pk).await;

        Ok((conn, true))
    }

    async fn send_profile_handshake_on_conn(&self, conn: &Connection) {
        let Ok(Some(local_profile)) = self.db.get_local_profile() else {
            return;
        };
        let remote_nid = conn.remote_id().to_string();
        let (mut hs_send, mut hs_recv) = match conn.open_bi().await {
            Ok(bi) => bi,
            Err(e) => {
                eprintln!("[DM] ProfileHandshake open_bi failed for {remote_nid}: {e}");
                return;
            }
        };

        let handshake_payload = DmPayload::ProfileHandshake {
            node_id: Some(local_profile.node_id),
            nickname: local_profile.nickname,
            avatar_hash: local_profile.avatar_hash,
        };
        if let Err(e) = write_framed_payload(&mut hs_send, &handshake_payload).await {
            eprintln!("[DM] ProfileHandshake write failed for {remote_nid}: {e}");
            return;
        }

        if let Ok(Ok(DmPayload::ProfileHandshake {
            node_id,
            nickname,
            avatar_hash,
        })) = tokio::time::timeout(Duration::from_millis(2000), read_framed_payload(&mut hs_recv)).await
        {
            let claimed_ok = node_id
                .as_deref()
                .map(|c| c.is_empty() || c == remote_nid)
                .unwrap_or(true);
            if claimed_ok {
                let _ = self.db.handle_profile_handshake(
                    &remote_nid,
                    &nickname,
                    avatar_hash.as_deref(),
                );
                if let Some(ref hash) = avatar_hash {
                    let ep = self.endpoint.clone();
                    let bs = self.blobs_store.clone();
                    let d = self.db.clone();
                    let ah = self.app_handle.clone();
                    let ad = self.app_data_dir.clone();
                    let pid = remote_nid.clone();
                    let h = hash.clone();
                    if let Ok(handle) = tokio::runtime::Handle::try_current() {
                        handle.spawn(async move {
                            download_avatar_blob(ep, bs, d, ah, ad, pid, h).await;
                        });
                    } else {
                        tauri::async_runtime::spawn(async move {
                            download_avatar_blob(ep, bs, d, ah, ad, pid, h).await;
                        });
                    }
                }
                if let Some(ref app) = *self.app_handle.read().await {
                    use tauri::Emitter;
                    let _ = app.emit("contacts://updated", ());
                }
            }
        }
    }

    async fn send_payload_on_conn(
        &self,
        conn: &Connection,
        recipient_node_id: &str,
        payload: &DmPayload,
    ) -> Result<(), String> {
        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| format!("open_bi failed for {recipient_node_id}: {e}"))?;

        write_framed_payload(&mut send, payload)
            .await
            .map_err(|e| format!("write_framed_payload failed for {recipient_node_id}: {e}"))?;

        match tokio::time::timeout(DM_ACK_TIMEOUT, read_framed_payload(&mut recv)).await {
            Ok(Ok(_ack)) => Ok(()),
            Ok(Err(e)) => {
                let err_str = e.to_string();
                if err_str.contains("eof") || err_str.contains("EOF") || err_str.contains("finished") {
                    Ok(())
                } else {
                    Err(format!("Ack read failed for {recipient_node_id}: {e}"))
                }
            }
            Err(_) => Err(format!("Ack timed out for {recipient_node_id}")),
        }
    }

    /// Sends a direct control payload (e.g. MessageEdited, MessageDeleted, DirectCallRing) over `DM_ALPN`,
    /// reusing the pooled connection and waiting for peer acknowledgement before returning.
    pub async fn send_dm_payload(
        &self,
        recipient_node_id: &str,
        payload: DmPayload,
    ) -> Result<(), String> {
        let (conn, is_new) = self.get_or_connect_dm(recipient_node_id).await.map_err(|e| {
            eprintln!("[DM] send_dm_payload connect error: {e}");
            e
        })?;

        match self
            .send_payload_on_conn(&conn, recipient_node_id, &payload)
            .await
        {
            Ok(()) => Ok(()),
            Err(first_err) => {
                self.invalidate_dm_connection(recipient_node_id).await;
                if !is_new {
                    let (fresh_conn, _) =
                        self.get_or_connect_dm(recipient_node_id).await.map_err(|e| {
                            eprintln!("[DM] send_dm_payload reconnect error: {e}");
                            e
                        })?;
                    if let Err(retry_err) = self
                        .send_payload_on_conn(&fresh_conn, recipient_node_id, &payload)
                        .await
                    {
                        self.invalidate_dm_connection(recipient_node_id).await;
                        eprintln!("[DM] send_dm_payload retry failed for {recipient_node_id}: {retry_err}");
                        return Err(retry_err);
                    }
                    return Ok(());
                }
                eprintln!("[DM] send_dm_payload failed for {recipient_node_id}: {first_err}");
                Err(first_err)
            }
        }
    }

    /// Sends a direct call ring request over `DM_ALPN` to initiate a private 1-on-1 call.
    pub async fn send_direct_call_ring(
        &self,
        recipient_node_id: &str,
        ring: DmPayload,
    ) -> Result<(), String> {
        self.send_dm_payload(recipient_node_id, ring).await
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

        self.flush_contact_outbox(recipient_node_id, pending).await
    }

    /// Attempts to deliver a list of queued messages for a specific contact over QUIC `DM_ALPN`.
    pub async fn flush_contact_outbox(
        &self,
        recipient_node_id: &str,
        messages: Vec<Message>,
    ) -> Result<usize, String> {
        let peer_mutex = self.get_peer_lock(recipient_node_id).await;
        let _peer_guard = peer_mutex.lock().await;

        let attempt_epoch = get_peer_backoff_epoch(&self.peer_backoff, recipient_node_id).await;

        let (mut conn, mut is_new_conn) = match self.get_or_connect_dm(recipient_node_id).await {
            Ok(res) => res,
            Err(err_msg) => {
                if PublicKey::from_str(recipient_node_id).is_ok() && err_msg != "Endpoint not ready" {
                    if let Some((failures, delay_secs)) = record_peer_backoff_failure(
                        &self.peer_backoff,
                        recipient_node_id,
                        attempt_epoch,
                    )
                    .await
                    {
                        eprintln!(
                            "[DM] Outbox connect failed for {recipient_node_id} (attempt={failures}, backoff={delay_secs}s): {err_msg}"
                        );
                    }
                }
                return Err(err_msg);
            }
        };

        if is_new_conn {
            self.send_profile_handshake_on_conn(&conn).await;
        }

        let mut delivered_count = 0;
        let mut retried_stale_conn = false;

        for mut msg in messages {
            // Skip if another flush already marked this message delivered before we acquired peer_lock
            if let Ok(Some(current)) = self.db.get_message(&msg.id) {
                if current.delivered == 1 {
                    continue;
                }
            }

            loop {
                let (mut send, mut recv) = match conn.open_bi().await {
                    Ok(bi) => bi,
                    Err(e) => {
                        eprintln!("[DM] open_bi failed for {recipient_node_id} (msg_id={}): {e}", msg.id);
                        self.invalidate_dm_connection(recipient_node_id).await;
                        if !is_new_conn && !retried_stale_conn {
                            retried_stale_conn = true;
                            if let Ok((fresh_conn, fresh_is_new)) = self.get_or_connect_dm(recipient_node_id).await {
                                conn = fresh_conn;
                                is_new_conn = fresh_is_new;
                                if is_new_conn {
                                    self.send_profile_handshake_on_conn(&conn).await;
                                }
                                continue;
                            }
                        }
                        return Ok(delivered_count);
                    }
                };

                let payload = DmPayload::DirectMessage {
                    message: msg.clone(),
                };

                if let Err(e) = write_framed_payload(&mut send, &payload).await {
                    eprintln!("[DM] write_framed_payload failed for {recipient_node_id} (msg_id={}): {e}", msg.id);
                    self.invalidate_dm_connection(recipient_node_id).await;
                    if !is_new_conn && !retried_stale_conn {
                        retried_stale_conn = true;
                        if let Ok((fresh_conn, fresh_is_new)) = self.get_or_connect_dm(recipient_node_id).await {
                            conn = fresh_conn;
                            is_new_conn = fresh_is_new;
                            if is_new_conn {
                                self.send_profile_handshake_on_conn(&conn).await;
                            }
                            continue;
                        }
                    }
                    return Ok(delivered_count);
                }

                let ack_res = tokio::time::timeout(
                    DM_ACK_TIMEOUT,
                    read_framed_payload(&mut recv),
                )
                .await;

                match ack_res {
                    Ok(Ok(DmPayload::DeliveryAck { message_id })) if message_id == msg.id => {
                        if let Err(e) = self.db.mark_message_delivered(&msg.id) {
                            eprintln!("[DM] Failed to mark message {} delivered in DB: {e}", msg.id);
                        }
                        msg.delivered = 1;
                        delivered_count += 1;

                        if let Some(ref app) = *self.app_handle.read().await {
                            use tauri::Emitter;
                            let _ = app.emit(
                                crate::ipc::events::EVENT_CHAT_MESSAGE_DELIVERED,
                                crate::ipc::events::MessageDeliveredPayload {
                                    message_id: msg.id.clone(),
                                },
                            );
                        }

                        if let Some(ref cb) = self.chat_cb {
                            cb(&msg);
                        }
                        break;
                    }
                    Ok(Ok(other)) => {
                        eprintln!(
                            "[DM] Unexpected ack payload from {recipient_node_id} for msg_id={}: {other:?}",
                            msg.id
                        );
                        self.invalidate_dm_connection(recipient_node_id).await;
                        return Ok(delivered_count);
                    }
                    Ok(Err(e)) => {
                        eprintln!("[DM] Ack read error from {recipient_node_id} (msg_id={}): {e}", msg.id);
                        self.invalidate_dm_connection(recipient_node_id).await;
                        if !is_new_conn && !retried_stale_conn {
                            retried_stale_conn = true;
                            if let Ok((fresh_conn, fresh_is_new)) = self.get_or_connect_dm(recipient_node_id).await {
                                conn = fresh_conn;
                                is_new_conn = fresh_is_new;
                                if is_new_conn {
                                    self.send_profile_handshake_on_conn(&conn).await;
                                }
                                continue;
                            }
                        }
                        return Ok(delivered_count);
                    }
                    Err(_) => {
                        eprintln!("[DM] Ack timed out for {recipient_node_id} (msg_id={})", msg.id);
                        self.invalidate_dm_connection(recipient_node_id).await;
                        return Ok(delivered_count);
                    }
                }
            }
        }

        Ok(delivered_count)
    }

    /// Queries SQLite for contacts with `delivered = 0` messages and synchronizes them concurrently.
    pub async fn sync_outbox(&self) {
        let undelivered = match self.db.get_all_undelivered_messages() {
            Ok(msgs) => msgs,
            Err(e) => {
                eprintln!("[DM] Failed to query undelivered messages: {e}");
                return;
            }
        };
        if undelivered.is_empty() {
            return;
        }

        let mut recipient_map: HashMap<String, Vec<Message>> = HashMap::new();
        for msg in undelivered {
            if PublicKey::from_str(&msg.chat_id).is_ok() {
                recipient_map.entry(msg.chat_id.clone()).or_default().push(msg);
            }
        }

        if recipient_map.is_empty() {
            return;
        }

        let now = Instant::now();
        let ready_recipients: Vec<(String, Vec<Message>)> = {
            let guard = self.peer_backoff.read().await;
            recipient_map
                .into_iter()
                .filter(|(recipient, _)| {
                    if let Some(state) = guard.get(recipient) {
                        if let Some(next_try) = state.next_try {
                            return now >= next_try;
                        }
                    }
                    true
                })
                .collect()
        };

        if ready_recipients.is_empty() {
            return;
        }

        use futures::stream::{self, StreamExt};
        stream::iter(ready_recipients)
            .for_each_concurrent(Some(8), |(recipient, messages)| async move {
                let _ = self.flush_contact_outbox(&recipient, messages).await;
            })
            .await;
    }

    /// Spawns the Briar-style outbox sync worker that runs every 5 seconds (and is triggered immediately on `send_dm`).
    pub fn start_offline_queue_watcher(self: &Arc<Self>) {
        let cancel = self.offline_watcher_cancel.clone();
        let notify = self.outbox_notify.clone();
        let this = self.clone();

        let task_fn = async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            while !cancel.load(Ordering::Acquire) {
                tokio::select! {
                    _ = interval.tick() => {},
                    _ = notify.notified() => {},
                }

                if cancel.load(Ordering::Acquire) {
                    break;
                }

                this.sync_outbox().await;
            }
        };

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(task_fn);
        } else {
            tauri::async_runtime::spawn(task_fn);
        }
    }

    /// Resolves a download destination path.
    /// Empty or relative paths (such as `./downloads/...`) are always mapped to
    /// `<app_data_dir>/downloads/<filename>` so Android's read-only `/` CWD is never used.
    pub fn resolve_download_path(
        &self,
        save_path: &str,
        blob_hash: &str,
        filename: Option<&str>,
    ) -> PathBuf {
        let trimmed = save_path.trim();
        if !trimmed.is_empty() && Path::new(trimmed).is_absolute() {
            return PathBuf::from(trimmed);
        }

        let safe_name = if let Some(name) = filename.filter(|n| !n.trim().is_empty()) {
            sanitize_attachment_filename(name)
        } else if !trimmed.is_empty() && trimmed != "." && trimmed != "./downloads" && trimmed != "downloads" {
            sanitize_attachment_filename(trimmed)
        } else {
            sanitize_attachment_filename(blob_hash)
        };

        self.app_data_dir.join("downloads").join(safe_name)
    }

    /// Downloads a blob from a remote peer via `iroh-blobs`, emitting progress events
    /// `transfer://progress { hash, percent }` to the UI, and saves it to the resolved path.
    pub async fn download_attachment(
        &self,
        node_id: &str,
        blob_hash: &str,
        save_path: &str,
        filename: Option<&str>,
    ) -> Result<DownloadAttachmentResult, String> {
        const MAX_PREVIEW_BYTES: usize = 20 * 1024 * 1024;
        let hash = Hash::from_str(blob_hash).map_err(|e| format!("Invalid blob hash: {e}"))?;
        let target_path = self.resolve_download_path(save_path, blob_hash, filename);
        eprintln!(
            "[VoxP2P] Downloading attachment hash={blob_hash} from peer={node_id} -> {}",
            target_path.display()
        );

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
                .map_err(|e| {
                    eprintln!("[VoxP2P] Blob connect failed for {blob_hash}: {e}");
                    format!("Failed to connect to peer for blob download: {e}")
                })?;

            self.emit_progress(blob_hash, 25.0, 0, 0, false).await;

            // 4. Download blob via verified BLAKE3 get_blob
            let (downloaded, _stats) = iroh_blobs::get::request::get_blob(conn, hash)
                .bytes_and_stats()
                .await
                .map_err(|e| {
                    eprintln!("[VoxP2P] Blob streaming failed for {blob_hash}: {e}");
                    format!("Blob streaming failed: {e}")
                })?;

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

        // 5. Write completed file to resolved target_path
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                eprintln!("[VoxP2P] Failed to create download dir {}: {e}", parent.display());
                format!("Failed to create destination dir: {e}")
            })?;
        }

        tokio::fs::write(&target_path, &bytes).await.map_err(|e| {
            eprintln!("[VoxP2P] Failed to save attachment to {}: {e}", target_path.display());
            format!("Failed to save downloaded file: {e}")
        })?;
        eprintln!(
            "[VoxP2P] Saved attachment ({} bytes) to {}",
            bytes.len(),
            target_path.display()
        );

        // 6. Emit completion progress
        self.emit_progress(
            blob_hash,
            100.0,
            bytes.len() as u64,
            bytes.len() as u64,
            true,
        )
        .await;

        let resolved_name = target_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment.bin");
        let data_url = if bytes.len() <= MAX_PREVIEW_BYTES
            && detect_mime_type(resolved_name).starts_with("image/")
        {
            Some(bytes_to_data_url(&bytes, Some(resolved_name)))
        } else {
            None
        };

        Ok(DownloadAttachmentResult {
            saved_path: target_path.to_string_lossy().to_string(),
            data_url,
        })
    }

    /// Retrieves a local attachment preview as a Base64 Data URL from `iroh-blobs`
    /// or `<app_data_dir>/downloads/` without relying on CWD relative paths.
    pub async fn get_attachment_preview(
        &self,
        attachment_hash: &str,
        attachment_name: Option<&str>,
    ) -> Option<String> {
        const MAX_PREVIEW_BYTES: usize = 20 * 1024 * 1024;
        let hash_trimmed = attachment_hash.trim();
        if hash_trimmed.is_empty() {
            return None;
        }

        let mut bytes_opt: Option<Vec<u8>> = None;

        if let Some(store) = self.blobs_store.read().await.clone() {
            if let Ok(hash) = Hash::from_str(hash_trimmed) {
                if let Ok(b) = store.blobs().get_bytes(hash).await {
                    if b.len() <= MAX_PREVIEW_BYTES {
                        bytes_opt = Some(b.to_vec());
                    } else {
                        return None;
                    }
                }
            }
        }

        if bytes_opt.is_none() {
            let downloads_dir = self.app_data_dir.join("downloads");
            let mut candidates = Vec::new();
            if let Some(name) = attachment_name.filter(|n| !n.trim().is_empty()) {
                candidates.push(downloads_dir.join(sanitize_attachment_filename(name)));
            }
            candidates.push(downloads_dir.join(sanitize_attachment_filename(hash_trimmed)));

            for candidate in candidates {
                if candidate.exists() {
                    if let Ok(meta) = tokio::fs::metadata(&candidate).await {
                        if meta.len() > MAX_PREVIEW_BYTES as u64 {
                            return None;
                        }
                    }
                    if let Ok(b) = tokio::fs::read(&candidate).await {
                        bytes_opt = Some(b);
                        break;
                    }
                }
            }
        }

        bytes_opt.map(|b| bytes_to_data_url(&b, attachment_name))
    }

    /// Automatically fetches and caches peer avatars when an avatar_hash is received during handshake.
    pub async fn handle_avatar_handshake(&self, peer_node_id: &str, avatar_hash: &str) {
        let ep = self.endpoint.clone();
        let bs = self.blobs_store.clone();
        let d = self.db.clone();
        let ah = self.app_handle.clone();
        let ad = self.app_data_dir.clone();
        let pid = peer_node_id.to_string();
        let h = avatar_hash.to_string();
        download_avatar_blob(ep, bs, d, ah, ad, pid, h).await;
    }

    /// Saves an avatar image into local `avatars` directory and imports it into `iroh-blobs`,
    /// returning the BLAKE3 hash string.
    pub async fn save_and_import_avatar(&self, avatar_bytes: &[u8]) -> Result<String, String> {
        let blobs_store = {
            let guard = self.blobs_store.read().await;
            guard
                .clone()
                .ok_or_else(|| "Blobs store not initialized".to_string())?
        };

        // 1. Add to iroh-blobs store
        let tag = blobs_store
            .add_bytes(avatar_bytes.to_vec())
            .temp_tag()
            .await
            .map_err(|e| format!("Failed to import avatar blob into store: {e}"))?;
        let hash_str = tag.hash().to_string();

        // 2. Save file to app_data_dir/avatars/<hash>.png
        let avatars_dir = self.app_data_dir.join("avatars");
        tokio::fs::create_dir_all(&avatars_dir)
            .await
            .map_err(|e| format!("Failed to create avatars dir: {e}"))?;
        let avatar_path = avatar_file_path(&self.app_data_dir, &hash_str);
        tokio::fs::write(&avatar_path, avatar_bytes)
            .await
            .map_err(|e| format!("Failed to save avatar image: {e}"))?;

        Ok(hash_str)
    }

    /// Retrieves an avatar image by its BLAKE3 hash from disk or `iroh-blobs` store,
    /// returning a Base64 Data URL (`data:image/png;base64,...`) for instant rendering.
    pub async fn get_avatar_data_url(&self, avatar_hash: &str) -> Option<String> {
        let hash_part = sanitize_avatar_hash(avatar_hash);
        let avatar_path = avatar_file_path(&self.app_data_dir, &hash_part);

        let bytes = if avatar_path.exists() {
            tokio::fs::read(&avatar_path).await.ok()
        } else if let Some(store) = self.blobs_store.read().await.clone() {
            if let Ok(hash) = Hash::from_str(&hash_part) {
                if let Ok(b) = store.blobs().get_bytes(hash).await {
                    let _ = tokio::fs::write(&avatar_path, &b).await;
                    Some(b.to_vec())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        bytes.map(|b| bytes_to_data_url(&b, None))
    }

    /// Broadcasts an updated profile (nickname and/or avatar_hash) to all contacts via 1-on-1 DM handshake.
    pub async fn broadcast_profile_handshake(
        self: &Arc<Self>,
        nickname: &str,
        avatar_hash: Option<&str>,
    ) {
        let contacts = match self.db.get_contacts() {
            Ok(c) => c,
            Err(_) => return,
        };
        if contacts.is_empty() {
            return;
        }

        let local_id = self.local_node_id.read().await.clone();
        let payload = DmPayload::ProfileHandshake {
            node_id: Some(local_id),
            nickname: nickname.to_string(),
            avatar_hash: avatar_hash.map(|s| s.to_string()),
        };

        let endpoint_opt = self.endpoint.read().await.clone();
        let Some(endpoint) = endpoint_opt else { return };

        for contact in contacts {
            if let Ok(pk) = PublicKey::from_str(&contact.node_id) {
                let ep = endpoint.clone();
                let p = payload.clone();
                let self_clone = self.clone();
                tauri::async_runtime::spawn(async move {
                    let addr = EndpointAddr::new(pk);
                    if let Ok(conn_res) = tokio::time::timeout(Duration::from_secs(3), ep.connect(addr, DM_ALPN)).await {
                        if let Ok(conn) = conn_res {
                            if let Ok((mut send, mut recv)) = conn.open_bi().await {
                                if write_framed_payload(&mut send, &p).await.is_ok() {
                                    if let Ok(Ok(DmPayload::ProfileHandshake { node_id, nickname, avatar_hash })) =
                                        tokio::time::timeout(Duration::from_millis(2000), read_framed_payload(&mut recv)).await
                                    {
                                        let remote_nid = conn.remote_id().to_string();
                                        let claimed_ok = node_id
                                            .as_deref()
                                            .map(|c| c.is_empty() || c == remote_nid)
                                            .unwrap_or(true);
                                        if claimed_ok {
                                            let _ = self_clone.db.handle_profile_handshake(&remote_nid, &nickname, avatar_hash.as_deref());
                                            if let Some(ref h) = avatar_hash {
                                                self_clone.handle_avatar_handshake(&remote_nid, h).await;
                                            }
                                            if let Some(ref app) = *self_clone.app_handle.read().await {
                                                use tauri::Emitter;
                                                let _ = app.emit("contacts://updated", ());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                });
            }
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

    /// Exports contacts and chat history into a serialized JSON blob in iroh-blobs,
    /// generating a pairing ticket so a mobile or second device can synchronize history.
    pub async fn sync_devices(&self) -> Result<DeviceSyncTicket, String> {
        let contacts = self.db.get_contacts().map_err(|e| e.to_string())?;
        let messages = self.db.get_all_messages().map_err(|e| e.to_string())?;
        let local_id = self.local_node_id.read().await.clone();
        let exported_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let package = DeviceSyncPackage {
            source_node_id: local_id,
            contacts: contacts.clone(),
            messages: messages.clone(),
            exported_at,
        };

        let json_bytes = serde_json::to_vec(&package).map_err(|e| e.to_string())?;

        let blobs_store = {
            let guard = self.blobs_store.read().await;
            guard
                .clone()
                .ok_or_else(|| "Blobs store not initialized".to_string())?
        };

        let tag = blobs_store
            .add_bytes(json_bytes)
            .temp_tag()
            .await
            .map_err(|e| format!("Failed to import sync blob into store: {e}"))?;
        let hash = tag.hash();
        let hash_str = hash.to_string();

        let endpoint_opt = self.endpoint.read().await.clone();
        let ticket_str = if let Some(endpoint) = endpoint_opt {
            let addr = endpoint.addr();
            let ticket = BlobTicket::new(addr, hash, BlobFormat::Raw);
            ticket.to_string()
        } else {
            hash_str.clone()
        };

        Ok(DeviceSyncTicket {
            ticket: ticket_str,
            blob_hash: hash_str,
            contacts_count: contacts.len(),
            messages_count: messages.len(),
            exported_at,
        })
    }

    /// Imports contacts and chat messages from a pairing ticket or raw sync package JSON.
    pub async fn import_sync_ticket(&self, ticket_str: &str) -> Result<ImportSyncResult, String> {
        let package: DeviceSyncPackage = if ticket_str.trim().starts_with('{') {
            serde_json::from_str(ticket_str).map_err(|e| format!("Invalid sync package JSON: {e}"))?
        } else {
            let ticket = BlobTicket::from_str(ticket_str)
                .map_err(|e| format!("Invalid sync blob ticket: {e}"))?;

            let endpoint = self
                .endpoint
                .read()
                .await
                .clone()
                .ok_or_else(|| "Endpoint not initialized".to_string())?;

            if let Some(ml) = self.memory_lookup.read().await.clone() {
                ml.add_endpoint_info(ticket.addr().clone());
            }

            let conn = endpoint
                .connect(ticket.addr().clone(), iroh_blobs::ALPN)
                .await
                .map_err(|e| format!("Failed to connect for sync import: {e}"))?;

            let (bytes, _) = iroh_blobs::get::request::get_blob(conn, ticket.hash())
                .bytes_and_stats()
                .await
                .map_err(|e| format!("Failed to download sync blob: {e}"))?;

            serde_json::from_slice(&bytes).map_err(|e| format!("Failed to deserialize sync package: {e}"))?
        };

        let mut contacts_imported = 0;
        for c in package.contacts {
            if self.db.insert_or_update_contact(&c).is_ok() {
                contacts_imported += 1;
            }
        }

        let mut messages_imported = 0;
        for m in package.messages {
            if self.db.insert_message(&m).is_ok() {
                messages_imported += 1;
            }
        }

        Ok(ImportSyncResult {
            contacts_imported,
            messages_imported,
        })
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

    #[tokio::test]
    async fn test_device_sync_export_and_import() {
        let db = Database::new_in_memory().unwrap();
        let temp_dir = std::env::temp_dir().join("voxp2p_test_sync");
        let ft = FileTransferManager::new(db.clone(), temp_dir);
        let blobs_store = MemStore::new();
        *ft.blobs_store.write().await = Some(blobs_store);

        // Insert contact and message
        let contact = crate::storage::models::Contact {
            node_id: "test_node_id_12345".to_string(),
            nickname: "TestContact".to_string(),
            avatar_path: None,
            custom_volume: 1.0,
            last_seen: 100,
        };
        db.insert_or_update_contact(&contact).unwrap();

        let msg = Message {
            id: "msg_sync_1".to_string(),
            chat_id: "test_node_id_12345".to_string(),
            sender_id: "local_node".to_string(),
            content: "Sync test content".to_string(),
            attachment_hash: None,
            attachment_name: None,
            timestamp: 200,
            delivered: 1,
            is_edited: 0,
        };
        db.insert_message(&msg).unwrap();

        // 1. Export sync package ticket
        let sync_ticket = ft.sync_devices().await.unwrap();
        assert_eq!(sync_ticket.contacts_count, 1);
        assert_eq!(sync_ticket.messages_count, 1);
        assert!(!sync_ticket.ticket.is_empty());

        // 2. Import into a fresh new second DB
        let db2 = Database::new_in_memory().unwrap();
        let temp_dir2 = std::env::temp_dir().join("voxp2p_test_sync2");
        let ft2 = FileTransferManager::new(db2.clone(), temp_dir2);

        let package = DeviceSyncPackage {
            source_node_id: "local_node".to_string(),
            contacts: vec![contact],
            messages: vec![msg],
            exported_at: 300,
        };
        let raw_json = serde_json::to_string(&package).unwrap();

        let import_res = ft2.import_sync_ticket(&raw_json).await.unwrap();
        assert_eq!(import_res.contacts_imported, 1);
        assert_eq!(import_res.messages_imported, 1);

        // Verify in db2
        let imported_contacts = db2.get_contacts().unwrap();
        assert_eq!(imported_contacts.len(), 1);
        assert_eq!(imported_contacts[0].nickname, "TestContact");

        let imported_msgs = db2.get_messages_for_chat("test_node_id_12345", 10).unwrap();
        assert_eq!(imported_msgs.len(), 1);
        assert_eq!(imported_msgs[0].content, "Sync test content");
    }

    #[tokio::test]
    async fn test_incoming_dm_normalizes_chat_and_sender_id_and_creates_contact() {
        let peer_a_id = "peer_a_authenticated_node_id".to_string();
        let peer_b_id = "peer_b_local_node_id".to_string();

        let db_b = Database::new_in_memory().unwrap();
        let temp_dir_b = std::env::temp_dir().join("voxp2p_test_incoming_dm_b");
        let ft_b = FileTransferManager::new(db_b.clone(), temp_dir_b.clone());
        let handler_b = ft_b.create_dm_handler();

        // Peer A sends a DM where chat_id on the wire was B's NodeId and sender_id was spoofed
        let wire_msg = Message {
            id: "dm_a_to_b_1".to_string(),
            chat_id: peer_b_id.clone(),
            sender_id: "spoofed_sender".to_string(),
            content: "Hello B from A!".to_string(),
            attachment_hash: None,
            attachment_name: None,
            timestamp: 12345,
            delivered: 0,
            is_edited: 0,
        };

        let reply = handler_b
            .handle_incoming_payload(
                &peer_a_id,
                DmPayload::DirectMessage { message: wire_msg.clone() },
            )
            .await;

        match reply {
            Some(DmPayload::DeliveryAck { message_id }) => {
                assert_eq!(message_id, "dm_a_to_b_1");
            }
            other => panic!("Expected DeliveryAck, got {other:?}"),
        }

        // Stored message on B must have chat_id == A and sender_id == A
        let stored = db_b.get_messages_for_chat(&peer_a_id, 50).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].chat_id, peer_a_id);
        assert_eq!(stored[0].sender_id, peer_a_id);
        assert_eq!(stored[0].delivered, 1);
        assert_eq!(stored[0].content, "Hello B from A!");

        // Must NOT be stored under peer_b_id
        assert!(db_b.get_messages_for_chat(&peer_b_id, 50).unwrap().is_empty());

        // Contact for peer A must have been auto-created
        let contact_a = db_b.get_contact(&peer_a_id).unwrap();
        assert!(contact_a.is_some());

        // Unauthorized peer C trying to edit or delete peer A's message must be rejected
        handler_b
            .handle_incoming_payload(
                "peer_c_attacker",
                DmPayload::MessageEdited {
                    message_id: "dm_a_to_b_1".to_string(),
                    new_content: "Hacked!".to_string(),
                },
            )
            .await;
        assert_eq!(
            db_b.get_message("dm_a_to_b_1").unwrap().unwrap().content,
            "Hello B from A!"
        );

        handler_b
            .handle_incoming_payload(
                "peer_c_attacker",
                DmPayload::MessageDeleted {
                    message_id: "dm_a_to_b_1".to_string(),
                },
            )
            .await;
        assert!(db_b.get_message("dm_a_to_b_1").unwrap().is_some());

        // Authorized edit from peer A succeeds, and redelivery must not overwrite is_edited
        handler_b
            .handle_incoming_payload(
                &peer_a_id,
                DmPayload::MessageEdited {
                    message_id: "dm_a_to_b_1".to_string(),
                    new_content: "Edited by A".to_string(),
                },
            )
            .await;
        let _ = handler_b
            .handle_incoming_payload(
                &peer_a_id,
                DmPayload::DirectMessage { message: wire_msg },
            )
            .await;
        let after_redelivery = db_b.get_message("dm_a_to_b_1").unwrap().unwrap();
        assert_eq!(after_redelivery.content, "Edited by A");
        assert_eq!(after_redelivery.is_edited, 1);

        tokio::fs::remove_dir_all(&temp_dir_b).await.ok();
    }

    #[tokio::test]
    async fn test_burst_dm_ids_are_unique() {
        let db = Database::new_in_memory().unwrap();
        let temp_dir = std::env::temp_dir().join("voxp2p_test_burst_ids");
        let ft = FileTransferManager::new(db, temp_dir.clone());
        let mut ids = std::collections::HashSet::new();
        for _ in 0..20 {
            let msg = ft
                .send_direct_message("peer_x", "burst", None, None)
                .await
                .unwrap();
            assert!(ids.insert(msg.id), "duplicate message ID generated in burst");
        }
        tokio::fs::remove_dir_all(&temp_dir).await.ok();
    }

    #[tokio::test]
    async fn test_dm_end_to_end_online_offline_and_control_frames() {
        use iroh::address_lookup::memory::MemoryLookup;
        use iroh::endpoint::presets;
        use iroh::protocol::Router;
        use iroh::SecretKey;

        let sk_a = SecretKey::generate();
        let sk_b = SecretKey::generate();
        let id_a = sk_a.public().to_string();
        let id_b = sk_b.public().to_string();

        let lookup_a = MemoryLookup::new();
        let lookup_b = MemoryLookup::new();

        let temp_a = std::env::temp_dir().join(format!("voxp2p_test_dm_a_{}", next_message_seq()));
        let temp_b = std::env::temp_dir().join(format!("voxp2p_test_dm_b_{}", next_message_seq()));

        let db_a = Database::new_in_memory().unwrap();
        let db_b = Database::new_in_memory().unwrap();

        let ft_a = FileTransferManager::new(db_a.clone(), temp_a.clone());
        let ft_b = FileTransferManager::new(db_b.clone(), temp_b.clone());

        let ep_a = Endpoint::builder(presets::Minimal)
            .secret_key(sk_a)
            .address_lookup(lookup_a.clone())
            .bind()
            .await
            .unwrap();
        ft_a.set_endpoint_and_store(ep_a.clone(), MemStore::new(), id_a.clone())
            .await;
        let router_a = Router::builder(ep_a.clone())
            .accept(DM_ALPN, ft_a.create_dm_handler())
            .spawn();
        ft_a.start_offline_queue_watcher();

        // 1. Offline send: B is not started yet -> message stays queued with delivered == 0
        let offline_msg = ft_a
            .send_direct_message(&id_b, "Queued while B is offline", None, None)
            .await
            .unwrap();
        assert_eq!(offline_msg.delivered, 0);
        assert_eq!(offline_msg.chat_id, id_b);

        tokio::time::sleep(Duration::from_millis(300)).await;
        let still_pending = db_a.get_undelivered_messages(&id_b).unwrap();
        assert_eq!(still_pending.len(), 1);
        assert_eq!(still_pending[0].delivered, 0);

        // 2. Start B's Endpoint and exchange MemoryLookup addresses
        let ep_b = Endpoint::builder(presets::Minimal)
            .secret_key(sk_b)
            .address_lookup(lookup_b.clone())
            .bind()
            .await
            .unwrap();
        ft_b.set_endpoint_and_store(ep_b.clone(), MemStore::new(), id_b.clone())
            .await;
        let router_b = Router::builder(ep_b.clone())
            .accept(DM_ALPN, ft_b.create_dm_handler())
            .spawn();
        ft_b.start_offline_queue_watcher();

        lookup_a.add_endpoint_info(ep_b.addr());
        lookup_b.add_endpoint_info(ep_a.addr());

        // Wait for A's outbox watcher to automatically deliver the offline message once backoff expires
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let a_msg = db_a.get_message(&offline_msg.id).unwrap().unwrap();
            let b_msgs = db_b.get_messages_for_chat(&id_a, 50).unwrap();
            if a_msg.delivered == 1 && b_msgs.iter().any(|m| m.id == offline_msg.id) {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "Timed out waiting for offline DM delivery: a_delivered={}, b_count={}",
                    a_msg.delivered,
                    b_msgs.len()
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // 3. Mutual online DM (A -> B and B -> A)
        let msg_ab = ft_a
            .send_direct_message(&id_b, "Online A to B", None, None)
            .await
            .unwrap();
        let msg_ba = ft_b
            .send_direct_message(&id_a, "Online B to A", None, None)
            .await
            .unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            let a_ab = db_a.get_message(&msg_ab.id).unwrap().unwrap();
            let b_ba = db_b.get_message(&msg_ba.id).unwrap().unwrap();
            let b_has_ab = db_b.get_message(&msg_ab.id).unwrap();
            let a_has_ba = db_a.get_message(&msg_ba.id).unwrap();
            if a_ab.delivered == 1
                && b_ba.delivered == 1
                && b_has_ab.is_some()
                && a_has_ba.is_some()
            {
                assert_eq!(a_ab.chat_id, id_b);
                assert_eq!(b_has_ab.unwrap().chat_id, id_a);
                assert_eq!(b_ba.chat_id, id_a);
                assert_eq!(a_has_ba.unwrap().chat_id, id_b);
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!("Timed out waiting for mutual online DM delivery");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // 4. Reliable delivery of MessageEdited and MessageDeleted via send_dm_payload
        ft_a.send_dm_payload(
            &id_b,
            DmPayload::MessageEdited {
                message_id: msg_ab.id.clone(),
                new_content: "Online A to B (edited)".to_string(),
            },
        )
        .await
        .unwrap();

        let edited_on_b = db_b.get_message(&msg_ab.id).unwrap().unwrap();
        assert_eq!(edited_on_b.content, "Online A to B (edited)");
        assert_eq!(edited_on_b.is_edited, 1);

        ft_a.send_dm_payload(
            &id_b,
            DmPayload::MessageDeleted {
                message_id: msg_ab.id.clone(),
            },
        )
        .await
        .unwrap();

        assert!(db_b.get_message(&msg_ab.id).unwrap().is_none());

        // Cleanup
        ft_a.offline_watcher_cancel.store(true, Ordering::Release);
        ft_b.offline_watcher_cancel.store(true, Ordering::Release);
        let _ = router_a.shutdown().await;
        let _ = router_b.shutdown().await;
        tokio::fs::remove_dir_all(&temp_a).await.ok();
        tokio::fs::remove_dir_all(&temp_b).await.ok();
    }

    #[tokio::test]
    async fn test_attach_file_bytes_and_preview() {
        let temp_dir = std::env::temp_dir().join("voxp2p_test_attach_bytes");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let db = Database::new_in_memory().unwrap();
        let ft = FileTransferManager::new(db.clone(), temp_dir.clone());
        let store = MemStore::new();
        let ep = iroh::Endpoint::builder(iroh::endpoint::presets::N0)
            .bind()
            .await
            .unwrap();
        let node_id = ep.id().to_string();
        ft.set_endpoint_and_store(ep.clone(), store.clone(), node_id)
            .await;

        let png_bytes = b"\x89PNG\r\n\x1a\nfake_png_data".to_vec();
        let resp = ft
            .attach_file_bytes(
                "subdir/photo.png",
                png_bytes.clone(),
                Some("chat_1".to_string()),
                Some("Shared photo".to_string()),
            )
            .await
            .unwrap();

        assert_eq!(resp.attachment_name, "photo.png");
        assert_eq!(resp.attachment_size, png_bytes.len() as u64);
        assert_eq!(resp.mime_type, "image/png");

        let preview = ft
            .get_attachment_preview(&resp.attachment_hash, Some(&resp.attachment_name))
            .await
            .expect("preview should exist in blobs_store");
        assert!(preview.starts_with("data:image/png;base64,"));

        // Verify disk fallback in <app_data_dir>/downloads/<filename>
        let ft_disk = FileTransferManager::new(db, temp_dir.clone());
        let downloads_dir = temp_dir.join("downloads");
        std::fs::create_dir_all(&downloads_dir).unwrap();
        std::fs::write(downloads_dir.join("photo.png"), &png_bytes).unwrap();

        let disk_preview = ft_disk
            .get_attachment_preview(&resp.attachment_hash, Some("photo.png"))
            .await
            .expect("preview should fall back to app_data_dir/downloads");
        assert_eq!(disk_preview, preview);

        let _ = ep.close().await;
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_resolve_download_path_avoids_cwd_relative() {
        let temp_dir = std::env::temp_dir().join("voxp2p_test_dl_path");
        let db = Database::new_in_memory().unwrap();
        let ft = FileTransferManager::new(db, temp_dir.clone());

        assert_eq!(
            ft.resolve_download_path("./downloads/pic.png", "abc123", Some("pic.png")),
            temp_dir.join("downloads").join("pic.png")
        );
        assert_eq!(
            ft.resolve_download_path("", "abc123", Some("folder/doc.pdf")),
            temp_dir.join("downloads").join("doc.pdf")
        );
        assert_eq!(
            ft.resolve_download_path("", "abc123", None),
            temp_dir.join("downloads").join("abc123")
        );

        let abs = temp_dir.join("custom").join("file.bin");
        assert_eq!(
            ft.resolve_download_path(abs.to_str().unwrap(), "abc123", Some("ignored.bin")),
            abs
        );
    }
}
