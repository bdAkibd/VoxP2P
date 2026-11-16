use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use socket2::{Domain, Protocol, Socket, Type};

/// Represents information about a detected local network interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalInterface {
    pub index: u32,
    pub name: String,
    pub is_physical: bool,
}

/// Binds a UDP socket directly to a physical network interface to bypass virtual TUN/TAP VPN adapters.
pub fn bind_physical_udp_socket(interface_index: u32, bind_port: u16) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_nonblocking(true)?;
    socket.set_reuse_address(true)?;

    #[cfg(target_os = "windows")]
    {
        apply_windows_unicast_if(&socket, interface_index)?;
    }

    #[cfg(target_os = "linux")]
    {
        apply_linux_bind_device(&socket, interface_index)?;
    }

    #[cfg(target_os = "macos")]
    {
        apply_macos_bound_if(&socket, interface_index)?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        let _ = interface_index;
    }

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
fn apply_linux_bind_device(socket: &Socket, interface_index: u32) -> std::io::Result<()> {
    let device_name = format!("if{}", interface_index);
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
