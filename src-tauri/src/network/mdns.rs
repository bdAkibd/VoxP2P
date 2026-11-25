use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use iroh::address_lookup::memory::MemoryLookup;
use iroh::address_lookup::{AddressLookup, EndpointData};
use iroh::{EndpointAddr, PublicKey, RelayUrl, TransportAddr};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::task::JoinHandle;

const MDNS_SERVICE_TYPE: &str = "_voxp2p._udp.local.";
const TXT_NODE_ID_KEY: &str = "node_id";
const TXT_RELAY_KEY: &str = "relay";

/// Parses mDNS service discovery attributes into an `EndpointAddr` for `MemoryLookup`.
/// Returns `None` if `peer_id_str` is invalid, matches `local_node_id`, or yields no usable addresses.
pub fn parse_mdns_endpoint_addr(
    local_node_id: PublicKey,
    peer_id_str: &str,
    ips: impl IntoIterator<Item = IpAddr>,
    port: u16,
    relay_url: Option<&str>,
) -> Option<EndpointAddr> {
    let peer_id = PublicKey::from_str(peer_id_str).ok()?;
    if peer_id == local_node_id {
        return None;
    }

    let mut transport_addrs: Vec<TransportAddr> = Vec::new();
    if port > 0 {
        for ip in ips {
            if !ip.is_unspecified() && !ip.is_multicast() {
                transport_addrs.push(TransportAddr::Ip(SocketAddr::new(ip, port)));
            }
        }
    }

    if let Some(relay) = relay_url {
        if let Ok(url) = RelayUrl::from_str(relay) {
            transport_addrs.push(TransportAddr::Relay(url));
        }
    }

    if transport_addrs.is_empty() {
        return None;
    }

    Some(EndpointAddr::from_parts(peer_id, transport_addrs))
}

/// Builds a `mdns_sd::ServiceInfo` advertising `node_id` and local socket addresses.
pub fn build_mdns_service_info(
    node_id: PublicKey,
    addrs: &[SocketAddr],
    relay_url: Option<&RelayUrl>,
) -> Result<Option<ServiceInfo>, String> {
    let valid_addrs: Vec<SocketAddr> = addrs
        .iter()
        .copied()
        .filter(|a| !a.ip().is_unspecified() && !a.ip().is_multicast() && a.port() > 0)
        .collect();

    // Prefer a non-loopback IPv4 address for the primary port if available
    let primary_port = valid_addrs
        .iter()
        .find(|a| a.is_ipv4() && !a.ip().is_loopback())
        .or_else(|| valid_addrs.first())
        .map(|a| a.port());

    let Some(port) = primary_port else {
        return Ok(None);
    };

    let ips: Vec<IpAddr> = valid_addrs
        .iter()
        .filter(|a| a.port() == port)
        .map(|a| a.ip())
        .collect();

    let node_id_str = node_id.to_string();
    let short_id = &node_id_str[..node_id_str.len().min(12)];
    let instance_name = format!("vox-{short_id}");
    let host_name = format!("vox-{short_id}.local.");

    let mut properties: Vec<(&str, String)> = vec![(TXT_NODE_ID_KEY, node_id_str)];
    if let Some(url) = relay_url {
        let url_str = url.to_string();
        // mDNS TXT key+value must stay under 255 bytes
        if TXT_RELAY_KEY.len() + url_str.len() < 250 {
            properties.push((TXT_RELAY_KEY, url_str));
        }
    }
    let prop_refs: Vec<(&str, &str)> = properties.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let mut info = ServiceInfo::new(
        MDNS_SERVICE_TYPE,
        &instance_name,
        &host_name,
        &ips[..],
        port,
        &prop_refs[..],
    )
    .map_err(|e| format!("Failed to create mDNS ServiceInfo: {e}"))?;

    if ips.is_empty() {
        info = info.enable_addr_auto();
    }

    Ok(Some(info))
}

/// LAN mDNS address lookup service backed by `mdns-sd` that publishes this node's
/// `NodeId` and local `SocketAddr`s and feeds discovered peers into `MemoryLookup`.
#[derive(Clone)]
pub struct MdnsAddressLookup {
    local_node_id: PublicKey,
    daemon: ServiceDaemon,
    last_published: Arc<Mutex<Option<(Vec<SocketAddr>, Option<String>)>>>,
    browse_task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl std::fmt::Debug for MdnsAddressLookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MdnsAddressLookup")
            .field("local_node_id", &self.local_node_id)
            .finish()
    }
}

