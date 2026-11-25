use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

use bytes::Bytes;
use futures::StreamExt;
use base64::Engine;
use iroh::endpoint::Connection;
use iroh::{EndpointAddr, PublicKey, SecretKey, Signature};
use iroh_gossip::api::{Event, GossipReceiver, GossipSender};
use iroh_gossip::proto::TopicId;

use crate::audio::{AudioManager, AudioPacketSender};
use crate::ipc::events::{HostChangedPayload, EVENT_HOST_CHANGED, EVENT_PEERS_UPDATED};
use crate::network::node::{NodeAddr, NodeManager};
use crate::storage::db::Database;
use crate::storage::models::{Contact, Message};

pub const AUDIO_ALPN: &[u8] = b"/p2pchat/audio/1";

// ============================================================================
// 0. Room Ticket (Compound Bootstrap Structure)
// ============================================================================

/// Compound Room Ticket containing gossip topic ID and bootstrap nodes for NAT discovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomTicket {
    pub topic_id: [u8; 32],
    pub bootstrap_nodes: Vec<NodeAddr>,
}

impl RoomTicket {
    pub fn new(topic_id: [u8; 32], bootstrap_nodes: Vec<NodeAddr>) -> Self {
        Self {
            topic_id,
            bootstrap_nodes,
        }
    }

    pub fn to_ticket_string(&self) -> Result<String, String> {
        let json_bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        let encoded = data_encoding::BASE32_NOPAD.encode(&json_bytes);
        Ok(format!("ticket_{}", encoded))
    }

    pub fn from_ticket_string(s: &str) -> Result<Self, String> {
        let clean = s.trim();
        let payload = clean.strip_prefix("ticket_").unwrap_or(clean);

        // 1. Try base32 decode (standard for ticket_...)
        if let Ok(bytes) = data_encoding::BASE32_NOPAD.decode(payload.as_bytes()) {
            if let Ok(ticket) = serde_json::from_slice::<Self>(&bytes) {
                return Ok(ticket);
            }
        }

        // 2. Try URL-safe base64 decode
        if let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload) {
            if let Ok(ticket) = serde_json::from_slice::<Self>(&bytes) {
                return Ok(ticket);
            }
        }

        // 3. Try standard base64 decode
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(payload) {
            if let Ok(ticket) = serde_json::from_slice::<Self>(&bytes) {
                return Ok(ticket);
            }
        }

        // 4. Try raw JSON string
        if let Ok(ticket) = serde_json::from_str::<Self>(payload) {
            return Ok(ticket);
        }

        Err(format!("Invalid room ticket format: {}", s))
    }
}

impl FromStr for RoomTicket {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_ticket_string(s)
    }
}


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
    #[serde(default)]
    pub peers: Vec<PeerInfo>,
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
        #[serde(default)]
        peers: Vec<PeerInfo>,
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
        #[serde(default)]
        node_id: String,
        nickname: String,
        avatar_hash: Option<String>,
    },
    ChatMessage {
        message_id: String,
        sender_id: String,
        content: String,
        attachment_hash: Option<String>,
        attachment_name: Option<String>,
        timestamp: i64,
    },
    MessageEdited {
        message_id: String,
        new_content: String,
    },
    MessageDeleted {
        message_id: String,
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
    #[serde(default)]
    pub ticket: Option<String>,
}

