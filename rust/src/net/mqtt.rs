//! Minimal MQTT 3.1.1 client (QoS 0 only) over plain TCP: enough to use a public broker as a meeting point and,
//! when two phones cannot reach each other directly, as a relay for game packets.
//! One background thread per broker; received publishes land in a shared queue.
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Broker { tx: Sender<Vec<u8>>, pub up: Arc<Mutex<bool>>, pub name: String, stop: Arc<Mutex<bool>> }

fn enc_len(mut n: usize, out: &mut Vec<u8>) { loop { let mut b = (n % 128) as u8; n /= 128; if n > 0 { b |= 0x80; } out.push(b); if n == 0 { break; } } }
fn str16(s: &[u8], out: &mut Vec<u8>) { out.extend_from_slice(&(s.len() as u16).to_be_bytes()); out.extend_from_slice(s); }
fn packet(kind: u8, body: &[u8]) -> Vec<u8> { let mut p = vec![kind]; enc_len(body.len(), &mut p); p.extend_from_slice(body); p }

pub fn connect_packet(client_id: &str, keepalive: u16) -> Vec<u8> {
    let mut b = Vec::new(); str16(b"MQTT", &mut b); b.push(4); b.push(0x02); b.extend_from_slice(&keepalive.to_be_bytes()); str16(client_id.as_bytes(), &mut b); packet(0x10, &b)
}
pub fn subscribe_packet(id: u16, filter: &str) -> Vec<u8> { let mut b = id.to_be_bytes().to_vec(); str16(filter.as_bytes(), &mut b); b.push(0); packet(0x82, &b) }
pub fn publish_packet(topic: &str, payload: &[u8]) -> Vec<u8> { let mut b = Vec::new(); str16(topic.as_bytes(), &mut b); b.extend_from_slice(payload); packet(0x30, &b) }

/// Parse one complete packet from the front of buf: (type byte, body, total length)
pub fn parse(buf: &[u8]) -> Option<(u8, &[u8], usize)> {
    if buf.len() < 2 { return None; }
    let (mut n, mut mul, mut i) = (0usize, 1usize, 1usize);
    loop { if i >= buf.len() || i > 4 { return None; } let b = buf[i]; n += (b & 0x7f) as usize * mul; mul *= 128; i += 1; if b & 0x80 == 0 { break; } }
    if buf.len() < i + n { return None; } Some((buf[0], &buf[i..i + n], i + n))
}
/// topic and payload of a PUBLISH body (QoS 0)
pub fn publish_body(b: &[u8]) -> Option<(&str, &[u8])> {
    if b.len() < 2 { return None; } let tl = u16::from_be_bytes([b[0], b[1]]) as usize; if b.len() < 2 + tl { return None; }
    Some((std::str::from_utf8(&b[2..2 + tl]).ok()?, &b[2 + tl..]))
}

impl Broker {
    /// Start a client for `host:port` subscribed to `filter`; incoming (topic, payload) go to `inbox`.
    pub fn start(host: &str, client_id: String, filter: String, inbox: Sender<(String, Vec<u8>)>) -> Broker {
        let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = channel(); let up = Arc::new(Mutex::new(false)); let stop = Arc::new(Mutex::new(false));
        let (h, u2, s2) = (host.to_string(), up.clone(), stop.clone());
        std::thread::Builder::new().name(format!("mqtt {}", host)).spawn(move || {
            let mut backoff = 1.0f64;
            while !*s2.lock().unwrap() {
                let r = Self::session(&h, &client_id, &filter, &rx, &inbox, &u2, &s2);
                *u2.lock().unwrap() = false;
                if r.is_ok() { backoff = 1.0; }
                while rx.try_recv().is_ok() {}   // drop what was queued while down
                std::thread::sleep(Duration::from_secs_f64(backoff)); backoff = (backoff * 2.0).min(20.0);
            }
        }).ok();
        Broker { tx, up, name: host.to_string(), stop }
    }
    fn session(host: &str, id: &str, filter: &str, rx: &Receiver<Vec<u8>>, inbox: &Sender<(String, Vec<u8>)>, up: &Arc<Mutex<bool>>, stop: &Arc<Mutex<bool>>) -> std::io::Result<()> {
        let addr = host.to_socket_addrs()?.next().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no address"))?;
        let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(5))?; s.set_nodelay(true)?; s.set_read_timeout(Some(Duration::from_millis(15)))?;
        s.write_all(&connect_packet(id, 30))?;
        let mut buf: Vec<u8> = Vec::with_capacity(8192); let mut tmp = [0u8; 4096]; let t0 = Instant::now(); let mut ok = false; let mut last_tx = Instant::now();
        loop {
            if *stop.lock().unwrap() { let _ = s.write_all(&[0xE0, 0]); return Ok(()); }
            match s.read(&mut tmp) { Ok(0) => return Err(std::io::ErrorKind::ConnectionAborted.into()), Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}, Err(e) => return Err(e) }
            while let Some((k, body, len)) = parse(&buf) {
                match k >> 4 {
                    2 => { if body.len() >= 2 && body[1] == 0 { ok = true; s.write_all(&subscribe_packet(1, filter))?; *up.lock().unwrap() = true; } else { return Err(std::io::ErrorKind::PermissionDenied.into()); } }
                    3 => { if let Some((t, p)) = publish_body(body) { let _ = inbox.send((t.to_string(), p.to_vec())); } }
                    _ => {}
                }
                buf.drain(..len);
            }
            if !ok { if t0.elapsed() > Duration::from_secs(6) { return Err(std::io::ErrorKind::TimedOut.into()); } continue; }
            while let Ok(p) = rx.try_recv() { s.write_all(&p)?; last_tx = Instant::now(); }
            if last_tx.elapsed() > Duration::from_secs(15) { s.write_all(&[0xC0, 0])?; last_tx = Instant::now(); }
        }
    }
    pub fn publish(&self, topic: &str, payload: &[u8]) { if *self.up.lock().unwrap() { let _ = self.tx.send(publish_packet(topic, payload)); } }
    pub fn is_up(&self) -> bool { *self.up.lock().unwrap() }
}
impl Drop for Broker { fn drop(&mut self) { *self.stop.lock().unwrap() = true; } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let p = publish_packet("fogwood/v1/123456/all", &[1, 2, 3, 250]);
        let (k, body, len) = parse(&p).unwrap(); assert_eq!(k, 0x30); assert_eq!(len, p.len());
        let (t, pl) = publish_body(body).unwrap(); assert_eq!(t, "fogwood/v1/123456/all"); assert_eq!(pl, &[1, 2, 3, 250]);
        let big = publish_packet("t", &vec![7u8; 300]); let (_, b, l) = parse(&big).unwrap(); assert_eq!(l, big.len()); assert_eq!(b.len(), 3 + 300);
        assert!(parse(&big[..big.len() - 1]).is_none());
    }
}
