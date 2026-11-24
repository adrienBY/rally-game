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
//! Contacts: each phone settles a bump between its car and another player's car as drawn on its screen, at once. Its
//! own car takes its share; the other car's share is shown on that car and sent to its phone (K_HIT), which takes it
//! unless it settled the same contact itself. A contact counter in every car state tells the other phones from which
//! state on that phone's answer is included, so the push shown on its car holds until then and is then handed over.
//!
//! The session lives in a global so it survives stage switches (the game object is rebuilt for every stage).
pub mod mqtt;
pub mod proto;
pub mod relay;
pub mod lobby;
#[cfg(not(target_arch = "wasm32"))]
pub mod ws;
pub mod smooth;
pub mod udp;
pub mod rtc;

use proto::*;
use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use web_time::Instant;

pub const BROKERS: [&str; 3] = ["broker.emqx.io:1883", "broker.hivemq.com:1883", "test.mosquitto.org:1883"];
const PEER_TIMEOUT: f64 = 20.0;
const DIRECT_TIMEOUT: f64 = 4.0;
/// A way of reaching a peer (direct, our relay) counts as gone quiet when nothing has come from them that way for this
/// long (s): racing, cars report at least every 0.75 s. Packets for them then also go the other ways, so a path that died
/// (a phone switching from Wi-Fi to 4G, a router forgetting the mapping, a stalled relay socket) costs a second, not a
/// dropped-out player.
const QUIET_RACING: f64 = 1.5; const QUIET_LOBBY: f64 = 3.5;
/// the ids AI cars go by on the guests' phones (BOT_ID | index in the start message)
pub const BOT_ID: u32 = 0xB07A_1000;
pub fn bot_id(k: u8) -> u32 { BOT_ID | k as u32 }
pub use crate::server::SERVER_ID;
pub fn bot_of(id: u32) -> Option<u8> { if id & !0xFF == BOT_ID { Some((id & 0xFF) as u8) } else { None } }

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Link { Connecting, Relay, Direct }

pub struct Peer {
    pub id: u32, pub name: String, pub map: String, pub host: bool, pub racing: bool, pub first_seen: f64, pub last_seen: f64, pub car: u8, pub paint: u8,
    /// what their build reads (proto::CAP_*)
    pub caps: u8,
    cands: Vec<SocketAddr>, punch_from: f64, last_probe: f64, pub direct: Option<SocketAddr>, last_udp: f64,
    seen: VecDeque<u32>, pub rtt: f64, last_tx: f64,
    /// this peer's car as shown here, and the sequence number of the newest state from them
    pub track: smooth::Track, last_seq: u8,
    /// the brokers this peer is connected to (bit k: BROKERS[k]; 0xFF until they tell us)
    pub brokers: u8,
    /// last time a packet from them came through our relay (they are on it too)
    pub via_relay: f64,
    /// until when to also send them our packets the indirect ways: their car came relayed although we have a direct path,
    /// so they are not hearing us on it (a one-sided failure we cannot see from here)
    mirror_until: f64,
    /// pushes we gave their car (by which of our cars: proto::BY_PLAYER or an AI car), summed until the next bump message
    /// goes out, and when the last one went
    hit_acc: Vec<(u8, [f32; 5], f32)>, hit_tx: f64,
    /// WebRTC (net::rtc): the negotiation round we are on, when it began, when to try again and how many tries in a row
    /// failed; when something last came over the data channel, and whether it is open
    rtc_gen: u8, rtc_since: f64, rtc_try_at: f64, rtc_tries: u32, pub last_rtc: f64, pub rtc_open: bool, last_tx_rtc: f64,
}
/// one of the host's AI cars as a guest sees it
#[derive(Default)]
pub struct BotIn { pub track: smooth::Track, last_seq: u8, hit_acc: Option<([f32; 5], f32)>, hit_tx: f64 }
/// one of our AI cars going out (host)
#[derive(Default)]
struct BotOut { policy: smooth::Policy, seq: u8 }
impl Peer {
    pub fn link(&self) -> Link { if self.direct.is_some() || self.rtc_open { Link::Direct } else if self.last_seen > 0.0 { Link::Relay } else { Link::Connecting } }
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
    /// this frame's length: other cars are drawn where they are at the end of the frame, like ours after its physics step
    frame_dt: f64,
    /// the clock our states are stamped with: the same vsync-paced clock, so the stamps don't jitter with the moment of
    /// the frame the game happens to run at (at 30 m/s, 4 ms of jitter is 12 cm of noise the receivers would chase)
    stamp: smooth::ViewClock,
    start_out: Option<(Start, f64)>, pub start_in: Option<Start>, last_start_go: f64,
    pub map: String, pub racing: bool, no_direct: bool,
    /// the car we drive and its livery (shown on the other phones)
    pub car: u8, pub paint: u8,
    /// bumps other phones gave our car, waiting for the game to apply them (sender, which of their cars, push, impact)
    pub hits_in: Vec<(u32, u8, [f32; 5], f32)>,
    /// this race's AI cars: on a guest, the host's as they arrive; on the host, ours going out; and the bumps guests gave
    /// ours (sender, AI car, push, impact)
    /// tile fall: the marks the host sends (the game fills it), and the latest list a guest heard (the game takes it)
    pub tiles_out: Vec<(u8, u16)>, pub tiles_in: Option<Vec<(u8, u16)>>, tiles_tx: f64, tiles_sent: usize,
    pub bots: HashMap<u8, BotIn>, bots_out: Vec<BotOut>, pub race_bots: usize, pub bot_hits_in: Vec<(u32, u8, [f32; 5], f32)>,
    /// when the current race began (our clock)
    race_t0: f64,
    /// this race's AI cars run on the relay server (SERVER_ID), and when it last sent one of them
    pub server_bots: bool, srv_heard: f64,
    /// send thresholds (FW_THR="pos,heading,speed[,heartbeat]" overrides) and a fixed heartbeat if set
    /// test hook FW_CUT="from,to" (seconds on our clock): the direct packets that arrive then are lost (a path dying)
    cut: Option<[f64; 2]>,
    thr: smooth::Thresholds, heartbeat: Option<f32>, loss: f64, lag: [f64; 2], held: VecDeque<(f64, Vec<u8>, Option<SocketAddr>)>, held_last: f64,
    /// estimated bytes on the wire, including UDP/IP and TCP/IP headers: [udp up, udp down, broker up, broker down]
    pub traffic: std::cell::Cell<[u64; 4]>, pub packets: std::cell::Cell<[u64; 4]>,
    /// our own relay server (None: off)
    relay: Option<relay::Client>, last_announce: f64,
    /// this frame's car states for everyone (ours, our AI cars), sent together by `flush`
    outq: Vec<Vec<u8>>,
    /// closed-loop budget: the rate model's estimate is scaled by this, steered by the traffic actually measured (every
    /// 2 s: total bytes then, and when)
    rate_k: f64, meter: (f64, u64),
    /// WebRTC links (UDP to and from browsers), and negotiation messages still arriving in parts
    rtc: std::cell::RefCell<rtc::Links>, rtc_in: HashMap<(u32, u8, u8), Vec<Option<Vec<u8>>>>,
    /// test hook FW_UDPLAG: datagrams held back like a slow network (due, bytes, from)
    udp_held: VecDeque<(f64, Vec<u8>, SocketAddr)>,
}
const UDP_OVERHEAD: u64 = 28; const TCP_OVERHEAD: u64 = 40;
/// a WebRTC data channel message beyond its payload: UDP/IP, DTLS record, SCTP common header and DATA chunk
const RTC_OVERHEAD: u64 = 28 + 29 + 28;
/// car states take a peer's WebRTC link when its car is this close to ours (m)
const RTC_NEAR: f32 = 90.0;
/// what this build tells the room it can do over WebRTC (FW_NORTC: nothing)
fn udp_lag() -> Option<(f64, f64, f64)> {
    static L: std::sync::OnceLock<Option<(f64, f64, f64)>> = std::sync::OnceLock::new();
    *L.get_or_init(|| { let v: Vec<f64> = std::env::var("FW_UDPLAG").ok()?.split(',').filter_map(|x| x.parse().ok()).collect(); if v.len() == 3 { Some((v[0] / 1000.0, v[1] / 1000.0, v[2])) } else { None } })
}
fn rtc_caps() -> u8 { if std::env::var("FW_NORTC").is_ok() { return 0; } CAP_RTC | if cfg!(target_arch = "wasm32") { CAP_WEB } else { 0 } }
/// a WebSocket message's wire cost beyond its payload: frame header (masked from us), TLS record, TCP/IP
const WS_OVERHEAD: f64 = 6.0 + 21.0 + 40.0;
/// Data budget for a session, both directions, every header counted: 22 kB/s is 79 MB an hour, under the 100 MB limit
/// with room for TCP acknowledgements, retransmissions and reconnects that are not counted here.
pub const BUDGET: f64 = 22000.0;
/// lobby share of the budget while racing: presence every 8 s and clock pings every 5 s on up to three brokers
const LOBBY_BYTES: f64 = 250.0;

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// Which of our brokers (`up`: bit k = connected to broker k) to publish on so every receiver in `masks` (the brokers
/// each is connected to) gets it, with as few publishes as possible (greedy: the broker most of them share first,
/// the earlier one on a tie). Nobody to reach, or nobody we share a broker with: the first broker that is up.
pub fn pick_brokers(up: u8, masks: &[u8]) -> Vec<usize> {
    let mut need: Vec<u8> = masks.iter().map(|m| m & up).filter(|m| *m != 0).collect(); let mut out = Vec::new();
    if need.is_empty() { if up != 0 { out.push(up.trailing_zeros() as usize); } return out; }
    while !need.is_empty() {
        let k = (0..8usize).filter(|k| up & (1 << k) != 0).max_by_key(|k| (need.iter().filter(|m| *m & (1 << k) != 0).count(), std::cmp::Reverse(*k))).unwrap();
        out.push(k); need.retain(|m| m & (1 << k) == 0);
    }
    out
}

