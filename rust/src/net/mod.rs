//! Online play without a game server.
//!
//! Phones meet in a room on public MQTT brokers (topic `fogwood/v1/<room code>/m`): presence, the race start and,
//! when needed, game packets go through there. Each phone also learns its public UDP address from a STUN server and
//! shares it with the room; peers then punch through their routers and exchange car states directly, which is faster.
//! Peers that cannot reach each other directly keep using the broker as a relay.
//!
//! The host (whoever created the room) is the clock and decides when a race starts and on which stage. Every phone is
//! the authority for its own car; other cars are predicted to the present from their last state and corrections are
//! blended in smoothly (see `smooth`). Car traffic stays inside a fixed data budget whatever the room size.
//!
//! The session lives in a global so it survives stage switches (the game object is rebuilt for every stage).
pub mod mqtt;
pub mod proto;
pub mod smooth;
pub mod udp;

use proto::*;
use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use web_time::Instant;

pub const BROKERS: [&str; 3] = ["broker.emqx.io:1883", "broker.hivemq.com:1883", "test.mosquitto.org:1883"];
const PEER_TIMEOUT: f64 = 15.0;
const DIRECT_TIMEOUT: f64 = 6.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Link { Connecting, Relay, Direct }

pub struct Peer {
    pub id: u32, pub name: String, pub map: String, pub host: bool, pub racing: bool, pub first_seen: f64, pub last_seen: f64, pub car: u8,
    cands: Vec<SocketAddr>, punch_from: f64, last_probe: f64, pub direct: Option<SocketAddr>, last_udp: f64,
    seen: VecDeque<u32>, pub rtt: f64, last_tx: f64,
    /// this peer's car as shown here, and the sequence number of the newest state from them
    pub track: smooth::Track, last_seq: u8,
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
    last_hello: f64, last_start_tx: f64, policy: smooth::Policy, car_seq: u8, pong_n: u32,
    /// the clock other cars are drawn at: advances with the frames (vsync), gently pulled onto the host clock
    view: smooth::ViewClock,
    start_out: Option<(Start, f64)>, pub start_in: Option<Start>, last_start_go: f64,
    pub map: String, pub racing: bool, no_direct: bool,
    /// the car we drive (shown on the other phones)
    pub car: u8,
    /// bumps other phones gave our car, waiting for the game to apply them
    pub hits_in: Vec<(u32, [f32; 5], f32)>,
    /// send thresholds (FW_THR="pos,heading,speed[,heartbeat]" overrides) and a fixed heartbeat if set
    thr: smooth::Thresholds, heartbeat: Option<f32>, loss: f64, lag: [f64; 2], held: VecDeque<(f64, Vec<u8>, Option<SocketAddr>)>, held_last: f64,
    /// estimated bytes on the wire, including UDP/IP and TCP/IP headers: [udp up, udp down, broker up, broker down]
    pub traffic: std::cell::Cell<[u64; 4]>, pub packets: std::cell::Cell<[u64; 4]>,
}
const UDP_OVERHEAD: u64 = 28; const TCP_OVERHEAD: u64 = 40;
/// Data budget for a session, both directions, every header counted: 8 kB/s is 28.8 MB an hour, well under the 40 MB
/// limit, leaving room for TCP acknowledgements, lost-packet retries and reconnects that are not counted here.
pub const BUDGET: f64 = 8000.0;
/// lobby share of the budget while racing: presence every 8 s and clock pings every 5 s on up to three brokers
const LOBBY_BYTES: f64 = 250.0;

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

pub fn with<T>(f: impl FnOnce(&mut Session) -> T) -> Option<T> { SESSION.lock().ok()?.as_mut().map(f) }
pub fn active() -> bool { SESSION.lock().map(|s| s.is_some()).unwrap_or(false) }
pub fn host(name: &str, map: &str) -> u32 { let code = rand_u32() % 900000 + 100000; open(code, name, map, true); code }
pub fn join(code: u32, name: &str, map: &str) { open(code, name, map, false); }
pub fn leave() { if let Ok(mut s) = SESSION.lock() { if let Some(x) = s.as_mut() { x.bye(); } *s = None; } }

