//! Client of the Fogwood relay (rust/relay): our own server that passes game packets between the phones of a room in
//! one hop. Much quicker and steadier than the public MQTT brokers (UDP, no TCP stalls, no shared load), and it
//! reaches phones whose network blocks direct connections. Apps talk to it over UDP, the browser build over a
//! secure WebSocket. If it can't be reached the game simply carries on with the brokers.
//! Wire format: see rust/relay/src/main.rs.
#[cfg(not(target_arch = "wasm32"))]
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{channel, Receiver};

/// the relay (Cloudflare Worker, relay-cf/): clients connect to URL/<room code>. FW_RELAY overrides on desktop builds:
/// a ws:// or wss:// base URL, or "udp:host:port" for the UDP relay in rust/relay.
pub const URL: &str = "wss://fogwood-relay.adrien-paul-bonnay.workers.dev/ws";
/// keep-alive / re-join interval (s; the relay answers every join), and how long without a word before the relay
/// counts as down and the socket is dropped and opened again
const JOIN_EVERY: f64 = 2.0;
const DOWN_AFTER: f64 = 5.0;

/// One WebSocket to the room. With a relay that speaks v2 there are two ("lanes"), both carrying everything, both ways,
/// the first copy winning: a TCP connection stalled on a lost segment (head-of-line blocking, common on mobile networks
/// and busy Wi-Fi) no longer holds the cars back, the other lane carries them. (`dual` off: one lane carries the game,
/// the other idles as a hot spare that takes over at once if the first dies.)
struct Lane { last_join: f64, last_rx: f64, last_kick: f64, last_data: f64,
    /// test hook FW_LANELAG: what arrived, held back like a slow TCP link (due time, message), and the last due time
    held: std::collections::VecDeque<(f64, Vec<u8>)>, held_last: f64,
    #[cfg(not(target_arch = "wasm32"))] ws: super::ws::Ws,
    #[cfg(target_arch = "wasm32")] ws: web::Sock }
impl Lane {
    fn open(url: &str, k: usize) -> Lane {
        #[cfg(not(target_arch = "wasm32"))]
        { let _ = k; Lane { last_join: -99.0, last_rx: -99.0, last_kick: 0.0, last_data: -99.0, held: Default::default(), held_last: 0.0, ws: super::ws::Ws::open(url.to_string()) } }
        #[cfg(target_arch = "wasm32")]
        { Lane { last_join: -99.0, last_rx: -99.0, last_kick: 0.0, last_data: -99.0, held: Default::default(), held_last: 0.0, ws: web::Sock::open(if k == 0 { web::SOCK } else { web::SOCK2 }, url.to_string()) } }
    }
}

/// join flags: we read coalesced downlink (0x88); bits 1-2: which lane this socket is; bit 3: write the room's packets
/// to this socket (a spare lane leaves it off)
const J_BATCH: u8 = 1; const J_RX: u8 = 8;
/// WELCOME capability bits: the relay drives AI cars; it speaks v2 (lanes, 0x09 SEND2 with dedup, 0x88 batches)
const C_SIM: u8 = 1; const C_V2: u8 = 2;

pub struct Client {
    room: u32, me: u32, pub members: u8,
    /// the relay can drive a race's AI cars (it said so when we joined), and what it said about the stage we uploaded
    /// (1 ready, 2 failed) and when
    pub sim: bool, pub sim_state: Option<(u8, f64)>,
    /// the relay speaks v2 (see Lane)
    pub v2: bool,
    url: String, lanes: Vec<Lane>, pseq: std::cell::Cell<u16>,
    /// both lanes carry everything (see Lane), and the lanes the relay was last told to write to
    pub dual: bool, rx_told: Vec<bool>,
    /// what came in lately (hashes), so a message that arrives on both lanes is taken once
    seen: std::collections::VecDeque<u64>, seen_set: std::collections::HashSet<u64>,
    /// UDP relay (desktop tests, rust/relay): one socket, no lanes
    #[cfg(not(target_arch = "wasm32"))] udp: Option<(UdpSocket, Option<SocketAddr>, Option<Receiver<Option<SocketAddr>>>, f64, f64)>,
}

fn join_msg(room: u32, me: u32, lane: usize, rx: bool) -> Vec<u8> { let mut w = vec![0x01]; w.extend_from_slice(&room.to_le_bytes()); w.extend_from_slice(&me.to_le_bytes()); w.push(if std::env::var("FW_NOCOALESCE").is_ok() { 0 } else { J_BATCH } | ((lane as u8) & 3) << 1 | if rx { J_RX } else { 0 }); w }
fn rnd() -> f64 {
    static S: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0x9E3779B97F4A7C15);
    #[cfg(not(target_arch = "wasm32"))] let pid = std::process::id() as u64;
    #[cfg(target_arch = "wasm32")] let pid = 7u64;
    let mut x = S.load(std::sync::atomic::Ordering::Relaxed) ^ pid.rotate_left(17); x ^= x << 13; x ^= x >> 7; x ^= x << 17; S.store(x, std::sync::atomic::Ordering::Relaxed); (x >> 11) as f64 / (1u64 << 53) as f64
}
fn lane_lag() -> Option<(f64, f64, f64)> {
    static L: std::sync::OnceLock<Option<(f64, f64, f64)>> = std::sync::OnceLock::new();
    *L.get_or_init(|| { let v: Vec<f64> = std::env::var("FW_LANELAG").ok()?.split(',').filter_map(|x| x.parse().ok()).collect(); if v.len() == 3 { Some((v[0] / 1000.0, v[1] / 1000.0, v[2])) } else { None } })
}
fn fnv(b: &[u8]) -> u64 { let mut h = 0xcbf29ce484222325u64; for x in b { h ^= *x as u64; h = h.wrapping_mul(0x100000001b3); } h }

