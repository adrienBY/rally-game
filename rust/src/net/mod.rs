//! Online play without a game server.
//!
//! Phones meet in a room on public MQTT brokers (topic `fogwood/v1/<room code>/m`): presence, the race start and,
//! when needed, game packets go through there. Each phone also learns its public UDP address from a STUN server and
//! shares it with the room; peers then punch through their routers and exchange car states directly, which is faster.
//! Peers that cannot reach each other directly keep using the broker as a relay.
//!
//! The host (whoever created the room) is the clock and decides when a race starts and on which stage. Every phone is
//! the authority for its own car and shows the others slightly in the past, interpolated between received states.
//!
//! The session lives in a global so it survives stage switches (the game object is rebuilt for every stage).
pub mod mqtt;
pub mod proto;
pub mod udp;

use proto::*;
use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::time::Instant;

pub const BROKERS: [&str; 3] = ["broker.emqx.io:1883", "broker.hivemq.com:1883", "test.mosquitto.org:1883"];
const PEER_TIMEOUT: f64 = 8.0;
const DIRECT_TIMEOUT: f64 = 5.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Link { Connecting, Relay, Direct }

pub struct Peer {
    pub id: u32, pub name: String, pub map: String, pub host: bool, pub racing: bool, pub first_seen: f64, pub last_seen: f64,
    cands: Vec<SocketAddr>, punch_from: f64, last_probe: f64, pub direct: Option<SocketAddr>, last_udp: f64,
    seen: VecDeque<u32>, pub buf: VecDeque<CarSnap>, pub rtt: f64,
}
impl Peer {
    pub fn link(&self) -> Link { if self.direct.is_some() { Link::Direct } else if self.last_seen > 0.0 { Link::Relay } else { Link::Connecting } }
    fn dup(&mut self, seq: u32) -> bool { if self.seen.contains(&seq) { return true; } self.seen.push_back(seq); if self.seen.len() > 96 { self.seen.pop_front(); } false }
}