impl MdnsAddressLookup {
    pub fn start(local_node_id: PublicKey, memory_lookup: MemoryLookup) -> Result<Self, String> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| format!("Failed to start mDNS daemon: {e}"))?;

        let receiver = daemon
            .browse(MDNS_SERVICE_TYPE)
            .map_err(|e| format!("Failed to browse mDNS service {MDNS_SERVICE_TYPE}: {e}"))?;

        let browse_task = tokio::spawn(async move {
            while let Ok(event) = receiver.recv_async().await {
                if let ServiceEvent::ServiceResolved(resolved) = event {
                    let Some(peer_id_str) = resolved.txt_properties.get_property_val_str(TXT_NODE_ID_KEY) else {
                        continue;
                    };
                    let relay_str = resolved.txt_properties.get_property_val_str(TXT_RELAY_KEY);
                    let ips = resolved.get_addresses().iter().map(|s| s.to_ip_addr());
                    if let Some(addr) = parse_mdns_endpoint_addr(
                        local_node_id,
                        peer_id_str,
                        ips,
                        resolved.get_port(),
                        relay_str,
                    ) {
                        memory_lookup.add_endpoint_info(addr);
                    }
                }
            }
        });

        Ok(Self {
            local_node_id,
            daemon,
            last_published: Arc::new(Mutex::new(None)),
            browse_task: Arc::new(Mutex::new(Some(browse_task))),
        })
    }

    pub fn shutdown(&self) {
        if let Ok(mut guard) = self.browse_task.lock() {
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }
        let _ = self.daemon.shutdown();
    }
}

impl AddressLookup for MdnsAddressLookup {
    fn publish(&self, data: &EndpointData) {
        let mut addrs: Vec<SocketAddr> = data.ip_addrs().copied().collect();
        addrs.sort();
        addrs.dedup();
        let relay = data.relay_urls().next().cloned();
        let relay_key = relay.as_ref().map(|u| u.to_string());

        {
            let mut guard = self.last_published.lock().unwrap();
            if let Some((prev_addrs, prev_relay)) = guard.as_ref() {
                if prev_addrs == &addrs && prev_relay == &relay_key {
                    return;
                }
            }
            *guard = Some((addrs.clone(), relay_key));
        }

        match build_mdns_service_info(self.local_node_id, &addrs, relay.as_ref()) {
            Ok(Some(info)) => {
                if let Err(e) = self.daemon.register(info) {
                    eprintln!("Warning: mDNS service registration failed: {e}");
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("Warning: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use iroh::SecretKey;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[tokio::test]
    async fn test_parse_mdns_endpoint_addr_populates_memory_lookup() {
        let local_sk = SecretKey::generate();
        let peer_sk = SecretKey::generate();
        let local_id = local_sk.public();
        let peer_id = peer_sk.public();

        // Self announcements must be ignored
        assert!(parse_mdns_endpoint_addr(
            local_id,
            &local_id.to_string(),
            [IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))],
            5000,
            None,
        )
        .is_none());

        // Invalid node_id must be ignored
        assert!(parse_mdns_endpoint_addr(
            local_id,
            "not-a-valid-key",
            [IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))],
            5000,
            None,
        )
        .is_none());

        // Valid peer with unspecified + LAN IPv4 + IPv6 addresses
        let addr = parse_mdns_endpoint_addr(
            local_id,
            &peer_id.to_string(),
            [
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42)),
                IpAddr::V6(Ipv6Addr::LOCALHOST),
            ],
            41234,
            None,
        )
        .expect("should parse valid mDNS peer");

        assert_eq!(addr.id, peer_id);
        let ip_addrs: Vec<SocketAddr> = addr.ip_addrs().copied().collect();
        assert_eq!(ip_addrs.len(), 2);
        assert!(ip_addrs.contains(&SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42)), 41234)));
        assert!(ip_addrs.contains(&SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 41234)));

        // Insert into MemoryLookup and verify resolution
        let lookup = MemoryLookup::new();
        lookup.add_endpoint_info(addr);

        let mut stream = lookup
            .resolve(peer_id)
            .expect("MemoryLookup should return a stream for known peer");
        let item = stream
            .next()
            .await
            .expect("stream should yield an item")
            .expect("item should be Ok");

        let resolved_ips: Vec<SocketAddr> = item.endpoint_info().ip_addrs().copied().collect();
        assert!(resolved_ips.contains(&SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42)), 41234)));
    }

    #[test]
    fn test_build_mdns_service_info_formats_txt_and_addresses() {
        let sk = SecretKey::generate();
        let node_id = sk.public();
        let addrs = vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 15)), 51515),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 51515),
        ];

        let info = build_mdns_service_info(node_id, &addrs, None)
            .expect("build_mdns_service_info should not error")
            .expect("should produce ServiceInfo");

        assert_eq!(info.get_port(), 51515);
        assert_eq!(
            info.get_property_val_str(TXT_NODE_ID_KEY),
            Some(node_id.to_string().as_str())
        );
        assert!(info.get_addresses().contains(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 15))));
        assert!(!info.get_addresses().contains(&IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
    }
}