impl RoomState {
    pub fn new(room_id: String, host_node_id: String) -> Self {
        Self {
            room_id,
            host_node_id,
            peers: Vec::new(),
            heartbeat_timeout_ms: 2000,
            ticket: None,
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
pub type ChatMessageCallback = Arc<dyn Fn(&Message) + Send + Sync>;
pub type ChatMessageEditedCallback = Arc<dyn Fn(&str, &str) + Send + Sync>;
pub type ChatMessageDeletedCallback = Arc<dyn Fn(&str) + Send + Sync>;
pub type PeersUpdatedCallback = Arc<dyn Fn(&RoomState) + Send + Sync>;
pub type AvatarHandshakeCallback = Arc<dyn Fn(&str, &str) + Send + Sync>;
pub type PeerJoinedCallback = Arc<dyn Fn(&str) + Send + Sync>;

pub struct RoomSession {
    pub room_id: String,
    pub local_node_id: String,
    pub secret_key: SecretKey,
    pub state: Arc<RwLock<RoomState>>,
    pub last_heartbeat_received: Arc<RwLock<Instant>>,
    pub has_received_heartbeat: Arc<AtomicBool>,
    pub joined_at: Instant,
    pub last_heartbeat_sent: Arc<RwLock<Instant>>,
    pub broadcast_tx: broadcast::Sender<RoomGossipMessage>,
    pub forwarded_datagram_tx: broadcast::Sender<(String, Vec<u8>)>,
    pub relay_targets: Arc<RwLock<HashSet<String>>>,
    pub db: Database,
    pub host_changed_cb: Option<HostChangedCallback>,
    pub chat_message_cb: Option<ChatMessageCallback>,
    pub chat_edit_cb: Option<ChatMessageEditedCallback>,
    pub chat_delete_cb: Option<ChatMessageDeletedCallback>,
    pub peers_updated_cb: Option<PeersUpdatedCallback>,
    pub peer_joined_cb: Option<PeerJoinedCallback>,
    pub avatar_cb: Arc<RwLock<Option<AvatarHandshakeCallback>>>,
    pub gossip_sender: Arc<Mutex<Option<GossipSender>>>,
    pub join_timestamp: u64,
    pub local_nickname: Arc<RwLock<String>>,
    pub local_avatar_hash: Arc<RwLock<Option<String>>>,
    cancel_tx: broadcast::Sender<()>,
    heartbeat_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    gossip_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl RoomSession {
    pub fn new(
        room_id: String,
        local_node_id: String,
        secret_key: SecretKey,
        initial_host_id: String,
        db: Database,
        host_changed_cb: Option<HostChangedCallback>,
        chat_message_cb: Option<ChatMessageCallback>,
    ) -> Arc<Self> {
        Self::with_all_callbacks(
            room_id,
            local_node_id,
            secret_key,
            initial_host_id,
            db,
            host_changed_cb,
            chat_message_cb,
            None,
            None,
            None,
            None,
        )
    }

    pub fn with_callbacks(
        room_id: String,
        local_node_id: String,
        secret_key: SecretKey,
        initial_host_id: String,
        db: Database,
        host_changed_cb: Option<HostChangedCallback>,
        chat_message_cb: Option<ChatMessageCallback>,
        peers_updated_cb: Option<PeersUpdatedCallback>,
    ) -> Arc<Self> {
        Self::with_all_callbacks(
            room_id,
            local_node_id,
            secret_key,
            initial_host_id,
            db,
            host_changed_cb,
            chat_message_cb,
            peers_updated_cb,
            None,
            None,
            None,
        )
    }

    pub fn with_all_callbacks(
        room_id: String,
        local_node_id: String,
        secret_key: SecretKey,
        initial_host_id: String,
        db: Database,
        host_changed_cb: Option<HostChangedCallback>,
        chat_message_cb: Option<ChatMessageCallback>,
        peers_updated_cb: Option<PeersUpdatedCallback>,
        peer_joined_cb: Option<PeerJoinedCallback>,
        chat_edit_cb: Option<ChatMessageEditedCallback>,
        chat_delete_cb: Option<ChatMessageDeletedCallback>,
    ) -> Arc<Self> {
        let (broadcast_tx, _) = broadcast::channel(256);
        let (forwarded_datagram_tx, _) = broadcast::channel(1024);
        let (cancel_tx, _) = broadcast::channel(1);

        let initial_state = RoomState::new(room_id.clone(), initial_host_id);
        let state = Arc::new(RwLock::new(initial_state));
        let last_heartbeat_received = Arc::new(RwLock::new(Instant::now()));
        let has_received_heartbeat = Arc::new(AtomicBool::new(false));
        let joined_at = Instant::now();
        let last_heartbeat_sent = Arc::new(RwLock::new(Instant::now()));
        let relay_targets = Arc::new(RwLock::new(HashSet::new()));
        let gossip_sender = Arc::new(Mutex::new(None));
        let join_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let session = Arc::new(Self {
            room_id: room_id.clone(),
            local_node_id: local_node_id.clone(),
            secret_key,
            state: state.clone(),
            last_heartbeat_received: last_heartbeat_received.clone(),
            has_received_heartbeat: has_received_heartbeat.clone(),
            joined_at,
            last_heartbeat_sent: last_heartbeat_sent.clone(),
            broadcast_tx: broadcast_tx.clone(),
            forwarded_datagram_tx,
            relay_targets,
            db,
            host_changed_cb: host_changed_cb.clone(),
            chat_message_cb,
            chat_edit_cb,
            chat_delete_cb,
            peers_updated_cb: peers_updated_cb.clone(),
            peer_joined_cb,
            avatar_cb: Arc::new(RwLock::new(None)),
            gossip_sender: gossip_sender.clone(),
            join_timestamp,
            local_nickname: Arc::new(RwLock::new("Me".to_string())),
            local_avatar_hash: Arc::new(RwLock::new(None)),
            cancel_tx: cancel_tx.clone(),
            heartbeat_task: Mutex::new(None),
            gossip_task: Mutex::new(None),
        });

        // Spawn background heartbeat and host-migration task if within a tokio runtime
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let task = handle.spawn(Self::run_background_loop(
                room_id,
                local_node_id,
                state,
                last_heartbeat_received,
                has_received_heartbeat,
                joined_at,
                last_heartbeat_sent,
                broadcast_tx,
                gossip_sender,
                host_changed_cb,
                peers_updated_cb,
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
    ///   and emit Tauri events `room://host-changed` and `room://peers-updated`.
    async fn run_background_loop(
        room_id: String,
        local_node_id: String,
        state: Arc<RwLock<RoomState>>,
        last_heartbeat_received: Arc<RwLock<Instant>>,
        has_received_heartbeat: Arc<AtomicBool>,
        joined_at: Instant,
        last_heartbeat_sent: Arc<RwLock<Instant>>,
        broadcast_tx: broadcast::Sender<RoomGossipMessage>,
        gossip_sender: Arc<Mutex<Option<GossipSender>>>,
        host_changed_cb: Option<HostChangedCallback>,
        peers_updated_cb: Option<PeersUpdatedCallback>,
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
                            let timestamp = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis() as u64;

                            let peers_snapshot = {
                                let st = state.read().await;
                                st.peers.clone()
                            };

                            let heartbeat = RoomGossipMessage::Heartbeat {
                                host_id: local_node_id.clone(),
                                timestamp,
                                peer_list: peer_ids,
                                peers: peers_snapshot,
                            };
                            let _ = broadcast_tx.send(heartbeat.clone());
                            if let Some(ref sender) = *gossip_sender.lock().await {
                                if let Ok(bytes) = heartbeat.to_bytes() {
                                    let _ = sender.broadcast(Bytes::from(bytes)).await;
                                }
                            }
                            *last_sent = now;
                            *last_heartbeat_received.write().await = now;
                        }
                    } else {
                        // Defer host-timeout migration until at least one Heartbeat is received
                        // from the host or the initial 9s gossip-mesh join grace window elapses.
                        let has_hb = has_received_heartbeat.load(Ordering::Acquire);
                        let grace_elapsed = now.duration_since(joined_at) >= Duration::from_secs(9);
                        if has_hb || grace_elapsed {
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
                                    let st_clone = st.clone();
                                    drop(st);

                                    let payload = HostChangedPayload {
                                        room_id: room_id.clone(),
                                        old_host_id: dead_host,
                                        new_host_id: new_host_id.clone(),
                                    };
                                    Self::emit_host_changed(&host_changed_cb, &payload);
                                    Self::emit_peers_updated(&peers_updated_cb, &st_clone);

                                    // If we were promoted to host, send immediate heartbeat
                                    if new_host_id == local_node_id {
                                        let timestamp = SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis() as u64;
                                        let updated_peers =
                                            st_clone.peers.iter().map(|p| p.node_id.clone()).collect();
                                        let heartbeat = RoomGossipMessage::Heartbeat {
                                            host_id: local_node_id.clone(),
                                            timestamp,
                                            peer_list: updated_peers,
                                            peers: st_clone.peers.clone(),
                                        };
                                        let _ = broadcast_tx.send(heartbeat.clone());
                                        if let Some(ref sender) = *gossip_sender.lock().await {
                                            if let Ok(bytes) = heartbeat.to_bytes() {
                                                let _ = sender.broadcast(Bytes::from(bytes)).await;
                                            }
                                        }
                                        *last_heartbeat_sent.write().await = now;
                                    }
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

    fn emit_peers_updated(cb: &Option<PeersUpdatedCallback>, state: &RoomState) {
        if let Some(f) = cb {
            f(state);
        }
    }

    pub async fn set_local_profile(&self, nickname: String, avatar_hash: Option<String>) {
        *self.local_nickname.write().await = nickname;
        *self.local_avatar_hash.write().await = avatar_hash;
    }

    pub fn attach_gossip_receiver(self: &Arc<Self>, mut receiver: GossipReceiver) {
        let session = self.clone();
        let mut cancel_rx = self.cancel_tx.subscribe();
        let task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_rx.recv() => break,
                    item = receiver.next() => {
                        match item {
                            Some(Ok(Event::Received(msg))) => {
                                if let Ok(room_msg) = RoomGossipMessage::from_bytes(&msg.content) {
                                    session.handle_gossip_message(room_msg).await.ok();
                                }
                            }
                            Some(Ok(Event::NeighborUp(_nid))) => {
                                let my_nickname = session.local_nickname.read().await.clone();
                                let my_avatar = session.local_avatar_hash.read().await.clone();
                                let (is_muted, is_deafened) = {
                                    let st = session.state.read().await;
                                    st.get_peer(&session.local_node_id)
                                        .map(|p| (p.is_muted, p.is_deafened))
                                        .unwrap_or((false, false))
                                };
                                let my_joined = RoomGossipMessage::PeerJoined {
                                    node_id: session.local_node_id.clone(),
                                    nickname: my_nickname,
                                    avatar_hash: my_avatar,
                                    join_timestamp: session.join_timestamp,
                                };
                                let _ = session.broadcast_gossip(my_joined).await;

                                let my_state = RoomGossipMessage::PeerStateUpdate {
                                    node_id: session.local_node_id.clone(),
                                    is_muted,
                                    is_deafened,
                                };
                                let _ = session.broadcast_gossip(my_state).await;
                            }
                            Some(Ok(Event::NeighborDown(_nid))) => {}
                            Some(Err(e)) => {
                                eprintln!("Gossip receiver error: {e}");
                            }
                            None => break,
                            _ => {}
                        }
                    }
                }
            }
        });
        let s_clone = self.clone();
        tokio::spawn(async move {
            *s_clone.gossip_task.lock().await = Some(task);
        });
    }

    pub async fn broadcast_gossip(&self, msg: RoomGossipMessage) -> Result<(), String> {
        let _ = self.broadcast_tx.send(msg.clone());
        if let Some(ref sender) = *self.gossip_sender.lock().await {
            let bytes = msg.to_bytes().map_err(|e| e.to_string())?;
            sender.broadcast(Bytes::from(bytes)).await.map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Implements manual host transfer:
    /// 1. Verifies caller is current host.
    /// 2. Signs target_node_id with host's Ed25519 SecretKey.
    /// 3. Broadcasts signed `TransferHost` message.
    /// 4. Updates local state immediately and emits `room://host-changed` and `room://peers-updated`.
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
        let _ = self.broadcast_gossip(msg).await;

        // Update state immediately
        let old_host_id = state.host_node_id.clone();
        state.set_host(target_node_id.to_string());
        *self.last_heartbeat_received.write().await = Instant::now();
        let st_clone = state.clone();
        drop(state);

        // Emit room://host-changed & room://peers-updated Tauri events
        let payload = HostChangedPayload {
            room_id: st_clone.room_id.clone(),
            old_host_id,
            new_host_id: target_node_id.to_string(),
        };
        Self::emit_host_changed(&self.host_changed_cb, &payload);
        Self::emit_peers_updated(&self.peers_updated_cb, &st_clone);

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
        let st_clone = state.clone();
        drop(state);

        let payload = HostChangedPayload {
            room_id: st_clone.room_id.clone(),
            old_host_id: current_host_id,
            new_host_id: new_host_id.to_string(),
        };
        Self::emit_host_changed(&self.host_changed_cb, &payload);
        Self::emit_peers_updated(&self.peers_updated_cb, &st_clone);

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
                if node_id == self.local_node_id {
                    return Ok(());
                }

                let is_new = {
                    let st = self.state.read().await;
                    !st.peers.iter().any(|p| p.node_id == node_id)
                };

                let st_clone = {
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
                    state.clone()
                };

                Self::emit_peers_updated(&self.peers_updated_cb, &st_clone);

                // Auto-update local contacts SQLite table
                self.db
                    .handle_profile_handshake(&node_id, &nickname, avatar_hash.as_deref())
                    .map_err(|e| e.to_string())?;

                if let Some(ref hash) = avatar_hash {
                    let cb_opt = self.avatar_cb.read().await.clone();
                    if let Some(cb) = cb_opt {
                        cb(&node_id, hash);
                    }
                }

                // If this is a newly seen peer, reply with our own info and state update
                if is_new {
                    let my_nickname = self.local_nickname.read().await.clone();
                    let my_avatar = self.local_avatar_hash.read().await.clone();
                    let my_joined = RoomGossipMessage::PeerJoined {
                        node_id: self.local_node_id.clone(),
                        nickname: my_nickname,
                        avatar_hash: my_avatar,
                        join_timestamp: self.join_timestamp,
                    };
                    let _ = self.broadcast_gossip(my_joined).await;

                    let my_state = RoomGossipMessage::PeerStateUpdate {
                        node_id: self.local_node_id.clone(),
                        is_muted: false,
                        is_deafened: false,
                    };
                    let _ = self.broadcast_gossip(my_state).await;
                }

                if let Some(ref cb) = self.peer_joined_cb {
                    cb(&node_id);
                }
            }
            RoomGossipMessage::Heartbeat {
                host_id,
                timestamp: _,
                peer_list: _,
                peers,
            } => {
                let mut updated = false;
                let mut new_peers = Vec::new();
                let st_clone = {
                    let mut state = self.state.write().await;
                    if host_id == state.host_node_id {
                        *self.last_heartbeat_received.write().await = Instant::now();
                        self.has_received_heartbeat.store(true, Ordering::Release);
                    }
                    for p in peers {
                        if p.node_id != self.local_node_id {
                            let existing = state.peers.iter_mut().find(|ep| ep.node_id == p.node_id);
                            if let Some(existing_peer) = existing {
                                if existing_peer.is_host != p.is_host
                                    || existing_peer.nickname != p.nickname
                                    || existing_peer.is_muted != p.is_muted
                                    || existing_peer.is_deafened != p.is_deafened
                                {
                                    *existing_peer = p;
                                    updated = true;
                                }
                            } else {
                                new_peers.push(p.node_id.clone());
                                state.peers.push(p);
                                updated = true;
                            }
                        }
                    }
                    if updated {
                        state.sort_peers();
                    }
                    state.clone()
                };
                if updated {
                    Self::emit_peers_updated(&self.peers_updated_cb, &st_clone);
                    if let Some(ref cb) = self.peer_joined_cb {
                        for nid in new_peers {
                            cb(&nid);
                        }
                    }
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
                let st_clone = {
                    let mut state = self.state.write().await;
                    if let Some(peer) = state.get_peer_mut(&node_id) {
                        peer.is_muted = is_muted;
                        peer.is_deafened = is_deafened;
                    }
                    state.clone()
                };
                Self::emit_peers_updated(&self.peers_updated_cb, &st_clone);
            }
            RoomGossipMessage::RelayRequest {
                from_node: _,
                target_node: _,
            } => {
                // Relay route setup/handshake
            }
            RoomGossipMessage::ProfileHandshake {
                node_id,
                nickname,
                avatar_hash,
            } => {
                if !node_id.is_empty() && node_id != self.local_node_id {
                    let st_clone = {
                        let mut state = self.state.write().await;
                        if let Some(peer) = state.get_peer_mut(&node_id) {
                            peer.nickname = nickname.clone();
                            peer.avatar_hash = avatar_hash.clone();
                        }
                        state.clone()
                    };
                    Self::emit_peers_updated(&self.peers_updated_cb, &st_clone);
                    let _ = self
                        .db
                        .handle_profile_handshake(&node_id, &nickname, avatar_hash.as_deref());
                    if let Some(ref hash) = avatar_hash {
                        let cb_opt = self.avatar_cb.read().await.clone();
                        if let Some(cb) = cb_opt {
                            cb(&node_id, hash);
                        }
                    }
                }
            }
            RoomGossipMessage::ChatMessage {
                message_id,
                sender_id,
                content,
                attachment_hash,
                attachment_name,
                timestamp,
            } => {
                if sender_id == self.local_node_id {
                    return Ok(());
                }
                let msg = Message {
                    id: message_id,
                    chat_id: self.room_id.clone(),
                    sender_id,
                    content,
                    attachment_hash,
                    attachment_name,
                    timestamp,
                    delivered: 1,
                    is_edited: 0,
                };
                self.db.insert_message(&msg).map_err(|e| e.to_string())?;
                if let Some(ref cb) = self.chat_message_cb {
                    cb(&msg);
                }
            }
            RoomGossipMessage::MessageEdited { message_id, new_content } => {
                let _ = self.db.edit_message(&message_id, &new_content);
                if let Some(ref cb) = self.chat_edit_cb {
                    cb(&message_id, &new_content);
                }
            }
            RoomGossipMessage::MessageDeleted { message_id } => {
                let _ = self.db.delete_message(&message_id);
                if let Some(ref cb) = self.chat_delete_cb {
                    cb(&message_id);
                }
            }
        }
        Ok(())
    }

    /// Sends a room chat message, saves it locally, and broadcasts it over the gossip channel.
    pub async fn send_room_chat(
        &self,
        content: String,
        attachment_hash: Option<String>,
        attachment_name: Option<String>,
    ) -> Result<Message, String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let id = format!(
            "msg_{}_{}_{}",
            self.local_node_id,
            now,
            crate::network::file_transfer::next_message_seq()
        );

        let msg = Message {
            id: id.clone(),
            chat_id: self.room_id.clone(),
            sender_id: self.local_node_id.clone(),
            content: content.clone(),
            attachment_hash: attachment_hash.clone(),
            attachment_name: attachment_name.clone(),
            timestamp: now,
            delivered: 1,
            is_edited: 0,
        };

        self.db.insert_message(&msg).map_err(|e| e.to_string())?;

        let gossip_msg = RoomGossipMessage::ChatMessage {
            message_id: id,
            sender_id: self.local_node_id.clone(),
            content,
            attachment_hash,
            attachment_name,
            timestamp: now,
        };

        let _ = self.broadcast_gossip(gossip_msg).await;

        Ok(msg)
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

    pub async fn set_avatar_cb(&self, cb: AvatarHandshakeCallback) {
        *self.avatar_cb.write().await = Some(cb);
    }

    /// Cleanly closes the room session and terminates background tasks.
    pub async fn close(&self) {
        let _ = self.cancel_tx.send(());
        let mut task_lock = self.heartbeat_task.lock().await;
        if let Some(task) = task_lock.take() {
            task.abort();
        }
        let mut g_lock = self.gossip_task.lock().await;
        if let Some(task) = g_lock.take() {
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
// 5. Room Manager & Audio Protocol Handler
// ============================================================================

#[derive(Clone)]
pub struct AudioProtocolHandler {
    room_manager: RoomManager,
}

impl AudioProtocolHandler {
    pub fn new(room_manager: RoomManager) -> Self {
        Self { room_manager }
    }
}

impl std::fmt::Debug for AudioProtocolHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AudioProtocolHandler")
    }
}

impl iroh::protocol::ProtocolHandler for AudioProtocolHandler {
    fn accept(
        &self,
        connection: Connection,
    ) -> impl std::future::Future<Output = Result<(), iroh::protocol::AcceptError>> + Send {
        let rm = self.room_manager.clone();
        async move {
            let remote_id = connection.remote_id().to_string();
            rm.register_audio_connection(remote_id, connection).await;
            Ok(())
        }
    }
}

struct RoomManagerInner {
    pub current_room: Option<Arc<RoomSession>>,
    pub local_node_id: String,
    pub secret_key: SecretKey,
    pub avatar_cb: Option<AvatarHandshakeCallback>,
    pub audio_manager: Option<Arc<AudioManager>>,
}

#[derive(Clone)]
pub struct RoomManager {
    pub node_manager: Option<NodeManager>,
    inner: Arc<RwLock<RoomManagerInner>>,
    audio_connections: Arc<RwLock<HashMap<String, Connection>>>,
    sent_audio_packets: Arc<AtomicU64>,
    recv_audio_packets: Arc<AtomicU64>,
    last_audio_log: Arc<RwLock<Instant>>,
    db: Database,
    app_handle: Option<tauri::AppHandle>,
}

impl RoomManager {
    pub fn new(
        node_manager: NodeManager,
        db: Database,
        app_handle: Option<tauri::AppHandle>,
    ) -> Self {
        let (secret_key, local_node_id) = node_manager.get_identity_sync();

        let inner = Arc::new(RwLock::new(RoomManagerInner {
            current_room: None,
            local_node_id,
            secret_key,
            avatar_cb: None,
            audio_manager: None,
        }));

        Self {
            node_manager: Some(node_manager),
            inner,
            audio_connections: Arc::new(RwLock::new(HashMap::new())),
            sent_audio_packets: Arc::new(AtomicU64::new(0)),
            recv_audio_packets: Arc::new(AtomicU64::new(0)),
            last_audio_log: Arc::new(RwLock::new(Instant::now())),
            db,
            app_handle,
        }
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
            avatar_cb: None,
            audio_manager: None,
        }));

        Self {
            node_manager: None,
            inner,
            audio_connections: Arc::new(RwLock::new(HashMap::new())),
            sent_audio_packets: Arc::new(AtomicU64::new(0)),
            recv_audio_packets: Arc::new(AtomicU64::new(0)),
            last_audio_log: Arc::new(RwLock::new(Instant::now())),
            db,
            app_handle,
        }
    }

    pub fn create_audio_handler(&self) -> AudioProtocolHandler {
        AudioProtocolHandler::new(self.clone())
    }

    pub async fn set_audio_manager(&self, am: Arc<AudioManager>) {
        let mut inner = self.inner.write().await;
        inner.audio_manager = Some(am);
    }

    pub async fn get_audio_manager(&self) -> Option<Arc<AudioManager>> {
        let inner = self.inner.read().await;
        inner.audio_manager.clone()
    }

    pub async fn get_local_node_id(&self) -> String {
        let inner = self.inner.read().await;
        inner.local_node_id.clone()
    }

    pub async fn register_audio_connection(&self, node_id: String, connection: Connection) {
        {
            let mut conns = self.audio_connections.write().await;
            conns.insert(node_id.clone(), connection.clone());
        }

        let rm = self.clone();
        let nid = node_id.clone();
        tokio::spawn(async move {
            while let Ok(dgram) = connection.read_datagram().await {
                rm.handle_incoming_audio_datagram(&nid, &dgram).await;
            }
            rm.remove_audio_connection(&nid).await;
        });
    }

    pub async fn remove_audio_connection(&self, node_id: &str) {
        let mut conns = self.audio_connections.write().await;
        conns.remove(node_id);
    }

    pub async fn handle_incoming_audio_datagram(&self, sender_id: &str, raw_data: &[u8]) {
        self.recv_audio_packets.fetch_add(1, Ordering::Relaxed);
        self.log_audio_stats_if_needed().await;

        let room = match self.get_current_room().await {
            Some(r) => r,
            None => return,
        };

        match room.handle_incoming_datagram(sender_id, raw_data).await {
            Ok(Some(packet)) => {
                if let Some(ref am) = self.get_audio_manager().await {
                    let _ = am.handle_incoming_audio_packet(&packet);
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("[Audio] Error decoding datagram from {sender_id}: {e}");
            }
        }
    }

    pub async fn broadcast_audio(&self, seq: u64, opus_data: Vec<u8>) {
        let room = match self.get_current_room().await {
            Some(r) => r,
            None => return,
        };

        let datagrams = room.broadcast_audio_frame(seq, &opus_data).await;
        if datagrams.is_empty() {
            return;
        }

        self.sent_audio_packets.fetch_add(1, Ordering::Relaxed);
        self.log_audio_stats_if_needed().await;

        let conns = self.audio_connections.read().await;
        for (dest_id, bytes) in datagrams {
            if let Some(conn) = conns.get(&dest_id) {
                let _ = conn.send_datagram(Bytes::from(bytes));
            }
        }
    }

    pub async fn log_audio_stats_if_needed(&self) {
        let now = Instant::now();
        let should_log = {
            let last = self.last_audio_log.read().await;
            now.duration_since(*last) >= Duration::from_secs(3)
        };

        if should_log {
            let mut last = self.last_audio_log.write().await;
            if now.duration_since(*last) >= Duration::from_secs(3) {
                *last = now;
                let sent = self.sent_audio_packets.load(Ordering::Relaxed);
                let recv = self.recv_audio_packets.load(Ordering::Relaxed);
                if sent > 0 || recv > 0 {
                    println!("[Audio Stats] Sent: {} packets | Received: {} packets", sent, recv);
                }
            }
        }
    }

    pub async fn connect_peer_audio(&self, peer_node_id: &str) {
        if peer_node_id == self.get_local_node_id().await {
            return;
        }

        {
            let conns = self.audio_connections.read().await;
            if conns.contains_key(peer_node_id) {
                return;
            }
        }

        let endpoint = match self.node_manager.as_ref() {
            Some(nm) => match nm.get_endpoint().await {
                Some(ep) => ep,
                None => return,
            },
            None => return,
        };

        let target_pk = match PublicKey::from_str(peer_node_id) {
            Ok(pk) => pk,
            Err(_) => return,
        };

        let addr = EndpointAddr::new(target_pk);
        let connect_res = tokio::time::timeout(
            Duration::from_millis(3000),
            endpoint.connect(addr, AUDIO_ALPN),
        )
        .await;

        match connect_res {
            Ok(Ok(connection)) => {
                if let Some(ref nm) = self.node_manager {
                    nm.record_peer_connection(target_pk).await;
                }
                self.register_audio_connection(peer_node_id.to_string(), connection).await;
                if let Some(room) = self.get_current_room().await {
                    room.set_peer_relay_fallback(peer_node_id, false).await;
                }
            }
            _ => {
                if let Some(room) = self.get_current_room().await {
                    room.set_peer_relay_fallback(peer_node_id, true).await;
                }
            }
        }
    }

    pub async fn set_avatar_cb(&self, cb: AvatarHandshakeCallback) {
        let mut inner = self.inner.write().await;
        inner.avatar_cb = Some(cb);
    }

    pub async fn create_or_join_room(
        &self,
        room_id_or_ticket: &str,
        host_id: Option<&str>,
    ) -> Arc<RoomSession> {
        let mut inner = self.inner.write().await;

        let parsed_ticket = RoomTicket::from_ticket_string(room_id_or_ticket).ok();

        let (topic_id, bootstrap_nodes, room_id, effective_host_id, is_joining_with_ticket) =
            if let Some(ticket) = parsed_ticket {
                let tid = ticket.topic_id;
                let short_hex = data_encoding::HEXLOWER.encode(&tid[..6]);
                let r_id = format!("room_{}", short_hex);
                let h_id = host_id
                    .map(|s| s.to_string())
                    .or_else(|| ticket.bootstrap_nodes.first().map(|a| a.id.to_string()))
                    .unwrap_or_else(|| inner.local_node_id.clone());
                (tid, ticket.bootstrap_nodes, r_id, h_id, true)
            } else {
                let tid = *iroh_blobs::Hash::new(room_id_or_ticket.as_bytes()).as_bytes();
                let r_id = room_id_or_ticket.to_string();
                let h_id = host_id
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| inner.local_node_id.clone());
                (tid, Vec::new(), r_id, h_id, false)
            };

        if let Some(existing) = &inner.current_room {
            if existing.room_id == room_id {
                return existing.clone();
            }
            existing.close().await;
        }

        let mut bootstrap_ids = Vec::new();
        if let Some(ref nm) = self.node_manager {
            for node_addr in &bootstrap_nodes {
                nm.add_node_addr(node_addr.clone()).await;
                bootstrap_ids.push(node_addr.id);
            }
        }

        let generated_ticket_str = if !is_joining_with_ticket {
            if let Some(ref nm) = self.node_manager {
                let host_addr = nm.get_node_addr().await;
                let nodes = host_addr.into_iter().collect::<Vec<_>>();
                let t = RoomTicket::new(topic_id, nodes);
                t.to_ticket_string().ok()
            } else {
                None
            }
        } else {
            Some(room_id_or_ticket.to_string())
        };

        // Subscribe to iroh-gossip
        let (gossip_sender, gossip_receiver) = if let Some(ref nm) = self.node_manager {
            if let Some(gossip) = nm.get_gossip().await {
                match gossip.subscribe(TopicId::from(topic_id), bootstrap_ids).await {
                    Ok(topic) => {
                        let (s, r) = topic.split();
                        (Some(s), Some(r))
                    }
                    Err(e) => {
                        eprintln!("Failed to subscribe to gossip topic: {e}");
                        (None, None)
                    }
                }
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        let host_changed_cb = self.app_handle.as_ref().map(|app| {
            let app_handle = app.clone();
            let cb: HostChangedCallback = Arc::new(move |payload: &HostChangedPayload| {
                use tauri::Emitter;
                let _ = app_handle.emit(EVENT_HOST_CHANGED, payload);
            });
            cb
        });

        let chat_message_cb = self.app_handle.as_ref().map(|app| {
            let app_handle = app.clone();
            let cb: ChatMessageCallback = Arc::new(move |msg: &Message| {
                use tauri::Emitter;
                let _ = app_handle.emit(
                    crate::ipc::events::EVENT_CHAT_MESSAGE,
                    crate::ipc::events::ChatMessagePayload {
                        message: msg.clone(),
                    },
                );
            });
            cb
        });

        let peers_updated_cb = self.app_handle.as_ref().map(|app| {
            let app_handle = app.clone();
            let cb: PeersUpdatedCallback = Arc::new(move |st: &RoomState| {
                use tauri::Emitter;
                let _ = app_handle.emit(EVENT_PEERS_UPDATED, st);
            });
            cb
        });

        let rm_for_peer = self.clone();
        let peer_joined_cb: PeerJoinedCallback = Arc::new(move |peer_id: &str| {
            let rm = rm_for_peer.clone();
            let pid = peer_id.to_string();
            tokio::spawn(async move {
                rm.connect_peer_audio(&pid).await;
            });
        });

        let chat_edit_cb = self.app_handle.as_ref().map(|app| {
            let app_handle = app.clone();
            let cb: ChatMessageEditedCallback = Arc::new(move |msg_id: &str, new_content: &str| {
                use tauri::Emitter;
                let _ = app_handle.emit(
                    crate::ipc::events::EVENT_CHAT_MESSAGE_EDITED,
                    crate::ipc::events::MessageEditedPayload {
                        message_id: msg_id.to_string(),
                        new_content: new_content.to_string(),
                    },
                );
            });
            cb
        });

        let chat_delete_cb = self.app_handle.as_ref().map(|app| {
            let app_handle = app.clone();
            let cb: ChatMessageDeletedCallback = Arc::new(move |msg_id: &str| {
                use tauri::Emitter;
                let _ = app_handle.emit(
                    crate::ipc::events::EVENT_CHAT_MESSAGE_DELETED,
                    crate::ipc::events::MessageDeletedPayload {
                        message_id: msg_id.to_string(),
                    },
                );
            });
            cb
        });

        let session = RoomSession::with_all_callbacks(
            room_id.clone(),
            inner.local_node_id.clone(),
            inner.secret_key.clone(),
            effective_host_id.clone(),
            self.db.clone(),
            host_changed_cb,
            chat_message_cb,
            peers_updated_cb.clone(),
            Some(peer_joined_cb),
            chat_edit_cb,
            chat_delete_cb,
        );

        if let Some(s) = gossip_sender {
            *session.gossip_sender.lock().await = Some(s);
        }

        // Set ticket in state
        {
            let mut st = session.state.write().await;
            st.ticket = generated_ticket_str;
        }

        // Fetch our user profile to set nickname & avatar
        let (nickname, avatar_hash) = if let Some(ref nm) = self.node_manager {
            let p = nm.get_my_profile().await;
            (p.nickname, p.avatar_hash)
        } else {
            ("Host".to_string(), None)
        };
        session.set_local_profile(nickname.clone(), avatar_hash.clone()).await;

        // Immediately insert Host/Self into active peers
        let now_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let is_me_host = inner.local_node_id == effective_host_id;

        {
            let mut st = session.state.write().await;
            st.add_or_update_peer(PeerInfo {
                node_id: inner.local_node_id.clone(),
                nickname: nickname.clone(),
                avatar_hash: avatar_hash.clone(),
                join_timestamp: now_ts,
                is_host: is_me_host,
                is_muted: false,
                is_deafened: false,
            });

            if !is_me_host {
                let host_contact = self.db.get_contact(&effective_host_id).ok().flatten();
                let host_nickname = host_contact
                    .as_ref()
                    .map(|c| c.nickname.clone())
                    .unwrap_or_else(|| {
                        let s = if effective_host_id.len() >= 4 {
                            &effective_host_id[..4]
                        } else {
                            &effective_host_id
                        };
                        format!("Host_{}", s)
                    });
                let host_avatar = host_contact.and_then(|c| c.avatar_path);

                st.add_or_update_peer(PeerInfo {
                    node_id: effective_host_id.clone(),
                    nickname: host_nickname,
                    avatar_hash: host_avatar,
                    join_timestamp: now_ts.saturating_sub(1000),
                    is_host: true,
                    is_muted: false,
                    is_deafened: false,
                });
            }
        }

        // Emit peers updated immediately so UI shows both host and joiner right away
        {
            let st = session.state.read().await;
            RoomSession::emit_peers_updated(&peers_updated_cb, &st);
        }

        // Wire packet sender from audio manager if available
        if let Some(ref am) = inner.audio_manager {
            let rm_sender = self.clone();
            let packet_sender: AudioPacketSender = Arc::new(move |seq, opus_data| {
                let rm = rm_sender.clone();
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    handle.spawn(async move {
                        rm.broadcast_audio(seq, opus_data).await;
                    });
                } else {
                    tauri::async_runtime::spawn(async move {
                        rm.broadcast_audio(seq, opus_data).await;
                    });
                }
            });
            am.set_packet_sender(Some(packet_sender));
        }

        // Host forwarding loop for relayed datagrams
        let mut fwd_rx = session.forwarded_datagram_tx.subscribe();
        let conns_ref = self.audio_connections.clone();
        tokio::spawn(async move {
            while let Ok((target_id, payload)) = fwd_rx.recv().await {
                let conn_opt = {
                    let conns = conns_ref.read().await;
                    conns.get(&target_id).cloned()
                };
                if let Some(conn) = conn_opt {
                    let _ = conn.send_datagram(Bytes::from(payload));
                }
            }
        });

        // Connect peer audio in the background for host and bootstrap nodes
        let rm_conn = self.clone();
        let eff_host = effective_host_id.clone();
        let boot_nodes = bootstrap_nodes.clone();
        tokio::spawn(async move {
            if eff_host != rm_conn.get_local_node_id().await {
                rm_conn.connect_peer_audio(&eff_host).await;
            }
            for node_addr in boot_nodes {
                rm_conn.connect_peer_audio(&node_addr.id.to_string()).await;
            }
        });

        // Attach gossip receiver loop if available
        if let Some(recv) = gossip_receiver {
            session.attach_gossip_receiver(recv);
        }

        // Broadcast PeerJoined over gossip (retry 3 times with 500ms intervals)
        let peer_joined_msg = RoomGossipMessage::PeerJoined {
            node_id: inner.local_node_id.clone(),
            nickname,
            avatar_hash,
            join_timestamp: now_ts,
        };
        let s_clone = session.clone();
        tokio::spawn(async move {
            for i in 0..3 {
                if i > 0 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                let _ = s_clone.broadcast_gossip(peer_joined_msg.clone()).await;
            }
        });

        if let Some(ref cb) = inner.avatar_cb {
            session.set_avatar_cb(cb.clone()).await;
        }

        inner.current_room = Some(session.clone());
        session
    }

    pub async fn get_room_ticket(&self) -> Option<String> {
        let inner = self.inner.read().await;
        if let Some(room) = &inner.current_room {
            let st = room.state.read().await;
            st.ticket.clone()
        } else {
            None
        }
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
        crate::network::mobile_service::set_foreground_call_service(false, "", "");
        self.audio_connections.write().await.clear();
        if let Some(ref am) = self.get_audio_manager().await {
            am.set_packet_sender(None);
        }
        let mut inner = self.inner.write().await;
        if let Some(room) = inner.current_room.take() {
            room.close().await;
        }
        if let Some(ref app) = self.app_handle {
            use tauri::Emitter;
            let _ = app.emit("room://left", ());
        }
    }

    pub async fn send_room_chat(
        &self,
        content: String,
        attachment_hash: Option<String>,
        attachment_name: Option<String>,
    ) -> Result<Message, String> {
        let room = {
            let inner = self.inner.read().await;
            inner
                .current_room
                .clone()
                .ok_or_else(|| "No active room session".to_string())?
        };
        room.send_room_chat(content, attachment_hash, attachment_name).await
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

    /// Broadcasts an updated profile (nickname and/or avatar_hash) to all peers in the active room.
    pub async fn broadcast_profile_handshake(&self, nickname: &str, avatar_hash: Option<&str>) {
        let room = {
            let inner = self.inner.read().await;
            inner.current_room.clone()
        };
        if let Some(r) = room {
            r.set_local_profile(nickname.to_string(), avatar_hash.map(|s| s.to_string())).await;
            let msg = RoomGossipMessage::ProfileHandshake {
                node_id: r.local_node_id.clone(),
                nickname: nickname.to_string(),
                avatar_hash: avatar_hash.map(|s| s.to_string()),
            };
            if let Ok(bytes) = msg.to_bytes() {
                let sender_guard = r.gossip_sender.lock().await;
                if let Some(ref sender) = *sender_guard {
                    let _ = sender.broadcast(Bytes::from(bytes)).await;
                }
            }
            // Also update our own peer info in room state and emit to UI
            let st_clone = {
                let mut state = r.state.write().await;
                if let Some(p) = state.get_peer_mut(&r.local_node_id) {
                    p.nickname = nickname.to_string();
                    p.avatar_hash = avatar_hash.map(|s| s.to_string());
                }
                state.clone()
            };
            RoomSession::emit_peers_updated(&r.peers_updated_cb, &st_clone);
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
    fn test_room_ticket_serialization_and_deserialization() {
        let topic_bytes = [42u8; 32];
        let sk = SecretKey::generate();
        let addr = iroh::EndpointAddr::new(sk.public());
        let ticket = RoomTicket::new(topic_bytes, vec![addr]);

        let ticket_str = ticket.to_ticket_string().unwrap();
        assert!(ticket_str.starts_with("ticket_"));

        let parsed = RoomTicket::from_ticket_string(&ticket_str).unwrap();
        assert_eq!(parsed.topic_id, topic_bytes);
        assert_eq!(parsed.bootstrap_nodes.len(), 1);
        assert_eq!(parsed.bootstrap_nodes[0].id, sk.public());

        // Test FromStr implementation
        let parsed_from_str: RoomTicket = ticket_str.parse().unwrap();
        assert_eq!(parsed_from_str.topic_id, topic_bytes);
    }

    #[test]
    fn test_room_manager_host_initial_peer_and_state() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let db = Database::new_in_memory().unwrap();
            let sk = SecretKey::generate();
            let local_id = sk.public().to_string();

            let mgr = RoomManager::new_standalone(local_id.clone(), sk, db, None);
            let session = mgr.create_or_join_room("lobby", None).await;

            let st = session.state.read().await;
            assert_eq!(st.room_id, "lobby");
            assert_eq!(st.peers.len(), 1);
            assert_eq!(st.peers[0].node_id, local_id);
            assert!(st.peers[0].is_host);

            mgr.leave_room().await;
        });
    }

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
                None,
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

            // Set last_heartbeat_received to 2500ms ago to simulate timeout after receiving a heartbeat
            *session.last_heartbeat_received.write().await =
                Instant::now() - Duration::from_millis(2500);
            session.has_received_heartbeat.store(true, Ordering::Release);

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

    #[test]
    fn test_room_chat_messaging() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let db = Database::new_in_memory().unwrap();
            let sk = SecretKey::generate();
            let node_id = sk.public().to_string();

            let session = RoomSession::new(
                "room_chat_test".to_string(),
                node_id.clone(),
                sk,
                node_id.clone(),
                db.clone(),
                None,
                None,
            );

            let mut rx = session.broadcast_tx.subscribe();

            let msg = session
                .send_room_chat("Hello room!".to_string(), None, None)
                .await
                .unwrap();

            assert_eq!(msg.content, "Hello room!");
            assert_eq!(msg.chat_id, "room_chat_test");

            // Verify stored in DB
            let msgs = db.get_messages_for_chat("room_chat_test", 10).unwrap();
            assert_eq!(msgs.len(), 1);
            assert_eq!(msgs[0].content, "Hello room!");

            // Verify broadcast over gossip
            let gossip_msg = rx.recv().await.unwrap();
            match gossip_msg {
                RoomGossipMessage::ChatMessage { content, .. } => {
                    assert_eq!(content, "Hello room!");
                }
                _ => panic!("Expected ChatMessage variant"),
            }

            session.close().await;
        });
    }

