//! Small security rules of the phone server: who may connect, key comparison, random values.

use std::{
    net::{IpAddr, Ipv4Addr},
    time::Instant,
};

/// Whether a peer address may talk to the phone server: loopback and private networks only.
/// A UPnP mapping or a global IPv6 address on the listening socket must not expose it.
pub fn is_allowed_peer(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => allowed_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => allowed_v4(v4),
            None => {
                let first = v6.segments()[0];
                v6.is_loopback()
                    || (first & 0xFE00) == 0xFC00 // fc00::/7 unique local
                    || (first & 0xFFC0) == 0xFE80 // fe80::/10 link local
            }
        },
    }
}

fn allowed_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || a == 10
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 169 && b == 254)
        || (a == 100 && (64..=127).contains(&b)) // carrier-grade NAT
}

/// A token bucket: `rate` events a second on average, bursts up to `burst`. The phone is one
/// player's hand, so a flood is a bug or an attack, and is dropped instead of queued.
#[derive(Debug)]
pub struct RateLimit {
    rate: f64,
    burst: f64,
    tokens: f64,
    last: Option<Instant>,
}

impl RateLimit {
    pub fn new(rate: f64, burst: f64) -> RateLimit {
        RateLimit {
            rate,
            burst,
            tokens: burst,
            last: None,
        }
    }

    /// Whether one more event at `now` is within the limit (and counts it if so).
    pub fn allow(&mut self, now: Instant) -> bool {
        if let Some(last) = self.last {
            let seconds = now.saturating_duration_since(last).as_secs_f64();
            self.tokens = (self.tokens + seconds * self.rate).min(self.burst);
        }
        self.last = Some(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// An address of this PC with its network prefix length, from the adapters.
pub type Subnet = (IpAddr, u8);

fn prefix_len(mask: IpAddr) -> u8 {
    match mask {
        IpAddr::V4(m) => u32::from(m).leading_ones() as u8,
        IpAddr::V6(m) => u128::from(m).leading_ones() as u8,
    }
}

/// The subnets of the adapters a phone can really sit on: no loopback, no virtual adapter.
pub fn local_subnets() -> Vec<Subnet> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback() && !super::discovery::is_virtual_adapter(&i.name))
        .map(|i| match i.addr {
            if_addrs::IfAddr::V4(v4) => (IpAddr::V4(v4.ip), prefix_len(IpAddr::V4(v4.netmask))),
            if_addrs::IfAddr::V6(v6) => (IpAddr::V6(v6.ip), prefix_len(IpAddr::V6(v6.netmask))),
        })
        .collect()
}

/// Whether `peer` sits on one of the networks this PC is plugged into: loopback always, otherwise
/// the same subnet as an adapter. A private address that arrives through a router or a VPN is not
/// "the home network" and does not pass.
pub fn is_on_link(peer: IpAddr, subnets: &[Subnet]) -> bool {
    let peer = peer.to_canonical();
    if peer.is_loopback() {
        return true;
    }
    subnets
        .iter()
        .any(|(ip, prefix)| match (peer, ip.to_canonical()) {
            (IpAddr::V4(a), IpAddr::V4(b)) => same_prefix(
                u128::from(u32::from(a)),
                u128::from(u32::from(b)),
                *prefix,
                32,
            ),
            (IpAddr::V6(a), IpAddr::V6(b)) => {
                same_prefix(u128::from(a), u128::from(b), *prefix, 128)
            }
            _ => false,
        })
}

fn same_prefix(a: u128, b: u128, prefix: u8, bits: u32) -> bool {
    let prefix = u32::from(prefix).min(bits);
    if prefix == 0 {
        return false;
    }
    let shift = bits - prefix;
    (a >> shift) == (b >> shift)
}

/// Compares two secrets without stopping at the first difference.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u8;
    for i in 0..a.len().max(b.len()) {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

/// Fills a buffer from the operating system's random source.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("the system random source works");
    bytes
}

/// A random, never zero session id (the UDP capability).
pub fn random_session() -> u64 {
    loop {
        let value = u64::from_le_bytes(random_bytes::<8>());
        if value != 0 {
            return value;
        }
    }
}

/// A 4-digit pairing code, uniformly "0000" to "9999".
pub fn random_code() -> String {
    loop {
        let value = u16::from_le_bytes(random_bytes::<2>());
        if value < 60000 {
            return format!("{:04}", value % 10000);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_private_peers_pass() {
        for ok in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.178.20",
            "169.254.3.4",
            "100.64.0.1",
            "::1",
            "fd12:3456::1",
            "fe80::1",
            "::ffff:192.168.1.5",
        ] {
            assert!(is_allowed_peer(ok.parse().unwrap()), "{ok}");
        }
        for bad in [
            "8.8.8.8",
            "172.32.0.1",
            "172.15.0.1",
            "100.128.0.1",
            "192.169.0.1",
            "2001:db8::1",
            "::ffff:8.8.8.8",
        ] {
            assert!(!is_allowed_peer(bad.parse().unwrap()), "{bad}");
        }
    }

    #[test]
    fn the_rate_limit_allows_a_burst_then_the_rate() {
        let start = Instant::now();
        let mut limit = RateLimit::new(10.0, 5.0);
        assert!((0..5).all(|_| limit.allow(start)));
        assert!(!limit.allow(start), "burst used up");
        assert!(
            limit.allow(start + std::time::Duration::from_millis(150)),
            "one token back"
        );
        assert!(!limit.allow(start + std::time::Duration::from_millis(160)));
        // A long pause refills to the burst and no further.
        let later = start + std::time::Duration::from_secs(60);
        assert_eq!((0..20).filter(|_| limit.allow(later)).count(), 5);
    }

    #[test]
    fn only_the_networks_of_this_pc_are_on_link() {
        let subnets: Vec<Subnet> = vec![
            ("192.168.178.20".parse().unwrap(), 24),
            ("fd12:3456::7".parse().unwrap(), 64),
        ];
        let on = |ip: &str| is_on_link(ip.parse().unwrap(), &subnets);
        assert!(on("192.168.178.77"));
        assert!(on("::ffff:192.168.178.77"));
        assert!(on("127.0.0.1") && on("::1"));
        assert!(on("fd12:3456::99"));
        assert!(!on("192.168.179.77"), "another subnet, routed in");
        assert!(!on("10.0.0.5"), "private but not ours");
        assert!(!on("fd12:3457::1"));
        assert!(!is_on_link("192.168.178.77".parse().unwrap(), &[]));
    }

    #[test]
    fn constant_time_compare_agrees_with_equality() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn random_values_have_their_shape() {
        assert_ne!(random_session(), 0);
        let code = random_code();
        assert_eq!(code.len(), 4);
        assert!(code.bytes().all(|b| b.is_ascii_digit()));
        assert_ne!(random_bytes::<32>(), random_bytes::<32>());
    }
}