pub struct Session {
    pub code: u32, pub me: u32, pub name: String, pub is_host: bool, t0: Instant, seq: u32,
    brokers: Vec<mqtt::Broker>, inbox: Receiver<(String, Vec<u8>)>, topic: String,
    sock: Option<UdpSocket>, stun: Option<Receiver<Option<SocketAddrV4>>>, pub public: Option<SocketAddrV4>, local: Option<SocketAddrV4>, stun_done: bool,
    pub peers: HashMap<u32, Peer>, pub host_id: Option<u32>,
    offset: f64, best_rtt: f64, last_ping: f64, pings: HashMap<u32, f64>,
    last_hello: f64, last_car_direct: f64, last_car_relay: f64,
    start_out: Option<(Start, f64)>, pub start_in: Option<Start>, last_start_go: f64,
    pub map: String, pub racing: bool, no_direct: bool,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

pub fn with<T>(f: impl FnOnce(&mut Session) -> T) -> Option<T> { SESSION.lock().ok()?.as_mut().map(f) }
pub fn active() -> bool { SESSION.lock().map(|s| s.is_some()).unwrap_or(false) }
pub fn host(name: &str, map: &str) -> u32 { let code = rand_u32() % 900000 + 100000; open(code, name, map, true); code }
pub fn join(code: u32, name: &str, map: &str) { open(code, name, map, false); }
pub fn leave() { if let Ok(mut s) = SESSION.lock() { if let Some(x) = s.as_mut() { x.bye(); } *s = None; } }

fn rand_u32() -> u32 {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(7) as u64;
    let mut x = n ^ 0x9E3779B97F4A7C15 ^ (std::process::id() as u64) << 32; x ^= x >> 33; x = x.wrapping_mul(0xff51afd7ed558ccd); x ^= x >> 33; x = x.wrapping_mul(0xc4ceb9fe1a85ec53); x ^= x >> 33; x as u32
}

fn open(code: u32, name: &str, map: &str, is_host: bool) {
    leave();
    let me = rand_u32().max(1); let topic = format!("fogwood/v1/{}/m", code);
    let (tx, inbox) = channel();
    let list: Vec<String> = match std::env::var("FW_BROKERS") { Ok(v) => v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(), Err(_) => BROKERS.iter().map(|s| s.to_string()).collect() };
    let brokers = list.iter().enumerate().map(|(k, b)| mqtt::Broker::start(b, format!("fw{:08x}{}", me, k), topic.clone(), tx.clone())).collect();
    let sock = udp::bind();
    let local = sock.as_ref().and_then(|s| { let port = s.local_addr().ok()?.port();
        let ip = if std::env::var("FW_LOOPBACK").is_ok() { Some(std::net::Ipv4Addr::LOCALHOST) } else { udp::local_ip() }; Some(SocketAddrV4::new(ip?, port)) });
    let stun = sock.as_ref().and_then(|s| s.try_clone().ok()).filter(|_| std::env::var("FW_NOSTUN").is_err()).map(|s2| {
        let (t, r) = channel(); std::thread::Builder::new().name("stun".into()).spawn(move || { let _ = t.send(udp::stun_query(&s2)); }).ok(); r });
    let stun_done = stun.is_none();
    let s = Session { code, me, name: name.to_string(), is_host, t0: Instant::now(), seq: 0, brokers, inbox, topic, sock, stun, public: None, local, stun_done,
        peers: HashMap::new(), host_id: if is_host { Some(me) } else { None }, offset: 0.0, best_rtt: 9.0, last_ping: -9.0, pings: HashMap::new(),
        last_hello: -9.0, last_car_direct: -9.0, last_car_relay: -9.0, start_out: None, start_in: None, last_start_go: -1.0, map: map.to_string(), racing: false, no_direct: std::env::var("FW_NODIRECT").is_ok() };
    if let Ok(mut g) = SESSION.lock() { *g = Some(s); }
}

impl Session {
    pub fn now(&self) -> f64 { self.t0.elapsed().as_secs_f64() }
    /// the host's clock (race times and the start moment are on it)
    pub fn host_now(&self) -> f64 { self.now() + self.offset }
    pub fn synced(&self) -> bool { self.is_host || self.best_rtt < 5.0 }
    pub fn brokers_up(&self) -> usize { self.brokers.iter().filter(|b| b.is_up()).count() }
    fn env(&mut self, kind: u8, to: u32) -> Env { self.seq = self.seq.wrapping_add(1); Env { kind, room: self.code, from: self.me, to, seq: self.seq } }
    fn hello_msg(&mut self) -> Vec<u8> {
        let mut cands = Vec::new(); for a in [self.local, self.public].into_iter().flatten() { cands.push((a.ip().octets(), a.port())); }
        let h = Hello { host: self.is_host, racing: self.racing, name: self.name.clone(), map: self.map.clone(), cands }; let e = self.env(K_HELLO, 0); hello(e, &h)
    }
    /// to every broker (lobby traffic: small, and it must reach phones that only got through to one of them)
    fn relay_all(&self, b: &[u8]) { for k in &self.brokers { k.publish(&self.topic, b); } }
    /// to the first broker that is up (game traffic)
    fn relay_one(&self, b: &[u8]) { if let Some(k) = self.brokers.iter().find(|k| k.is_up()) { k.publish(&self.topic, b); } }
    fn send_to(&self, to: u32, b: &[u8]) {
        match self.peers.get(&to).and_then(|p| p.direct) { Some(a) => { if let Some(s) = &self.sock { let _ = s.send_to(b, a); } } None => self.relay_one(b) }
    }
    fn bye(&mut self) { let e = self.env(K_BYE, 0); let b = bare(e); self.relay_all(&b); for p in self.peers.values() { if let (Some(a), Some(s)) = (p.direct, &self.sock) { let _ = s.send_to(&b, a); } } }

