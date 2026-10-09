use nix::sys::socket::{
    bind, recv, socket, AddressFamily, NetlinkAddr, SockFlag, SockProtocol, SockType,
};
use std::os::fd::{AsRawFd, OwnedFd};
use wattwarden_core::{Result, WattWardenError};

pub struct NetlinkUeventListener {
    fd: OwnedFd,
}

impl NetlinkUeventListener {
    pub fn new() -> Result<Self> {
        let fd = socket(
            AddressFamily::Netlink,
            SockType::Raw,
            SockFlag::SOCK_CLOEXEC,
            SockProtocol::NetlinkKObjectUEvent,
        )
        .map_err(|e| WattWardenError::Ipc(format!("Failed to create netlink socket: {}", e)))?;

        // Group 1 is the kernel multicast group for kobject_uevent
        let addr = NetlinkAddr::new(0, 1);
        bind(fd.as_raw_fd(), &addr)
            .map_err(|e| WattWardenError::Ipc(format!("Failed to bind netlink socket: {}", e)))?;

        Ok(Self { fd })
    }

    /// Blocks until a relevant power_supply uevent occurs
    pub fn wait_for_power_event(&self) -> Result<String> {
        let mut buf = [0u8; 4096];
        loop {
            let bytes_read = recv(
                self.fd.as_raw_fd(),
                &mut buf,
                nix::sys::socket::MsgFlags::empty(),
            )
            .map_err(|e| WattWardenError::Ipc(format!("Netlink recv error: {}", e)))?;

            if bytes_read > 0 {
                let msg = String::from_utf8_lossy(&buf[..bytes_read]);
                if msg.contains("SUBSYSTEM=power_supply") {
                    return Ok(msg.to_string());
                }
            }
        }
    }
}
