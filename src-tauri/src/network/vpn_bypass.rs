use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use socket2::{Domain, Protocol, Socket, Type};

const VIRTUAL_ADAPTER_KEYWORDS: &[&str] = &[
    "tun",
    "tap",
    "wintun",
    "wireguard",
    "wg",
    "nekobox",
    "hamachi",
    "radmin",
];

/// Represents information about a detected local network interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalInterface {
    pub index: u32,
    pub name: String,
    pub friendly_name: Option<String>,
    pub is_physical: bool,
}

/// Checks if an interface name or friendly name corresponds to a known virtual VPN/tunnel adapter.
pub fn is_virtual_adapter(name: &str, friendly_name: Option<&str>) -> bool {
    let lower_name = name.to_ascii_lowercase();
    for kw in VIRTUAL_ADAPTER_KEYWORDS {
        if lower_name.contains(kw) {
            return true;
        }
    }
    if let Some(fname) = friendly_name {
        let lower_fname = fname.to_ascii_lowercase();
        for kw in VIRTUAL_ADAPTER_KEYWORDS {
            if lower_fname.contains(kw) {
                return true;
            }
        }
    }
    false
}

/// Detects the default physical network interface, ignoring virtual adapters matching
/// `tun`, `tap`, `wintun`, `wireguard`, `nekobox`, `hamachi`, `radmin`.
#[cfg(not(target_os = "android"))]
pub fn detect_default_physical_interface() -> Option<PhysicalInterface> {
    let interfaces = netdev::get_interfaces();

    // First try the default interface if netdev identifies one
    if let Ok(default_iface) = netdev::get_default_interface() {
        if !is_virtual_adapter(&default_iface.name, default_iface.friendly_name.as_deref())
            && !default_iface.is_loopback()
            && default_iface.is_up()
        {
            return Some(PhysicalInterface {
                index: default_iface.index,
                name: default_iface.name,
                friendly_name: default_iface.friendly_name,
                is_physical: true,
            });
        }
    }

    // Fall back to scanning all interfaces
    for iface in interfaces {
        if iface.is_loopback() || !iface.is_up() {
            continue;
        }

        if is_virtual_adapter(&iface.name, iface.friendly_name.as_deref()) {
            continue;
        }

        // Must have at least one IPv4 address
        if iface.ipv4.is_empty() {
            continue;
        }

        return Some(PhysicalInterface {
            index: iface.index,
            name: iface.name,
            friendly_name: iface.friendly_name,
            is_physical: true,
        });
    }

    None
}

/// On Android, raw interface indexing and netlink scanning are bypassed
/// to prevent permission faults and panics within the mobile sandbox.
#[cfg(target_os = "android")]
pub fn detect_default_physical_interface() -> Option<PhysicalInterface> {
    None
}

/// Creates and binds a UDP socket. If `bypass_vpn` is true, attempts to bind directly
/// to the physical network adapter to bypass virtual VPN tunnels.
#[cfg(not(target_os = "android"))]
pub fn create_udp_socket(bind_port: u16, bypass_vpn: bool) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_nonblocking(true)?;
    socket.set_reuse_address(true)?;

    if bypass_vpn {
        if let Some(physical_if) = detect_default_physical_interface() {
            #[cfg(target_os = "windows")]
            {
                apply_windows_unicast_if(&socket, physical_if.index)?;
            }

            #[cfg(target_os = "linux")]
            {
                apply_linux_bind_device(&socket, &physical_if.name)?;
            }

            #[cfg(target_os = "macos")]
            {
                apply_macos_bound_if(&socket, physical_if.index)?;
            }

            #[cfg(not(any(
                target_os = "windows",
                target_os = "linux",
                target_os = "macos"
            )))]
            {
                let _ = physical_if;
            }
        }
    }

    let bind_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, bind_port));
    socket.bind(&bind_addr.into())?;

    Ok(socket.into())
}

/// On Android, creates a standard bound non-blocking UDP socket.
/// VPN tunnel exclusion on Android requires VpnService.protect() socket file-descriptor
/// pass-through via Android Java SDK, so raw socket binding is bypassed safely.
#[cfg(target_os = "android")]
pub fn create_udp_socket(bind_port: u16, _bypass_vpn: bool) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_nonblocking(true)?;
    socket.set_reuse_address(true)?;
    let bind_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, bind_port));
    socket.bind(&bind_addr.into())?;
    Ok(socket.into())
}

#[cfg(target_os = "windows")]
fn apply_windows_unicast_if(socket: &Socket, interface_index: u32) -> std::io::Result<()> {
    use std::os::windows::io::AsRawSocket;
    const IPPROTO_IP: i32 = 0;
    const IP_UNICAST_IF: i32 = 31;

    let net_order_index = interface_index.to_be();
    let optval = &net_order_index as *const u32 as *const i8;
    let optlen = std::mem::size_of::<u32>() as i32;

    unsafe extern "system" {
        fn setsockopt(
            s: usize,
            level: i32,
            optname: i32,
            optval: *const i8,
            optlen: i32,
        ) -> i32;
    }

    let ret = unsafe {
        setsockopt(
            socket.as_raw_socket() as usize,
            IPPROTO_IP,
            IP_UNICAST_IF,
            optval,
            optlen,
        )
    };

    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn apply_linux_bind_device(socket: &Socket, device_name: &str) -> std::io::Result<()> {
    socket.bind_device(Some(device_name.as_bytes()))
}

#[cfg(target_os = "macos")]
fn apply_macos_bound_if(socket: &Socket, interface_index: u32) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    const IPPROTO_IP: i32 = 0;
    const IP_BOUND_IF: i32 = 25;

    let optval = &interface_index as *const u32 as *const libc::c_void;
    let optlen = std::mem::size_of::<u32>() as libc::socklen_t;

    unsafe extern "C" {
        fn setsockopt(
            sockfd: i32,
            level: i32,
            optname: i32,
            optval: *const libc::c_void,
            optlen: libc::socklen_t,
        ) -> i32;
    }

    let ret = unsafe {
        setsockopt(
            socket.as_raw_fd(),
            IPPROTO_IP,
            IP_BOUND_IF,
            optval,
            optlen,
        )
    };

    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_virtual_adapter() {
        assert!(is_virtual_adapter("wintun-adapter", None));
        assert!(is_virtual_adapter("Ethernet 2", Some("TAP-Windows Adapter V9")));
        assert!(is_virtual_adapter("wg0", None));
        assert!(is_virtual_adapter("NekoBox Virtual Tun", None));
        assert!(is_virtual_adapter("hamachi0", None));
        assert!(is_virtual_adapter("radmin vpn", None));

        assert!(!is_virtual_adapter("eth0", Some("Intel(R) Ethernet Connection")));
        assert!(!is_virtual_adapter("Wi-Fi", Some("Intel(R) Wi-Fi 6 AX200")));
    }
}