    #[test]
    fn test_heartbeat_peers_merging() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let db = Database::new_in_memory().unwrap();
            let host_sk = SecretKey::generate();
            let host_id = host_sk.public().to_string();
            let my_sk = SecretKey::generate();
            let my_id = my_sk.public().to_string();

            let session = RoomSession::new(
                "room_merge_test".to_string(),
                my_id.clone(),
                my_sk,
                host_id.clone(),
                db,
                None,
                None,
            );

            // Heartbeat received from host containing a 3rd peer
            let peer3 = PeerInfo {
                node_id: "peer_3_nid".to_string(),
                nickname: "Charlie".to_string(),
                avatar_hash: None,
                join_timestamp: 150,
                is_host: false,
                is_muted: false,
                is_deafened: false,
            };

            let heartbeat = RoomGossipMessage::Heartbeat {
                host_id: host_id.clone(),
                timestamp: 1000,
                peer_list: vec![host_id.clone(), my_id.clone(), peer3.node_id.clone()],
                peers: vec![peer3.clone()],
            };

            session.handle_gossip_message(heartbeat).await.unwrap();

            let st = session.state.read().await;
            assert!(st.peers.iter().any(|p| p.node_id == "peer_3_nid" && p.nickname == "Charlie"));

            session.close().await;
        });
    }

    #[test]
    fn test_join_room_prepopulates_host() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let db = Database::new_in_memory().unwrap();
            let my_sk = SecretKey::generate();
            let my_id = my_sk.public().to_string();
            let host_sk = SecretKey::generate();
            let host_id = host_sk.public().to_string();

            let mgr = RoomManager::new_standalone(my_id.clone(), my_sk, db, None);
            let session = mgr.create_or_join_room("room_prepop", Some(&host_id)).await;

            let st = session.state.read().await;
            // Both local user AND host should be in state.peers
            assert_eq!(st.peers.len(), 2);
            assert!(st.peers.iter().any(|p| p.node_id == my_id && !p.is_host));
            assert!(st.peers.iter().any(|p| p.node_id == host_id && p.is_host));

            mgr.leave_room().await;
        });
    }

    #[test]
    fn test_guest_defers_host_migration_until_heartbeat_or_grace_window() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let db = Database::new_in_memory().unwrap();
            let host_id = "connecting_host_node".to_string();
            let peer1_sk = SecretKey::generate();
            let peer1_id = peer1_sk.public().to_string();

            let session = RoomSession::new(
                "room_grace_window".to_string(),
                peer1_id.clone(),
                peer1_sk,
                host_id.clone(),
                db,
                None,
                None,
            );

            session.state.write().await.add_or_update_peer(PeerInfo {
                node_id: host_id.clone(),
                nickname: "Host".to_string(),
                avatar_hash: None,
                join_timestamp: 10,
                is_host: true,
                is_muted: false,
                is_deafened: false,
            });
            session.state.write().await.add_or_update_peer(PeerInfo {
                node_id: peer1_id.clone(),
                nickname: "Guest".to_string(),
                avatar_hash: None,
                join_timestamp: 20,
                is_host: false,
                is_muted: false,
                is_deafened: false,
            });

            // Even if last_heartbeat_received looks >2000ms old, has_received_heartbeat is false
            // and we are still inside the 9s startup grace window, so host must NOT be migrated.
            *session.last_heartbeat_received.write().await =
                Instant::now() - Duration::from_millis(2500);

            tokio::time::sleep(Duration::from_millis(250)).await;

            let st = session.state.read().await;
            assert_eq!(st.host_node_id, host_id);
            assert!(st.get_peer(&host_id).is_some());
            drop(st);

            session.close().await;
        });
    }
}

