//! Network connections for the Task Manager's Connections page: every TCP connection and listener
//! and every UDP endpoint, IPv4 and IPv6, with the process that owns it (IP Helper's owner-PID
//! tables, no admin rights needed). Remote addresses are named by reverse DNS on background
//! threads (cached; an address with no name is remembered as such). Traffic per connection would
//! need an administrator's event trace, so the page shows the system's traffic instead.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Condvar, Mutex, Once};

use serde::Serialize;
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::{GetNameInfoW, WSAStartup, AF_INET, AF_INET6, SOCKADDR, WSADATA};

#[derive(Serialize, Clone, Default)]
pub struct Conn {
    /// "TCP" or "UDP".
    pub proto: &'static str,
    pub local: String,
    pub lport: u16,
    /// Empty for listeners and UDP endpoints.
    pub remote: String,
    pub rport: u16,
    /// "Connected", "Listening", "Connecting", "Time wait"… (UDP: "Open").
    pub state: &'static str,
    pub pid: u32,
    pub image: String,
    /// "loopback" (between programs on this PC), "lan", "public", or "local" (no remote end).
    pub scope: &'static str,
    /// The remote address's name, when reverse DNS has one.
    pub host: String,
    /// The well-known service on the port that matters ("https", "dns"…).
    pub service: &'static str,
}

fn tcp_state(s: u32) -> &'static str {
    match s {
        1 => "Closed",
        2 => "Listening",
        3 => "Connecting",
        4 => "Connecting",
        5 => "Connected",
        6 | 7 | 9 | 10 => "Closing",
        8 => "Close wait",
        11 => "Time wait",
        12 => "Closing",
        _ => "Unknown",
    }
}

pub(crate) fn scope_of(ip: &IpAddr) -> &'static str {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            if v.is_loopback() {
                "loopback"
            } else if v.is_unspecified() {
                "local"
            } else if v.is_private() || v.is_link_local() || (o[0] == 100 && (64..128).contains(&o[1])) || v.is_broadcast() || v.is_multicast() {
                "lan"
            } else {
                "public"
            }
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return scope_of(&IpAddr::V4(v4));
            }
            let s = v.segments()[0];
            if v.is_loopback() {
                "loopback"
            } else if v.is_unspecified() {
                "local"
            } else if (s & 0xfe00) == 0xfc00 || (s & 0xffc0) == 0xfe80 || v.is_multicast() {
                "lan"
            } else {
                "public"
            }
        }
    }
}

pub(crate) fn service(port: u16) -> &'static str {
    match port {
        20 | 21 => "ftp",
        22 => "ssh",
        25 | 587 | 465 => "smtp",
        53 => "dns",
        67 | 68 => "dhcp",
        80 | 8080 => "http",
        110 | 995 => "pop3",
        123 => "ntp",
        135 => "rpc",
        137..=139 => "netbios",
        143 | 993 => "imap",
        443 | 8443 => "https",
        445 => "smb",
        853 => "dns over tls",
        1900 => "ssdp",
        3389 => "remote desktop",
        3478..=3481 => "stun",
        5040 => "connected devices",
        5222 | 5223 => "push",
        5353 => "mdns",
        5355 => "llmnr",
        6463..=6472 => "discord rpc",
        7680 => "delivery optimization",
        25565 => "minecraft",
        27015..=27050 => "steam",
        41641 => "tailscale",
        _ => "",
    }
}

/* ---------- reverse DNS, on its own thread ---------- */

static NAMES: Mutex<Option<HashMap<IpAddr, String>>> = Mutex::new(None);
static QUEUE: Mutex<VecDeque<IpAddr>> = Mutex::new(VecDeque::new());
static WAKE: Condvar = Condvar::new();
static RESOLVER: Once = Once::new();

