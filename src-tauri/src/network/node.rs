use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use iroh::{
    address_lookup::memory::MemoryLookup,
    endpoint::presets,
    protocol::Router,
    Endpoint, EndpointAddr, PublicKey, SecretKey, TransportAddr, Watcher,
};
pub type NodeAddr = EndpointAddr;
use iroh_blobs::{store::mem::MemStore, BlobsProtocol};
use iroh_gossip::net::Gossip;
use tauri::Emitter;

pub use crate::network::file_transfer::DM_ALPN;
pub use crate::network::room::{AudioProtocolHandler, AUDIO_ALPN};
use crate::ipc::events::{NodeNetworkStatus, RelayStatusInfo, EVENT_NODE_STATUS_CHANGED};
use crate::network::device_sync::{DynamicSyncHandler, SYNC_ALPN};
use crate::network::file_transfer::{DirectMessageHandler, DIRECT_MESSAGE_ALPN};
use crate::network::mdns::MdnsAddressLookup;
use crate::storage::db::Database;
use crate::storage::models::{Contact, LocalProfile};

/// Records the latest known transport addresses for `peer_id` into `MemoryLookup`.
pub async fn record_connected_peer(
    endpoint: &Endpoint,
    memory_lookup: &MemoryLookup,
    peer_id: PublicKey,
) {
    if let Some(info) = endpoint.remote_info(peer_id).await {
        let addrs: Vec<TransportAddr> = info.into_addrs().map(|a| a.into_addr()).collect();
        if !addrs.is_empty() {
            memory_lookup.add_endpoint_info(EndpointAddr::from_parts(peer_id, addrs));
        }
    }
}

#[cfg(target_os = "android")]
fn init_android_dns_jni_context() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        if std::panic::catch_unwind(ndk_context::android_context).is_ok() {
            // `ndk_context` is already initialized by Tauri/android-activity;
            // calling `iroh::dns::install_android_jni_context` would re-initialize it and panic.
            return;
        }
        eprintln!("Warning: ndk_context not initialized; skipping iroh Android DNS JNI context setup");
    });
}

#[derive(Clone, Default)]
pub struct DynamicAudioHandler {
    inner: Arc<RwLock<Option<AudioProtocolHandler>>>,
    endpoint: Arc<RwLock<Option<Endpoint>>>,
    memory_lookup: Option<MemoryLookup>,
}

impl std::fmt::Debug for DynamicAudioHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DynamicAudioHandler")
    }
}

