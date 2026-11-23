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
//! v2 (WELCOME caps bit 1, same as relay-cf/worker.js): JOIN may carry a flags byte (bit 0: reads 0x88 coalesced
//! messages, bits 1-2: lane); 0x09 SEND2 to u32, seq u16, payload is passed on once whichever lane it came by, to every
//! lane of the others; v2 sockets get what is due within 16 ms of their last write together as 0x88 + [u16 len][msg]...
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
const ROOM_MAX: usize = 24;
const FLUSH: f64 = 0.016;
const IDLE: f64 = 20.0;
const RATE: f64 = 200.0;
const BURST: f64 = 50.0;

#[derive(Clone)]
enum Link { Udp(SocketAddr), Ws(Sender<Vec<u8>>, u64) }
/// one socket of a member: its lane, link, whether it reads 0x88, what waits for it and when it was last written to
struct Sock { lane: u8, link: Link, v2: bool, rx: bool, outq: Vec<Vec<u8>>, out_n: usize, last_flush: f64 }
/// a member's sockets and the v2 packet seqs seen lately
struct Member { id: u32, links: Vec<Sock>, seq_seen: std::collections::VecDeque<u16>, seen: f64, tokens: f64, last: f64 }
#[derive(Default)]
struct Rooms { rooms: HashMap<u32, Vec<Member>>, by_udp: HashMap<SocketAddr, (u32, u32)>, stats: [u64; 4], sims: HashMap<u32, fogwood_core::server::Sim> }

struct Relay { t0: Instant, rooms: Mutex<Rooms>, udp: UdpSocket }
impl Relay {
    fn now(&self) -> f64 { self.t0.elapsed().as_secs_f64() }
    fn deliver(&self, link: &Link, b: &[u8]) { match link { Link::Udp(a) => { let _ = self.udp.send_to(b, a); } Link::Ws(tx, _) => { let _ = tx.send(b.to_vec()); } } }