fn resolve(ip: IpAddr) -> String {
    let mut sa = [0u8; 28];
    let len = match ip {
        IpAddr::V4(v) => {
            sa[0..2].copy_from_slice(&AF_INET.0.to_le_bytes());
            sa[4..8].copy_from_slice(&v.octets());
            16
        }
        IpAddr::V6(v) => {
            sa[0..2].copy_from_slice(&AF_INET6.0.to_le_bytes());
            sa[8..24].copy_from_slice(&v.octets());
            28
        }
    };
    let mut host = [0u16; 1025];
    // NI_NAMEREQD: no name rather than the address echoed back.
    let r = unsafe { GetNameInfoW(sa.as_ptr() as *const SOCKADDR, windows::Win32::Networking::WinSock::socklen_t(len), Some(&mut host), None, 0x04) };
    if r != 0 {
        return String::new();
    }
    String::from_utf16_lossy(&host[..host.iter().position(|&c| c == 0).unwrap_or(host.len())])
}

fn resolver() {
    unsafe {
        let mut d = WSADATA::default();
        WSAStartup(0x0202, &mut d);
    }
    loop {
        let ip = {
            let mut q = QUEUE.lock().unwrap();
            loop {
                if let Some(ip) = q.pop_front() {
                    break ip;
                }
                q = WAKE.wait(q).unwrap();
            }
        };
        let name = resolve(ip);
        NAMES.lock().unwrap().get_or_insert_with(HashMap::new).insert(ip, name);
    }
}

fn name_of(ip: IpAddr, wanted: &mut HashSet<IpAddr>) -> String {
    if let Some(n) = NAMES.lock().unwrap().get_or_insert_with(HashMap::new).get(&ip) {
        return n.clone();
    }
    wanted.insert(ip);
    String::new()
}

/* ---------- the tables ---------- */

fn table(get: impl Fn(Option<*mut core::ffi::c_void>, &mut u32) -> u32) -> Vec<u8> {
    let mut len = 0u32;
    get(None, &mut len);
    for _ in 0..4 {
        let mut buf = vec![0u8; len as usize + 4096];
        len = buf.len() as u32;
        if get(Some(buf.as_mut_ptr() as *mut _), &mut len) == 0 {
            return buf;
        }
    }
    vec![]
}

/// Rows of a MIB table: a u32 count, then the rows (aligned to the row type).
fn rows<T: Copy>(buf: &[u8]) -> Vec<T> {
    if buf.len() < 4 {
        return vec![];
    }
    let n = u32::from_le_bytes(buf[0..4].try_into().unwrap()) as usize;
    let align = std::mem::align_of::<T>().max(4);
    let start = 4usize.div_ceil(align) * align;
    let size = std::mem::size_of::<T>();
    let n = n.min(buf.len().saturating_sub(start) / size);
    (0..n).map(|i| unsafe { std::ptr::read_unaligned(buf.as_ptr().add(start + i * size) as *const T) }).collect()
}

fn port(p: u32) -> u16 {
    u16::from_be((p & 0xFFFF) as u16)
}