impl iroh::protocol::ProtocolHandler for DynamicAudioHandler {
    fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> impl std::future::Future<Output = Result<(), iroh::protocol::AcceptError>> + Send {
        let inner = self.inner.clone();
        let endpoint = self.endpoint.clone();
        let memory_lookup = self.memory_lookup.clone();
        async move {
            if let Some(ml) = memory_lookup {
                let remote_id = connection.remote_id();
                if let Some(ep) = endpoint.read().await.clone() {
                    tokio::spawn(async move {
                        record_connected_peer(&ep, &ml, remote_id).await;
                    });
                }
            }
            let handler_opt = {
                let guard = inner.read().await;
                guard.clone()
            };
            if let Some(handler) = handler_opt {
                handler.accept(connection).await
            } else {
                Ok(())
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct DynamicDmHandler {
    inner: Arc<RwLock<Option<DirectMessageHandler>>>,
    endpoint: Arc<RwLock<Option<Endpoint>>>,
    memory_lookup: Option<MemoryLookup>,
}

impl std::fmt::Debug for DynamicDmHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DynamicDmHandler")
    }
}

impl iroh::protocol::ProtocolHandler for DynamicDmHandler {
    fn accept(
        &self,
        connection: iroh::endpoint::Connection,
    ) -> impl std::future::Future<Output = Result<(), iroh::protocol::AcceptError>> + Send {
        let inner = self.inner.clone();
        let endpoint = self.endpoint.clone();
        let memory_lookup = self.memory_lookup.clone();
        async move {
            if let Some(ml) = memory_lookup {
                let remote_id = connection.remote_id();
                if let Some(ep) = endpoint.read().await.clone() {
                    tokio::spawn(async move {
                        record_connected_peer(&ep, &ml, remote_id).await;
                    });
                }
            }
            let handler_opt = {
                let guard = inner.read().await;
                guard.clone()
            };
            if let Some(handler) = handler_opt {
                handler.accept(connection).await
            } else {
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStatus {
    Stopped,
    Starting,
    Running,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub node_id: String,
    pub nickname: String,
    pub avatar_hash: Option<String>,
}

impl From<LocalProfile> for UserProfile {
    fn from(p: LocalProfile) -> Self {
        Self {
            node_id: p.node_id,
            nickname: p.nickname,
            avatar_hash: p.avatar_hash,
        }
    }
}

pub struct InnerNodeState {
    pub status: NodeStatus,
    pub endpoint: Option<Endpoint>,
    pub router: Option<Router>,
    pub gossip: Option<Gossip>,
    pub blobs_store: Option<MemStore>,
    pub dm_handler: DynamicDmHandler,
    pub sync_handler: DynamicSyncHandler,
    pub audio_handler: DynamicAudioHandler,
    pub memory_lookup: MemoryLookup,
    pub mdns_lookup: Option<MdnsAddressLookup>,
    pub status_task: Option<tokio::task::JoinHandle<()>>,
    pub handler_endpoint: Arc<RwLock<Option<Endpoint>>>,
    pub secret_key: SecretKey,
    pub profile: LocalProfile,
    pub bypass_vpn: bool,
}


#[derive(Clone)]
pub struct NodeManager {
    db: Database,
    state: Arc<RwLock<InnerNodeState>>,
    sync_handler: DynamicSyncHandler,
    identity_sync: Arc<std::sync::RwLock<(SecretKey, String)>>,
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    network_status: Arc<RwLock<NodeNetworkStatus>>,
}

impl NodeManager {
    pub fn new(db: Database) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (secret_key, profile) = Self::load_or_create_identity(&db)?;
        let sync_handler = DynamicSyncHandler::default();
        let identity_sync = Arc::new(std::sync::RwLock::new((
            secret_key.clone(),
            profile.node_id.clone(),
        )));
        let memory_lookup = MemoryLookup::new();
        let handler_endpoint = Arc::new(RwLock::new(None));

        let dm_handler = DynamicDmHandler {
            inner: Arc::new(RwLock::new(None)),
            endpoint: handler_endpoint.clone(),
            memory_lookup: Some(memory_lookup.clone()),
        };
        let audio_handler = DynamicAudioHandler {
            inner: Arc::new(RwLock::new(None)),
            endpoint: handler_endpoint.clone(),
            memory_lookup: Some(memory_lookup.clone()),
        };

        let inner = InnerNodeState {
            status: NodeStatus::Stopped,
            endpoint: None,
            router: None,
            gossip: None,
            blobs_store: None,
            dm_handler,
            sync_handler: sync_handler.clone(),
            audio_handler,
            memory_lookup,
            mdns_lookup: None,
            status_task: None,
            handler_endpoint,
            secret_key,
            profile,
            bypass_vpn: true,
        };

        Ok(Self {
            db,
            state: Arc::new(RwLock::new(inner)),
            sync_handler,
            identity_sync,
            app_handle: Arc::new(RwLock::new(None)),
            network_status: Arc::new(RwLock::new(NodeNetworkStatus::default())),
        })
    }

    pub async fn set_app_handle(&self, app_handle: tauri::AppHandle) {
        *self.app_handle.write().await = Some(app_handle);
    }

    pub async fn get_network_status(&self) -> NodeNetworkStatus {
        self.network_status.read().await.clone()
    }

    pub async fn get_memory_lookup(&self) -> MemoryLookup {
        self.state.read().await.memory_lookup.clone()
    }

    pub async fn record_peer_connection(&self, peer_id: PublicKey) {
        let (ep_opt, ml) = {
            let state = self.state.read().await;
            (state.endpoint.clone(), state.memory_lookup.clone())
        };
        if let Some(ep) = ep_opt {
            record_connected_peer(&ep, &ml, peer_id).await;
        }
    }

    /// Returns the current `(SecretKey, node_id)` synchronously without waiting on the async state lock.
    pub fn get_identity_sync(&self) -> (SecretKey, String) {
        self.identity_sync.read().unwrap().clone()
    }

    fn load_or_create_identity(
        db: &Database,
    ) -> Result<(SecretKey, LocalProfile), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(profile) = db.get_local_profile()? {
            if profile.secret_key_bytes.len() == 32 {
                let mut key_bytes = [0u8; 32];
                key_bytes.copy_from_slice(&profile.secret_key_bytes);
                let secret_key = SecretKey::from_bytes(&key_bytes);
                return Ok((secret_key, profile));
            }
        }

        let secret_key = SecretKey::generate();
        let node_id = secret_key.public().to_string();
        let short_id = if node_id.len() >= 4 {
            &node_id[..4]
        } else {
            &node_id
        };
        let nickname = format!("User_{}", short_id);
        let key_bytes = secret_key.to_bytes().to_vec();

        let profile = LocalProfile {
            node_id: node_id.clone(),
            secret_key_bytes: key_bytes,
            nickname,
            avatar_hash: None,
        };

        db.save_local_profile(&profile)?;
        Ok((secret_key, profile))
    }

    /// Registers DM and audio protocol handlers before starting the P2P node and router.
    pub async fn start_with_handlers(
        &self,
        dm_handler: DirectMessageHandler,
        audio_handler: AudioProtocolHandler,
    ) -> Result<(), String> {
        self.set_dm_handler(dm_handler).await;
        self.set_audio_handler(audio_handler).await;
        self.start().await
    }

    /// Initializes and starts the iroh Endpoint with DHT/Pkarr discovery, LAN mDNS discovery,
    /// and iroh-gossip + iroh-blobs protocols registered.
    pub async fn start(&self) -> Result<(), String> {
        let mut state = self.state.write().await;
        self.bind_and_start_locked(&mut state).await
    }

    pub fn get_sync_handler(&self) -> DynamicSyncHandler {
        self.sync_handler.clone()
    }

    pub async fn reload_identity(&self) -> Result<UserProfile, String> {
        let (new_secret_key, new_profile) = Self::load_or_create_identity(&self.db)
            .map_err(|e| format!("Failed to load synced identity from DB: {e}"))?;

        let mut state = self.state.write().await;
        if let Some(handle) = state.status_task.take() {
            handle.abort();
        }
        if let Some(mdns) = state.mdns_lookup.take() {
            mdns.shutdown();
        }
        if let Some(router) = state.router.take() {
            let _ = router.shutdown().await;
        }
        if let Some(ep) = state.endpoint.take() {
            let _ = ep.close().await;
        }
        *state.handler_endpoint.write().await = None;
        state.status = NodeStatus::Stopped;

        state.secret_key = new_secret_key.clone();
        state.profile = new_profile.clone();
        if let Ok(mut id_guard) = self.identity_sync.write() {
            *id_guard = (new_secret_key, new_profile.node_id.clone());
        }

        self.bind_and_start_locked(&mut state).await?;

        Ok(new_profile.into())
    }

    async fn bind_and_start_locked(&self, state: &mut InnerNodeState) -> Result<(), String> {
        if let Some(handle) = state.status_task.take() {
            handle.abort();
        }
        if let Some(mdns) = state.mdns_lookup.take() {
            mdns.shutdown();
        }

        state.status = NodeStatus::Starting;
        eprintln!(
            "[VoxP2P] Starting P2P node (node_id={})",
            state.secret_key.public()
        );
        self.publish_network_status(NodeNetworkStatus {
            status: "Starting".to_string(),
            ..Default::default()
        })
        .await;

        #[cfg(target_os = "android")]
        init_android_dns_jni_context();

        let secret_key = state.secret_key.clone();
        let memory_lookup = state.memory_lookup.clone();
        let mdns_lookup = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            MdnsAddressLookup::start(secret_key.public(), memory_lookup.clone())
        })) {
            Ok(Ok(mdns)) => {
                eprintln!("[VoxP2P] LAN mDNS discovery started");
                Some(mdns)
            }
            Ok(Err(e)) => {
                eprintln!("[VoxP2P] Warning: LAN mDNS discovery unavailable: {e}");
                None
            }
            Err(_) => {
                eprintln!("[VoxP2P] Warning: LAN mDNS discovery panicked during startup; continuing without mDNS");
                None
            }
        };

        // Note: iroh 1.4.1 manages its own IpTransport UDP socket internally and does not expose
        // a stable public API to inject a pre-bound socket2::Socket (see ARCHITECTURE.md).
        let mut builder = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .address_lookup(memory_lookup);

        if let Some(ref mdns) = mdns_lookup {
            builder = builder.address_lookup(mdns.clone());
        }

        let endpoint = match builder.bind().await {
            Ok(ep) => {
                eprintln!("[VoxP2P] iroh Endpoint bound (node_id={})", ep.id());
                ep
            }
            Err(e) => {
                if let Some(mdns) = mdns_lookup {
                    mdns.shutdown();
                }
                let err_msg = format!("Failed to bind iroh endpoint: {e}");
                eprintln!("[VoxP2P] {err_msg}");
                state.status = NodeStatus::Error(err_msg.clone());
                self.publish_network_status(NodeNetworkStatus {
                    status: "Error".to_string(),
                    last_error: Some(err_msg.clone()),
                    ..Default::default()
                })
                .await;
                return Err(err_msg);
            }
        };

        // Initialize iroh-blobs protocol with in-memory store
        let blobs_store = MemStore::new();
        let blobs = BlobsProtocol::new(&blobs_store, None);

        // Initialize iroh-gossip protocol
        let gossip = Gossip::builder().spawn(endpoint.clone());

        // Spawn ALPN router for accepting blobs, gossip, direct messages, device sync, and audio datagrams
        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs)
            .accept(iroh_gossip::ALPN, gossip.clone())
            .accept(DIRECT_MESSAGE_ALPN, state.dm_handler.clone())
            .accept(SYNC_ALPN, state.sync_handler.clone())
            .accept(AUDIO_ALPN, state.audio_handler.clone())
            .spawn();

        *state.handler_endpoint.write().await = Some(endpoint.clone());
        state.endpoint = Some(endpoint.clone());
        state.router = Some(router);
        state.gossip = Some(gossip);
        state.blobs_store = Some(blobs_store);
        state.mdns_lookup = mdns_lookup;
        state.status = NodeStatus::Running;

        let initial_snapshot = build_network_status_snapshot(&endpoint, false);
        self.publish_network_status(initial_snapshot).await;

        let app_handle = self.app_handle.clone();
        let network_status = self.network_status.clone();
        state.status_task = Some(tokio::spawn(async move {
            watch_endpoint_status(endpoint, app_handle, network_status).await;
        }));

        Ok(())
    }

