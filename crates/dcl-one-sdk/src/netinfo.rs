use std::net::Ipv4Addr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IfaceClass {
    Loopback,
    Lan,
    Overlay,
    Bridge,
    LinkLocal,
}

#[derive(Debug, Clone)]
pub struct Iface {
    pub name: String,
    pub ip: Ipv4Addr,
    pub class: IfaceClass,
}

impl Iface {
    pub fn new(name: &str, ip: Ipv4Addr) -> Self {
        Iface {
            name: name.to_string(),
            class: classify(name, ip),
            ip,
        }
    }
}

const BRIDGE_NAME_PREFIXES: [&str; 4] = ["docker", "br-", "veth", "virbr"];

pub fn classify(name: &str, ip: Ipv4Addr) -> IfaceClass {
    let o = ip.octets();
    if ip.is_loopback() {
        return IfaceClass::Loopback;
    }
    if o[0] == 169 && o[1] == 254 {
        return IfaceClass::LinkLocal;
    }
    if BRIDGE_NAME_PREFIXES.iter().any(|p| name.starts_with(p)) {
        return IfaceClass::Bridge;
    }
    if o[0] == 100 && (64..=127).contains(&o[1]) {
        return IfaceClass::Overlay;
    }
    if o[0] == 172 && (17..=31).contains(&o[1]) {
        return IfaceClass::Bridge;
    }
    IfaceClass::Lan
}

pub fn enumerate() -> Vec<Iface> {
    let mut out: Vec<Iface> = Vec::new();
    if let Ok(addrs) = if_addrs::get_if_addrs() {
        for a in addrs {
            if let std::net::IpAddr::V4(ip) = a.ip() {
                out.push(Iface::new(&a.name, ip));
            }
        }
    }
    out.sort_by_key(|i| i.class);
    out
}

pub fn share_ip(ifaces: &[Iface]) -> Option<Ipv4Addr> {
    [IfaceClass::Lan, IfaceClass::Overlay, IfaceClass::Bridge]
        .iter()
        .find_map(|c| ifaces.iter().find(|i| i.class == *c).map(|i| i.ip))
}

pub fn nat_vm_guest(ifaces: &[Iface]) -> bool {
    let routable: Vec<&Iface> = ifaces
        .iter()
        .filter(|i| !matches!(i.class, IfaceClass::Loopback | IfaceClass::LinkLocal))
        .collect();
    routable.len() == 1 && routable[0].ip == Ipv4Addr::new(10, 0, 2, 15)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn classify_covers_every_class() {
        assert_eq!(classify("lo", ip("127.0.0.1")), IfaceClass::Loopback);
        assert_eq!(classify("eth0", ip("10.1.2.20")), IfaceClass::Lan);
        assert_eq!(classify("eth0", ip("10.1.2.3")), IfaceClass::Lan);
        assert_eq!(classify("eth0", ip("172.16.0.5")), IfaceClass::Lan);
        assert_eq!(classify("eth0", ip("203.0.113.9")), IfaceClass::Lan);
        assert_eq!(classify("eth1", ip("169.254.7.42")), IfaceClass::LinkLocal);
        assert_eq!(classify("wg0", ip("100.101.102.103")), IfaceClass::Overlay);
        assert_eq!(classify("wg0", ip("100.127.255.1")), IfaceClass::Overlay);
        assert_eq!(classify("eth0", ip("100.128.0.1")), IfaceClass::Lan);
        assert_eq!(classify("docker0", ip("172.17.0.1")), IfaceClass::Bridge);
        assert_eq!(classify("virbr0", ip("10.88.0.1")), IfaceClass::Bridge);
        assert_eq!(classify("br-abc123", ip("10.9.0.1")), IfaceClass::Bridge);
        assert_eq!(classify("veth99", ip("10.9.0.2")), IfaceClass::Bridge);
        assert_eq!(classify("eth0", ip("172.20.0.7")), IfaceClass::Bridge);
    }

    #[test]
    fn share_ip_prefers_lan_over_overlay_over_bridge() {
        let bridge = Iface::new("docker0", ip("172.17.0.1"));
        let overlay = Iface::new("wg0", ip("100.101.102.103"));
        let lan = Iface::new("wlan0", ip("10.1.2.20"));
        assert_eq!(
            share_ip(&[bridge.clone(), overlay.clone(), lan.clone()]),
            Some(ip("10.1.2.20"))
        );
        assert_eq!(
            share_ip(&[bridge.clone(), overlay.clone()]),
            Some(ip("100.101.102.103"))
        );
        assert_eq!(share_ip(&[bridge]), Some(ip("172.17.0.1")));
        assert_eq!(share_ip(&[Iface::new("lo", ip("127.0.0.1"))]), None);
        assert_eq!(share_ip(&[Iface::new("eth1", ip("169.254.9.9"))]), None);
    }

    #[test]
    fn nat_vm_guest_matches_only_the_qemu_default() {
        let lo = Iface::new("lo", ip("127.0.0.1"));
        let nat = Iface::new("enp0s3", ip("10.0.2.15"));
        let lan = Iface::new("enp0s8", ip("10.1.2.20"));
        assert!(nat_vm_guest(&[lo.clone(), nat.clone()]));
        assert!(!nat_vm_guest(&[lo.clone(), nat, lan]));
        assert!(!nat_vm_guest(&[lo]));
    }
}
