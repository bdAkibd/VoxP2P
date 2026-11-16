use std::collections::{BTreeMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

use iroh::{PublicKey, SecretKey, Signature};

use crate::ipc::events::{HostChangedPayload, EVENT_HOST_CHANGED};
use crate::network::node::NodeManager;
use crate::storage::db::Database;
use crate::storage::models::Contact;

// ============================================================================
// 1. Gossip Control Messages & Handshake
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileHandshake {
    pub nickname: String,
    pub avatar_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerJoinedMessage {
    pub node_id: String,
    pub nickname: String,
    pub avatar_hash: Option<String>,
    pub join_timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeartbeatMessage {
    pub host_id: String,
    pub timestamp: u64,
    pub peer_list: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransferHostMessage {
    pub new_host_id: String,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerStateUpdateMessage {
    pub node_id: String,
    pub is_muted: bool,
    pub is_deafened: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayRequestMessage {
    pub from_node: String,
    pub target_node: String,
}

/// Serializable gossip control messages for room management.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum RoomGossipMessage {
    PeerJoined {
        node_id: String,
        nickname: String,
        avatar_hash: Option<String>,
        join_timestamp: u64,
    },
    Heartbeat {
        host_id: String,
        timestamp: u64,
        peer_list: Vec<String>,
    },
    TransferHost {
        new_host_id: String,
        signature: Vec<u8>,
    },
    PeerStateUpdate {
        node_id: String,
        is_muted: bool,
        is_deafened: bool,
    },
    RelayRequest {
        from_node: String,
        target_node: String,
    },
    ProfileHandshake {
        nickname: String,
        avatar_hash: Option<String>,
    },
}

impl RoomGossipMessage {
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

// ============================================================================
// 2. Audio Datagram Packets & Symmetric NAT Relay Envelopes
// ============================================================================

/// Audio frame traveling via QUIC Unreliable Datagrams.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AudioPacket {
    pub sender_id: String,
    pub seq: u64,
    pub opus_data: Vec<u8>,
}

impl AudioPacket {
    pub fn new(sender_id: String, seq: u64, opus_data: Vec<u8>) -> Self {
        Self {
            sender_id,
            seq,
            opus_data,
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

/// Relay envelope for Symmetric NAT fallback.
/// If Peer A cannot reach Peer B directly within 3 seconds, it sends this envelope to Room Host.
/// Host immediately forwards `payload` via datagram to `target_id` without decoding it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayEnvelope {
    pub target_id: String,
    pub payload: Vec<u8>,
}

impl RelayEnvelope {
    pub fn new(target_id: String, payload: Vec<u8>) -> Self {
        Self { target_id, payload }
    }

    pub fn wrap_audio(target_id: String, packet: &AudioPacket) -> Result<Self, serde_json::Error> {
        let payload = packet.to_bytes()?;
        Ok(Self { target_id, payload })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

// ============================================================================
// 3. Room Peer & State Machine (Deterministic Ordering)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerInfo {
    pub node_id: String,
    pub nickname: String,
    pub avatar_hash: Option<String>,
    pub join_timestamp: u64,
    pub is_host: bool,
    #[serde(default)]
    pub is_muted: bool,
    #[serde(default)]
    pub is_deafened: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomState {
    pub room_id: String,
    pub host_node_id: String,
    /// Active peers deterministically ordered by (join_timestamp, node_id).
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
        self.sort_peers();
    }

    pub fn remove_peer(&mut self, node_id: &str) {
        self.peers.retain(|p| p.node_id != node_id);
    }

    /// Sorts peers deterministically by (join_timestamp, node_id).
    pub fn sort_peers(&mut self) {
        self.peers.sort_by(|a, b| {
            a.join_timestamp
                .cmp(&b.join_timestamp)
                .then_with(|| a.node_id.cmp(&b.node_id))
        });
    }

    /// Converts active peers into a deterministic BTreeMap ordered by (join_timestamp, node_id).
    pub fn as_deterministic_map(&self) -> BTreeMap<(u64, String), PeerInfo> {
        let mut map = BTreeMap::new();
        for peer in &self.peers {
            map.insert((peer.join_timestamp, peer.node_id.clone()), peer.clone());
        }
        map
    }

    /// Deterministic Host Migration:
    /// Peers are sorted by (join_timestamp, node_id).
    /// If the current host leaves or times out, the first eligible peer in order is promoted.
    pub fn compute_next_host(&self) -> Option<String> {
        self.peers.first().map(|p| p.node_id.clone())
    }

    pub fn set_host(&mut self, new_host_id: String) {
        self.host_node_id = new_host_id.clone();
        for peer in self.peers.iter_mut() {
            peer.is_host = peer.node_id == new_host_id;
        }
    }

    pub fn is_host(&self, node_id: &str) -> bool {
        self.host_node_id == node_id
    }

    pub fn get_peer(&self, node_id: &str) -> Option<&PeerInfo> {
        self.peers.iter().find(|p| p.node_id == node_id)
    }

    pub fn get_peer_mut(&mut self, node_id: &str) -> Option<&mut PeerInfo> {
        self.peers.iter_mut().find(|p| p.node_id == node_id)
    }
}

// ============================================================================
// 4. Room Session Runtime
// ============================================================================

pub type HostChangedCallback = Arc<dyn Fn(&HostChangedPayload) + Send + Sync>;

pub struct RoomSession {
    pub room_id: String,
    pub local_node_id: String,
    pub secret_key: SecretKey,
    pub state: Arc<RwLock<RoomState>>,
    pub last_heartbeat_received: Arc<RwLock<Instant>>,
    pub last_heartbeat_sent: Arc<RwLock<Instant>>,
    pub broadcast_tx: broadcast::Sender<RoomGossipMessage>,
    pub forwarded_datagram_tx: broadcast::Sender<(String, Vec<u8>)>,
    pub relay_targets: Arc<RwLock<HashSet<String>>>,
    pub db: Database,
    pub host_changed_cb: Option<HostChangedCallback>,
    cancel_tx: broadcast::Sender<()>,
    heartbeat_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl RoomSession {
    pub fn new(
        room_id: String,
        local_node_id: String,
        secret_key: SecretKey,
        initial_host_id: String,
        db: Database,
        host_changed_cb: Option<HostChangedCallback>,
    ) -> Arc<Self> {
        let (broadcast_tx, _) = broadcast::channel(256);
        let (forwarded_datagram_tx, _) = broadcast::channel(1024);
        let (cancel_tx, _) = broadcast::channel(1);

        let initial_state = RoomState::new(room_id.clone(), initial_host_id);
        let state = Arc::new(RwLock::new(initial_state));
        let last_heartbeat_received = Arc::new(RwLock::new(Instant::now()));
        let last_heartbeat_sent = Arc::new(RwLock::new(Instant::now()));
        let relay_targets = Arc::new(RwLock::new(HashSet::new()));

        let session = Arc::new(Self {
            room_id: room_id.clone(),
            local_node_id: local_node_id.clone(),
            secret_key,
            state: state.clone(),
            last_heartbeat_received: last_heartbeat_received.clone(),
            last_heartbeat_sent: last_heartbeat_sent.clone(),
            broadcast_tx: broadcast_tx.clone(),
            forwarded_datagram_tx,
            relay_targets,
            db,
            host_changed_cb: host_changed_cb.clone(),
            cancel_tx: cancel_tx.clone(),
            heartbeat_task: Mutex::new(None),
        });

        // Spawn the background heartbeat and host-migration task if within a tokio runtime
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let task = handle.spawn(Self::run_background_loop(
                room_id,
                local_node_id,
                state,
                last_heartbeat_received,
                last_heartbeat_sent,
                broadcast_tx,
                host_changed_cb,
                cancel_tx.subscribe(),
            ));

            let s_clone = session.clone();
            handle.spawn(async move {
                *s_clone.heartbeat_task.lock().await = Some(task);
            });
        }

        session
    }

    /// Background loop for heartbeat broadcast and timeout migration.
    /// - If self.node_id == current_host_id: broadcast Heartbeat every 700ms.
    /// - If self.node_id != current_host_id and no Heartbeat received within 2000ms:
    ///   remove dead host, deterministically select first peer in sorted list as new host,
    ///   and emit Tauri event `room://host-changed`.
    async fn run_background_loop(
        room_id: String,
        local_node_id: String,
        state: Arc<RwLock<RoomState>>,
        last_heartbeat_received: Arc<RwLock<Instant>>,
        last_heartbeat_sent: Arc<RwLock<Instant>>,
        broadcast_tx: broadcast::Sender<RoomGossipMessage>,
        host_changed_cb: Option<HostChangedCallback>,
        mut cancel_rx: broadcast::Receiver<()>,
    ) {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            tokio::select! {
                _ = cancel_rx.recv() => {
                    break;
                }
                _ = interval.tick() => {
                    let now = Instant::now();
                    let (is_host, _current_host_id, peer_ids) = {
                        let st = state.read().await;
                        let is_h = st.host_node_id == local_node_id;
                        let h_id = st.host_node_id.clone();
                        let ids = st.peers.iter().map(|p| p.node_id.clone()).collect::<Vec<_>>();
                        (is_h, h_id, ids)
                    };

                    if is_host {
                        // Current Host broadcasts Heartbeat every 700ms
                        let mut last_sent = last_heartbeat_sent.write().await;
                        if now.duration_since(*last_sent) >= Duration::from_millis(700) {
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;

                            let heartbeat = RoomGossipMessage::Heartbeat {
                                host_id: local_node_id.clone(),
                                timestamp,
                                peer_list: peer_ids,
                            };
                            let _ = broadcast_tx.send(heartbeat);
                            *last_sent = now;
                            *last_heartbeat_received.write().await = now;
                        }
                    } else {
                        // Non-host checks if current host has timed out (> 2000ms)
                        let last_rx = *last_heartbeat_received.read().await;
                        if now.duration_since(last_rx) >= Duration::from_millis(2000) {
                            let mut st = state.write().await;
                            let dead_host = st.host_node_id.clone();

                            // Remove dead host from active peers
                            st.remove_peer(&dead_host);

                            // Deterministically select first peer in sorted active list
                            if let Some(new_host_id) = st.compute_next_host() {
                                st.set_host(new_host_id.clone());
                                *last_heartbeat_received.write().await = now;

                                let payload = HostChangedPayload {
                                    room_id: room_id.clone(),
                                    old_host_id: dead_host,
                                    new_host_id: new_host_id.clone(),
                                };
                                Self::emit_host_changed(&host_changed_cb, &payload);

                                // If we were promoted to host, send immediate heartbeat
                                if new_host_id == local_node_id {
                                    let timestamp = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis() as u64;
                                    let updated_peers =
                                        st.peers.iter().map(|p| p.node_id.clone()).collect();
                                    let heartbeat = RoomGossipMessage::Heartbeat {
                                        host_id: local_node_id.clone(),
                                        timestamp,
                                        peer_list: updated_peers,
                                    };
                                    let _ = broadcast_tx.send(heartbeat);
                                    *last_heartbeat_sent.write().await = now;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn emit_host_changed(cb: &Option<HostChangedCallback>, payload: &HostChangedPayload) {
        if let Some(f) = cb {
            f(payload);
        }
    }

    /// Implements manual host transfer:
    /// 1. Verifies caller is current host.
    /// 2. Signs target_node_id with host's Ed25519 SecretKey.
    /// 3. Broadcasts signed `TransferHost` message.
    /// 4. Updates local state immediately and emits `room://host-changed`.
    pub async fn transfer_host(&self, target_node_id: &str) -> Result<(), String> {
        let mut state = self.state.write().await;

        // Verify caller is current host
        if state.host_node_id != self.local_node_id {
            return Err(format!(
                "Caller '{}' is not the current room host ('{}')",
                self.local_node_id, state.host_node_id
            ));
        }

        // Verify target exists in peers
        if !state.peers.iter().any(|p| p.node_id == target_node_id) {
            return Err(format!(
                "Target peer '{}' not found in active room peers",
                target_node_id
            ));
        }

        // Sign target_node_id with host's Ed25519 SecretKey
        let sig = self.secret_key.sign(target_node_id.as_bytes());
        let sig_bytes = sig.to_bytes().to_vec();

        // Broadcast TransferHost gossip control message
        let msg = RoomGossipMessage::TransferHost {
            new_host_id: target_node_id.to_string(),
            signature: sig_bytes,
        };
        let _ = self.broadcast_tx.send(msg);

        // Update state immediately
        let old_host_id = state.host_node_id.clone();
        state.set_host(target_node_id.to_string());
        *self.last_heartbeat_received.write().await = Instant::now();

        // Emit room://host-changed Tauri event
        let payload = HostChangedPayload {
            room_id: state.room_id.clone(),
            old_host_id,
            new_host_id: target_node_id.to_string(),
        };
        Self::emit_host_changed(&self.host_changed_cb, &payload);

        Ok(())
    }

    /// Handles an incoming TransferHost message from gossip:
    /// Verifies signature using current host's Ed25519 public key.
    pub async fn handle_transfer_host(&self, new_host_id: &str, signature: &[u8]) -> Result<(), String> {
        let mut state = self.state.write().await;
        let current_host_id = state.host_node_id.clone();

        let host_pk = PublicKey::from_str(&current_host_id)
            .map_err(|e| format!("Invalid current host public key: {e}"))?;
        let sig = Signature::try_from(signature)
            .map_err(|_| "Invalid Ed25519 signature format".to_string())?;

        host_pk
            .verify(new_host_id.as_bytes(), &sig)
            .map_err(|e| format!("TransferHost signature verification failed: {e}"))?;

        // Valid signature from current host: apply immediately
        state.set_host(new_host_id.to_string());
        *self.last_heartbeat_received.write().await = Instant::now();

        let payload = HostChangedPayload {
            room_id: state.room_id.clone(),
            old_host_id: current_host_id,
            new_host_id: new_host_id.to_string(),
        };
        Self::emit_host_changed(&self.host_changed_cb, &payload);

        Ok(())
    }

    /// Handles incoming gossip control messages.
    pub async fn handle_gossip_message(&self, msg: RoomGossipMessage) -> Result<(), String> {
        match msg {
            RoomGossipMessage::PeerJoined {
                node_id,
                nickname,
                avatar_hash,
                join_timestamp,
            } => {
                let mut state = self.state.write().await;
                let is_host = node_id == state.host_node_id;
                state.add_or_update_peer(PeerInfo {
                    node_id: node_id.clone(),
                    nickname: nickname.clone(),
                    avatar_hash: avatar_hash.clone(),
                    join_timestamp,
                    is_host,
                    is_muted: false,
                    is_deafened: false,
                });

                // Auto-update local contacts SQLite table
                self.db
                    .handle_profile_handshake(&node_id, &nickname, avatar_hash.as_deref())
                    .map_err(|e| e.to_string())?;
            }
            RoomGossipMessage::Heartbeat {
                host_id,
                timestamp: _,
                peer_list: _,
            } => {
                let state = self.state.read().await;
                if host_id == state.host_node_id {
                    *self.last_heartbeat_received.write().await = Instant::now();
                }
            }
            RoomGossipMessage::TransferHost {
                new_host_id,
                signature,
            } => {
                self.handle_transfer_host(&new_host_id, &signature).await?;
            }
            RoomGossipMessage::PeerStateUpdate {
                node_id,
                is_muted,
                is_deafened,
            } => {
                let mut state = self.state.write().await;
                if let Some(peer) = state.get_peer_mut(&node_id) {
                    peer.is_muted = is_muted;
                    peer.is_deafened = is_deafened;
                }
            }
            RoomGossipMessage::RelayRequest {
                from_node: _,
                target_node: _,
            } => {
                // Relay route setup/handshake
            }
            RoomGossipMessage::ProfileHandshake {
                nickname: _,
                avatar_hash: _,
            } => {}
        }
        Ok(())
    }

    /// Handles an incoming QUIC datagram:
    /// - If it is a `RelayEnvelope`:
    ///   - If current node is Host: immediately forwards `payload` via datagram to `target_id` WITHOUT decoding it.
    ///   - If current node is the target: decodes `payload` as `AudioPacket` and returns it.
    /// - If it is an `AudioPacket`: returns the decoded packet.
    pub async fn handle_incoming_datagram(
        &self,
        _from_node: &str,
        raw_data: &[u8],
    ) -> Result<Option<AudioPacket>, String> {
        // Check if raw datagram is a RelayEnvelope
        if let Ok(envelope) = serde_json::from_slice::<RelayEnvelope>(raw_data) {
            let is_host = {
                let state = self.state.read().await;
                state.host_node_id == self.local_node_id
            };

            if is_host {
                // "If the current node is Host and receives a RelayEnvelope,
                // it immediately forwards payload via datagram to target_id without decoding it."
                let _ = self
                    .forwarded_datagram_tx
                    .send((envelope.target_id.clone(), envelope.payload));
                return Ok(None);
            } else if envelope.target_id == self.local_node_id {
                // We are the intended recipient of this relayed audio packet
                let audio_packet = AudioPacket::from_bytes(&envelope.payload)
                    .map_err(|e| format!("Failed to decode relayed AudioPacket: {e}"))?;
                return Ok(Some(audio_packet));
            } else {
                return Ok(None);
            }
        }

        // Direct AudioPacket
        let packet = AudioPacket::from_bytes(raw_data)
            .map_err(|e| format!("Failed to decode AudioPacket: {e}"))?;
        Ok(Some(packet))
    }

    /// Prepares an audio frame for transmission:
    /// - If direct connection to `target_id` is available, returns the raw `AudioPacket` bytes.
    /// - If direct connection failed or timed out (> 3 seconds), wraps in `RelayEnvelope` destined for Host.
    pub async fn prepare_audio_datagram(
        &self,
        target_id: &str,
        seq: u64,
        opus_data: Vec<u8>,
    ) -> Result<(String, Vec<u8>), String> {
        let packet = AudioPacket::new(self.local_node_id.clone(), seq, opus_data);
        let packet_bytes = packet.to_bytes().map_err(|e| e.to_string())?;

        let requires_relay = {
            let relays = self.relay_targets.read().await;
            relays.contains(target_id)
        };

        if requires_relay {
            let host_id = {
                let state = self.state.read().await;
                state.host_node_id.clone()
            };
            let envelope = RelayEnvelope::new(target_id.to_string(), packet_bytes);
            let envelope_bytes = envelope.to_bytes().map_err(|e| e.to_string())?;
            // Send envelope to current Room Host
            Ok((host_id, envelope_bytes))
        } else {
            // Direct datagram to target_id
            Ok((target_id.to_string(), packet_bytes))
        }
    }

    /// Broadcasts an Opus audio frame to all remote peers in the room.
    /// Returns destination node IDs and prepared datagram bytes (either direct or relayed via Host).
    pub async fn broadcast_audio_frame(&self, seq: u64, opus_data: &[u8]) -> Vec<(String, Vec<u8>)> {
        let peers = {
            let st = self.state.read().await;
            st.peers.iter().map(|p| p.node_id.clone()).collect::<Vec<_>>()
        };

        let mut datagrams = Vec::new();
        for peer_id in peers {
            if peer_id == self.local_node_id {
                continue;
            }
            if let Ok(datagram) = self.prepare_audio_datagram(&peer_id, seq, opus_data.to_vec()).await {
                datagrams.push(datagram);
            }
        }
        datagrams
    }



    /// Evaluates direct connection status with 3-second timeout.
    /// If direct QUIC connection cannot be established within 3s, marks peer as requiring relay fallback.
    pub async fn evaluate_direct_connection<F, Fut>(&self, target_id: &str, connect_fn: F)
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), String>>,
    {
        match tokio::time::timeout(Duration::from_secs(3), connect_fn()).await {
            Ok(Ok(())) => {
                let mut relays = self.relay_targets.write().await;
                relays.remove(target_id);
            }
            _ => {
                // Timed out (> 3s) or failed: activate Symmetric NAT relay fallback
                let mut relays = self.relay_targets.write().await;
                relays.insert(target_id.to_string());
            }
        }
    }

    /// Marks a peer as requiring relay fallback or not.
    pub async fn set_peer_relay_fallback(&self, target_id: &str, needs_relay: bool) {
        let mut relays = self.relay_targets.write().await;
        if needs_relay {
            relays.insert(target_id.to_string());
        } else {
            relays.remove(target_id);
        }
    }

    /// Exchanges profile handshake: auto-updates local `contacts` SQLite table.
    pub fn handle_profile_handshake(
        &self,
        peer_node_id: &str,
        handshake: &ProfileHandshake,
    ) -> Result<Contact, String> {
        self.db
            .handle_profile_handshake(peer_node_id, &handshake.nickname, handshake.avatar_hash.as_deref())
            .map_err(|e| e.to_string())
    }

    /// Cleanly closes the room session and terminates background tasks.
    pub async fn close(&self) {
        let _ = self.cancel_tx.send(());
        let mut task_lock = self.heartbeat_task.lock().await;
        if let Some(task) = task_lock.take() {
            task.abort();
        }
    }
}

impl Drop for RoomSession {
    fn drop(&mut self) {
        let _ = self.cancel_tx.send(());
    }
}

// ============================================================================
// 5. Room Manager
// ============================================================================

#[derive(Clone)]
pub struct RoomManager {
    inner: Arc<RwLock<RoomManagerInner>>,
    db: Database,
    app_handle: Option<tauri::AppHandle>,
}

struct RoomManagerInner {
    pub current_room: Option<Arc<RoomSession>>,
    pub local_node_id: String,
    pub secret_key: SecretKey,
}

impl RoomManager {
    pub fn new(
        node_manager: NodeManager,
        db: Database,
        app_handle: Option<tauri::AppHandle>,
    ) -> Self {
        let secret_key = SecretKey::generate();
        let local_node_id = secret_key.public().to_string();

        let inner = Arc::new(RwLock::new(RoomManagerInner {
            current_room: None,
            local_node_id,
            secret_key,
        }));

        let mgr = Self {
            inner: inner.clone(),
            db,
            app_handle,
        };

        // Asynchronously load real identity from node_manager
        let mgr_clone = mgr.clone();
        tokio::spawn(async move {
            let key = node_manager.get_secret_key().await;
            let nid = node_manager.get_node_id().await;
            let mut w = mgr_clone.inner.write().await;
            w.secret_key = key;
            w.local_node_id = nid;
        });

        mgr
    }

    pub fn new_standalone(
        local_node_id: String,
        secret_key: SecretKey,
        db: Database,
        app_handle: Option<tauri::AppHandle>,
    ) -> Self {
        let inner = Arc::new(RwLock::new(RoomManagerInner {
            current_room: None,
            local_node_id,
            secret_key,
        }));

        Self {
            inner,
            db,
            app_handle,
        }
    }

    pub async fn create_or_join_room(
        &self,
        room_id: &str,
        host_id: Option<&str>,
    ) -> Arc<RoomSession> {
        let mut inner = self.inner.write().await;
        if let Some(existing) = &inner.current_room {
            if existing.room_id == room_id {
                return existing.clone();
            }
            existing.close().await;
        }

        let host = host_id
            .map(|s| s.to_string())
            .unwrap_or_else(|| inner.local_node_id.clone());

        let host_changed_cb = self.app_handle.as_ref().map(|app| {
            let app_handle = app.clone();
            let cb: HostChangedCallback = Arc::new(move |payload: &HostChangedPayload| {
                use tauri::Emitter;
                let _ = app_handle.emit(EVENT_HOST_CHANGED, payload);
            });
            cb
        });

        let session = RoomSession::new(
            room_id.to_string(),
            inner.local_node_id.clone(),
            inner.secret_key.clone(),
            host,
            self.db.clone(),
            host_changed_cb,
        );

        inner.current_room = Some(session.clone());
        session
    }

    pub async fn get_current_room(&self) -> Option<Arc<RoomSession>> {
        let inner = self.inner.read().await;
        inner.current_room.clone()
    }

    pub async fn get_room_state(&self) -> Result<Option<RoomState>, String> {
        let inner = self.inner.read().await;
        if let Some(room) = &inner.current_room {
            let st = room.state.read().await;
            Ok(Some(st.clone()))
        } else {
            Ok(None)
        }
    }

    /// Tauri command implementation:
    /// Verifies caller is current host, signs target_node_id, broadcasts `TransferHost`,
    /// updates state immediately, and emits `room://host-changed`.
    pub async fn transfer_host(&self, target_node_id: &str) -> Result<(), String> {
        let room = {
            let inner = self.inner.read().await;
            inner
                .current_room
                .clone()
                .ok_or_else(|| "No active room session".to_string())?
        };

        room.transfer_host(target_node_id).await
    }

    pub async fn leave_room(&self) {
        let mut inner = self.inner.write().await;
        if let Some(room) = inner.current_room.take() {
            room.close().await;
        }
    }

    /// Broadcasts an Opus audio frame across all peers in the active room.
    pub async fn broadcast_audio_frame(&self, seq: u64, opus_data: &[u8]) -> Vec<(String, Vec<u8>)> {
        let room = {
            let inner = self.inner.read().await;
            inner.current_room.clone()
        };
        if let Some(r) = room {
            r.broadcast_audio_frame(seq, opus_data).await
        } else {
            Vec::new()
        }
    }
}


// ============================================================================
// 6. Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secret_key_sign_and_verify() {
        let sk = SecretKey::generate();
        let pk = sk.public();
        let msg = b"peer_target_node_1234";
        let sig = sk.sign(msg);
        let sig_bytes = sig.to_bytes().to_vec();

        let sig_parsed = Signature::try_from(sig_bytes.as_slice()).unwrap();
        pk.verify(msg, &sig_parsed).unwrap();
    }

    #[test]
    fn test_deterministic_peer_ordering() {
        let mut state = RoomState::new("room_alpha".to_string(), "host_init".to_string());

        let p1 = PeerInfo {
            node_id: "node_z".to_string(),
            nickname: "Zeta".to_string(),
            avatar_hash: None,
            join_timestamp: 200,
            is_host: false,
            is_muted: false,
            is_deafened: false,
        };
        let p2 = PeerInfo {
            node_id: "node_b".to_string(),
            nickname: "Beta".to_string(),
            avatar_hash: None,
            join_timestamp: 100,
            is_host: false,
            is_muted: false,
            is_deafened: false,
        };
        let p3 = PeerInfo {
            node_id: "node_a".to_string(),
            nickname: "Alpha".to_string(),
            avatar_hash: None,
            join_timestamp: 100,
            is_host: false,
            is_muted: false,
            is_deafened: false,
        };

        state.add_or_update_peer(p1);
        state.add_or_update_peer(p2);
        state.add_or_update_peer(p3);

        // Sorting is by (join_timestamp, node_id)
        // 100, "node_a" -> p3
        // 100, "node_b" -> p2
        // 200, "node_z" -> p1
        assert_eq!(state.peers[0].node_id, "node_a");
        assert_eq!(state.peers[1].node_id, "node_b");
        assert_eq!(state.peers[2].node_id, "node_z");

        // First peer is deterministically promoted
        assert_eq!(state.compute_next_host(), Some("node_a".to_string()));

        // Check BTreeMap deterministic representation
        let map = state.as_deterministic_map();
        let map_keys: Vec<_> = map.keys().cloned().collect();
        assert_eq!(
            map_keys,
            vec![
                (100, "node_a".to_string()),
                (100, "node_b".to_string()),
                (200, "node_z".to_string()),
            ]
        );
    }

    #[test]
    fn test_transfer_host_success_and_signature_verification() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let db = Database::new_in_memory().unwrap();
            let host_sk = SecretKey::generate();
            let host_id = host_sk.public().to_string();
            let target_sk = SecretKey::generate();
            let target_id = target_sk.public().to_string();

            let session = RoomSession::new(
                "room_test".to_string(),
                host_id.clone(),
                host_sk,
                host_id.clone(),
                db,
                None,
            );

            // Add target to peers
            session.state.write().await.add_or_update_peer(PeerInfo {
                node_id: target_id.clone(),
                nickname: "TargetPeer".to_string(),
                avatar_hash: None,
                join_timestamp: 50,
                is_host: false,
                is_muted: false,
                is_deafened: false,
            });

            // Successful transfer by host
            session.transfer_host(&target_id).await.unwrap();
            assert_eq!(session.state.read().await.host_node_id, target_id);

            session.close().await;
        });
    }

    #[test]
    fn test_transfer_host_fails_if_not_host() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let db = Database::new_in_memory().unwrap();
            let local_sk = SecretKey::generate();
            let local_id = local_sk.public().to_string();
            let other_host_id = SecretKey::generate().public().to_string();
            let target_id = SecretKey::generate().public().to_string();

            let session = RoomSession::new(
                "room_test".to_string(),
                local_id.clone(),
                local_sk,
                other_host_id, // someone else is host
                db,
                None,
            );

            session.state.write().await.add_or_update_peer(PeerInfo {
                node_id: target_id.clone(),
                nickname: "TargetPeer".to_string(),
                avatar_hash: None,
                join_timestamp: 50,
                is_host: false,
                is_muted: false,
                is_deafened: false,
            });

            let err = session.transfer_host(&target_id).await.unwrap_err();
            assert!(err.contains("not the current room host"));

            session.close().await;
        });
    }

    #[test]
    fn test_relay_fallback_and_host_packet_forwarding() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let db = Database::new_in_memory().unwrap();
            let host_sk = SecretKey::generate();
            let host_id = host_sk.public().to_string();
            let peer_a_id = "peer_a_id".to_string();
            let peer_b_id = "peer_b_id".to_string();

            // 1. Host session
            let host_session = RoomSession::new(
                "room_relay".to_string(),
                host_id.clone(),
                host_sk,
                host_id.clone(),
                db.clone(),
                None,
            );

            // 2. Peer A session
            let peer_a_sk = SecretKey::generate();
            let peer_a_session = RoomSession::new(
                "room_relay".to_string(),
                peer_a_id.clone(),
                peer_a_sk,
                host_id.clone(),
                db.clone(),
                None,
            );

            // Peer A cannot reach Peer B directly (simulate fallback)
            peer_a_session.set_peer_relay_fallback(&peer_b_id, true).await;

            let opus_data = vec![0x12, 0x34, 0x56, 0x78];
            let (dest, datagram_bytes) = peer_a_session
                .prepare_audio_datagram(&peer_b_id, 1, opus_data.clone())
                .await
                .unwrap();

            // Destination is host because relay is needed
            assert_eq!(dest, host_id);

            // Host receives the datagram
            let mut forwarded_rx = host_session.forwarded_datagram_tx.subscribe();
            let host_result = host_session
                .handle_incoming_datagram(&peer_a_id, &datagram_bytes)
                .await
                .unwrap();

            // Host does not decode payload!
            assert!(host_result.is_none());

            // Host immediately forwards payload to Peer B
            let (forwarded_to, forwarded_payload) = forwarded_rx.recv().await.unwrap();
            assert_eq!(forwarded_to, peer_b_id);

            // Peer B decodes the forwarded payload as AudioPacket
            let decoded = AudioPacket::from_bytes(&forwarded_payload).unwrap();
            assert_eq!(decoded.sender_id, peer_a_id);
            assert_eq!(decoded.seq, 1);
            assert_eq!(decoded.opus_data, opus_data);

            host_session.close().await;
            peer_a_session.close().await;
        });
    }

    #[test]
    fn test_direct_connection_evaluation_timeout_activates_relay() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let db = Database::new_in_memory().unwrap();
            let sk = SecretKey::generate();
            let node_id = sk.public().to_string();

            let session = RoomSession::new(
                "room_eval".to_string(),
                node_id.clone(),
                sk,
                node_id.clone(),
                db,
                None,
            );

            // Simulate a connect call that takes longer than 3 seconds
            session
                .evaluate_direct_connection("target_unreachable", || async {
                    tokio::time::sleep(Duration::from_millis(3500)).await;
                    Ok(())
                })
                .await;

            // Since it timed out, relay should be active
            assert!(session.relay_targets.read().await.contains("target_unreachable"));

            // Simulate a fast connection (<3s)
            session
                .evaluate_direct_connection("target_reachable", || async {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    Ok(())
                })
                .await;

            assert!(!session.relay_targets.read().await.contains("target_reachable"));

            session.close().await;
        });
    }

    #[test]
    fn test_profile_handshake_updates_db() {
        let db = Database::new_in_memory().unwrap();
        let sk = SecretKey::generate();
        let node_id = sk.public().to_string();

        let session = RoomSession::new(
            "room_handshake".to_string(),
            node_id.clone(),
            sk,
            node_id.clone(),
            db.clone(),
            None,
        );

        let handshake = ProfileHandshake {
            nickname: "Alice_Handshake".to_string(),
            avatar_hash: Some("avatar_hash_xyz".to_string()),
        };

        let contact = session
            .handle_profile_handshake("peer_alice", &handshake)
            .unwrap();
        assert_eq!(contact.nickname, "Alice_Handshake");
        assert_eq!(contact.avatar_path, Some("avatar_hash_xyz".to_string()));

        // Verify in DB directly
        let loaded = db.get_contact("peer_alice").unwrap().unwrap();
        assert_eq!(loaded.nickname, "Alice_Handshake");

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            session.close().await;
        });
    }

    #[test]
    fn test_background_heartbeat_and_automatic_host_migration_on_timeout() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let db = Database::new_in_memory().unwrap();
            let host_id = "dead_host_node".to_string();
            let peer1_sk = SecretKey::generate();
            let peer1_id = peer1_sk.public().to_string();

            let changed_payloads = Arc::new(tokio::sync::Mutex::new(Vec::new()));
            let cp_clone = changed_payloads.clone();
            let cb: HostChangedCallback = Arc::new(move |payload: &HostChangedPayload| {
                let cp = cp_clone.clone();
                let p = payload.clone();
                tokio::spawn(async move {
                    cp.lock().await.push(p);
                });
            });

            let session = RoomSession::new(
                "room_auto_migration".to_string(),
                peer1_id.clone(),
                peer1_sk,
                host_id.clone(),
                db,
                Some(cb),
            );

            // Add dead host and peer 1
            session.state.write().await.add_or_update_peer(PeerInfo {
                node_id: host_id.clone(),
                nickname: "OldHost".to_string(),
                avatar_hash: None,
                join_timestamp: 10,
                is_host: true,
                is_muted: false,
                is_deafened: false,
            });
            session.state.write().await.add_or_update_peer(PeerInfo {
                node_id: peer1_id.clone(),
                nickname: "Peer1".to_string(),
                avatar_hash: None,
                join_timestamp: 20,
                is_host: false,
                is_muted: false,
                is_deafened: false,
            });

            // Set last_heartbeat_received to 2500ms ago to simulate timeout
            *session.last_heartbeat_received.write().await =
                Instant::now() - Duration::from_millis(2500);

            // Give background loop time to detect timeout and trigger migration
            tokio::time::sleep(Duration::from_millis(250)).await;

            let st = session.state.read().await;
            // Dead host must be removed
            assert!(st.get_peer(&host_id).is_none());
            // peer1 should now be deterministically promoted to host!
            assert_eq!(st.host_node_id, peer1_id);

            session.close().await;
        });
    }

    #[test]
    fn test_host_heartbeat_broadcast_every_700ms() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let db = Database::new_in_memory().unwrap();
            let host_sk = SecretKey::generate();
            let host_id = host_sk.public().to_string();

            let session = RoomSession::new(
                "room_hb".to_string(),
                host_id.clone(),
                host_sk,
                host_id.clone(),
                db,
                None,
            );

            let mut rx = session.broadcast_tx.subscribe();

            // Set last_heartbeat_sent to 800ms ago to trigger immediate broadcast
            *session.last_heartbeat_sent.write().await =
                Instant::now() - Duration::from_millis(800);

            // Wait for interval tick
            tokio::time::sleep(Duration::from_millis(200)).await;

            let msg = rx.recv().await.unwrap();
            match msg {
                RoomGossipMessage::Heartbeat { host_id: h_id, .. } => {
                    assert_eq!(h_id, host_id);
                }
                _ => panic!("Expected Heartbeat message"),
            }

            session.close().await;
        });
    }
}
