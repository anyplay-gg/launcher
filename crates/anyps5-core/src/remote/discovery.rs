//! Telling phones on the network that this PC is here: mDNS service `_anyps5-remote._tcp`, no TXT
//! records, the control port in SRV (what the macOS launcher advertises).
//!
//! A PC often has virtual adapters (Hyper-V, WSL, Docker, VPN) and the phone takes the first IPv4
//! address it resolves, so only the address of the real LAN adapter is announced.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, UdpSocket},
};

use mdns_sd::{IfKind, ServiceDaemon, ServiceInfo};

use super::security::is_allowed_peer;

pub const SERVICE_TYPE: &str = "_anyps5-remote._tcp.local.";

const VIRTUAL_NAMES: &[&str] = &[
    "vethernet",
    "wsl",
    "hyper-v",
    "docker",
    "vmware",
    "vmnet",
    "virtualbox",
    "vboxnet",
    "veth",
    "br-",
    "virbr",
    "tailscale",
    "zerotier",
    "tun",
    "tap",
    "vpn",
    "loopback",
    "bluetooth",
    "npcap",
    "podman",
    "lxc",
    "utun",
];

/// Whether an adapter name belongs to a virtual network (VM, container, VPN, Bluetooth).
pub(super) fn is_virtual_adapter(name: &str) -> bool {
    let lower = name.to_lowercase();
    VIRTUAL_NAMES.iter().any(|v| lower.contains(v))
}

/// Picks the IPv4 addresses worth announcing from `(adapter name, address)` pairs: private,
/// not link-local, not a virtual adapter. The address that would be used to reach the internet
/// comes first, when it is one of them.
pub fn choose_addresses(
    adapters: &[(String, Ipv4Addr)],
    primary: Option<Ipv4Addr>,
) -> Vec<Ipv4Addr> {
    let mut chosen: Vec<Ipv4Addr> = adapters
        .iter()
        .filter(|(name, ip)| {
            !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_unspecified()
                && is_allowed_peer(IpAddr::V4(*ip))
                && !is_virtual_adapter(name)
        })
        .map(|(_, ip)| *ip)
        .collect();
    chosen.dedup();
    if let Some(primary) = primary
        && let Some(at) = chosen.iter().position(|ip| *ip == primary)
    {
        chosen.swap(0, at);
    }
    chosen
}

/// The local address the operating system would use towards the internet. Connecting a UDP socket
/// sends nothing; it only makes the system pick a route.
pub fn primary_address() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) => Some(ip),
        IpAddr::V6(_) => None,
    }
}

pub fn lan_addresses() -> Vec<Ipv4Addr> {
    let adapters: Vec<(String, Ipv4Addr)> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.ip() {
            IpAddr::V4(ip) => Some((i.name, ip)),
            IpAddr::V6(_) => None,
        })
        .collect();
    choose_addresses(&adapters, primary_address())
}

pub struct Discovery {
    daemon: ServiceDaemon,
    pub addresses: Vec<Ipv4Addr>,
}

impl Discovery {
    pub fn start(instance: &str, port: u16) -> Result<Discovery, String> {
        let addresses = lan_addresses();
        if addresses.is_empty() {
            return Err("no private network address to announce".into());
        }
        let daemon = ServiceDaemon::new().map_err(|e| e.to_string())?;
        // Only the chosen adapters take part; the rest stay silent.
        daemon
            .disable_interface(IfKind::All)
            .map_err(|e| e.to_string())?;
        for ip in &addresses {
            daemon
                .enable_interface(IfKind::Addr(IpAddr::V4(*ip)))
                .map_err(|e| e.to_string())?;
        }
        let host = format!("{}.local.", host_label(instance));
        let ips: Vec<IpAddr> = addresses.iter().map(|ip| IpAddr::V4(*ip)).collect();
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            instance,
            &host,
            &ips[..],
            port,
            None::<HashMap<String, String>>,
        )
        .map_err(|e| e.to_string())?;
        daemon.register(info).map_err(|e| e.to_string())?;
        Ok(Discovery { daemon, addresses })
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        let _ = self.daemon.shutdown();
    }
}

/// A DNS host label from a computer name: letters, digits and hyphens.
pub fn host_label(name: &str) -> String {
    let label: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let label = label.trim_matches('-');
    if label.is_empty() {
        "anyps5-pc".into()
    } else {
        label.chars().take(60).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> Ipv4Addr {
        text.parse().unwrap()
    }

    #[test]
    fn virtual_and_public_adapters_are_not_announced() {
        let adapters = vec![
            ("vEthernet (WSL)".to_string(), ip("172.24.16.1")),
            ("Ethernet".to_string(), ip("192.168.178.20")),
            ("Wi-Fi".to_string(), ip("10.0.0.7")),
            (
                "VMware Network Adapter VMnet8".to_string(),
                ip("192.168.60.1"),
            ),
            ("Ethernet 2".to_string(), ip("169.254.9.9")),
            ("Loopback".to_string(), ip("127.0.0.1")),
            ("Modem".to_string(), ip("84.12.0.3")),
        ];
        assert_eq!(
            choose_addresses(&adapters, Some(ip("10.0.0.7"))),
            vec![ip("10.0.0.7"), ip("192.168.178.20")]
        );
        assert_eq!(
            choose_addresses(&adapters, None),
            vec![ip("192.168.178.20"), ip("10.0.0.7")]
        );
    }

    #[test]
    fn host_labels_are_dns_safe() {
        assert_eq!(host_label("Alec's PC"), "Alec-s-PC");
        assert_eq!(host_label("***"), "anyps5-pc");
    }
}