    async fn publish_network_status(&self, next: NodeNetworkStatus) {
        {
            let mut guard = self.network_status.write().await;
            if *guard == next {
                return;
            }
            *guard = next.clone();
        }
        if let Some(app) = self.app_handle.read().await.as_ref() {
            let _ = app.emit(EVENT_NODE_STATUS_CHANGED, &next);
        }
    }

    pub async fn set_audio_handler(&self, handler: AudioProtocolHandler) {
        let state = self.state.read().await;
        *state.audio_handler.inner.write().await = Some(handler);
    }

    pub async fn set_dm_handler(&self, handler: DirectMessageHandler) {
        let state = self.state.read().await;
        *state.dm_handler.inner.write().await = Some(handler);
    }

    pub async fn get_blobs_store(&self) -> Option<MemStore> {
        let state = self.state.read().await;
        state.blobs_store.clone()
    }

    pub async fn get_status(&self) -> NodeStatus {
        let state = self.state.read().await;
        state.status.clone()
    }

    pub async fn get_secret_key(&self) -> SecretKey {
        let state = self.state.read().await;
        state.secret_key.clone()
    }

    pub async fn get_node_id(&self) -> String {
        let state = self.state.read().await;
        state.profile.node_id.clone()
    }

    pub async fn get_endpoint(&self) -> Option<Endpoint> {
        let state = self.state.read().await;
        state.endpoint.clone()
    }

