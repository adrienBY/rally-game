//! UDP for the direct path between phones: one socket, its public address learned from a STUN server, and the local
//! network address (so phones on the same Wi-Fi connect straight away). All calls are non-blocking.
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs, UdpSocket};
use std::time::Duration;

pub const STUN_SERVERS: [&str; 3] = ["stun.l.google.com:19302", "stun.cloudflare.com:3478", "stun1.l.google.com:19302"];
const MAGIC: u32 = 0x2112A442;

pub fn bind() -> Option<UdpSocket> { let s = UdpSocket::bind("0.0.0.0:0").ok()?; s.set_nonblocking(true).ok()?; Some(s) }

/// The address other phones on the same network would use (no packet is sent: connecting a UDP socket only picks a route).
pub fn local_ip() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?; s.connect("8.8.8.8:80").ok()?;
    match s.local_addr().ok()? { SocketAddr::V4(a) if !a.ip().is_loopback() && !a.ip().is_unspecified() => Some(*a.ip()), _ => None }
}

pub fn stun_request(tid: &[u8; 12]) -> Vec<u8> { let mut p = vec![0x00, 0x01, 0x00, 0x00]; p.extend_from_slice(&MAGIC.to_be_bytes()); p.extend_from_slice(tid); p }

/// XOR-MAPPED-ADDRESS (or MAPPED-ADDRESS) from a binding response with our transaction id
pub fn stun_parse(b: &[u8], tid: &[u8; 12]) -> Option<SocketAddrV4> {
    if b.len() < 20 || b[0] != 0x01 || b[1] != 0x01 || &b[8..20] != tid { return None; }
    let n = u16::from_be_bytes([b[2], b[3]]) as usize; let mut i = 20; let end = (20 + n).min(b.len()); let mut plain = None;
    while i + 4 <= end {
        let t = u16::from_be_bytes([b[i], b[i + 1]]); let l = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize; let v = &b[i + 4..(i + 4 + l).min(end)];
        if v.len() >= 8 && v[1] == 1 {
            let port = u16::from_be_bytes([v[2], v[3]]); let ip = u32::from_be_bytes([v[4], v[5], v[6], v[7]]);
            if t == 0x0020 { return Some(SocketAddrV4::new(Ipv4Addr::from(ip ^ MAGIC), port ^ (MAGIC >> 16) as u16)); }
            if t == 0x0001 { plain = Some(SocketAddrV4::new(Ipv4Addr::from(ip), port)); }
        }
        i += 4 + ((l + 3) & !3);
    }
    plain
}

/// Ask STUN servers (from the game socket itself, so the mapping is the one peers must use) for our public address.
/// Blocks up to ~2.5 s; run it off the game thread with a cloned socket.
pub fn stun_query(sock: &UdpSocket) -> Option<SocketAddrV4> {
    let mut tid = [0u8; 12]; let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
    for (k, b) in tid.iter_mut().enumerate() { *b = ((seed >> (k * 5)) as u8) ^ (k as u8).wrapping_mul(37); }
    let req = stun_request(&tid); let addrs: Vec<SocketAddr> = STUN_SERVERS.iter().filter_map(|h| h.to_socket_addrs().ok()?.find(|a| a.is_ipv4())).collect();
    if addrs.is_empty() { return None; }
    let mut buf = [0u8; 576];
    for _ in 0..5 {
        for a in &addrs { let _ = sock.send_to(&req, a); }
        let t0 = std::time::Instant::now();
        while t0.elapsed() < Duration::from_millis(500) {
            match sock.recv_from(&mut buf) { Ok((n, from)) => { if addrs.contains(&from) { if let Some(m) = stun_parse(&buf[..n], &tid) { return Some(m); } } }
                Err(_) => std::thread::sleep(Duration::from_millis(10)) }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stun_xor_address() {
        let tid = [7u8; 12]; let ip = u32::from(Ipv4Addr::new(203, 0, 113, 9)); let port: u16 = 51234;
        let mut r = vec![0x01, 0x01, 0x00, 12]; r.extend_from_slice(&MAGIC.to_be_bytes()); r.extend_from_slice(&tid);
        r.extend_from_slice(&[0x00, 0x20, 0x00, 0x08, 0x00, 0x01]); r.extend_from_slice(&(port ^ 0x2112).to_be_bytes()); r.extend_from_slice(&(ip ^ MAGIC).to_be_bytes());
        assert_eq!(stun_parse(&r, &tid), Some(SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 9), 51234)));
        assert_eq!(stun_parse(&r, &[8u8; 12]), None);
    }
}
