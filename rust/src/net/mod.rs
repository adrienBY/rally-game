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
const PEER_TIMEOUT: f64 = 15.0;
const DIRECT_TIMEOUT: f64 = 6.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Link { Connecting, Relay, Direct }

pub struct Peer {
    pub id: u32, pub name: String, pub map: String, pub host: bool, pub racing: bool, pub first_seen: f64, pub last_seen: f64,
    cands: Vec<SocketAddr>, punch_from: f64, last_probe: f64, pub direct: Option<SocketAddr>, last_udp: f64,
    seen: VecDeque<u32>, pub rtt: f64, last_tx: f64,
    /// newest state from this peer, the smoothing offset still being blended out (x, z, heading) and when it was last shown
    pub latest: Option<CarSnap>, last_seq: u8, offset: [f32; 3], shown_t: f64,
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
    offset: f64, offset_target: f64, best_rtt: f64, last_ping: f64, pings: HashMap<u32, f64>,
    last_hello: f64, last_start_tx: f64, sent: Option<CarSnap>, car_seq: u8, pong_n: u32,
    start_out: Option<(Start, f64)>, pub start_in: Option<Start>, last_start_go: f64,
    pub map: String, pub racing: bool, no_direct: bool,
    /// bumps other phones gave our car, waiting for the game to apply them
    pub hits_in: Vec<(u32, [f32; 5], f32)>,
    /// estimated bytes on the wire, including UDP/IP and TCP/IP headers: [udp up, udp down, broker up, broker down]
    /// send thresholds: position m, heading rad, speed m/s, longest gap s
    thr: [f32; 4], loss: f64, lag: [f64; 2], held: VecDeque<(f64, Vec<u8>, Option<SocketAddr>)>, held_last: f64,
    pub traffic: std::cell::Cell<[u64; 4]>, pub packets: std::cell::Cell<[u64; 4]>,
}
const UDP_OVERHEAD: u64 = 28; const TCP_OVERHEAD: u64 = 40;

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
    let me = rand_u32().max(1); let topic = format!("fw1/{}", code);
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
        peers: HashMap::new(), host_id: if is_host { Some(me) } else { None }, offset: 0.0, offset_target: 0.0, best_rtt: 9.0, last_ping: -9.0, pings: HashMap::new(),
        last_hello: -9.0, last_start_tx: -9.0, sent: None, car_seq: 0, pong_n: 0, start_out: None, start_in: None, last_start_go: -1.0, map: map.to_string(), racing: false, hits_in: Vec::new(), no_direct: std::env::var("FW_NODIRECT").is_ok(), traffic: Default::default(), packets: Default::default(),
        lag: std::env::var("FW_LAG").ok().and_then(|v| { let q: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect(); if q.len() == 2 { Some([q[0], q[1]]) } else { None } }).unwrap_or([0.0, 0.0]), held: VecDeque::new(), held_last: 0.0,
        loss: std::env::var("FW_LOSS").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0),
        thr: std::env::var("FW_THR").ok().and_then(|v| { let q: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect(); if q.len() == 4 { Some([q[0], q[1], q[2], q[3]]) } else { None } }).unwrap_or([0.15, 0.026, 0.8, 0.4]) };
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
    fn count(&self, k: usize, bytes: u64) { let mut t = self.traffic.get(); t[k] += bytes; self.traffic.set(t); let mut p = self.packets.get(); p[k] += 1; self.packets.set(p); }
    fn mqtt_bytes(&self, payload: usize) -> u64 { (payload + self.topic.len() + 4) as u64 + TCP_OVERHEAD }
    fn udp(&self, b: &[u8], a: SocketAddr) { if let Some(s) = &self.sock { if s.send_to(b, a).is_ok() { self.count(0, b.len() as u64 + UDP_OVERHEAD); } } }
    /// to every broker (lobby traffic: small, and it must reach phones that only got through to one of them)
    fn relay_all(&self, b: &[u8]) { for k in &self.brokers { if k.is_up() { k.publish(&self.topic, b); self.count(2, self.mqtt_bytes(b.len())); } } }
    /// to the first broker that is up (game traffic)
    fn relay_one(&self, b: &[u8]) { if let Some(k) = self.brokers.iter().find(|k| k.is_up()) { k.publish(&self.topic, b); self.count(2, self.mqtt_bytes(b.len())); } }
    fn send_to(&self, to: u32, b: &[u8]) {
        match self.peers.get(&to).and_then(|p| p.direct) { Some(a) => self.udp(b, a), None => self.relay_one(b) }
    }
    fn bye(&mut self) { let e = self.env(K_BYE, 0); let b = bare(e); self.relay_all(&b); for p in self.peers.values() { if let Some(a) = p.direct { self.udp(&b, a); } } }

    /// Network work for one frame: read the broker queue and the UDP socket, presence, clock pings, punching, start repeats.
    pub fn poll(&mut self) {
        let now = self.now();
        // clock corrections are eased in (at most 3 ms per frame) so other cars never jump when a better estimate arrives
        let d = self.offset_target - self.offset; self.offset += d.clamp(-0.003, 0.003);
        if let Some(r) = &self.stun { if let Ok(a) = r.try_recv() { self.public = a; self.stun_done = true; self.stun = None; self.last_hello = -9.0; } }
        if !self.stun_done && now > 4.0 { self.stun_done = true; self.stun = None; }
        // broker messages
        let mut msgs = Vec::new(); while let Ok((t, p)) = self.inbox.try_recv() { self.count(3, self.mqtt_bytes(p.len())); if t == self.topic { msgs.push((p, None)); } }
        // direct messages (only once STUN has finished with the socket)
        if self.stun_done { if let Some(s) = &self.sock { let mut buf = [0u8; 1500]; while let Ok((n, a)) = s.recv_from(&mut buf) { msgs.push((buf[..n].to_vec(), Some(a))); self.count(1, n as u64 + UDP_OVERHEAD); } } }
        // test hooks: FW_LOSS drops a share of what arrives, FW_LAG="mean_ms,jitter_ms" holds it back like a slow network
        for (b, from) in msgs { if self.loss > 0.0 && (rand_u32() as f64 / u32::MAX as f64) < self.loss { continue; }
            if self.lag[0] > 0.0 { let j = (rand_u32() as f64 / u32::MAX as f64 * 2.0 - 1.0) * self.lag[1]; let mut at = now + (self.lag[0] + j) / 1000.0;
                if from.is_none() { at = at.max(self.held_last); self.held_last = at; }   // the broker link is TCP: late, never out of order
                let k = self.held.iter().position(|h| h.0 > at).unwrap_or(self.held.len()); self.held.insert(k, (at, b, from)); } else { self.handle(&b, from, now); } }
        while self.held.front().map(|h| h.0 <= now).unwrap_or(false) { let (_, b, from) = self.held.pop_front().unwrap(); self.handle(&b, from, now); }
        // presence
        // presence: every 2 s in the lobby, every 8 s while racing (car states already show who is there)
        if now - self.last_hello > if self.racing { 8.0 } else { 2.0 } { self.last_hello = now; let h = self.hello_msg(); self.relay_all(&h); }
        self.peers.retain(|_, p| now - p.last_seen < PEER_TIMEOUT);
        if let Some(h) = self.host_id { if !self.is_host && !self.peers.contains_key(&h) { self.host_id = None; } }
        // punching and keep-alive
        let no_direct = self.no_direct; let (code, me) = (self.code, self.me); let mut out: Vec<(SocketAddr, Vec<u8>)> = Vec::new();
        for p in self.peers.values_mut() {
            if p.direct.is_some() && now - p.last_udp > DIRECT_TIMEOUT { p.direct = None; p.punch_from = now; }
            let probing = p.direct.is_none() && !p.cands.is_empty() && now - p.punch_from < 20.0 && !no_direct;
            let keep = p.direct.is_some() && now - p.last_probe.max(p.last_tx) > 2.5;   // car states keep the path open while racing
            if (probing && now - p.last_probe > 0.25) || keep { p.last_probe = now;
                let e = Env { kind: K_PROBE, room: code, from: me, to: p.id, seq: 0 }; let b = bare(e);
                match p.direct { Some(a) => out.push((a, b)), None => for a in &p.cands { out.push((*a, b.clone())); } } }
        }
        if self.stun_done { for (a, b) in out { self.udp(&b, a); } }
        // clock: guests ping the host
        if !self.is_host { if let Some(h) = self.host_id { if now - self.last_ping > if self.pong_n < 6 { 0.3 } else { 5.0 } { self.last_ping = now;
            let e = self.env(K_PING, h); self.pings.insert(e.seq, now); let b = time_pair(e, now, 0.0); self.send_to(h, &b); if self.pings.len() > 32 { self.pings.clear(); } } } }
        // repeat the start a few times (broker messages can be lost)
        if let Some((s, until)) = self.start_out.clone() { if now < until { if now - self.last_start_tx >= 0.5 { self.last_start_tx = now; let e = self.env(K_START, 0); let b = start(e, &s); self.relay_all(&b); } } else { self.start_out = None; } }
    }

    fn handle(&mut self, b: &[u8], from: Option<SocketAddr>, now: f64) {
        if b.first() == Some(&K_CAR2) { self.handle_car(b, from, now); return; }
        let Some((e, mut r)) = read_env(b) else { return };
        if e.room != self.code || e.from == self.me || (e.to != 0 && e.to != self.me) { return; }
        if e.kind == K_BYE { self.peers.remove(&e.from); return; }
        let p = self.peers.entry(e.from).or_insert_with(|| Peer { id: e.from, name: "?".into(), map: String::new(), host: false, racing: false, first_seen: now, last_seen: now,
            cands: Vec::new(), punch_from: now, last_probe: -9.0, direct: None, last_udp: -9.0, seen: VecDeque::new(), rtt: -1.0, last_tx: -9.0, latest: None, last_seq: 0, offset: [0.0; 3], shown_t: now });
        p.last_seen = now;
        if let Some(a) = from { p.last_udp = now; if e.kind == K_PROBE || e.kind == K_PROBE_ACK { if p.direct != Some(a) { p.direct = Some(a); } } }
        if from.is_none() && e.seq != 0 && p.dup(e.seq) { return; }
        match e.kind {
            K_HELLO => { if let Some(h) = read_hello(&mut r) {
                let cands: Vec<SocketAddr> = h.cands.iter().map(|(ip, port)| SocketAddr::V4(SocketAddrV4::new((*ip).into(), *port))).collect();
                if cands != p.cands { p.cands = cands; p.punch_from = now; }
                p.name = h.name; p.map = h.map; p.host = h.host; p.racing = h.racing;
                if h.host && !self.is_host { self.host_id = Some(e.from); } } }
            K_PROBE => { if let Some(a) = from { let b = bare(Env { kind: K_PROBE_ACK, room: self.code, from: self.me, to: e.from, seq: 0 }); self.udp(&b, a); } }
            K_PING => { if self.is_host { if let Some(t) = r.f64() { let o = self.env(K_PONG, e.from); let b = time_pair(Env { seq: e.seq, ..o }, t, now); self.send_to(e.from, &b); } } }
            K_PONG => { if let (Some(t), Some(ht)) = (r.f64(), r.f64()) { if self.pings.remove(&e.seq).is_some() || true {
                let rtt = now - t; if rtt >= 0.0 && rtt < 3.0 { self.pong_n += 1; if let Some(p) = self.peers.get_mut(&e.from) { p.rtt = if p.rtt < 0.0 { rtt } else { p.rtt * 0.8 + rtt * 0.2 }; }
                    // keep the estimate from the quickest round trip; slowly forget it so a better path can take over
                    self.best_rtt += 0.002; if rtt <= self.best_rtt { self.best_rtt = rtt; self.offset_target = ht - (t + rtt / 2.0); if self.pong_n <= 2 { self.offset = self.offset_target; } } } } } }
            K_START => { if !self.is_host { if let Some(s) = read_start(&mut r) { if (s.go_at - self.last_start_go).abs() > 0.01 { self.last_start_go = s.go_at; self.start_in = Some(s); } } } }
            K_HIT => { if let Some((d, imp)) = read_hit(&mut r) { if self.hits_in.len() < 64 { self.hits_in.push((e.from, d, imp)); } } }
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

    /// Our car. Every phone predicts other cars from their last state (straight on along the current arc), and the sender runs the
    /// same prediction: a new state goes out only when the prediction has drifted (15 cm, 1.5 degrees, 0.8 m/s), the gear changed,
    /// or 0.4 s have passed. Smooth, steady driving costs a few packets a second; at most 30 a second in busy moments.
    pub fn send_car(&mut self, mut c: CarSnap) {
        let now = self.now(); let th = self.host_now(); c.t = th;
        let due = match &self.sent { None => true, Some(s) => {
            let age = th - s.t; let p = predict(s, age as f32);
            let dh = (wrap(c.h - p.h)).abs(); let de = ((c.x - p.x).powi(2) + (c.z - p.z).powi(2) + (c.y - p.y).powi(2)).sqrt();
            age >= self.thr[3] as f64 || (age >= 1.0 / 31.0 && (de > self.thr[0] || dh > self.thr[1] || (c.u - p.u).abs() > self.thr[2] || c.gear != s.gear || c.flags != s.flags)) } };
        if !due { return; }
        self.car_seq = self.car_seq.wrapping_add(1); let b = car2(self.me, self.car_seq, &c);
        // remember exactly what receivers will decode, so both sides predict from the same numbers
        if let Some((_, _, _, mut q)) = read_car2(&b) { q.t = th; self.sent = Some(q); }
        let mut relay = false; let mut out = Vec::new();
        for p in self.peers.values_mut() { match p.direct { Some(a) => { out.push(a); p.last_tx = now; } None => relay = true } }
        for a in out { self.udp(&b, a); }
        if relay { self.relay_one(&b); }
    }

    /// Tell a player's phone about a bump we resolved (their phone owns their car), and show the push here right away.
    pub fn send_hit(&mut self, id: u32, d: [f32; 5], imp: f32) {
        let e = self.env(K_HIT, id); let b = hit(e, d, imp); self.send_to(id, &b);
        let th = self.host_now();
        if let Some(c) = self.peers.get_mut(&id).and_then(|p| p.latest.as_mut()) { let age = (th - c.t).clamp(0.0, MAX_PREDICT) as f32;
            // the new velocity applies from now, not from when the last state was sent
            c.vx += d[0]; c.vz += d[1]; c.r += d[2]; c.x += d[3] - d[0] * age; c.z += d[4] - d[1] * age; c.h -= d[2] * age; }
    }
    /// seconds since this peer's car last reported (None once they have left the room)
    pub fn car_silence(&self, id: u32) -> Option<f64> { let p = self.peers.get(&id)?; Some(match p.latest { Some(c) => self.host_now() - c.t, None => self.now() - p.first_seen }) }

    fn handle_car(&mut self, b: &[u8], from: Option<SocketAddr>, now: f64) {
        let Some((sid, seq, ms, mut c)) = read_car2(b) else { return };
        let th = self.host_now();
        let Some(p) = self.peers.values_mut().find(|p| p.id as u16 == sid && (from.is_none() || p.direct == from || p.cands.contains(&from.unwrap()))) else { return };
        p.last_seen = now; if from.is_some() { p.last_udp = now; }
        if p.latest.is_some() && (seq.wrapping_sub(p.last_seq) as i8) <= 0 { return; }   // late or duplicate
        p.last_seq = seq;
        let age = ((th * 1000.0).rem_euclid(65536.0) - ms as f64).rem_euclid(65536.0) / 1000.0; c.t = th - if age < 10.0 { age } else { 0.0 };
        // keep the car where it is on screen and blend the correction out over the next frames
        if let Some(old) = p.latest { let before = predict(&old, (th - old.t).min(MAX_PREDICT) as f32); let after = predict(&c, (th - c.t).min(MAX_PREDICT) as f32);
            let shown = [before.x + p.offset[0], before.z + p.offset[1], before.h + p.offset[2]];
            let off = [shown[0] - after.x, shown[1] - after.z, wrap(shown[2] - after.h)];
            p.offset = if off[0].hypot(off[1]) < 8.0 { off } else { [0.0; 3] }; }
        p.latest = Some(c);
    }

    /// Where a peer's car is shown now: its last state predicted forward to the present, plus a correction that fades over ~0.12 s.
    pub fn remote(&mut self, id: u32) -> Option<CarSnap> {
        let th = self.host_now(); let now = self.now(); let p = self.peers.get_mut(&id)?; let c = p.latest?;
        let tau = (SMOOTH_TAU + (th - c.t).max(0.0) * 0.6).min(0.3);   // older news, gentler corrections
        let k = (-(now - p.shown_t).max(0.0) / tau).exp() as f32; p.shown_t = now; for o in p.offset.iter_mut() { *o *= k; }
        let mut s = predict(&c, (th - c.t).clamp(0.0, MAX_PREDICT) as f32); s.x += p.offset[0]; s.z += p.offset[1]; s.h += p.offset[2]; Some(s)
    }
}

const MAX_PREDICT: f64 = 0.6; const SMOOTH_TAU: f64 = 0.12;
fn wrap(mut a: f32) -> f32 { let tau = std::f32::consts::TAU; a = a.rem_euclid(tau); if a > std::f32::consts::PI { a - tau } else { a } }
/// constant forward speed, sideways slip and yaw rate: the car keeps following the arc it is on
pub fn predict(c: &CarSnap, dt: f32) -> CarSnap {
    let mut s = *c; let (sh, ch) = (c.h.sin(), c.h.cos()); let u = c.vx * sh + c.vz * ch; let vl = c.vx * ch - c.vz * sh;
    let n = 4; let st = dt / n as f32; let mut h = c.h;
    for _ in 0..n { let hm = h + c.r * st * 0.5; let (a, b) = (hm.sin(), hm.cos()); s.x += (u * a + vl * b) * st; s.z += (u * b - vl * a) * st; h += c.r * st; }
    s.h = h; let (a, b) = (h.sin(), h.cos()); s.vx = u * a + vl * b; s.vz = u * b - vl * a; s.u = u; s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prediction_follows_arc() {
        // 20 m/s with yaw rate 0.5 rad/s: after 2*pi/0.5 s the car is back where it started
        let c = CarSnap { h: 0.0, vz: 20.0, r: 0.5, ..Default::default() };
        let mut s = c; for _ in 0..126 { s = predict(&s, 0.1); } let p = predict(&s, (std::f32::consts::TAU / 0.5) - 12.6);
        assert!(p.x.hypot(p.z) < 0.5, "{} {}", p.x, p.z); assert!(wrap(p.h).abs() < 0.01);
        let q = predict(&c, 1.0); assert!((q.u - 20.0).abs() < 1e-3);
    }
}
