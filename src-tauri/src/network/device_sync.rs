use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use iroh::{
    endpoint::Connection,
    SecretKey,
};
use qrcode::render::svg;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::audio::AudioManager;
use crate::network::file_transfer::FileTransferManager;
use crate::network::node::{NodeAddr, NodeManager};
use crate::storage::db::Database;
use crate::storage::models::{Contact, LocalProfile, Message};

pub const SYNC_ALPN: &[u8] = b"/p2pchat/sync/1";
pub const SYNC_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncTicketPayload {
    pub ticket: String,
    pub qr_svg: String,
    pub expires_in_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    pub success: bool,
    pub error: Option<String>,
    pub payload: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBundle {
    pub secret_key_bytes: Vec<u8>,
    pub profile: LocalProfile,
    pub avatar_bytes: Option<Vec<u8>>,
    pub contacts: Vec<Contact>,
    pub messages: Vec<Message>,
    pub exported_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSyncImportResult {
    pub node_id: String,
    pub nickname: String,
    pub contacts_imported: usize,
    pub messages_imported: usize,
}

/// Derives a 32-byte symmetric key from the one-time token using Blake3 KDF.
pub fn derive_sync_key(token: &str) -> [u8; 32] {
    blake3::derive_key("voxp2p-sync-encryption-v1", token.as_bytes())
}

/// Encrypts plaintext bytes using Blake3 authenticated stream cipher:
/// Layout: [16-byte random nonce] + [32-byte Blake3 MAC] + [ciphertext]
pub fn encrypt_sync_bundle(key: &[u8; 32], plaintext: &[u8]) -> Vec<u8> {
    let nonce = SecretKey::generate().to_bytes()[..16].to_vec();

    let mut enc_hasher = blake3::Hasher::new_keyed(key);
    enc_hasher.update(b"enc:");
    enc_hasher.update(&nonce);
    let mut xof = enc_hasher.finalize_xof();

    let mut ciphertext = vec![0u8; plaintext.len()];
    let mut keystream = vec![0u8; plaintext.len()];
    xof.fill(&mut keystream);

    for i in 0..plaintext.len() {
        ciphertext[i] = plaintext[i] ^ keystream[i];
    }

    let mut mac_hasher = blake3::Hasher::new_keyed(key);
    mac_hasher.update(b"mac:");
    mac_hasher.update(&nonce);
    mac_hasher.update(&ciphertext);
    let mac = mac_hasher.finalize();

    let mut result = Vec::with_capacity(16 + 32 + ciphertext.len());
    result.extend_from_slice(&nonce);
    result.extend_from_slice(mac.as_bytes());
    result.extend_from_slice(&ciphertext);
    result
}

/// Decrypts and verifies the payload using Blake3 authenticated stream cipher.
pub fn decrypt_sync_bundle(key: &[u8; 32], payload: &[u8]) -> Result<Vec<u8>, String> {
    if payload.len() < 16 + 32 {
        return Err("Payload too short for decryption".to_string());
    }
    let nonce = &payload[..16];
    let expected_mac = &payload[16..48];
    let ciphertext = &payload[48..];

    let mut mac_hasher = blake3::Hasher::new_keyed(key);
    mac_hasher.update(b"mac:");
    mac_hasher.update(nonce);
    mac_hasher.update(ciphertext);
    let mac = mac_hasher.finalize();
    if mac.as_bytes() != expected_mac {
        return Err("Integrity check failed: invalid MAC or wrong sync key".to_string());
    }

    let mut enc_hasher = blake3::Hasher::new_keyed(key);
    enc_hasher.update(b"enc:");
    enc_hasher.update(nonce);
    let mut xof = enc_hasher.finalize_xof();

    let mut keystream = vec![0u8; ciphertext.len()];
    xof.fill(&mut keystream);

    let mut plaintext = vec![0u8; ciphertext.len()];
    for i in 0..ciphertext.len() {
        plaintext[i] = ciphertext[i] ^ keystream[i];
    }

    Ok(plaintext)
}

/// Generates an SVG string representation of the QR code.
pub fn generate_qr_svg(content: &str) -> Result<String, String> {
    let code = QrCode::new(content.as_bytes())
        .map_err(|e| format!("Failed to generate QR code: {e}"))?;
    let image = code
        .render::<svg::Color>()
        .min_dimensions(260, 260)
        .dark_color(svg::Color("#0f172a"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(image)
}

/// Dynamic sync protocol handler registered on Iroh Router for `SYNC_ALPN`.
#[derive(Clone, Default)]
pub struct DynamicSyncHandler {
    inner: Arc<RwLock<Option<Arc<SyncHostSession>>>>,
}

impl std::fmt::Debug for DynamicSyncHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DynamicSyncHandler")
    }
}

impl iroh::protocol::ProtocolHandler for DynamicSyncHandler {
    fn accept(
        &self,
        connection: Connection,
    ) -> impl std::future::Future<Output = Result<(), iroh::protocol::AcceptError>> + Send {
        let inner = self.inner.clone();
        async move {
            let session_opt = {
                let guard = inner.read().await;
                guard.clone()
            };
            if let Some(session) = session_opt {
                session.handle_incoming_connection(connection).await;
            }
            Ok(())
        }
    }
}

pub struct SyncHostSession {
    pub token: String,
    pub key: [u8; 32],
    pub expires_at: Instant,
    pub db: Database,
    pub app_data_dir: PathBuf,
    pub app_handle: Option<tauri::AppHandle>,
    pub completed: AtomicBool,
}

impl SyncHostSession {
    pub async fn handle_incoming_connection(&self, connection: Connection) {
        if self.completed.load(Ordering::Acquire) {
            return;
        }

        let (mut send, mut recv) = match connection.accept_bi().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to accept sync bi-stream: {e}");
                return;
            }
        };

        // Read 4-byte length prefix
        let mut len_buf = [0u8; 4];
        if let Err(e) = recv.read_exact(&mut len_buf).await {
            eprintln!("Failed to read sync request len: {e}");
            return;
        }
        let req_len = u32::from_be_bytes(len_buf) as usize;
        if req_len > 65536 {
            eprintln!("Sync request len exceeded limit: {req_len}");
            return;
        }

        let mut req_bytes = vec![0u8; req_len];
        if let Err(e) = recv.read_exact(&mut req_bytes).await {
            eprintln!("Failed to read sync request body: {e}");
            return;
        }

        let req: SyncRequest = match serde_json::from_slice(&req_bytes) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Invalid sync request JSON: {e}");
                return;
            }
        };

        // Verify token & expiration
        if Instant::now() > self.expires_at || req.token != self.token {
            let resp = SyncResponse {
                success: false,
                error: Some("Sync token expired or invalid".to_string()),
                payload: None,
            };
            if let Ok(bytes) = serde_json::to_vec(&resp) {
                let len = (bytes.len() as u32).to_be_bytes();
                let _ = send.write_all(&len).await;
                let _ = send.write_all(&bytes).await;
                let _ = send.finish();
            }
            return;
        }

        // Gather database & profile data
        let contacts = self.db.get_contacts().unwrap_or_default();
        let messages = self.db.get_all_messages().unwrap_or_default();
        let local_profile = match self.db.get_local_profile() {
            Ok(Some(p)) => p,
            _ => {
                eprintln!("Local profile not found on host");
                return;
            }
        };

        let avatar_bytes = if let Some(ref hash) = local_profile.avatar_hash {
            let p = crate::network::file_transfer::avatar_file_path(&self.app_data_dir, hash);
            if p.exists() {
                std::fs::read(p).ok()
            } else {
                None
            }
        } else {
            None
        };

        let bundle = SyncBundle {
            secret_key_bytes: local_profile.secret_key_bytes.clone(),
            profile: local_profile.clone(),
            avatar_bytes,
            contacts,
            messages,
            exported_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
        };

        let bundle_bytes = match serde_json::to_vec(&bundle) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Failed to serialize sync bundle: {e}");
                return;
            }
        };

        let encrypted = encrypt_sync_bundle(&self.key, &bundle_bytes);
        let resp = SyncResponse {
            success: true,
            error: None,
            payload: Some(encrypted),
        };

        if let Ok(resp_bytes) = serde_json::to_vec(&resp) {
            let len = (resp_bytes.len() as u32).to_be_bytes();
            let _ = send.write_all(&len).await;
            let _ = send.write_all(&resp_bytes).await;
            let _ = send.finish();
        }

        self.completed.store(true, Ordering::Release);

        if let Some(ref app) = self.app_handle {
            use tauri::Emitter;
            let _ = app.emit(
                crate::ipc::events::EVENT_SYNC_HOST_COMPLETED,
                crate::ipc::events::SyncHostCompletedPayload {
                    nickname: local_profile.nickname,
                    node_id: local_profile.node_id,
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64,
                },
            );
        }
    }
}

