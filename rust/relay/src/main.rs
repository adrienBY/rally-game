//! Fogwood relay. Phones and browsers in an online room send their game packets here; the relay passes each one on
//! to the others in the room at once. It never looks inside them (the netcode stays on the phones) and keeps no
//! state but who is in which room, so it is tiny, fast and cheap to run.
//!
//! Wire format (every message, both transports): first byte the kind.
//!   client -> relay   0x01 JOIN  room u32, id u32         (also a keep-alive; answered with 0x81)
//!                     0x02 SEND  to u32 (0: everyone else), payload
//!                     0x03 PING  t f64                     (answered with 0x83 t, relay clock f64)
//!                     0x04 LEAVE
//!   relay -> client   0x81 WELCOME relay clock f64, members u8
//!                     0x82 FROM  from u32, payload
//!                     0x83 PONG  t f64, relay clock f64
//! Apps use UDP (port 7777), browsers a WebSocket (port 8080, path /ws; TLS in front on the host).
//! Little-endian numbers. A member silent for 20 s is dropped; rooms hold up to 16; each member may send up to
//! 200 messages a second (bursts of 50) and payloads up to 1200 bytes.
//! AI cars (WebSocket members): WELCOME carries one more byte, bit 0 set: the relay can drive a race's AI cars. The host
//! then sends 0x07 STAGE + server::stage_blob, answered with 0x87 + 1 (ready) or 2 (failed); from then on the relay
//! feeds the room's packets (to everyone, or to server::SERVER_ID) to that room's fogwood_core::server::Sim and sends
//! what it returns as from SERVER_ID.
use std::collections::HashMap;
use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_PAYLOAD: usize = 1200;
use fogwood_core::server::SERVER_ID;
const ROOM_MAX: usize = 16;
const IDLE: f64 = 20.0;
const RATE: f64 = 200.0;
const BURST: f64 = 50.0;

#[derive(Clone)]
enum Link { Udp(SocketAddr), Ws(Sender<Vec<u8>>) }
struct Member { id: u32, link: Link, seen: f64, tokens: f64, last: f64 }
#[derive(Default)]
struct Rooms { rooms: HashMap<u32, Vec<Member>>, by_udp: HashMap<SocketAddr, (u32, u32)>, stats: [u64; 4], sims: HashMap<u32, fogwood_core::server::Sim> }

struct Relay { t0: Instant, rooms: Mutex<Rooms>, udp: UdpSocket }
impl Relay {
    fn now(&self) -> f64 { self.t0.elapsed().as_secs_f64() }
    fn deliver(&self, link: &Link, b: &[u8]) { match link { Link::Udp(a) => { let _ = self.udp.send_to(b, a); } Link::Ws(tx) => { let _ = tx.send(b.to_vec()); } } }