impl Client {
    pub fn start(room: u32, me: u32) -> Client {
        let mut c = Client { room, me, members: 0, sim: false, sim_state: None, v2: false, url: String::new(), lanes: Vec::new(), pseq: Default::default(), dual: false, rx_told: Vec::new(), seen: Default::default(), seen_set: Default::default(),
            #[cfg(not(target_arch = "wasm32"))] udp: None };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let target = std::env::var("FW_RELAY").unwrap_or_else(|_| URL.to_string());
            if let Some(host) = target.strip_prefix("udp:").map(|h| h.to_string()) {
                if let Some(sock) = UdpSocket::bind("0.0.0.0:0").ok().filter(|s| s.set_nonblocking(true).is_ok()) {
                    // name lookup can block: on a thread
                    let (t, r) = channel(); std::thread::Builder::new().name("relay dns".into()).spawn(move || { let _ = t.send(host.to_socket_addrs().ok().and_then(|mut a| a.find(|x| x.is_ipv4()))); }).ok();
                    c.udp = Some((sock, None, Some(r), -99.0, -99.0)); }
                return c;
            }
            c.url = format!("{}/{}", target.trim_end_matches('/'), room);
        }
        #[cfg(target_arch = "wasm32")]
        { c.url = format!("{}/{}", URL, room); }
        c.lanes.push(Lane::open(&c.url, 0)); c
    }
    fn last_rx(&self) -> f64 {
        let mut t = self.lanes.iter().map(|l| l.last_rx).fold(-99.0, f64::max);
        #[cfg(not(target_arch = "wasm32"))] if let Some(u) = &self.udp { t = t.max(u.4); }
        t
    }
    pub fn is_up(&self, now: f64) -> bool { now - self.last_rx() < DOWN_AFTER }
    /// how many copies of each packet go out (and come back): the lanes that are up
    pub fn lanes_up(&self, now: f64) -> usize { if !self.dual { return 1; } self.lanes.iter().filter(|l| now - l.last_rx < DOWN_AFTER).count().max(1) }
    /// the lane that carries the game when only one does: the first that is up
    fn primary(&self, now: f64) -> usize { self.lanes.iter().position(|l| now - l.last_rx < DOWN_AFTER).unwrap_or(0) }
    /// whether lane k carries the game now
    fn carries(&self, k: usize, now: f64) -> bool { if self.dual { now - self.lanes[k].last_rx < DOWN_AFTER || k == self.primary(now) } else { k == self.primary(now) } }
    /// a WebSocket relay (TCP + TLS framing on every message) rather than the UDP one
    pub fn is_ws(&self) -> bool { !self.lanes.is_empty() }
    fn raw_lane(&self, k: usize, b: &[u8]) -> bool { self.lanes.get(k).map(|l| l.ws.send(b)).unwrap_or(false) }
    fn raw(&self, b: &[u8]) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((s, Some(a), ..)) = &self.udp { return s.send_to(b, *a).is_ok(); }
        self.raw_lane(0, b)
    }
    /// a packet for one member (`to`) or everyone else in the room (0); false if the relay is down
    pub fn send(&self, to: u32, payload: &[u8], now: f64) -> bool {
        if !self.is_up(now) { return false; }
        if self.v2 {
            let q = self.pseq.get().wrapping_add(1); self.pseq.set(q);
            let mut w = Vec::with_capacity(payload.len() + 7); w.push(0x09); w.extend_from_slice(&to.to_le_bytes()); w.extend_from_slice(&q.to_le_bytes()); w.extend_from_slice(payload);
            #[cfg(not(target_arch = "wasm32"))]
            if self.udp.is_some() { return self.raw(&w); }
            let mut ok = false; for k in 0..self.lanes.len() { if self.carries(k, now) { ok |= self.raw_lane(k, &w); } } return ok;
        }
        let mut w = Vec::with_capacity(payload.len() + 5); w.push(0x02); w.extend_from_slice(&to.to_le_bytes()); w.extend_from_slice(payload); self.raw(&w)
    }
    /// once a frame: join / keep alive, and the packets that arrived (sender id, payload)
    pub fn poll(&mut self, now: f64) -> Vec<(u32, Vec<u8>)> {
        let mut out = Vec::new(); let mut msgs: Vec<(usize, Vec<u8>)> = Vec::new();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(u) = self.udp.as_mut() {
            if let Some(r) = &u.2 { if let Ok(a) = r.try_recv() { u.1 = a; u.2 = None; u.3 = -99.0; } }
            if let Some(a) = u.1 { let mut buf = [0u8; 1500]; while let Ok((n, from)) = u.0.recv_from(&mut buf) { if from == a { msgs.push((usize::MAX, buf[..n].to_vec())); } } }
        }
        for (k, l) in self.lanes.iter_mut() .enumerate() {
            #[cfg(target_arch = "wasm32")] l.ws.tick();
            let got = l.ws.take(); if l.ws.fresh() { l.last_join = -99.0; }
            // test hook: FW_LANELAG="ms,jitter ms,loss": each lane a TCP link of its own (late, in order, a lost segment
            // holding everything behind it for a retransmission)
            match lane_lag() { Some((lat, jit, loss)) => {
                    for m in got {
                        let mut at = now + lat + (rnd() * 2.0 - 1.0) * jit; if rnd() < loss { at += 0.2 + 2.0 * lat; }
                        at = at.max(l.held_last); l.held_last = at; l.held.push_back((at, m)); }
                    while l.held.front().map(|h| h.0 <= now).unwrap_or(false) { msgs.push((k, l.held.pop_front().unwrap().1)); } }
                None => for m in got { msgs.push((k, m)); } }
        }
        // a coalesced message (0x88) holds several, each [len u16][message]
        let mut flat: Vec<(usize, Vec<u8>)> = Vec::with_capacity(msgs.len());
        for (k, m) in msgs { if m.first() == Some(&0x88) { let mut i = 1; while i + 2 <= m.len() { let n = u16::from_le_bytes([m[i], m[i + 1]]) as usize; i += 2; if i + n > m.len() { break; } flat.push((k, m[i..i + n].to_vec())); i += n; } } else { flat.push((k, m)); } }
        let two = self.lanes.len() > 1;
        for (k, m) in flat {
            match k { usize::MAX => { #[cfg(not(target_arch = "wasm32"))] if let Some(u) = self.udp.as_mut() { u.4 = now; } } k => { if let Some(l) = self.lanes.get_mut(k) { l.last_rx = now; } } }
            match m.first() {
                Some(0x81) => { self.members = m.get(9).copied().unwrap_or(0); let caps = m.get(10).copied().unwrap_or(0); self.sim = caps & C_SIM != 0; self.v2 = caps & C_V2 != 0; }
                Some(0x87) => { self.sim_state = m.get(1).map(|v| (*v, now)); }
                Some(0x82) if m.len() >= 5 => {
                    if k != usize::MAX { if let Some(l) = self.lanes.get_mut(k) { l.last_data = now; } }
                    // the same message on both lanes: once
                    if two { let h = fnv(&m); if !self.seen_set.insert(h) { continue; } self.seen.push_back(h); if self.seen.len() > 512 { if let Some(o) = self.seen.pop_front() { self.seen_set.remove(&o); } } }
                    out.push((u32::from_le_bytes([m[1], m[2], m[3], m[4]]), m[5..].to_vec())); }
                _ => {}
            }
        }
        // v2: the second lane, once the relay has said it speaks it (WebSockets only)
        if self.v2 && self.lanes.len() == 1 && !self.url.is_empty() && std::env::var("FW_ONELANE").is_err() { let l = Lane::open(&self.url, 1); self.lanes.push(l); }
        let (room, me) = (self.room, self.me);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(u) = self.udp.as_mut() { if u.1.is_some() && now - u.3 > JOIN_EVERY { u.3 = now; let j = join_msg(room, me, 0, true); let _ = u.0.send_to(&j, u.1.unwrap()); } }
        // which lanes the relay should write to: tell it at once when that changes
        let rx: Vec<bool> = (0..self.lanes.len()).map(|k| self.carries(k, now)).collect();
        if rx != self.rx_told { for (k, l) in self.lanes.iter_mut().enumerate() { if self.rx_told.get(k) != rx.get(k) { l.last_join = -99.0; } } self.rx_told = rx.clone(); }
        for (k, l) in self.lanes.iter_mut().enumerate() {
            if now - l.last_join > JOIN_EVERY { l.last_join = now; let j = join_msg(room, me, k, rx[k]); l.ws.send(&j); }
            // connected but no answer to our joins: the connection died without saying so (network switch, sleeping radio):
            // open a new one rather than sending into it for good
            if l.last_rx > -99.0 && now - l.last_rx > DOWN_AFTER && now - l.last_kick > DOWN_AFTER {
                l.last_kick = now; if std::env::var("FW_WSDEBUG").is_ok() { eprintln!("relay lane {} silent {:.1}s: reconnect", k, now - l.last_rx); }
                l.ws.reconnect(); }
        }
        out
    }
    pub fn leave(&self) { #[cfg(not(target_arch = "wasm32"))] if self.udp.is_some() { self.raw(&[0x04]); } for k in 0..self.lanes.len() { self.raw_lane(k, &[0x04]); } }
    /// the stage for the relay's AI cars (server::stage_blob); only over a WebSocket (it is too big for a datagram)
    pub fn stage(&mut self, blob: &[u8]) -> bool {
        if self.lanes.is_empty() { return false; }
        self.sim_state = None; let mut w = Vec::with_capacity(blob.len() + 1); w.push(0x07); w.extend_from_slice(blob); self.raw_lane(0, &w)
    }
    /// a host's room for the open-rooms list (`closed`: take it off)
    pub fn announce(&self, name: &str, map: &str, players: usize, racing: bool, closed: bool) {
        let clean: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ').take(16).collect();
        let j = format!("{{\"c\":{},\"n\":\"{}\",\"m\":\"{}\",\"p\":{},\"r\":{},\"x\":{}}}", self.room, clean, map.chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>(), players, racing, closed);
        let mut w = vec![0x05]; w.extend_from_slice(j.as_bytes()); self.raw(&w);
    }
}

/// the browser build: the page holds the WebSockets (window.__fwWs); bytes come back through web_api
#[cfg(target_arch = "wasm32")]
pub mod web {
    use std::cell::RefCell;
    use std::collections::HashMap;
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
    /// socket ids from here up belong to the relay (the room) and the lobby; the MQTT brokers use 0, 1, 2...
    pub const SOCK: u32 = 1000;
    pub const LOBBY: u32 = 1001;
    /// the room's second lane
    pub const SOCK2: u32 = 1002;
    #[derive(Default)]
    struct St { open: bool, fresh: bool, inbox: Vec<Vec<u8>>, retry_at: f64, backoff: f64, live: bool, url: String }
    thread_local! { static ST: RefCell<HashMap<u32, St>> = RefCell::new(HashMap::new()); }
    fn now() -> f64 { js_sys::Date::now() / 1000.0 }
    pub struct Sock(u32);
    impl Sock {
        pub fn open(id: u32, url: String) -> Sock {
            ST.with(|s| { let mut m = s.borrow_mut(); m.insert(id, St { live: true, backoff: 1.0, url: url.clone(), ..Default::default() }); }); ws_open(id, &url); Sock(id)
        }
        pub fn send(&self, b: &[u8]) -> bool { if ST.with(|s| s.borrow().get(&self.0).map(|x| x.open).unwrap_or(false)) { ws_send(self.0, b); true } else { false } }
        pub fn take(&self) -> Vec<Vec<u8>> { ST.with(|s| s.borrow_mut().get_mut(&self.0).map(|x| std::mem::take(&mut x.inbox)).unwrap_or_default()) }
        /// just connected
        pub fn fresh(&self) -> bool { ST.with(|s| s.borrow_mut().get_mut(&self.0).map(|x| std::mem::replace(&mut x.fresh, false)).unwrap_or(false)) }
        /// drop the socket and open a new one
        pub fn reconnect(&self) {
            let url = ST.with(|s| { let mut m = s.borrow_mut(); let x = m.get_mut(&self.0)?; x.open = false; x.retry_at = 0.0; Some(x.url.clone()) });
            if let Some(u) = url { ws_open(self.0, &u); }
        }
        pub fn tick(&self) {
            let again = ST.with(|s| { let mut m = s.borrow_mut(); let Some(x) = m.get_mut(&self.0) else { return None };
                if x.live && !x.open && x.retry_at > 0.0 && now() >= x.retry_at { x.retry_at = 0.0; Some(x.url.clone()) } else { None } });
            if let Some(u) = again { ws_open(self.0, &u); }
        }
    }
    impl Drop for Sock { fn drop(&mut self) { ST.with(|s| { s.borrow_mut().remove(&self.0); }); ws_close(self.0); } }
    pub fn on_open(id: u32) { ST.with(|s| if let Some(x) = s.borrow_mut().get_mut(&id) { x.open = true; x.fresh = true; x.backoff = 1.0; }); }
    pub fn on_data(id: u32, b: &[u8]) { ST.with(|s| if let Some(x) = s.borrow_mut().get_mut(&id) { if x.inbox.len() < 512 { x.inbox.push(b.to_vec()); } }); }
    pub fn on_close(id: u32) { ST.with(|s| if let Some(x) = s.borrow_mut().get_mut(&id) { if !x.open && x.retry_at > now() { return; } x.open = false; x.retry_at = now() + x.backoff.max(1.0); x.backoff = (x.backoff * 2.0).min(30.0); }); }
}