/// Central DeviceSyncManager coordinating one-time sync host sessions and client QR import.
#[derive(Clone)]
pub struct DeviceSyncManager {
    node_manager: NodeManager,
    db: Database,
    app_data_dir: PathBuf,
    ft_manager: Arc<FileTransferManager>,
    audio_manager: Arc<AudioManager>,
    app_handle: Option<tauri::AppHandle>,
    sync_handler: DynamicSyncHandler,
}

impl DeviceSyncManager {
    pub fn new(
        node_manager: NodeManager,
        db: Database,
        app_data_dir: PathBuf,
        ft_manager: Arc<FileTransferManager>,
        audio_manager: Arc<AudioManager>,
        app_handle: Option<tauri::AppHandle>,
        sync_handler: DynamicSyncHandler,
    ) -> Self {
        Self {
            node_manager,
            db,
            app_data_dir,
            ft_manager,
            audio_manager,
            app_handle,
            sync_handler,
        }
    }

    /// Generates a one-time symmetric key, registers an active host listener on `SYNC_ALPN` for 120s,
    /// and generates a QR SVG of `p2psync://<node_addr_ticket>/<one_time_token>`.
    pub async fn start_device_sync_host(&self) -> Result<DeviceSyncTicketPayload, String> {
        let node_addr = self
            .node_manager
            .get_node_addr()
            .await
            .ok_or_else(|| "Node endpoint not initialized or running".to_string())?;

        let token_bytes = SecretKey::generate().to_bytes();
        let one_time_token = data_encoding::HEXLOWER.encode(&token_bytes);
        let key = derive_sync_key(&one_time_token);

        let node_addr_bytes = serde_json::to_vec(&node_addr)
            .map_err(|e| format!("Failed to serialize node addr: {e}"))?;
        let node_addr_ticket = data_encoding::BASE32_NOPAD.encode(&node_addr_bytes);

        let sync_ticket = format!("p2psync://{}/{}", node_addr_ticket, one_time_token);
        let qr_svg = generate_qr_svg(&sync_ticket)?;

        let session = Arc::new(SyncHostSession {
            token: one_time_token,
            key,
            expires_at: Instant::now() + Duration::from_secs(SYNC_TIMEOUT_SECS),
            db: self.db.clone(),
            app_data_dir: self.app_data_dir.clone(),
            app_handle: self.app_handle.clone(),
            completed: AtomicBool::new(false),
        });

        *self.sync_handler.inner.write().await = Some(session.clone());

        let handler_clone = self.sync_handler.inner.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(SYNC_TIMEOUT_SECS)).await;
            let mut guard = handler_clone.write().await;
            if let Some(ref current) = *guard {
                if Arc::ptr_eq(current, &session) {
                    *guard = None;
                }
            }
        });

        Ok(DeviceSyncTicketPayload {
            ticket: sync_ticket,
            qr_svg,
            expires_in_secs: SYNC_TIMEOUT_SECS,
        })
    }

    /// Connects to the PC over `b"/p2pchat/sync/1"`, authenticates with `one_time_token`,
    /// downloads and decrypts the account identity and SQLite history, updates local database,
    /// and reloads the local Iroh endpoint.
    pub async fn import_from_sync_qr(
        &self,
        qr_string: &str,
    ) -> Result<DeviceSyncImportResult, String> {
        let clean = qr_string.trim();
        let payload = clean.strip_prefix("p2psync://").unwrap_or(clean);

        let parts: Vec<&str> = payload.split('/').collect();
        if parts.len() < 2 {
            return Err("Invalid sync ticket format. Expected p2psync://<node_addr_ticket>/<token>".to_string());
        }

        let node_addr_ticket = parts[0].trim();
        let token = parts[1].trim();

        if node_addr_ticket.is_empty() || token.is_empty() {
            return Err("Sync ticket contains empty node address or token".to_string());
        }

        let node_addr_bytes = data_encoding::BASE32_NOPAD
            .decode(node_addr_ticket.as_bytes())
            .map_err(|e| format!("Failed to decode node address ticket: {e}"))?;

        let pc_node_addr: NodeAddr = serde_json::from_slice(&node_addr_bytes)
            .map_err(|e| format!("Failed to parse PC node address: {e}"))?;

        let key = derive_sync_key(token);

        let endpoint = self
            .node_manager
            .get_endpoint()
            .await
            .ok_or_else(|| "Endpoint not initialized".to_string())?;

        self.node_manager.add_node_addr(pc_node_addr.clone()).await;

        let conn = endpoint
            .connect(pc_node_addr, SYNC_ALPN)
            .await
            .map_err(|e| format!("Failed to connect to PC sync host: {e}"))?;

        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| format!("Failed to open sync bi-stream: {e}"))?;

        let req = SyncRequest {
            token: token.to_string(),
        };
        let req_bytes = serde_json::to_vec(&req)
            .map_err(|e| format!("Failed to serialize sync request: {e}"))?;
        let req_len = (req_bytes.len() as u32).to_be_bytes();
        send.write_all(&req_len)
            .await
            .map_err(|e| format!("Failed to write sync request len: {e}"))?;
        send.write_all(&req_bytes)
            .await
            .map_err(|e| format!("Failed to write sync request body: {e}"))?;
        send.finish()
            .map_err(|e| format!("Failed to finish sync request stream: {e}"))?;

        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf)
            .await
            .map_err(|e| format!("Failed to read sync response len: {e}"))?;
        let resp_len = u32::from_be_bytes(len_buf) as usize;
        if resp_len > 100 * 1024 * 1024 {
            return Err("Sync response exceeded size limit (100MB)".to_string());
        }

        let mut resp_bytes = vec![0u8; resp_len];
        recv.read_exact(&mut resp_bytes)
            .await
            .map_err(|e| format!("Failed to read sync response body: {e}"))?;

        let resp: SyncResponse = serde_json::from_slice(&resp_bytes)
            .map_err(|e| format!("Invalid sync response: {e}"))?;

        if !resp.success {
            return Err(resp
                .error
                .unwrap_or_else(|| "Sync failed on host".to_string()));
        }

        let enc_payload = resp
            .payload
            .ok_or_else(|| "Missing sync payload in response".to_string())?;

        let decrypted = decrypt_sync_bundle(&key, &enc_payload)?;
        let bundle: SyncBundle = serde_json::from_slice(&decrypted)
            .map_err(|e| format!("Failed to parse sync bundle: {e}"))?;

        // 1. Save local profile with synced identity
        self.db
            .save_local_profile(&bundle.profile)
            .map_err(|e| format!("Failed to save synced local profile: {e}"))?;

        // 2. Save avatar file if present
        if let (Some(hash), Some(bytes)) =
            (&bundle.profile.avatar_hash, &bundle.avatar_bytes)
        {
            let p = crate::network::file_transfer::avatar_file_path(&self.app_data_dir, hash);
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(p, bytes);
        }

        // 3. Import contacts
        let mut contacts_imported = 0;
        for c in bundle.contacts {
            if self.db.insert_or_update_contact(&c).is_ok() {
                contacts_imported += 1;
            }
        }

        // 4. Import messages
        let mut messages_imported = 0;
        for m in bundle.messages {
            if self.db.insert_message(&m).is_ok() {
                messages_imported += 1;
            }
        }

        let new_node_id = bundle.profile.node_id.clone();
        let new_nickname = bundle.profile.nickname.clone();

        // 5. Reload NodeManager identity
        let profile = self.node_manager.reload_identity().await?;
        if let (Some(ep), Some(store)) = (
            self.node_manager.get_endpoint().await,
            self.node_manager.get_blobs_store().await,
        ) {
            self.ft_manager
                .set_endpoint_and_store(ep, store, profile.node_id.clone())
                .await;
        }
        self.audio_manager.set_local_node_id(&profile.node_id);

        // 6. Notify frontend
        if let Some(ref app) = self.app_handle {
            use tauri::Emitter;
            let _ = app.emit(
                crate::ipc::events::EVENT_SYNC_IMPORT_COMPLETED,
                crate::ipc::events::SyncImportCompletedPayload {
                    node_id: new_node_id.clone(),
                    nickname: new_nickname.clone(),
                    contacts_imported,
                    messages_imported,
                },
            );
        }

        Ok(DeviceSyncImportResult {
            node_id: new_node_id,
            nickname: new_nickname,
            contacts_imported,
            messages_imported,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_crypto_roundtrip() {
        let token = "test_one_time_token_1234567890abcdef";
        let key = derive_sync_key(token);

        let data = b"Hello VoxP2P sync payload test string with contacts and messages!";
        let encrypted = encrypt_sync_bundle(&key, data);
        assert_ne!(&encrypted[48..], data);

        let decrypted = decrypt_sync_bundle(&key, &encrypted).unwrap();
        assert_eq!(decrypted, data);

        // Wrong key must fail decryption
        let wrong_key = derive_sync_key("wrong_token");
        assert!(decrypt_sync_bundle(&wrong_key, &encrypted).is_err());
    }

    #[test]
    fn test_qr_svg_generation() {
        let ticket = "p2psync://MFRGGZDFMY======/abcdef123456";
        let svg = generate_qr_svg(ticket).unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
    }
}