pub fn with<T>(f: impl FnOnce(&mut Session) -> T) -> Option<T> { SESSION.lock().ok()?.as_mut().map(f) }
pub fn active() -> bool { SESSION.lock().map(|s| s.is_some()).unwrap_or(false) }
pub fn host(name: &str, map: &str) -> u32 { let code = rand_u32() % 900000 + 100000; open(code, name, map, true); code }
pub fn join(code: u32, name: &str, map: &str) { open(code, name, map, false); }
pub fn leave() { if let Ok(mut s) = SESSION.lock() { if let Some(x) = s.as_mut() { x.bye(); } *s = None; } }

fn rand_u32() -> u32 {
    let n = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(7) as u64;
    #[cfg(not(target_arch = "wasm32"))]
    let pid = std::process::id() as u64;
    #[cfg(target_arch = "wasm32")]
    let pid = (js_sys::Math::random() * 4294967295.0) as u64;   // no process ids in a browser
    let mut x = n ^ 0x9E3779B97F4A7C15 ^ pid << 32; x ^= x >> 33; x = x.wrapping_mul(0xff51afd7ed558ccd); x ^= x >> 33; x = x.wrapping_mul(0xc4ceb9fe1a85ec53); x ^= x >> 33; x as u32
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
        last_hello: -9.0, last_start_tx: -9.0, policy: Default::default(), car_seq: 0, view: Default::default(), frame_dt: 0.0, stamp: Default::default(), pong_n: 0, start_out: None, start_in: None, last_start_go: -1.0, map: map.to_string(), racing: false, car: 0, paint: 0, hits_in: Vec::new(), bots: HashMap::new(), bots_out: Vec::new(), race_bots: 0, bot_hits_in: Vec::new(), tiles_out: Vec::new(), tiles_in: None, tiles_tx: -9.0, tiles_sent: 0, race_t0: 0.0, no_direct: std::env::var("FW_NODIRECT").is_ok(), traffic: Default::default(), packets: Default::default(),
        lag: std::env::var("FW_LAG").ok().and_then(|v| { let q: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect(); if q.len() == 2 { Some([q[0], q[1]]) } else { None } }).unwrap_or([0.0, 0.0]), held: VecDeque::new(), held_last: 0.0,
        cut: std::env::var("FW_CUT").ok().and_then(|v| { let q: Vec<f64> = v.split(',').filter_map(|x| x.parse().ok()).collect(); if q.len() == 2 { Some([q[0], q[1]]) } else { None } }),
        loss: std::env::var("FW_LOSS").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0),
        thr: smooth::Thresholds::default(), heartbeat: None,
        server_bots: false, srv_heard: -9.0, relay: if std::env::var("FW_NORELAY").is_ok() { None } else { Some(relay::Client::start(code, me)) }, last_announce: -9.0, outq: Vec::new(), rate_k: 1.0, meter: (0.0, 0), rtc: Default::default(), rtc_in: HashMap::new(), udp_held: VecDeque::new() };
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
        let h = Hello { host: self.is_host, racing: self.racing, name: self.name.clone(), map: self.map.clone(), cands, car: self.car, paint: self.paint, brokers: self.broker_mask(), caps: CAP_BATCH | rtc_caps() }; let e = self.env(K_HELLO, 0); hello(e, &h)
    }
    fn count(&self, k: usize, bytes: u64) { let mut t = self.traffic.get(); t[k] += bytes; self.traffic.set(t); let mut p = self.packets.get(); p[k] += 1; self.packets.set(p); }
    fn mqtt_bytes(&self, payload: usize) -> u64 { (payload + self.topic.len() + 4) as u64 + TCP_OVERHEAD }
    fn udp(&self, b: &[u8], a: SocketAddr) { if let Some(s) = &self.sock { if s.send_to(b, a).is_ok() { self.count(0, b.len() as u64 + UDP_OVERHEAD); } } }
    /// to every broker (lobby traffic: small, and it must reach phones that only got through to one of them)
    fn relay_all(&self, b: &[u8]) { let now = self.now(); self.relay_send(0, b, now); for k in &self.brokers { if k.is_up() { k.publish(&self.topic, b); self.count(2, self.mqtt_bytes(b.len())); } } }
    /// the brokers we are connected to (bit k: brokers[k])
    pub fn broker_mask(&self) -> u8 { self.brokers.iter().enumerate().filter(|(_, b)| b.is_up()).fold(0u8, |m, (k, _)| m | (1 << k.min(7))) }
    /// Game traffic through the relay, reaching every phone in `masks` (the brokers each is connected to) with as few
    /// publishes as possible: phones that ended up on different brokers (one of them unreachable from a network) still
    /// hear each other, where sending on our first broker alone left them frozen.
    fn relay_for(&self, b: &[u8], masks: &[u8]) {
        for k in pick_brokers(self.broker_mask(), masks) { if let Some(br) = self.brokers.get(k) { br.publish(&self.topic, b); self.count(2, self.mqtt_bytes(b.len())); } }
    }
    /// our relay is up and this peer is on it too
    fn on_relay(&self, p: &Peer, now: f64) -> bool { self.relay.as_ref().map(|r| r.is_up(now)).unwrap_or(false) && now - p.via_relay < 9.0 }
    fn relay_send(&self, to: u32, b: &[u8], now: f64) -> bool { match &self.relay { Some(r) => { let ok = r.send(to, b, now); if ok { self.count(0, self.relay_up_bytes(b.len(), now) as u64); } ok } None => false } }
    /// bytes on the wire for one message to our relay: every lane carries a copy; over a WebSocket each also pays the
    /// frame, TLS record and TCP/IP headers
    fn relay_up_bytes(&self, payload: usize, now: f64) -> f64 {
        let Some(r) = &self.relay else { return 0.0 }; let one = payload as f64 + if r.v2 { 7.0 } else { 5.0 } + if r.is_ws() { WS_OVERHEAD } else { UDP_OVERHEAD as f64 };
        one * if r.v2 { r.lanes_up(now) as f64 } else { 1.0 } }
    /// bytes for one message from our relay (every lane brings a copy), `per_s` of them arriving a second: a v2 relay
    /// writes what is due within 16 ms together, so in a busy room the framing is shared
    fn relay_down_bytes(&self, payload: usize, per_s: f64) -> f64 {
        let Some(r) = &self.relay else { return 0.0 }; let ovh = if r.is_ws() { WS_OVERHEAD } else { UDP_OVERHEAD as f64 };
        if !r.v2 { return payload as f64 + 5.0 + ovh; } let lanes = r.lanes_up(self.now()) as f64; let frames = per_s.min(62.5).max(1e-3); lanes * (payload as f64 + 7.0 + ovh * frames / per_s.max(1e-3)) }
    /// How to reach a peer: (direct address, through our relay, through the brokers). The direct path alone while it is
    /// lively; once it has gone quiet, our relay too; the brokers as well when the relay has gone quiet for them too.
    fn route(&self, p: &Peer, now: f64) -> (Option<SocketAddr>, bool, bool) {
        let quiet = if self.racing { QUIET_RACING } else { QUIET_LOBBY };
        if p.direct.is_some() && now - p.last_udp < quiet && now > p.mirror_until { return (p.direct, false, false); }
        // a lively WebRTC link (the caller sends on it: see rtc_route) with our relay alongside: two independent paths,
        // UDP straight to the phone and TCP through the relay, the first copy winning (FW_RTCONLY: the link alone)
        if self.rtc_route(p, now) && now - p.last_rtc < quiet { let ours = self.on_relay(p, now) && std::env::var("FW_RTCONLY").is_err(); return (None, ours, false); }
        let ours = self.on_relay(p, now); (p.direct, ours, !(ours && now - p.via_relay < quiet))
    }
    /// packets for this peer go over its WebRTC data channel (also while it has gone quiet: then the relay as well)
    fn rtc_route(&self, p: &Peer, _now: f64) -> bool { p.rtc_open }
    fn rtc_send(&self, id: u32, b: &[u8]) { if self.rtc.borrow_mut().send(id, b) { self.count(0, b.len() as u64 + RTC_OVERHEAD); } }
    fn mark_rtc_tx(&mut self, id: u32, now: f64) { if let Some(p) = self.peers.get_mut(&id) { p.last_tx_rtc = now; } }
    fn send_to(&self, to: u32, b: &[u8]) {
        let now = self.now();
        if to == SERVER_ID { self.relay_send(to, b, now); return; }
        let Some(p) = self.peers.get(&to) else { self.relay_for(b, &[]); return };
        let (d, ours, brokers) = self.route(p, now);
        if self.rtc_route(p, now) { self.rtc_send(to, b); }
        if let Some(a) = d { self.udp(b, a); }
        if ours { self.relay_send(to, b, now); }
        if brokers { self.relay_for(b, &[p.brokers]); }
    }
    /// a car state for everyone: queued, out with the rest of the frame's states in `flush`
    fn deliver(&mut self, b: &[u8], _now: f64) { if self.outq.len() < 64 { self.outq.push(b.to_vec()); } }
    /// Send this frame's car states: packed together (one datagram per direct peer, one relay message) when everyone's
    /// build reads batches, one by one otherwise. Call once a frame after the cars (poll also flushes what is left).
    pub fn flush(&mut self) {
        let q = std::mem::take(&mut self.outq); if q.is_empty() { return; } let now = self.now();
        let batch = q.len() > 1 && self.peers.values().all(|p| p.caps & CAP_BATCH != 0) && std::env::var("FW_NOBATCH").is_err();
        if batch { for b in batches(&q) { self.deliver_now(&b, now); } } else { for b in &q { self.deliver_now(b, now); } }
    }
    /// a car state (or batch) to everyone in the room, each the way `route` says (one message to our relay covers all on it)
    fn deliver_now(&mut self, b: &[u8], now: f64) {
        let mut relayed = Vec::new(); let mut out = Vec::new(); let mut via_ours = false;
        // car states go over the WebRTC links (the relay's copy goes too: two independent paths, the first copy winning).
        // In a big room (more than 5 phones) only to the phones whose cars are near ours, where precision shows (and to any
        // the link is the only quick way to): a link per phone costs a packet per phone, the relay one for the room, and
        // the room's data must stay inside its budget. Tuned with net_bench, 4 phones + 4 AI cars at 150 ms / 5% loss:
        // worst shake 184 (relay alone) -> 38, path error 0.20 -> 0.08 m, ~45 MB/h.
        let me = self.policy.sent.map(|c| (c.x, c.z)); let small = self.peers.len() <= 4;
        let near = |p: &Peer| small || match (me, p.track.latest) { (Some(a), Some(b)) => (a.0 - b.x).hypot(a.1 - b.z) < RTC_NEAR, _ => true };
        let routes: Vec<(u32, (Option<SocketAddr>, bool, bool), u8, bool)> = self.peers.values().map(|p| { let r = self.route(p, now); let rtc = self.rtc_route(p, now) && (near(p) || !r.1 || std::env::var("FW_RTCALLCARS").is_ok()); (p.id, r, p.brokers, rtc) }).collect();
        for (id, (d, ours, brokers), mask, rtc) in routes { if rtc { self.rtc_send(id, b); self.mark_rtc_tx(id, now); if let Some(p) = self.peers.get_mut(&id) { p.last_tx = now; } } if let Some(a) = d { out.push(a); if let Some(p) = self.peers.get_mut(&id) { p.last_tx = now; } } if ours { via_ours = true; } if brokers { relayed.push(mask); } }
        for a in out { self.udp(b, a); }
        if via_ours { self.relay_send(0, b, now); } else if self.server_bots && self.racing { self.relay_send(SERVER_ID, b, now); }
        if !relayed.is_empty() { self.relay_for(b, &relayed); }
    }
    fn bye(&mut self) { let e = self.env(K_BYE, 0); let b = bare(e); self.relay_all(&b); for p in self.peers.values() { if let Some(a) = p.direct { self.udp(&b, a); } }  if let Some(r) = &self.relay { if self.is_host { r.announce(&self.name, &self.map, 0, false, true); } r.leave(); } }

    /// Network work for one frame: read the broker queue and the UDP socket, presence, clock pings, punching, start repeats.
    pub fn poll(&mut self) {
        self.flush();
        mqtt::tick();
        let now = self.now();
        // the model behind car_rate is an estimate (how often cars really need sending, how much the relay's framing is
        // shared): every 2 s while racing, the traffic actually measured steers it, up while under the budget, down when
        // over (a tenth of the way each time, between half and three times the model)
        if now - self.meter.0 >= 2.0 { let t = self.traffic.get(); let total = t.iter().sum::<u64>();
            if self.racing && self.meter.0 > 0.0 { let bps = (total - self.meter.1) as f64 / (now - self.meter.0); let want = BUDGET * 0.92;
                self.rate_k = (self.rate_k * (want / bps.max(200.0)).powf(0.35).clamp(0.85, 1.2)).clamp(0.5, 3.0); }
            self.meter = (now, total); }
        // clock corrections are eased in (at most 3 ms per frame) so other cars never jump when a better estimate arrives
        let d = self.offset_target - self.offset; self.offset += d.clamp(-0.003, 0.003);
        if let Some(r) = &self.stun { if let Ok(a) = r.try_recv() { self.public = a; self.stun_done = true; self.stun = None; self.last_hello = -9.0; } }
        if !self.stun_done && now > 4.0 { self.stun_done = true; self.stun = None; }
        // broker messages
        let mut msgs = Vec::new(); while let Ok((t, p)) = self.inbox.try_recv() { self.count(3, self.mqtt_bytes(p.len())); if t == self.topic { msgs.push((p, None)); } }
        // our relay
        let mut via: Vec<u32> = Vec::new();
        // both relay lanes carry everything (FW_DUAL=0: one lane, the other a spare)
        if let Some(r) = self.relay.as_mut() { r.dual = r.v2 && std::env::var("FW_DUAL").map(|v| v != "0").unwrap_or(true); }
        if let Some(r) = self.relay.as_mut() { let got = r.poll(now); let per_s = (got.len() as f64 * 60.0).max(1.0);
            for (from, p) in got { via.push(from); let b = self.relay_down_bytes(p.len(), per_s); self.count(1, b as u64); msgs.push((p, None)); } }
        for id in via { if let Some(p) = self.peers.get_mut(&id) { p.via_relay = now; } }
        // direct messages (only once STUN has finished with the socket)
        if self.stun_done { if let Some(s) = &self.sock { let mut buf = [0u8; 2048]; let rn = rtc::now();
            let mut got: Vec<(Vec<u8>, SocketAddr)> = Vec::new(); while let Ok((n, a)) = s.recv_from(&mut buf) { got.push((buf[..n].to_vec(), a)); }
            if let Some((lat, jit, loss)) = udp_lag() { for (b, a) in got.drain(..) { if (rand_u32() as f64 / u32::MAX as f64) < loss { continue; }
                    // a network path is a queue: late by a varying amount, rarely out of order (FW_UDPREORDER=1: independent delays)
                    let mut at = now + lat + (rand_u32() as f64 / u32::MAX as f64 * 2.0 - 1.0) * jit; if std::env::var("FW_UDPREORDER").is_err() { at = at.max(self.udp_held.back().map(|h| h.0).unwrap_or(0.0)); }
                    let k = self.udp_held.iter().position(|h| h.0 > at).unwrap_or(self.udp_held.len()); self.udp_held.insert(k, (at, b, a)); }
                while self.udp_held.front().map(|h| h.0 <= now).unwrap_or(false) { let (_, b, a) = self.udp_held.pop_front().unwrap(); got.push((b, a)); } }
            for (b, a) in got { let n = b.len(); buf[..n].copy_from_slice(&b); self.count(1, n as u64 + UDP_OVERHEAD);
                // STUN (0-3) and DTLS (20-63) from a WebRTC peer go to its link (RFC 7983); ours start with FW1 or 0xC2..
                if (buf[0] <= 3 || (20..=63).contains(&buf[0])) && self.rtc.borrow_mut().datagram(a, &buf[..n], rn) { continue; }
                msgs.push((buf[..n].to_vec(), Some(a))); } } }
        // test hooks: FW_LOSS drops a share of what arrives, FW_LAG="mean_ms,jitter_ms" holds it back like a slow network
        let cut = self.cut.map(|c| now >= c[0] && now < c[1]).unwrap_or(false);
        for (b, from) in msgs { if self.loss > 0.0 && (rand_u32() as f64 / u32::MAX as f64) < self.loss { continue; }
            if cut && from.is_some() { continue; }
            if self.lag[0] > 0.0 { let j = (rand_u32() as f64 / u32::MAX as f64 * 2.0 - 1.0) * self.lag[1]; let mut at = now + (self.lag[0] + j) / 1000.0;
                if from.is_none() { at = at.max(self.held_last); self.held_last = at; }   // the broker link is TCP: late, never out of order
                let k = self.held.iter().position(|h| h.0 > at).unwrap_or(self.held.len()); self.held.insert(k, (at, b, from)); } else { self.handle(&b, from, now); } }
        while self.held.front().map(|h| h.0 <= now).unwrap_or(false) { let (_, b, from) = self.held.pop_front().unwrap(); self.handle(&b, from, now); }
        self.rtc_step(now);
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
        // bumps we gave other players' cars: at most one message per car every 60 ms (a long scrape sums its pushes)
        let due: Vec<(u32, Vec<(u8, [f32; 5], f32)>)> = self.peers.values_mut().filter(|p| !p.hit_acc.is_empty() && now - p.hit_tx >= 0.06)
            .map(|p| { p.hit_tx = now; (p.id, std::mem::take(&mut p.hit_acc)) }).collect();
        for (id, list) in due { for (by, d, imp) in list { let e = self.env(K_HIT, id); let b = hit(e, d, imp, by); self.send_to(id, &b); } }
        // bumps we gave the host's AI cars go to the host, whose phone drives them
        let driver = if self.server_bots { Some(SERVER_ID) } else { self.host_id.filter(|_| !self.is_host) };
        if let Some(h) = driver {
            let due: Vec<(u8, [f32; 5], f32)> = self.bots.iter_mut().filter(|(_, b)| b.hit_acc.is_some() && now - b.hit_tx >= 0.06)
                .map(|(k, b)| { b.hit_tx = now; let (d, imp) = b.hit_acc.take().unwrap(); (*k, d, imp) }).collect();
            for (k, d, imp) in due { let e = self.env(K_BOTHIT, h); let b = bot_hit(e, k, d, imp); self.send_to(h, &b); } }
        // tile fall: the host repeats the whole list of marked tiles (at once when it grew, else every 0.25 s)
        if self.is_host && self.racing && !self.tiles_out.is_empty() && (self.tiles_out.len() != self.tiles_sent || now - self.tiles_tx > 0.25) {
            self.tiles_tx = now; self.tiles_sent = self.tiles_out.len(); let ids: Vec<u32> = self.peers.keys().copied().collect(); let list = self.tiles_out.clone();
            for id in ids { let e = self.env(K_TILES, id); let b = tiles(e, &list); self.send_to(id, &b); } }
        // clock: guests ping the host
        if !self.is_host { if let Some(h) = self.host_id { if now - self.last_ping > if self.pong_n < 6 { 0.3 } else { 5.0 } { self.last_ping = now;
            let e = self.env(K_PING, h); self.pings.insert(e.seq, now); let b = time_pair(e, now, 0.0); self.send_to(h, &b); if self.pings.len() > 32 { self.pings.clear(); } } } }
        // hosts list their room for the open-rooms screen
        if self.is_host && now - self.last_announce > 4.0 { self.last_announce = now;
            if let Some(r) = &self.relay { if r.is_up(now) { r.announce(&self.name, &self.map, self.peers.len() + 1, self.racing, false); } } }
        // repeat the start a few times (broker messages can be lost)
        if let Some((s, until)) = self.start_out.clone() { if now < until { if now - self.last_start_tx >= 0.5 { self.last_start_tx = now; let e = self.env(K_START, 0); let b = start(e, &s); self.relay_all(&b); } } else { self.start_out = None; } }
    }

    fn handle(&mut self, b: &[u8], from: Option<SocketAddr>, now: f64) {
        if let Some(list) = unbatch(b) { for m in list { if m.first() != Some(&K_BATCH) { self.handle(m, from, now); } } return; }
        if matches!(b.first(), Some(&K_CAR2) | Some(&K_CAR3) | Some(&K_BOT3)) { self.handle_car(b, from, now); return; }
        let Some((e, mut r)) = read_env(b) else { return };
        if e.room != self.code || e.from == self.me || (e.to != 0 && e.to != self.me) { return; }
        if e.kind == K_BYE { self.peers.remove(&e.from); return; }
        if e.from == SERVER_ID { match e.kind {
            K_PING => { if self.is_host { if let Some(t) = r.f64() { let o = self.env(K_PONG, SERVER_ID); let b = time_pair(Env { seq: e.seq, ..o }, t, now); self.send_to(SERVER_ID, &b); } } }
            K_HIT => { if let Some((d, imp, by)) = read_hit(&mut r) { if self.hits_in.len() < 64 { self.hits_in.push((SERVER_ID, by, d, imp)); } } }
            _ => {} } return; }
        let p = self.peers.entry(e.from).or_insert_with(|| Peer { id: e.from, name: "?".into(), map: String::new(), host: false, racing: false, first_seen: now, last_seen: now, car: 0, paint: 0, caps: 0,
            cands: Vec::new(), punch_from: now, last_probe: -9.0, direct: None, last_udp: -9.0, seen: VecDeque::new(), rtt: -1.0, last_tx: -9.0, track: Default::default(), last_seq: 0, brokers: 0xFF, via_relay: -99.0, hit_acc: Vec::new(), hit_tx: -9.0, mirror_until: -9.0, rtc_gen: 0, rtc_since: -99.0, rtc_try_at: 0.0, rtc_tries: 0, last_rtc: -99.0, rtc_open: false, last_tx_rtc: -99.0 });
        p.last_seen = now;
        if let Some(a) = from { p.last_udp = now; if e.kind == K_PROBE || e.kind == K_PROBE_ACK { if p.direct != Some(a) { p.direct = Some(a); } } }
        // a message can now come twice (direct and relayed while a path is quiet): once is enough
        if e.seq != 0 && p.dup(e.seq) { return; }
        match e.kind {
            K_HELLO => { if let Some(h) = read_hello(&mut r) {
                let cands: Vec<SocketAddr> = h.cands.iter().map(|(ip, port)| SocketAddr::V4(SocketAddrV4::new((*ip).into(), *port))).collect();
                if cands != p.cands { p.cands = cands; p.punch_from = now; }
                p.name = h.name; p.map = h.map; p.host = h.host; p.racing = h.racing; p.car = h.car; p.paint = h.paint; p.brokers = h.brokers; p.caps = h.caps;
                if h.host && !self.is_host { self.host_id = Some(e.from); } } }
            K_PROBE => { if let Some(a) = from { let b = bare(Env { kind: K_PROBE_ACK, room: self.code, from: self.me, to: e.from, seq: 0 }); self.udp(&b, a); } }
            K_PING => { if self.is_host { if let Some(t) = r.f64() { let o = self.env(K_PONG, e.from); let b = time_pair(Env { seq: e.seq, ..o }, t, now); self.send_to(e.from, &b); } } }
            K_PONG => { if let (Some(t), Some(ht)) = (r.f64(), r.f64()) { if self.pings.remove(&e.seq).is_some() || true {
                let rtt = now - t; if rtt >= 0.0 && rtt < 3.0 { self.pong_n += 1; if let Some(p) = self.peers.get_mut(&e.from) { p.rtt = if p.rtt < 0.0 { rtt } else { p.rtt * 0.8 + rtt * 0.2 }; }
                    // keep the estimate from the quickest round trip; slowly forget it so a better path can take over
                    self.best_rtt += 0.002; if rtt <= self.best_rtt { self.best_rtt = rtt; self.offset_target = ht - (t + rtt / 2.0); if self.pong_n <= 2 { self.offset = self.offset_target; } } } } } }
            K_START => { if !self.is_host { if let Some(s) = read_start(&mut r) { if (s.go_at - self.last_start_go).abs() > 0.01 { self.last_start_go = s.go_at; self.start_in = Some(s); } } } }
            K_HIT => { if let Some((d, imp, by)) = read_hit(&mut r) { if self.hits_in.len() < 64 { self.hits_in.push((e.from, by, d, imp)); } } }
            K_RTC => { if let Some(m) = read_rtc(&mut r) { self.rtc_in(e.from, m, now); } }
            K_TILES => { if !self.is_host && Some(e.from) == self.host_id { if let Some(v) = read_tiles(&mut r) { self.tiles_in = Some(v); } } }
            K_BOTHIT => { if self.is_host { if let Some((k, d, imp)) = read_bot_hit(&mut r) { if self.bot_hits_in.len() < 64 { self.bot_hits_in.push((e.from, k, d, imp)); } } } }
            _ => {}
        }
    }

    /// WebRTC with a peer: both builds speak it, and it is the way to a direct path (one of us is a browser, which
    /// has no UDP of its own; FW_RTC_ALL: every pair, for tests). The app also needs its socket and addresses.
    fn rtc_wanted(&self, p: &Peer) -> bool {
        let mine = rtc_caps(); if mine & CAP_RTC == 0 || p.caps & CAP_RTC == 0 { return false; }
        let web = mine & CAP_WEB != 0 || p.caps & CAP_WEB != 0 || std::env::var("FW_RTC_ALL").is_ok();
        web && (cfg!(target_arch = "wasm32") || (self.sock.is_some() && self.stun_done && self.local.is_some()))
    }
    /// once a frame: start, retry and drop links, move their packets
    fn rtc_step(&mut self, now: f64) {
        let rn = rtc::now();
        self.rtc.borrow_mut().set_addrs(self.local.map(SocketAddr::V4), self.public.map(SocketAddr::V4));
        let ids: Vec<u32> = self.peers.keys().copied().collect(); let mut ka: Vec<u32> = Vec::new();
        for id in ids {
            let want = { let p = &self.peers[&id]; self.rtc_wanted(p) };
            let (open, dead, exists) = { let l = self.rtc.borrow(); (l.is_open(id), l.is_dead(id), l.exists(id)) };
            let me = self.me; let p = self.peers.get_mut(&id).unwrap();
            if open && !p.rtc_open { p.rtc_tries = 0; p.last_rtc = now; if std::env::var("FW_WSDEBUG").is_ok() { eprintln!("rtc: open with {:08x}", id); } }
            p.rtc_open = open;
            if !want && !(exists && me > id) { if exists { self.rtc.borrow_mut().close(id); } continue; }
            // a link that died, or went silent while open (nothing, not even the keep-alives, for 6 s), is dropped (the
            // other side, or our next try, starts again)
            let silent = open && now - p.last_rtc > 6.0;
            if open && now - p.last_tx_rtc > 1.0 { p.last_tx_rtc = now; ka.push(id); }
            if dead || silent { self.rtc.borrow_mut().close(id); p.rtc_open = false; if std::env::var("FW_WSDEBUG").is_ok() { eprintln!("rtc: {:08x} {}", id, if dead { "failed" } else { "went silent" }); } }
            // the lower id offers; a try that hasn't opened within 12 s is given up, then tried again later (12 s, 24 s, ... 2 min)
            if me < id && !open && now >= p.rtc_try_at && (!exists || dead || silent || now - p.rtc_since > 12.0) {
                p.rtc_gen = p.rtc_gen.wrapping_add(1); p.rtc_since = now; p.rtc_tries += 1; p.rtc_try_at = now + (6.0 * (1u32 << p.rtc_tries.min(5)) as f64).min(120.0);
                if std::env::var("FW_WSDEBUG").is_ok() { eprintln!("rtc: offer {} to {:08x}", p.rtc_gen, id); }
                self.rtc.borrow_mut().open(id, true, rn);
            }
        }
        // keep-alives on links that carried nothing for a second (a bare probe: it only says the link works)
        for id in ka { let b = bare(Env { kind: K_PROBE, room: self.code, from: self.me, to: id, seq: 0 }); self.rtc_send(id, &b); }
        // links to phones that left
        let gone: Vec<u32> = self.rtc.borrow().ids().into_iter().filter(|id| !self.peers.contains_key(id)).collect();
        for id in gone { self.rtc.borrow_mut().close(id); }
        self.rtc.borrow_mut().tick(rn);
        // negotiation out, through the room (never over the link being negotiated)
        let sig = self.rtc.borrow_mut().take_signals();
        for (id, s) in sig {
            let gen = self.peers.get(&id).map(|p| p.rtc_gen).unwrap_or(0); let b = s.text().as_bytes();
            let parts: Vec<&[u8]> = if b.is_empty() { vec![b] } else { b.chunks(RTC_PART).collect() }; let n = parts.len().min(255) as u8;
            for (k, part) in parts.iter().take(n as usize).enumerate() { let e = self.env(K_RTC, id); let m = rtc_part(e, gen, s.kind(), k as u8, n, part); self.send_room(id, &m, now); }
        }
        // the app's link packets out on our socket
        if let Some(s) = &self.sock { for (a, b) in self.rtc.borrow_mut().take_transmit() { if s.send_to(&b, a).is_ok() { self.count(0, b.len() as u64 + UDP_OVERHEAD); } } }
        // what came over the links: handled like anything else from those phones
        let inbox = self.rtc.borrow_mut().take_inbox();
        for (id, b) in inbox { self.count(1, b.len() as u64 + RTC_OVERHEAD); if let Some(p) = self.peers.get_mut(&id) { p.last_rtc = now; p.last_seen = now; } self.handle(&b, None, now); }
    }
    /// a message to one peer through our relay or the brokers only (WebRTC negotiation)
    fn send_room(&self, to: u32, b: &[u8], now: f64) {
        let mask = self.peers.get(&to).map(|p| p.brokers).unwrap_or(0xFF);
        if !self.relay_send(to, b, now) { self.relay_for(b, &[mask]); }
    }
    /// part of a negotiation message from `from`: once all its parts are here, to the link
    fn rtc_in(&mut self, from: u32, (gen, kind, part, parts, bytes): (u8, u8, u8, u8, Vec<u8>), _now: f64) {
        if std::env::var("FW_RTCDEBUG").is_ok() { eprintln!("rtc-in from {:08x} gen {} kind {} part {}/{} len {}", from, gen, kind, part, parts, bytes.len()); }
        if parts == 0 || part >= parts || !self.peers.contains_key(&from) { return; }
        let key = (from, gen, kind); let slot = self.rtc_in.entry(key).or_insert_with(|| vec![None; parts as usize]);
        if slot.len() != parts as usize { *slot = vec![None; parts as usize]; }
        slot[part as usize] = Some(bytes);
        if slot.iter().any(|x| x.is_none()) { if self.rtc_in.len() > 64 { self.rtc_in.clear(); } return; }
        let whole: Vec<u8> = self.rtc_in.remove(&key).unwrap().into_iter().flatten().flatten().collect();
        let Some(sig) = rtc::Signal::from(kind, String::from_utf8_lossy(&whole).to_string()) else { return };
        // (an offer says the sender speaks it, even if its hello hasn't arrived yet)
        if kind == 0 { if let Some(p) = self.peers.get_mut(&from) { p.caps |= CAP_RTC; } }
        let ready = cfg!(target_arch = "wasm32") || (self.sock.is_some() && self.stun_done && self.local.is_some());
        let want = rtc_caps() & CAP_RTC != 0 && ready && (kind == 0 || self.rtc_wanted(&self.peers[&from]));
        if !want { if std::env::var("FW_RTCDEBUG").is_ok() { eprintln!("rtc-in: not wanted (caps {})", self.peers[&from].caps); } return; }
        let p = self.peers.get_mut(&from).unwrap();
        // an offer starts its round (we answer within it); answers and candidates only count for the current round
        match sig { rtc::Signal::Offer(_) => { if self.me < from { return; } p.rtc_gen = gen; p.rtc_open = false; } _ => { if gen != p.rtc_gen { return; } } }
        if std::env::var("FW_WSDEBUG").is_ok() { eprintln!("rtc: {} {} from {:08x} ({} bytes)", ["offer", "answer", "candidate"][kind.min(2) as usize], gen, from, whole.len()); }
        self.rtc.borrow_mut().signal(from, sig, rtc::now());
    }

    /// Host: everyone in the room races on `map`, lights out `delay` seconds from now, with `bots` AI cars (rivals, fastest
    /// first). Like a solo race the players start mid-grid: the quicker half of the AI cars ahead of them, the rest behind; the players'
    /// own order is shuffled each race.
    /// Returns the start for the host itself.
    pub fn start_race(&mut self, map: &str, laps: u8, delay: f64, bots: &[u8], srv: bool, mode: u8) -> Option<Start> {
        if !self.is_host { return None; }
        let mut ids: Vec<(f64, u32)> = self.peers.values().map(|p| (p.first_seen, p.id)).collect(); ids.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut players = vec![self.me]; players.extend(ids.iter().take(11).map(|x| x.1));
        // the players' order on the grid is drawn fresh each race (the host sends it, so every phone agrees) - whoever made the room
        // no longer always starts in front
        for i in (1..players.len()).rev() { let j = rand_u32() as usize % (i + 1); players.swap(i, j); }
        let nb = bots.len().min(12 - players.len()); let ahead = if mode == MODE_DUOS { 0 } else { (nb + 1) / 2 };
        let bot_slots: Vec<(u8, u8)> = bots.iter().take(nb).enumerate().map(|(k, r)| (*r, if k < ahead { k } else { k + players.len() } as u8)).collect();
        // duos: in grid order, two by two (the players first, then the AI cars), each pair side by side on its row
        let (teams, bot_teams) = if mode == MODE_DUOS { ((0..players.len()).map(|k| (k / 2) as u8).collect(), (0..nb).map(|k| ((players.len() + k) / 2).min(5) as u8).collect()) } else { (Vec::new(), Vec::new()) };
        let slots = players.iter().enumerate().map(|(k, id)| (*id, (ahead + k) as u8)).collect();
        let s = Start { map: map.to_string(), laps, go_at: self.host_now() + delay, slots, bots: bot_slots, srv: srv && !bots.is_empty(), mode, teams, bot_teams }; self.start_out = Some((s.clone(), self.now() + 3.0)); Some(s)
    }
    /// a race begins with `n` AI cars (driven by the host's phone): fresh tracks and senders
    pub fn begin_race(&mut self, n: usize, srv: bool) {
        self.tiles_out.clear(); self.tiles_in = None; self.tiles_sent = 0;
        self.race_bots = n; self.race_t0 = self.now(); self.bots.clear(); self.bot_hits_in.clear(); self.hits_in.clear();
        self.bots_out = (0..n).map(|_| BotOut::default()).collect(); self.server_bots = srv && n > 0; self.srv_heard = -9.0;
        if !self.is_host || self.server_bots { for k in 0..n { self.bots.insert(k as u8, BotIn::default()); } }
    }
    /// the relay can drive this race's AI cars
    pub fn sim_capable(&self) -> bool { let now = self.now(); self.relay.as_ref().map(|r| r.is_up(now) && r.sim).unwrap_or(false) }
    /// host: the stage for the relay's AI cars (server::stage_blob)
    pub fn upload_stage(&mut self, blob: &[u8]) -> bool { self.relay.as_mut().map(|r| r.stage(blob)).unwrap_or(false) }
    /// what the relay said about the stage: 1 ready, 2 failed (None: nothing yet)
    pub fn sim_state(&self) -> Option<u8> { self.relay.as_ref().and_then(|r| r.sim_state.map(|s| s.0)) }
    /// seconds since the relay last sent one of this race's AI cars (since the race began if it never did)
    pub fn server_silence(&self) -> f64 { self.now() - self.srv_heard.max(self.race_t0) }
    /// host: the relay has gone quiet: this phone drives the AI cars from here on (and sends them like before)
    pub fn take_back_bots(&mut self) { self.server_bots = false; }

    /// How fast car states may go out, and the longest gap between them, so the whole session stays inside BUDGET.
    /// Every phone applies the same rule, so what we receive is bounded the same way as what we send.
    /// 2-4 players: up to ~20 a second; 8 players: ~9; 12 players: ~6.
    pub fn car_rate(&self) -> (f32, f32) {
        let now = self.now(); let n = self.peers.len().max(1) as f64;
        let ours = self.peers.values().filter(|p| p.direct.is_none() && self.on_relay(p, now)).count() as f64;
        let direct = self.peers.values().filter(|p| p.direct.is_some()).count() as f64;
        let rtcs = self.peers.values().filter(|p| p.direct.is_none() && p.rtc_open).count() as f64;
        let ours = (ours - rtcs).max(0.0); let brokered = (n - direct - ours - rtcs).max(0.0);
        // our AI cars (host) and the room's go out in the same packets as the cars of their frame: a state costs its bytes
        // plus a share of the packet (about two states a packet in a room with AI cars)
        let nb = self.race_bots as f64; let share = if nb > 0.0 { 0.5 } else { 1.0 };
        let udp = CAR3_LEN as f64 + 1.0 + UDP_OVERHEAD as f64 * share; let mq = self.mqtt_bytes(CAR3_LEN) as f64 + 20.0;   // + the TCP acks we send back
        // what a phone receives through our relay a second (to size the shared framing): every relayed car
        let r0 = 8.0; let per_s = (ours + if self.is_host { 0.0 } else { nb }) * r0;
        let ru = self.relay_up_bytes(CAR3_LEN + 1, now) * share; let rd = self.relay_down_bytes(CAR3_LEN + 1, per_s.max(1.0));
        // ours: a datagram per direct peer, one relay message for all on it, one broker publish when anyone needs that
        let rtc_b = udp - UDP_OVERHEAD as f64 * share + RTC_OVERHEAD as f64 * share + 14.0;   // + the SCTP acknowledgements
        let up = direct * udp + rtcs * rtc_b + if ours > 0.0 { ru } else { 0.0 } + if brokered > 0.0 { 2.0 * mq } else { 0.0 };
        // theirs: each other phone, by the way it reaches us
        let down = direct * udp + rtcs * rtc_b + ours * rd + brokered * (udp.max(mq) + mq * 0.5);
        // AI cars: the host sends each like its own car, every guest receives each
        let bot_in = if self.server_bots || ours > 0.0 { rd } else if direct > 0.0 { udp } else { mq };
        let (up, down) = if self.is_host && !self.server_bots { (up * (1.0 + nb), down) } else { (up, down + nb * bot_in) };
        let r = ((BUDGET - LOBBY_BYTES) / (up + down).max(1.0) * self.rate_k).clamp(4.0, 24.0) as f32;
        (r, self.heartbeat.unwrap_or((3.0 / r).clamp(0.4, 0.75)))
    }

    /// thresholds, rate and heartbeat for a car whose nearest other car is `dist` m away (see smooth::relevance)
    pub fn relevance(&self, dist: f32) -> (smooth::Thresholds, f32, f32) { let (rate, beat) = self.car_rate(); smooth::relevance(self.thr, rate, beat, dist) }

    /// Our car (`dt`: this frame's length). A state goes out when the receivers' prediction from our last packet has
    /// drifted (15 cm, 1.5 degrees, 0.8 m/s, 12 cm of height, ~3.5 degrees of body angle), the gear or air changed, or the
    /// heartbeat is due; never faster than `car_rate` allows. Steady driving costs a few packets a second.
    /// `dist`: how far (m) the nearest other car is: see `relevance`
    pub fn send_car(&mut self, mut c: CarSnap, dt: f64, g: smooth::Ground, dist: f32) {
        let now = self.now(); let hn = self.host_now(); let th = self.stamp.advance(hn, dt); c.t = th; c.a = self.policy.accel(c.u, dt as f32);
        let (thr, rate, beat) = self.relevance(dist);
        if !self.policy.due(&c, g, &thr, rate, beat) { return; }
        self.car_seq = self.car_seq.wrapping_add(1); let b = car3(self.me, self.car_seq, &c);
        // remember exactly what receivers will decode, so both sides predict from the same numbers
        if let Some((_, _, _, mut q)) = read_car2(&b) { q.t = th; self.policy.sent = Some(q); }
        self.deliver(&b, now);
    }
    /// Host: one of our AI cars (index `k` in the start message), right after our own car in the frame (same stamp, same
    /// rules: it goes out when the receivers' prediction has drifted, within the data budget).
    /// (`dist`: how far the nearest player's car is)
    pub fn send_bot(&mut self, k: usize, mut c: CarSnap, dt: f64, g: smooth::Ground, dist: f32) {
        if !self.is_host || self.server_bots || k >= self.bots_out.len() { return; }
        let now = self.now(); let th = self.stamp.get().unwrap_or_else(|| self.host_now()); c.t = th; c.a = self.bots_out[k].policy.accel(c.u, dt as f32);
        let (thr, rate, beat) = self.relevance(dist);
        let bo = &mut self.bots_out[k]; if !bo.policy.due(&c, g, &thr, rate, beat) { return; }
        bo.seq = bo.seq.wrapping_add(1); let b = bot3(self.me, bo.seq, k as u8, &c);
        if let Some((_, _, _, mut q)) = read_car2(&b) { q.t = th; bo.policy.sent = Some(q); }
        self.deliver(&b, now);
    }

    /// A bump we settled with a player's car (`d`: the push their car got here): shown on their car at once, and sent
    /// to their phone, which owns that car. Their phone takes the push unless it settled the same contact itself.
    /// `by`: which of our cars did it (proto::BY_PLAYER, or the index of one of our AI cars when we are the host). The
    /// host's AI cars (ids from `bot_id`) are pushed the same way, the push going to the host.
    pub fn bump(&mut self, id: u32, by: u8, d: [f32; 5], imp: f32) {
        let shown = d.map(|v| v * crate::game::race::SHOW as f32);   // shown at SHOW, sent in full (see race::contacts)
        if let Some(k) = bot_of(id) { if let Some(b) = self.bots.get_mut(&k) { b.track.nudge(shown);
            let a = b.hit_acc.get_or_insert(([0.0; 5], 0.0)); for q in 0..5 { a.0[q] += d[q]; } a.1 = a.1.max(imp); } return; }
        if let Some(p) = self.peers.get_mut(&id) { p.track.nudge(shown);
            match p.hit_acc.iter_mut().find(|x| x.0 == by) { Some(a) => { for q in 0..5 { a.1[q] += d[q]; } a.2 = a.2.max(imp); } None => p.hit_acc.push((by, d, imp)) } }
    }
    fn track(&self, id: u32) -> Option<&smooth::Track> { match bot_of(id) { Some(k) => self.bots.get(&k).map(|b| &b.track), None => self.peers.get(&id).map(|p| &p.track) } }
    fn track_mut(&mut self, id: u32) -> Option<&mut smooth::Track> { match bot_of(id) { Some(k) => self.bots.get_mut(&k).map(|b| &mut b.track), None => self.peers.get_mut(&id).map(|p| &mut p.track) } }
    /// whether a car is, for now, moved only by its own phone (a contact it has already taken in)
    pub fn owned(&self, id: u32) -> bool { self.track(id).map(|t| t.owned()).unwrap_or(false) }
    /// how far ahead of its newest state a car is drawn (s)
    pub fn horizon(&self, id: u32) -> f32 { self.track(id).map(|t| t.horizon()).unwrap_or(0.0) }
    /// how close a car is to ours (0 far .. 1 close; close cars are drawn at the present)
    pub fn set_near(&mut self, id: u32, near: f32) { if let Some(t) = self.track_mut(id) { t.near = near; } }
    /// bumps other phones gave our car since the last call (sender, which of their cars, push, impact speed)
    pub fn take_hits(&mut self) -> Vec<(u32, u8, [f32; 5], f32)> { std::mem::take(&mut self.hits_in) }
    /// host: bumps guests gave our AI cars since the last call (sender, AI car, push, impact speed)
    pub fn take_bot_hits(&mut self) -> Vec<(u32, u8, [f32; 5], f32)> { std::mem::take(&mut self.bot_hits_in) }
    /// seconds since a car last reported (None once its player, or the host for an AI car, has left the room)
    pub fn car_silence(&self, id: u32) -> Option<f64> {
        let first = match bot_of(id) { Some(_) if self.server_bots => self.race_t0, Some(_) => self.peers.get(&self.host_id?)?.first_seen, None => self.peers.get(&id)?.first_seen };
        Some(match self.track(id)?.latest { Some(c) => self.host_now() - c.t, None => (self.now() - first).min(self.now() - self.race_t0) }) }

    fn handle_car(&mut self, b: &[u8], from: Option<SocketAddr>, now: f64) {
        let Some((sid, seq, ms, mut c)) = read_car2(b) else { return };
        let th = self.host_now();
        if sid == SERVER_ID as u16 { if let Some(k) = bot_idx(b) { if !self.server_bots { return; } self.srv_heard = now;
            let Some(bt) = self.bots.get_mut(&k) else { return };
            if bt.track.latest.is_some() && (seq.wrapping_sub(bt.last_seq) as i8) <= 0 { return; }
            let age = ((th * 1000.0).rem_euclid(65536.0) - ms as f64).rem_euclid(65536.0) / 1000.0; c.t = th - if age < 10.0 { age } else { 0.0 };
            bt.last_seq = seq; bt.track.push_at(c, th); } return; }
        let Some(p) = self.peers.values_mut().find(|p| p.id as u16 == sid && (from.is_none() || p.direct == from || p.cands.contains(&from.unwrap()))) else { return };
        p.last_seen = now; if from.is_some() { p.last_udp = now; } else if p.direct.is_some() { p.mirror_until = now + 2.0; }
        let age = ((th * 1000.0).rem_euclid(65536.0) - ms as f64).rem_euclid(65536.0) / 1000.0; c.t = th - if age < 10.0 { age } else { 0.0 };
        // one of the host's AI cars
        if let Some(k) = bot_idx(b) { if !p.host || self.is_host { return; } if self.server_bots { self.server_bots = false; }   // the host took the AI cars back
            let Some(bt) = self.bots.get_mut(&k) else { return };
            if bt.track.latest.is_some() && (seq.wrapping_sub(bt.last_seq) as i8) <= 0 { return; }
            bt.last_seq = seq; bt.track.push_at(c, th); return; }
        if p.track.latest.is_some() && (seq.wrapping_sub(p.last_seq) as i8) <= 0 { return; }   // late or duplicate
        p.last_seq = seq;
        p.track.push_at(c, th);
    }

    /// Call once per frame with the frame's length, before `remote`: advances the clock other cars are shown at.
    pub fn advance_view(&mut self, dt: f64) -> f64 { let hn = self.host_now(); self.frame_dt = dt.clamp(0.0, 0.05); self.view.advance(hn, dt) }

    /// Where a peer's car is shown this frame.
    pub fn remote(&mut self, id: u32, g: smooth::Ground) -> Option<CarSnap> {
        let t = self.view.get().unwrap_or_else(|| self.host_now()) + self.frame_dt; self.track_mut(id)?.sample(t, g)
    }
}

#[cfg(test)]
mod tests {
    use super::pick_brokers;
    #[test]
    fn brokers_cover_everyone() {
        assert_eq!(pick_brokers(0b111, &[0b111, 0b111]), vec![0]);       // everyone on everything: one publish
        assert_eq!(pick_brokers(0b111, &[0b010, 0b011]), vec![1]);       // the broker both share
        assert_eq!(pick_brokers(0b111, &[0b001, 0b100]), vec![0, 2]);    // split room: both brokers
        assert_eq!(pick_brokers(0b011, &[0b100]), vec![0]);              // unreachable peer: still the first broker
        assert_eq!(pick_brokers(0b110, &[0xFF]), vec![1]);               // older build (unknown): our first broker
        assert!(pick_brokers(0, &[1]).is_empty());
    }
}