fn rand_u32() -> u32 {
    let n = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(7) as u64;
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
        last_hello: -9.0, last_start_tx: -9.0, policy: Default::default(), car_seq: 0, view: Default::default(), pong_n: 0, start_out: None, start_in: None, last_start_go: -1.0, map: map.to_string(), racing: false, car: 0, hits_in: Vec::new(), no_direct: std::env::var("FW_NODIRECT").is_ok(), traffic: Default::default(), packets: Default::default(),
        lag: std::env::var("FW_LAG").ok().and_then(|v| { let q: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect(); if q.len() == 2 { Some([q[0], q[1]]) } else { None } }).unwrap_or([0.0, 0.0]), held: VecDeque::new(), held_last: 0.0,
        loss: std::env::var("FW_LOSS").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0),
        thr: smooth::Thresholds::default(), heartbeat: None };
    let mut s = s;
    if let Ok(v) = std::env::var("FW_THR") { let q: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        if q.len() >= 3 { s.thr.pos = q[0]; s.thr.head = q[1]; s.thr.speed = q[2]; } if q.len() >= 4 { s.heartbeat = Some(q[3]); } }
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
        let h = Hello { host: self.is_host, racing: self.racing, name: self.name.clone(), map: self.map.clone(), cands, car: self.car }; let e = self.env(K_HELLO, 0); hello(e, &h)
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
        if matches!(b.first(), Some(&K_CAR2) | Some(&K_CAR3)) { self.handle_car(b, from, now); return; }
        let Some((e, mut r)) = read_env(b) else { return };
        if e.room != self.code || e.from == self.me || (e.to != 0 && e.to != self.me) { return; }
        if e.kind == K_BYE { self.peers.remove(&e.from); return; }
        let p = self.peers.entry(e.from).or_insert_with(|| Peer { id: e.from, name: "?".into(), map: String::new(), host: false, racing: false, first_seen: now, last_seen: now, car: 0,
            cands: Vec::new(), punch_from: now, last_probe: -9.0, direct: None, last_udp: -9.0, seen: VecDeque::new(), rtt: -1.0, last_tx: -9.0, track: Default::default(), last_seq: 0 });
        p.last_seen = now;
        if let Some(a) = from { p.last_udp = now; if e.kind == K_PROBE || e.kind == K_PROBE_ACK { if p.direct != Some(a) { p.direct = Some(a); } } }
        if from.is_none() && e.seq != 0 && p.dup(e.seq) { return; }
        match e.kind {
            K_HELLO => { if let Some(h) = read_hello(&mut r) {
                let cands: Vec<SocketAddr> = h.cands.iter().map(|(ip, port)| SocketAddr::V4(SocketAddrV4::new((*ip).into(), *port))).collect();
                if cands != p.cands { p.cands = cands; p.punch_from = now; }
                p.name = h.name; p.map = h.map; p.host = h.host; p.racing = h.racing; p.car = h.car;
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

    /// How fast car states may go out, and the longest gap between them, so the whole session stays inside BUDGET.
    /// Every phone applies the same rule, so what we receive is bounded the same way as what we send.
    /// 2-4 players: up to ~20 a second; 8 players: ~9; 12 players: ~6.
    pub fn car_rate(&self) -> (f32, f32) {
        let n = self.peers.len().max(1) as f64; let direct = self.peers.values().filter(|p| p.direct.is_some()).count() as f64;
        let udp = (CAR3_LEN as u64 + UDP_OVERHEAD) as f64; let relay = self.mqtt_bytes(CAR3_LEN) as f64 + 20.0;   // + the TCP acks we send back
        // ours: a datagram per direct peer, one broker publish if anyone needs the relay (the broker echoes it back to us too)
        let up = direct * udp + if direct < n { 2.0 * relay } else { 0.0 };
        // theirs: each other phone, by datagram, plus a relayed copy when a room has relayed players
        let down = n * if direct < n { udp.max(relay) + relay * 0.5 } else { udp };
        let r = ((BUDGET - LOBBY_BYTES) / (up + down)).clamp(4.0, 24.0) as f32;
        (r, self.heartbeat.unwrap_or((3.0 / r).clamp(0.4, 0.75)))
    }

    /// Our car (`dt`: this frame's length). A state goes out when the receivers' prediction from our last packet has
    /// drifted (15 cm, 1.5 degrees, 0.8 m/s, 12 cm of height, ~3.5 degrees of body angle), the gear or air changed, or the
    /// heartbeat is due; never faster than `car_rate` allows. Steady driving costs a few packets a second.
    pub fn send_car(&mut self, mut c: CarSnap, dt: f64, g: smooth::Ground) {
        let now = self.now(); let th = self.host_now(); c.t = th; c.a = self.policy.accel(c.u, dt as f32);
        let (rate, beat) = self.car_rate();
        if !self.policy.due(&c, g, &self.thr, rate, beat) { return; }
        self.car_seq = self.car_seq.wrapping_add(1); let b = car3(self.me, self.car_seq, &c);
        // remember exactly what receivers will decode, so both sides predict from the same numbers
        if let Some((_, _, _, mut q)) = read_car2(&b) { q.t = th; self.policy.sent = Some(q); }
        let mut relay = false; let mut out = Vec::new();
        for p in self.peers.values_mut() { match p.direct { Some(a) => { out.push(a); p.last_tx = now; } None => relay = true } }
        for a in out { self.udp(&b, a); }
        if relay { self.relay_one(&b); }
    }

    /// Tell a player's phone about a bump we resolved (their phone owns their car), and show the push here right away.
    pub fn send_hit(&mut self, id: u32, d: [f32; 5], imp: f32) {
        let e = self.env(K_HIT, id); let b = hit(e, d, imp); self.send_to(id, &b);
        let t = self.view.get().unwrap_or_else(|| self.host_now());
        if let Some(p) = self.peers.get_mut(&id) { p.track.nudge(d, t); }
    }
    /// seconds since this peer's car last reported (None once they have left the room)
    pub fn car_silence(&self, id: u32) -> Option<f64> { let p = self.peers.get(&id)?; Some(match p.track.latest { Some(c) => self.host_now() - c.t, None => self.now() - p.first_seen }) }

    fn handle_car(&mut self, b: &[u8], from: Option<SocketAddr>, now: f64) {
        let Some((sid, seq, ms, mut c)) = read_car2(b) else { return };
        let th = self.host_now();
        let Some(p) = self.peers.values_mut().find(|p| p.id as u16 == sid && (from.is_none() || p.direct == from || p.cands.contains(&from.unwrap()))) else { return };
        p.last_seen = now; if from.is_some() { p.last_udp = now; }
        if p.track.latest.is_some() && (seq.wrapping_sub(p.last_seq) as i8) <= 0 { return; }   // late or duplicate
        p.last_seq = seq;
        let age = ((th * 1000.0).rem_euclid(65536.0) - ms as f64).rem_euclid(65536.0) / 1000.0; c.t = th - if age < 10.0 { age } else { 0.0 };
        p.track.push(c);
    }

    /// Call once per frame with the frame's length, before `remote`: advances the clock other cars are shown at.
    pub fn advance_view(&mut self, dt: f64) -> f64 { let hn = self.host_now(); self.view.advance(hn, dt) }

    /// Where a peer's car is shown this frame.
    pub fn remote(&mut self, id: u32, g: smooth::Ground) -> Option<CarSnap> {
        let t = self.view.get().unwrap_or_else(|| self.host_now()); self.peers.get_mut(&id)?.track.sample(t, g)
    }
}