    /// Network work for one frame: read the broker queue and the UDP socket, presence, clock pings, punching, start repeats.
    pub fn poll(&mut self) {
        let now = self.now();
        if let Some(r) = &self.stun { if let Ok(a) = r.try_recv() { self.public = a; self.stun_done = true; self.stun = None; self.last_hello = -9.0; } }
        if !self.stun_done && now > 4.0 { self.stun_done = true; self.stun = None; }
        // broker messages
        let mut msgs = Vec::new(); while let Ok((t, p)) = self.inbox.try_recv() { if t == self.topic { msgs.push((p, None)); } }
        // direct messages (only once STUN has finished with the socket)
        if self.stun_done { if let Some(s) = &self.sock { let mut buf = [0u8; 1500]; while let Ok((n, a)) = s.recv_from(&mut buf) { msgs.push((buf[..n].to_vec(), Some(a))); } } }
        for (b, from) in msgs { self.handle(&b, from, now); }
        // presence
        if now - self.last_hello > 1.0 { self.last_hello = now; let h = self.hello_msg(); self.relay_all(&h); }
        self.peers.retain(|_, p| now - p.last_seen < PEER_TIMEOUT);
        if let Some(h) = self.host_id { if !self.is_host && !self.peers.contains_key(&h) { self.host_id = None; } }
        // punching and keep-alive
        let no_direct = self.no_direct; let (code, me) = (self.code, self.me); let mut out: Vec<(SocketAddr, Vec<u8>)> = Vec::new();
        for p in self.peers.values_mut() {
            if p.direct.is_some() && now - p.last_udp > DIRECT_TIMEOUT { p.direct = None; p.punch_from = now; }
            let probing = p.direct.is_none() && !p.cands.is_empty() && now - p.punch_from < 20.0 && !no_direct;
            let keep = p.direct.is_some() && now - p.last_probe > 1.5;
            if (probing && now - p.last_probe > 0.25) || keep { p.last_probe = now;
                let e = Env { kind: K_PROBE, room: code, from: me, to: p.id, seq: 0 }; let b = bare(e);
                match p.direct { Some(a) => out.push((a, b)), None => for a in &p.cands { out.push((*a, b.clone())); } } }
        }
        if self.stun_done { if let Some(s) = &self.sock { for (a, b) in out { let _ = s.send_to(&b, a); } } }
        // clock: guests ping the host
        if !self.is_host { if let Some(h) = self.host_id { if now - self.last_ping > if self.best_rtt > 5.0 { 0.3 } else { 1.0 } { self.last_ping = now;
            let e = self.env(K_PING, h); self.pings.insert(e.seq, now); let b = time_pair(e, now, 0.0); self.send_to(h, &b); if self.pings.len() > 32 { self.pings.clear(); } } } }
        // repeat the start a few times (broker messages can be lost)
        if let Some((s, until)) = self.start_out.clone() { if now < until { if ((now * 4.0) as i64) % 2 == 0 { let e = self.env(K_START, 0); let b = start(e, &s); self.relay_all(&b); } } else { self.start_out = None; } }
    }

    fn handle(&mut self, b: &[u8], from: Option<SocketAddr>, now: f64) {
        let Some((e, mut r)) = read_env(b) else { return };
        if e.room != self.code || e.from == self.me || (e.to != 0 && e.to != self.me) { return; }
        if e.kind == K_BYE { self.peers.remove(&e.from); return; }
        let p = self.peers.entry(e.from).or_insert_with(|| Peer { id: e.from, name: "?".into(), map: String::new(), host: false, racing: false, first_seen: now, last_seen: now,
            cands: Vec::new(), punch_from: now, last_probe: -9.0, direct: None, last_udp: -9.0, seen: VecDeque::new(), buf: VecDeque::new(), rtt: -1.0 });
        p.last_seen = now;
        if let Some(a) = from { p.last_udp = now; if e.kind == K_PROBE || e.kind == K_PROBE_ACK { if p.direct != Some(a) { p.direct = Some(a); } } }
        if from.is_none() && e.seq != 0 && p.dup(e.seq) { return; }
        match e.kind {
            K_HELLO => { if let Some(h) = read_hello(&mut r) {
                let cands: Vec<SocketAddr> = h.cands.iter().map(|(ip, port)| SocketAddr::V4(SocketAddrV4::new((*ip).into(), *port))).collect();
                if cands != p.cands { p.cands = cands; p.punch_from = now; }
                p.name = h.name; p.map = h.map; p.host = h.host; p.racing = h.racing;
                if h.host && !self.is_host { self.host_id = Some(e.from); } } }
            K_PROBE => { if let (Some(a), Some(s)) = (from, &self.sock) { let b = bare(Env { kind: K_PROBE_ACK, room: self.code, from: self.me, to: e.from, seq: 0 }); let _ = s.send_to(&b, a); } }
            K_PING => { if self.is_host { if let Some(t) = r.f64() { let o = self.env(K_PONG, e.from); let b = time_pair(Env { seq: e.seq, ..o }, t, now); self.send_to(e.from, &b); } } }
            K_PONG => { if let (Some(t), Some(ht)) = (r.f64(), r.f64()) { if self.pings.remove(&e.seq).is_some() || true {
                let rtt = now - t; if rtt >= 0.0 && rtt < 3.0 { if let Some(p) = self.peers.get_mut(&e.from) { p.rtt = if p.rtt < 0.0 { rtt } else { p.rtt * 0.8 + rtt * 0.2 }; }
                    // keep the estimate from the quickest round trip; slowly forget it so a better path can take over
                    self.best_rtt += 0.002; if rtt <= self.best_rtt { self.best_rtt = rtt; self.offset = ht - (t + rtt / 2.0); } } } } }
            K_START => { if !self.is_host { if let Some(s) = read_start(&mut r) { if (s.go_at - self.last_start_go).abs() > 0.01 { self.last_start_go = s.go_at; self.start_in = Some(s); } } } }
            K_CAR => { if let Some(c) = read_car(&mut r) { if let Some(p) = self.peers.get_mut(&e.from) {
                if p.buf.back().map(|l| c.t > l.t).unwrap_or(true) { p.buf.push_back(c); }
                while p.buf.len() > 2 && p.buf.front().map(|f| c.t - f.t > 1.5).unwrap_or(false) { p.buf.pop_front(); } } } }
            _ => {}
        }
    }

