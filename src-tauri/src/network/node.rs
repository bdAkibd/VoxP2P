use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use iroh::{
    address_lookup::memory::MemoryLookup,
    endpoint::presets,
    protocol::Router,
    Endpoint, EndpointAddr, PublicKey, SecretKey,
};
pub type NodeAddr = EndpointAddr;
use iroh_blobs::{store::mem::MemStore, BlobsProtocol};
use iroh_gossip::net::Gossip;

pub use crate::network::file_transfer::DM_ALPN;
use crate::network::device_sync::{DynamicSyncHandler, SYNC_ALPN};
use crate::network::file_transfer::{DirectMessageHandler, DIRECT_MESSAGE_ALPN};
use crate::network::vpn_bypass::detect_default_physical_interface;
use crate::storage::db::Database;
use crate::storage::models::{Contact, LocalProfile};

#[derive(Clone, Default)]
pub struct DynamicDmHandler {
    inner: Arc<RwLock<Option<DirectMessageHandler>>>,
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
        async move {
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
    pub memory_lookup: MemoryLookup,
    pub secret_key: SecretKey,
    pub profile: LocalProfile,
    pub bypass_vpn: bool,
}


#[derive(Clone)]
pub struct NodeManager {
    db: Database,
    state: Arc<RwLock<InnerNodeState>>,
    sync_handler: DynamicSyncHandler,
}

impl NodeManager {
    pub fn new(db: Database) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (secret_key, profile) = Self::load_or_create_identity(&db)?;
        let sync_handler = DynamicSyncHandler::default();

        let inner = InnerNodeState {
            status: NodeStatus::Stopped,
            endpoint: None,
            router: None,
            gossip: None,
            blobs_store: None,
            dm_handler: DynamicDmHandler::default(),
            sync_handler: sync_handler.clone(),
            memory_lookup: MemoryLookup::new(),
            secret_key,
            profile,
            bypass_vpn: true,
        };

        Ok(Self {
            db,
            state: Arc::new(RwLock::new(inner)),
            sync_handler,
        })
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

    /// Initializes and starts the iroh Endpoint with DHT/Pkarr discovery enabled
    /// and iroh-gossip + iroh-blobs protocols registered.
    pub async fn start(&self) -> Result<(), String> {
        let mut state = self.state.write().await;
        state.status = NodeStatus::Starting;

        let secret_key = state.secret_key.clone();
        let bypass_vpn = state.bypass_vpn;

        let memory_lookup = state.memory_lookup.clone();
        // Build the Endpoint with N0 preset (Pkarr/DNS discovery + relay fallback) + in-memory lookup
        let builder = Endpoint::builder(presets::N0)
            .secret_key(secret_key)
            .address_lookup(memory_lookup);

        if bypass_vpn {
            if let Some(_iface) = detect_default_physical_interface() {
                // Physical interface detected for direct P2P binding
            }
        }

        let endpoint = match builder.bind().await {
            Ok(ep) => ep,
            Err(e) => {
                let err_msg = format!("Failed to bind iroh endpoint: {e}");
                state.status = NodeStatus::Error(err_msg.clone());
                return Err(err_msg);
            }
        };

        // Initialize iroh-blobs protocol with in-memory store
        let blobs_store = MemStore::new();
        let blobs = BlobsProtocol::new(&blobs_store, None);

        // Initialize iroh-gossip protocol
        let gossip = Gossip::builder().spawn(endpoint.clone());

        // Spawn ALPN router for accepting blobs, gossip, direct messages, and device sync
        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs)
            .accept(iroh_gossip::ALPN, gossip.clone())
            .accept(DIRECT_MESSAGE_ALPN, state.dm_handler.clone())
            .accept(SYNC_ALPN, state.sync_handler.clone())
            .spawn();

        state.endpoint = Some(endpoint);
        state.router = Some(router);
        state.gossip = Some(gossip);
        state.blobs_store = Some(blobs_store);
        state.status = NodeStatus::Running;

        Ok(())
    }

    pub fn get_sync_handler(&self) -> DynamicSyncHandler {
        self.sync_handler.clone()
    }

    pub async fn reload_identity(&self) -> Result<UserProfile, String> {
        let (new_secret_key, new_profile) = Self::load_or_create_identity(&self.db)
            .map_err(|e| format!("Failed to load synced identity from DB: {e}"))?;

        let mut state = self.state.write().await;
        if let Some(router) = state.router.take() {
            let _ = router.shutdown().await;
        }
        if let Some(ep) = state.endpoint.take() {
            let _ = ep.close().await;
        }
        state.status = NodeStatus::Stopped;

        state.secret_key = new_secret_key.clone();
        state.profile = new_profile.clone();

        let memory_lookup = state.memory_lookup.clone();
        let builder = Endpoint::builder(presets::N0)
            .secret_key(new_secret_key)
            .address_lookup(memory_lookup);

        let endpoint = builder
            .bind()
            .await
            .map_err(|e| format!("Failed to bind new endpoint: {e}"))?;

        let blobs_store = MemStore::new();
        let blobs = BlobsProtocol::new(&blobs_store, None);
        let gossip = Gossip::builder().spawn(endpoint.clone());

        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs)
            .accept(iroh_gossip::ALPN, gossip.clone())
            .accept(DIRECT_MESSAGE_ALPN, state.dm_handler.clone())
            .accept(SYNC_ALPN, state.sync_handler.clone())
            .spawn();

        state.endpoint = Some(endpoint);
        state.router = Some(router);
        state.gossip = Some(gossip);
        state.blobs_store = Some(blobs_store);
        state.status = NodeStatus::Running;

        Ok(new_profile.into())
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

    pub async fn add_contact(&self, node_id: String) -> Result<Contact, String> {
        // Validate that node_id is a valid Ed25519 public key
        PublicKey::from_str(&node_id)
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

        Ok(contact)
    }

    pub async fn set_bypass_vpn(&self, bypass: bool) {
        let mut state = self.state.write().await;
        state.bypass_vpn = bypass;
    }
}