    pub async fn get_node_addr(&self) -> Option<NodeAddr> {
        let state = self.state.read().await;
        state.endpoint.as_ref().map(|ep| ep.addr())
    }

    pub async fn add_node_addr(&self, addr: NodeAddr) {
        let state = self.state.read().await;
        state.memory_lookup.add_endpoint_info(addr);
    }

    pub async fn get_gossip(&self) -> Option<Gossip> {
        let state = self.state.read().await;
        state.gossip.clone()
    }

    pub async fn get_my_profile(&self) -> UserProfile {
        let state = self.state.read().await;
        state.profile.clone().into()
    }

    pub async fn update_profile(
        &self,
        nickname: String,
        avatar_path: Option<String>,
    ) -> Result<UserProfile, String> {
        let mut state = self.state.write().await;

        let avatar_hash = if let Some(ref path) = avatar_path {
            if std::path::Path::new(path).exists() {
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                let hash = iroh_blobs::Hash::new(&bytes).to_hex().to_string();
                Some(hash)
            } else {
                avatar_path.clone()
            }
        } else {
            state.profile.avatar_hash.clone()
        };

        self.db
            .update_profile(&nickname, avatar_hash.as_deref())
            .map_err(|e| e.to_string())?;

        state.profile.nickname = nickname;
        state.profile.avatar_hash = avatar_hash;

        Ok(state.profile.clone().into())
    }