    /// Host: everyone in the room races on `map`, lights out `delay` seconds from now. Returns the start for the host itself.
    pub fn start_race(&mut self, map: &str, laps: u8, delay: f64) -> Option<Start> {
        if !self.is_host { return None; }
        let mut ids: Vec<(f64, u32)> = self.peers.values().map(|p| (p.first_seen, p.id)).collect(); ids.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut slots = vec![(self.me, 0u8)]; for (k, (_, id)) in ids.iter().enumerate().take(11) { slots.push((*id, (k + 1) as u8)); }
        let s = Start { map: map.to_string(), laps, go_at: self.host_now() + delay, slots }; self.start_out = Some((s.clone(), self.now() + 3.0)); Some(s)
    }

    /// Our car, at most 30 times a second to direct peers and 15 through the relay.
    pub fn send_car(&mut self, mut c: CarSnap) {
        let now = self.now(); c.t = self.host_now();
        let direct: Vec<(u32, SocketAddr)> = self.peers.values().filter_map(|p| p.direct.map(|a| (p.id, a))).collect();
        let relay_needed = self.peers.values().any(|p| p.direct.is_none());
        if !direct.is_empty() && now - self.last_car_direct >= 1.0 / 31.0 { self.last_car_direct = now;
            for (id, a) in direct { let e = self.env(K_CAR, id); let b = car(e, &c); if let Some(s) = &self.sock { let _ = s.send_to(&b, a); } } }
        if relay_needed && now - self.last_car_relay >= 1.0 / 15.5 { self.last_car_relay = now; let e = self.env(K_CAR, 0); let b = car(e, &c); self.relay_one(&b); }
    }

    /// Where a peer's car is shown now: a little in the past (more over the relay), interpolated; extrapolated briefly if packets are late.
    pub fn remote(&self, id: u32) -> Option<CarSnap> {
        let p = self.peers.get(&id)?; let delay = if p.direct.is_some() { 0.1 } else { 0.25 }; let t = self.host_now() - delay; snap_at(&p.buf, t)
    }
}

fn lerp_a(a: f32, b: f32, k: f32) -> f32 { let mut d = b - a; while d > std::f32::consts::PI { d -= 2.0 * std::f32::consts::PI; } while d < -std::f32::consts::PI { d += 2.0 * std::f32::consts::PI; } a + d * k }
pub fn snap_at(buf: &VecDeque<CarSnap>, t: f64) -> Option<CarSnap> {
    let last = *buf.back()?;
    if t >= last.t { let dt = ((t - last.t).min(0.3)) as f32; let mut c = last; c.x += c.vx * dt; c.y += c.vy * dt; c.z += c.vz * dt; c.h += c.r * dt; return Some(c); }
    let first = *buf.front()?; if t <= first.t { return Some(first); }
    for w in 0..buf.len() - 1 { let (a, b) = (buf[w], buf[w + 1]); if t >= a.t && t <= b.t {
        let k = ((t - a.t) / (b.t - a.t).max(1e-6)) as f32; let l = |x: f32, y: f32| x + (y - x) * k;
        let mut c = b; c.t = t; c.x = l(a.x, b.x); c.y = l(a.y, b.y); c.z = l(a.z, b.z); c.h = lerp_a(a.h, b.h, k); c.pitch = l(a.pitch, b.pitch); c.roll = l(a.roll, b.roll);
        c.steer = l(a.steer, b.steer); c.vx = l(a.vx, b.vx); c.vz = l(a.vz, b.vz); c.u = l(a.u, b.u); c.rpm = l(a.rpm as f32, b.rpm as f32) as u16;
        for q in 0..4 { c.d[q] = l(a.d[q], b.d[q]); c.ang[q] = l(a.ang[q], b.ang[q]); } return Some(c); } }
    Some(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interpolation() {
        let mut b = VecDeque::new();
        b.push_back(CarSnap { t: 1.0, x: 0.0, h: 3.1, vx: 10.0, ..Default::default() });
        b.push_back(CarSnap { t: 1.1, x: 1.0, h: -3.1, vx: 10.0, ..Default::default() });
        let c = snap_at(&b, 1.05).unwrap(); assert!((c.x - 0.5).abs() < 1e-5); assert!(c.h.abs() > 3.1);   // heading wraps the short way
        let c = snap_at(&b, 1.2).unwrap(); assert!((c.x - 2.0).abs() < 1e-4);   // extrapolated
    }
}