    /// one message from a client; `key` identifies the sender's link (for UDP, its address)
    fn handle(&self, b: &[u8], link: Link, ws_member: &mut Option<(u32, u32)>) {
        let now = self.now(); if b.is_empty() { return; }
        let u32_at = |i: usize| -> Option<u32> { b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])) };
        let mut out: Vec<(Link, Vec<u8>)> = Vec::new();
        {
            let mut g = self.rooms.lock().unwrap();
            let who = match &link { Link::Udp(a) => g.by_udp.get(a).copied(), Link::Ws(_) => *ws_member };
            match b[0] {
                0x01 => { let (Some(room), Some(id)) = (u32_at(1), u32_at(5)) else { return };
                    if let Some((r0, i0)) = who { if (r0, i0) != (room, id) { remove(&mut g, r0, i0); } }
                    let list = g.rooms.entry(room).or_default();
                    match list.iter_mut().find(|m| m.id == id) {
                        Some(m) => { m.link = link.clone(); m.seen = now; }
                        None => { if list.len() >= ROOM_MAX { return; } list.push(Member { id, link: link.clone(), seen: now, tokens: BURST, last: now }); }
                    }
                    let n = list.len() as u8;
                    if let Link::Udp(a) = &link { g.by_udp.insert(*a, (room, id)); } else { *ws_member = Some((room, id)); }
                    let caps = if matches!(link, Link::Ws(_)) { 1u8 } else { 0 };   // AI cars: the stage only fits a WebSocket
                    let mut w = vec![0x81]; w.extend_from_slice(&now.to_le_bytes()); w.push(n); w.push(caps); out.push((link.clone(), w)); }
                0x02 => { let Some((room, id)) = who else { return }; let Some(to) = u32_at(1) else { return }; let payload = &b[5..];
                    if payload.len() > MAX_PAYLOAD { return; }
                    let Some(list) = g.rooms.get_mut(&room) else { return };
                    let Some(me) = list.iter_mut().find(|m| m.id == id) else { return };
                    me.tokens = (me.tokens + (now - me.last) * RATE).min(BURST); me.last = now; me.seen = now;
                    if me.tokens < 1.0 { return; } me.tokens -= 1.0;
                    let mut w = Vec::with_capacity(payload.len() + 5); w.push(0x82); w.extend_from_slice(&id.to_le_bytes()); w.extend_from_slice(payload);
                    if to != SERVER_ID { for m in list.iter() { if m.id != id && (to == 0 || m.id == to) { out.push((m.link.clone(), w.clone())); } } }
                    if to == 0 || to == SERVER_ID { if let Some(s) = g.sims.get_mut(&room) { s.packet(id, payload, now); } }
                    g.stats[0] += 1; g.stats[1] += (w.len() * out.len()) as u64; }
                0x03 => { if b.len() < 9 { return; } if let Some((room, id)) = who { if let Some(m) = g.rooms.get_mut(&room).and_then(|l| l.iter_mut().find(|m| m.id == id)) { m.seen = now; } }
                    let mut w = vec![0x83]; w.extend_from_slice(&b[1..9]); w.extend_from_slice(&now.to_le_bytes()); out.push((link.clone(), w)); }
                0x07 => { let Some((room, id)) = who else { return }; drop(g);
                    // the stage for this room's AI cars: built outside the lock (a second or so), then it takes over the room's AI
                    let r = if std::env::var("RELAY_SIM_FAIL").is_ok() { Err("test: refused".to_string()) } else { fogwood_core::server::Sim::new(&b[1..], room, id, now) };
                    let ok = match r { Ok(s) => { println!("room {room}: AI cars on {} ({:.2}s to build)", s.map(), self.now() - now); self.rooms.lock().unwrap().sims.insert(room, s); 1u8 } Err(e) => { println!("room {room}: stage refused: {e}"); 2 } };
                    self.deliver(&link, &[0x87, ok]); return; }
                0x04 => { if let Some((r, i)) = who { remove(&mut g, r, i); if let Link::Udp(a) = &link { g.by_udp.remove(a); } else { *ws_member = None; } } }
                _ => {}
            }
        }
        for (l, w) in out { self.deliver(&l, &w); }
    }
    /// the AI cars: each room's race moved on, and what it has to send
    fn sims(&self) {
        let now = self.now(); let mut out: Vec<(Link, Vec<u8>)> = Vec::new();
        { let mut g = self.rooms.lock().unwrap(); let g = &mut *g; let t0 = Instant::now();
            for (room, s) in g.sims.iter_mut() { s.step(now); for l in s.log.drain(..) { println!("room {room}: {l}"); }
                for (to, p) in s.take_out() { let mut w = Vec::with_capacity(p.len() + 5); w.push(0x82); w.extend_from_slice(&SERVER_ID.to_le_bytes()); w.extend_from_slice(&p);
                    if let Some(list) = g.rooms.get(room) { for m in list { if to == 0 || m.id == to { out.push((m.link.clone(), w.clone())); } } } } }
            if !g.sims.is_empty() { g.stats[2] += 1; g.stats[3] += t0.elapsed().as_micros() as u64; }
            let rooms = &g.rooms; g.sims.retain(|r, s| rooms.contains_key(r) && !s.idle(now)); }
        for (l, w) in out { self.deliver(&l, &w); }
    }
    /// drop silent members and empty rooms; a status line now and then
    fn sweep(&self) {
        let now = self.now(); let mut g = self.rooms.lock().unwrap();
        let mut gone = Vec::new();
        for (r, list) in g.rooms.iter() { for m in list { if now - m.seen > IDLE { gone.push((*r, m.id)); } } }
        for (r, i) in gone { remove(&mut g, r, i); }
        let members: usize = g.rooms.values().map(|l| l.len()).sum();
        println!("rooms {} members {} relayed {} msgs {} kB, AI cars {:.3} ms a step", g.rooms.len(), members, g.stats[0], g.stats[1] / 1000, g.stats[3] as f64 / 1000.0 / g.stats[2].max(1) as f64);
    }
}
fn remove(g: &mut Rooms, room: u32, id: u32) {
    if let Some(list) = g.rooms.get_mut(&room) { list.retain(|m| m.id != id); if list.is_empty() { g.rooms.remove(&room); } }
    g.by_udp.retain(|_, v| *v != (room, id));
}

fn ws_client(relay: Arc<Relay>, stream: std::net::TcpStream) {
    let _ = stream.set_nodelay(true);
    let Ok(mut ws) = tungstenite::accept(stream) else { return };
    let _ = ws.get_ref().set_read_timeout(Some(Duration::from_millis(4)));
    let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = channel(); let mut me: Option<(u32, u32)> = None;
    loop {
        match ws.read() {
            Ok(tungstenite::Message::Binary(b)) => relay.handle(&b, Link::Ws(tx.clone()), &mut me),
            Ok(tungstenite::Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
        let mut sent = false;
        while let Ok(b) = rx.try_recv() { if ws.write(tungstenite::Message::Binary(b.into())).is_err() { sent = false; break; } sent = true; }
        if sent && ws.flush().is_err() { break; }
    }
    if let Some((r, i)) = me { let mut g = relay.rooms.lock().unwrap(); remove(&mut g, r, i); }
}

fn main() {
    let udp_bind = std::env::var("RELAY_UDP").unwrap_or_else(|_| "0.0.0.0:7777".into());
    let ws_bind = std::env::var("RELAY_WS").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let udp = UdpSocket::bind(&udp_bind).expect("udp bind");
    let relay = Arc::new(Relay { t0: Instant::now(), rooms: Mutex::new(Rooms::default()), udp });
    println!("fogwood relay: udp {udp_bind}, websocket {ws_bind}");
    { let r = relay.clone(); std::thread::spawn(move || loop { std::thread::sleep(Duration::from_secs(30)); r.sweep(); }); }
    { let r = relay.clone(); std::thread::spawn(move || loop { std::thread::sleep(Duration::from_millis(10)); r.sims(); }); }
    { let r = relay.clone(); let l = TcpListener::bind(&ws_bind).expect("ws bind");
        std::thread::spawn(move || for s in l.incoming().flatten() { let r = r.clone(); std::thread::spawn(move || ws_client(r, s)); }); }
    let mut buf = [0u8; 1500]; let mut none = None;
    loop { if let Ok((n, a)) = relay.udp.recv_from(&mut buf) { relay.handle(&buf[..n], Link::Udp(a), &mut none); } }
}