pub fn list() -> Vec<Conn> {
    // Some lookups take seconds (a slow or silent DNS server), so several run side by side.
    RESOLVER.call_once(|| {
        for _ in 0..6 {
            std::thread::spawn(resolver);
        }
    });
    let images = crate::taskmgr::images();
    let image = |pid: u32| match pid {
        0 => "System Idle Process".to_string(),
        4 => "System".to_string(),
        _ => images.get(&pid).cloned().unwrap_or_default(),
    };
    let mut wanted = HashSet::new();
    let mut out = vec![];
    let mut push = |proto: &'static str, l: IpAddr, lp: u16, r: Option<(IpAddr, u16)>, state: &'static str, pid: u32, wanted: &mut HashSet<IpAddr>| {
        let (remote, rport, scope, host) = match r {
            Some((ip, p)) if !ip.is_unspecified() => {
                let scope = scope_of(&ip);
                let host = if scope == "public" || scope == "lan" { name_of(ip, wanted) } else { String::new() };
                (ip.to_string(), p, scope, host)
            }
            _ => (String::new(), 0, if l.is_loopback() { "loopback" } else { "local" }, String::new()),
        };
        let svc = if rport != 0 && !service(rport).is_empty() { service(rport) } else { service(lp) };
        out.push(Conn { proto, local: l.to_string(), lport: lp, remote, rport, state, pid, image: image(pid), scope, host, service: svc });
    };
    unsafe {
        let t4 = table(|b, len| GetExtendedTcpTable(b, len, false, AF_INET.0 as u32, TCP_TABLE_OWNER_PID_ALL, 0));
        for r in rows::<MIB_TCPROW_OWNER_PID>(&t4) {
            let l = IpAddr::V4(Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes()));
            let st = tcp_state(r.dwState);
            let remote = (st != "Listening").then(|| (IpAddr::V4(Ipv4Addr::from(r.dwRemoteAddr.to_ne_bytes())), port(r.dwRemotePort)));
            push("TCP", l, port(r.dwLocalPort), remote, st, r.dwOwningPid, &mut wanted);
        }
        let t6 = table(|b, len| GetExtendedTcpTable(b, len, false, AF_INET6.0 as u32, TCP_TABLE_OWNER_PID_ALL, 0));
        for r in rows::<MIB_TCP6ROW_OWNER_PID>(&t6) {
            let l = IpAddr::V6(Ipv6Addr::from(r.ucLocalAddr));
            let st = tcp_state(r.dwState);
            let remote = (st != "Listening").then(|| (IpAddr::V6(Ipv6Addr::from(r.ucRemoteAddr)), port(r.dwRemotePort)));
            push("TCP", l, port(r.dwLocalPort), remote, st, r.dwOwningPid, &mut wanted);
        }
        let u4 = table(|b, len| GetExtendedUdpTable(b, len, false, AF_INET.0 as u32, UDP_TABLE_OWNER_PID, 0));
        for r in rows::<MIB_UDPROW_OWNER_PID>(&u4) {
            push("UDP", IpAddr::V4(Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes())), port(r.dwLocalPort), None, "Open", r.dwOwningPid, &mut wanted);
        }
        let u6 = table(|b, len| GetExtendedUdpTable(b, len, false, AF_INET6.0 as u32, UDP_TABLE_OWNER_PID, 0));
        for r in rows::<MIB_UDP6ROW_OWNER_PID>(&u6) {
            push("UDP", IpAddr::V6(Ipv6Addr::from(r.ucLocalAddr)), port(r.dwLocalPort), None, "Open", r.dwOwningPid, &mut wanted);
        }
    }
    if !wanted.is_empty() {
        let mut q = QUEUE.lock().unwrap();
        for ip in wanted {
            if !q.contains(&ip) {
                q.push_back(ip);
            }
        }
        WAKE.notify_all();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes() {
        let s = |x: &str| scope_of(&x.parse().unwrap());
        assert_eq!(s("127.0.0.1"), "loopback");
        assert_eq!(s("192.168.1.4"), "lan");
        assert_eq!(s("100.101.5.6"), "lan"); // Tailscale / carrier NAT
        assert_eq!(s("162.159.130.234"), "public");
        assert_eq!(s("::1"), "loopback");
        assert_eq!(s("fe80::1"), "lan");
        assert_eq!(s("2606:4700::6810:84e5"), "public");
        assert_eq!(s("::ffff:10.0.0.1"), "lan");
    }

    #[test]
    #[ignore]
    fn reverse_dns() {
        unsafe {
            let mut d = WSADATA::default();
            WSAStartup(0x0202, &mut d);
        }
        for ip in ["8.8.8.8", "1.1.1.1", "34.107.243.93", "160.79.104.10"] {
            let t = std::time::Instant::now();
            println!("{ip} -> {:?} in {:?}", resolve(ip.parse().unwrap()), t.elapsed());
        }
    }

    #[test]
    #[ignore]
    fn live() {
        let a = list();
        std::thread::sleep(std::time::Duration::from_secs(3));
        let b = list();
        println!("{} rows; named {}", b.len(), b.iter().filter(|c| !c.host.is_empty()).count());
        for c in b.iter().filter(|c| c.state == "Connected").take(25) {
            println!("{} {:>22}:{:<5} -> {:>28}:{:<5} {:<10} {:<6} {:<24} {:<8} {}", c.proto, c.local, c.lport, c.remote, c.rport, c.state, c.pid, c.image, c.scope, c.host);
        }
        assert!(!a.is_empty());
    }
}