    /// one message from a client; `key` identifies the sender's link (for UDP, its address)
    fn handle(&self, b: &[u8], link: Link, ws_member: &mut Option<(u32, u32)>) {
        let now = self.now(); if b.is_empty() { return; }
        let u32_at = |i: usize| -> Option<u32> { b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])) };
        let mut out: Vec<(Link, Vec<u8>)> = Vec::new();
        {
            let mut g = self.rooms.lock().unwrap();
            let who = match &link { Link::Udp(a) => g.by_udp.get(a).copied(), Link::Ws(..) => *ws_member };
            match b[0] {
                0x01 => { let (Some(room), Some(id)) = (u32_at(1), u32_at(5)) else { return };
                    if let Some((r0, i0)) = who { if (r0, i0) != (room, id) { remove(&mut g, r0, i0); } }
                    let list = g.rooms.entry(room).or_default();
                    let f = b.get(9).copied().unwrap_or(0); let lane = (f >> 1) & 3; let v2 = f & 1 != 0; let rx = b.len() < 10 || f & 8 != 0;
                    match list.iter_mut().find(|m| m.id == id) {
                        Some(m) => { match m.links.iter_mut().find(|l| l.lane == lane) { Some(l) => { l.link = link.clone(); l.v2 = v2; l.rx = rx; } None => m.links.push(Sock { lane, link: link.clone(), v2, rx, outq: Vec::new(), out_n: 0, last_flush: 0.0 }) } m.seen = now; }
                        None => { if list.len() >= ROOM_MAX { return; } list.push(Member { id, links: vec![Sock { lane, link: link.clone(), v2, rx, outq: Vec::new(), out_n: 0, last_flush: 0.0 }], seq_seen: Default::default(), seen: now, tokens: BURST, last: now }); }
                    }
                    let n = list.len() as u8;
                    if let Link::Udp(a) = &link { g.by_udp.insert(*a, (room, id)); } else { *ws_member = Some((room, id)); }
                    let caps = if matches!(link, Link::Ws(..)) { 1u8 | 2 } else { 2 };   // AI cars: the stage only fits a WebSocket; v2 on both
                    let mut w = vec![0x81]; w.extend_from_slice(&now.to_le_bytes()); w.push(n); w.push(caps); out.push((link.clone(), w)); }
                0x02 | 0x09 => { let Some((room, id)) = who else { return }; let Some(to) = u32_at(1) else { return };
                    let v2 = b[0] == 0x09; if v2 && b.len() < 7 { return; } let payload = &b[if v2 { 7 } else { 5 }..];
                    if payload.len() > MAX_PAYLOAD { return; }
                    let Some(list) = g.rooms.get_mut(&room) else { return };
                    let Some(me) = list.iter_mut().find(|m| m.id == id) else { return };
                    if v2 { let q = u16::from_le_bytes([b[5], b[6]]); if me.seq_seen.contains(&q) { return; } me.seq_seen.push_back(q); if me.seq_seen.len() > 256 { me.seq_seen.pop_front(); } }
                    me.tokens = (me.tokens + (now - me.last) * RATE).min(BURST); me.last = now; me.seen = now;
                    if me.tokens < 1.0 { return; } me.tokens -= 1.0;
                    let mut w = Vec::with_capacity(payload.len() + 5); w.push(0x82); w.extend_from_slice(&id.to_le_bytes()); w.extend_from_slice(payload);
                    if to != SERVER_ID { for m in list.iter_mut() { if m.id != id && (to == 0 || m.id == to) { for l in m.links.iter_mut().filter(|l| l.rx) { if let Some(x) = queue(l, &w, now) { out.push(x); } } } } }
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
                    if let Some(list) = g.rooms.get_mut(room) { for m in list.iter_mut() { if to == 0 || m.id == to { for l in m.links.iter_mut().filter(|l| l.rx) { if let Some(x) = queue(l, &w, now) { out.push(x); } } } } } } }
            // what waited its 16 ms
            for list in g.rooms.values_mut() { for m in list.iter_mut() { for l in m.links.iter_mut() { if !l.outq.is_empty() && now - l.last_flush >= FLUSH { out.push(flush(l, now)); } } } }
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
/// a message for a member: written now (an older build, or nothing written in the last 16 ms), or queued for the next write
fn queue(l: &mut Sock, w: &[u8], now: f64) -> Option<(Link, Vec<u8>)> {
    if !l.v2 { return Some((l.link.clone(), w.to_vec())); }
    if l.outq.is_empty() && now - l.last_flush >= FLUSH { l.last_flush = now; return Some((l.link.clone(), w.to_vec())); }
    l.out_n += w.len() + 2; l.outq.push(w.to_vec());
    if l.out_n > 1100 { return Some(flush(l, now)); }
    None
}
fn flush(l: &mut Sock, now: f64) -> (Link, Vec<u8>) {
    l.last_flush = now; let q = std::mem::take(&mut l.outq); l.out_n = 0;
    if q.len() == 1 { return (l.link.clone(), q.into_iter().next().unwrap()); }
    let mut w = vec![0x88]; for x in &q { w.extend_from_slice(&(x.len() as u16).to_le_bytes()); w.extend_from_slice(x); } (l.link.clone(), w)
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
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1); let tok = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    loop {
        match ws.read() {
            Ok(tungstenite::Message::Binary(b)) => relay.handle(&b, Link::Ws(tx.clone(), tok), &mut me),
            Ok(tungstenite::Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => break,
        }
        let mut sent = false;
        while let Ok(b) = rx.try_recv() { if ws.write(tungstenite::Message::Binary(b.into())).is_err() { sent = false; break; } sent = true; }
        if sent && ws.flush().is_err() { break; }
    }
    // this socket only (the member may have another lane)
    if let Some((r, i)) = me { let mut g = relay.rooms.lock().unwrap();
        let empty = g.rooms.get_mut(&r).and_then(|l| l.iter_mut().find(|m| m.id == i)).map(|m| { m.links.retain(|l| !matches!(&l.link, Link::Ws(_, t) if *t == tok)); m.links.is_empty() }).unwrap_or(false);
        if empty { remove(&mut g, r, i); } }
}

fn main() {
    let udp_bind = std::env::var("RELAY_UDP").unwrap_or_else(|_| "0.0.0.0:7777".into());
    let ws_bind = std::env::var("RELAY_WS").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let udp = UdpSocket::bind(&udp_bind).expect("udp bind");
    let relay = Arc::new(Relay { t0: Instant::now(), rooms: Mutex::new(Rooms::default()), udp });
    println!("fogwood relay: udp {udp_bind}, websocket {ws_bind}");
    { let r = relay.clone(); std::thread::spawn(move || loop { std::thread::sleep(Duration::from_secs(30)); r.sweep(); }); }
    { let r = relay.clone(); std::thread::spawn(move || loop { std::thread::sleep(Duration::from_millis(4)); r.sims(); }); }
    { let r = relay.clone(); let l = TcpListener::bind(&ws_bind).expect("ws bind");
        std::thread::spawn(move || for s in l.incoming().flatten() { let r = r.clone(); std::thread::spawn(move || ws_client(r, s)); }); }
    let mut buf = [0u8; 1500]; let mut none = None;
    loop { if let Ok((n, a)) = relay.udp.recv_from(&mut buf) { relay.handle(&buf[..n], Link::Udp(a), &mut none); } }
}
