//! Minimal MQTT 3.1.1 client (QoS 0 only) over plain TCP: enough to use a public broker as a meeting point and,
//! when two phones cannot reach each other directly, as a relay for game packets.
//! One background thread per broker; received publishes land in a shared queue.
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::Sender;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use web_time::Instant;

#[cfg(not(target_arch = "wasm32"))]
pub struct Broker { tx: Sender<Vec<u8>>, pub up: Arc<Mutex<bool>>, pub name: String, stop: Arc<Mutex<bool>> }
#[cfg(target_arch = "wasm32")]
pub use web::Broker;
/// periodic work (the browser's brokers reconnect and keep alive from here; native brokers have their own threads)
pub fn tick() { #[cfg(target_arch = "wasm32")] web::tick(); }

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

#[cfg(not(target_arch = "wasm32"))]
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
#[cfg(not(target_arch = "wasm32"))]
impl Drop for Broker { fn drop(&mut self) { *self.stop.lock().unwrap() = true; } }

/// The browser build: the same brokers over secure WebSockets (browsers can't open TCP sockets). Same MQTT packets,
/// same topics, so browsers and phones meet in the same rooms. The page owns the sockets (window.__fwWs) and hands
/// their bytes back through web_api.
#[cfg(target_arch = "wasm32")]
pub mod web {
    use super::*;
    use std::cell::RefCell;
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = __fwWs, js_name = open)]
        fn ws_open(id: u32, url: &str);
        #[wasm_bindgen(js_namespace = __fwWs, js_name = send)]
        fn ws_send(id: u32, b: &[u8]);
        #[wasm_bindgen(js_namespace = __fwWs, js_name = close)]
        fn ws_close(id: u32);
    }
    /// the WebSocket address of each broker the phones use over TCP
    fn wss(host: &str) -> String {
        let h = host.split(':').next().unwrap_or(host);
        match h { "broker.emqx.io" => "wss://broker.emqx.io:8084/mqtt".into(), "broker.hivemq.com" => "wss://broker.hivemq.com:8884/mqtt".into(),
            "test.mosquitto.org" => "wss://test.mosquitto.org:8081/mqtt".into(), _ => format!("wss://{h}/mqtt") }
    }
    struct Conn { url: String, id: String, filter: String, inbox: Sender<(String, Vec<u8>)>, up: Arc<Mutex<bool>>, buf: Vec<u8>,
        open: bool, ok: bool, since: f64, last_tx: f64, retry_at: f64, backoff: f64, gone: bool }
    thread_local! { static CONNS: RefCell<Vec<Conn>> = const { RefCell::new(Vec::new()) }; }
    fn now() -> f64 { js_sys::Date::now() / 1000.0 }

    pub struct Broker { k: u32, up: Arc<Mutex<bool>>, pub name: String }
    impl Broker {
        pub fn start(host: &str, client_id: String, filter: String, inbox: Sender<(String, Vec<u8>)>) -> Broker {
            let up = Arc::new(Mutex::new(false)); let url = wss(host);
            let k = CONNS.with(|c| { let mut c = c.borrow_mut(); c.push(Conn { url: url.clone(), id: client_id, filter, inbox, up: up.clone(), buf: Vec::new(), open: false, ok: false,
                since: now(), last_tx: now(), retry_at: 0.0, backoff: 1.0, gone: false }); (c.len() - 1) as u32 });
            ws_open(k, &url);
            Broker { k, up, name: host.to_string() }
        }
        pub fn publish(&self, topic: &str, payload: &[u8]) { if self.is_up() { ws_send(self.k, &publish_packet(topic, payload)); CONNS.with(|c| if let Some(x) = c.borrow_mut().get_mut(self.k as usize) { x.last_tx = now(); }); } }
        pub fn is_up(&self) -> bool { *self.up.lock().unwrap() }
    }
    impl Drop for Broker {
        fn drop(&mut self) {
            CONNS.with(|c| if let Some(x) = c.borrow_mut().get_mut(self.k as usize) { x.gone = true; *x.up.lock().unwrap() = false; if x.open { ws_send(self.k, &[0xE0, 0]); } });
            ws_close(self.k);
        }
    }
    /// the socket opened: log in
    pub fn on_open(k: u32) {
        let pk = CONNS.with(|c| { let mut c = c.borrow_mut(); let x = c.get_mut(k as usize)?; if x.gone { return None; } x.open = true; x.buf.clear(); x.since = now(); x.last_tx = now(); Some(connect_packet(&x.id, 30)) });
        if let Some(p) = pk { ws_send(k, &p); }
    }
    /// bytes from the broker (a WebSocket message may hold part of a packet, or several)
    pub fn on_data(k: u32, b: &[u8]) {
        let mut out: Vec<Vec<u8>> = Vec::new(); let mut bad = false;
        CONNS.with(|c| { let mut c = c.borrow_mut(); let Some(x) = c.get_mut(k as usize) else { return }; if x.gone { return; }
            x.buf.extend_from_slice(b);
            while let Some((kind, body, len)) = parse(&x.buf) {
                match kind >> 4 {
                    2 => { if body.len() >= 2 && body[1] == 0 { x.ok = true; out.push(subscribe_packet(1, &x.filter)); *x.up.lock().unwrap() = true; x.backoff = 1.0; } else { bad = true; } }
                    3 => { if let Some((t, p)) = publish_body(body) { let _ = x.inbox.send((t.to_string(), p.to_vec())); } }
                    _ => {}
                }
                x.buf.drain(..len);
            } });
        for p in out { ws_send(k, &p); }
        if bad { ws_close(k); on_close(k); }
    }
    /// the socket closed or failed: try again later (1 s, doubling up to 20 s)
    pub fn on_close(k: u32) {
        CONNS.with(|c| { let mut c = c.borrow_mut(); let Some(x) = c.get_mut(k as usize) else { return };
            if !x.open && x.retry_at > now() { return; }
            *x.up.lock().unwrap() = false; x.open = false; x.ok = false; x.retry_at = now() + x.backoff; x.backoff = (x.backoff * 2.0).min(20.0); });
    }
    /// once a frame: reconnects, keep-alive pings, a login that never answered
    pub fn tick() {
        let t = now(); let mut reopen = Vec::new(); let mut ping = Vec::new(); let mut stuck = Vec::new();
        CONNS.with(|c| { for (k, x) in c.borrow_mut().iter_mut().enumerate() { if x.gone { continue; }
            if !x.open && x.retry_at > 0.0 && t >= x.retry_at { x.retry_at = 0.0; x.since = t; reopen.push((k as u32, x.url.clone())); }
            else if x.open && x.ok && t - x.last_tx > 15.0 { x.last_tx = t; ping.push(k as u32); }
            else if x.open && !x.ok && t - x.since > 8.0 { stuck.push(k as u32); } } });
        for (k, u) in reopen { ws_open(k, &u); }
        for k in ping { ws_send(k, &[0xC0, 0]); }
        for k in stuck { ws_close(k); on_close(k); }
    }
}

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
