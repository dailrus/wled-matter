//! Built-in mDNS responder bound to an explicitly named interface.
//!
//! The station's wlan0 has only a link-local IPv6 address, which `if-addrs` does not report on
//! musl, so IPv6 addresses and the interface index are read from /proc and /sys directly.

use std::net::UdpSocket;

use log::info;

use rs_matter::crypto::Crypto;
use rs_matter::error::{Error, ErrorCode};
use rs_matter::transport::network::mdns::builtin::{BuiltinMdns, Host};
use rs_matter::transport::network::mdns::{
    MDNS_IPV4_BROADCAST_ADDR, MDNS_IPV6_BROADCAST_ADDR, MDNS_SOCKET_DEFAULT_BIND_ADDR,
};
use rs_matter::transport::network::{Ipv4Addr, Ipv6Addr};
use rs_matter::Matter;

use socket2::{Domain, Protocol, Socket, Type};

fn ipv4_of(iface: &str) -> Option<std::net::Ipv4Addr> {
    if_addrs::get_if_addrs().ok()?.into_iter().find_map(|ia| match ia.addr {
        if_addrs::IfAddr::V4(v4) if ia.name == iface => Some(v4.ip),
        _ => None,
    })
}

/// Lines of /proc/net/if_inet6: `<32 hex addr> <idx> <plen> <scope> <flags> <name>`
fn ipv6_of(iface: &str) -> Vec<Ipv6Addr> {
    let text = std::fs::read_to_string("/proc/net/if_inet6").unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() < 6 || cols[5] != iface || cols[0].len() != 32 {
                return None;
            }
            let mut octets = [0u8; 16];
            for (i, o) in octets.iter_mut().enumerate() {
                *o = u8::from_str_radix(&cols[0][i * 2..i * 2 + 2], 16).ok()?;
            }
            Some(octets.into())
        })
        .collect()
}

fn sys(iface: &str, file: &str) -> Option<String> {
    std::fs::read_to_string(format!("/sys/class/net/{iface}/{file}"))
        .ok()
        .map(|s| s.trim().to_string())
}

pub async fn run_mdns<C: Crypto>(
    matter: &Matter<'_>,
    crypto: C,
    iface: &str,
    node: u8,
) -> Result<(), Error> {
    let ipv4 = ipv4_of(iface).ok_or_else(|| {
        log::error!("Interface {iface} has no IPv4 address");
        ErrorCode::StdIoError
    })?;
    let ipv6_addrs = ipv6_of(iface);
    let index: u32 = sys(iface, "ifindex").and_then(|s| s.parse().ok()).unwrap_or(0);
    // Hostname must be unique on the LAN: interface MAC + device index (several nodes run here)
    let hostname = format!(
        "{}{node:02X}",
        sys(iface, "address")
            .map(|mac| mac.replace(':', "").to_uppercase())
            .unwrap_or_else(|| "WLEDMATTER".to_string())
    );

    info!("mDNS on {iface} (#{index}) {ipv4} / {ipv6_addrs:?} as {hostname}");

    let ipv4_addr: Ipv4Addr = ipv4.octets().into();

    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_only_v6(false)?;
    socket.bind(&MDNS_SOCKET_DEFAULT_BIND_ADDR.into())?;
    let socket = async_io::Async::<UdpSocket>::new_nonblocking(socket.into())?;

    if !ipv6_addrs.is_empty() {
        socket.get_ref().join_multicast_v6(&MDNS_IPV6_BROADCAST_ADDR, index)?;
    }
    socket.get_ref().join_multicast_v4(&MDNS_IPV4_BROADCAST_ADDR, &ipv4_addr)?;

    BuiltinMdns::new()
        .run(
            &socket,
            &socket,
            &Host {
                hostname: &hostname,
                ip: ipv4_addr,
                ipv6: &ipv6_addrs,
            },
            Some(ipv4_addr),
            Some(index),
            matter,
            crypto,
        )
        .await
}