    pub async fn update_avatar_hash(
        &self,
        avatar_hash: Option<String>,
    ) -> Result<UserProfile, String> {
        let mut state = self.state.write().await;
        self.db
            .update_profile(&state.profile.nickname, avatar_hash.as_deref())
            .map_err(|e| e.to_string())?;

        state.profile.avatar_hash = avatar_hash;
        Ok(state.profile.clone().into())
    }

    pub async fn add_contact(&self, node_id: String) -> Result<Contact, String> {
        // Validate that node_id is a valid Ed25519 public key
        let peer_pk = PublicKey::from_str(&node_id)
            .map_err(|e| format!("Invalid node ID format: {e}"))?;

        let short_id = if node_id.len() >= 4 {
            &node_id[..4]
        } else {
            &node_id
        };
        let nickname = format!("Peer_{}", short_id);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let contact = Contact {
            node_id: node_id.clone(),
            nickname,
            avatar_path: None,
            custom_volume: 1.0,
            last_seen: now,
        };

        self.db
            .insert_or_update_contact(&contact)
            .map_err(|e| e.to_string())?;

        self.record_peer_connection(peer_pk).await;

        Ok(contact)
    }

    pub async fn set_bypass_vpn(&self, bypass: bool) {
        let mut state = self.state.write().await;
        state.bypass_vpn = bypass;
    }
}

fn build_network_status_snapshot(endpoint: &Endpoint, online_completed: bool) -> NodeNetworkStatus {
    let addr = endpoint.addr();
    let mut direct_addrs: Vec<String> = addr.ip_addrs().map(|a| a.to_string()).collect();
    direct_addrs.sort();
    direct_addrs.dedup();

    let mut home_relay_url = addr.relay_urls().next().map(|u| u.to_string());

    let mut relay_watcher = endpoint.home_relay_status();
    let relay_statuses = relay_watcher.get();
    let relays: Vec<RelayStatusInfo> = relay_statuses
        .iter()
        .map(|r| RelayStatusInfo {
            url: r.url().to_string(),
            is_connected: r.is_connected(),
            last_error: r.last_error().map(|e| format!("{e:#}")),
        })
        .collect();

    let relay_connected = relays.iter().any(|r| r.is_connected);
    if home_relay_url.is_none() {
        home_relay_url = relays.first().map(|r| r.url.clone());
    }
    let last_error = if relay_connected {
        None
    } else {
        relays.iter().find_map(|r| r.last_error.clone())
    };

    NodeNetworkStatus {
        status: "Running".to_string(),
        is_online: online_completed || relay_connected,
        relay_connected,
        home_relay_url,
        direct_addrs,
        relays,
        last_error,
    }
}

async fn watch_endpoint_status(
    endpoint: Endpoint,
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    network_status: Arc<RwLock<NodeNetworkStatus>>,
) {
    let mut addr_watcher = endpoint.watch_addr();
    let mut relay_watcher = endpoint.home_relay_status();
    let mut online_completed = false;
    let online_fut = endpoint.online();
    tokio::pin!(online_fut);

    loop {
        tokio::select! {
            _ = &mut online_fut, if !online_completed => {
                online_completed = true;
            }
            res = addr_watcher.updated() => {
                if res.is_err() {
                    break;
                }
            }
            res = relay_watcher.updated() => {
                if res.is_err() {
                    break;
                }
            }
            _ = endpoint.closed() => {
                break;
            }
        }

        let snapshot = build_network_status_snapshot(&endpoint, online_completed);
        let changed = {
            let mut guard = network_status.write().await;
            if *guard != snapshot {
                *guard = snapshot.clone();
                true
            } else {
                false
            }
        };

        if changed {
            if let Some(app) = app_handle.read().await.as_ref() {
                let _ = app.emit(EVENT_NODE_STATUS_CHANGED, &snapshot);
            }
        }
    }
}
