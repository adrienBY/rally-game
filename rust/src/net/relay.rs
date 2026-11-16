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
pub const URL: &str = "wss://fogwood-relay.adrienby.workers.dev/ws";
/// keep-alive / re-join interval (s), and how long without a word before the relay counts as down
const JOIN_EVERY: f64 = 4.0;
const DOWN_AFTER: f64 = 9.0;

pub struct Client {
    room: u32, me: u32, last_join: f64, last_rx: f64, pub members: u8,
    #[cfg(not(target_arch = "wasm32"))] ws: Option<super::ws::Ws>,
    #[cfg(not(target_arch = "wasm32"))] sock: Option<UdpSocket>,
    #[cfg(not(target_arch = "wasm32"))] addr: Option<SocketAddr>,
    #[cfg(not(target_arch = "wasm32"))] resolving: Option<Receiver<Option<SocketAddr>>>,
    #[cfg(target_arch = "wasm32")] ws: web::Sock,
}

fn join_msg(room: u32, me: u32) -> Vec<u8> { let mut w = vec![0x01]; w.extend_from_slice(&room.to_le_bytes()); w.extend_from_slice(&me.to_le_bytes()); w }

impl Client {
    pub fn start(room: u32, me: u32) -> Client {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let target = std::env::var("FW_RELAY").unwrap_or_else(|_| URL.to_string());
            if let Some(host) = target.strip_prefix("udp:").map(|h| h.to_string()) {
                let sock = UdpSocket::bind("0.0.0.0:0").ok().filter(|s| s.set_nonblocking(true).is_ok());
                // name lookup can block: on a thread
                let (t, r) = channel(); std::thread::Builder::new().name("relay dns".into()).spawn(move || { let _ = t.send(host.to_socket_addrs().ok().and_then(|mut a| a.find(|x| x.is_ipv4()))); }).ok();
                return Client { room, me, last_join: -99.0, last_rx: -99.0, members: 0, ws: None, sock, addr: None, resolving: Some(r) };
            }
            Client { room, me, last_join: -99.0, last_rx: -99.0, members: 0, ws: Some(super::ws::Ws::open(format!("{}/{}", target.trim_end_matches('/'), room))), sock: None, addr: None, resolving: None }
        }
        #[cfg(target_arch = "wasm32")]
        { Client { room, me, last_join: -99.0, last_rx: -99.0, members: 0, ws: web::Sock::open(room) } }
    }
    pub fn is_up(&self, now: f64) -> bool { now - self.last_rx < DOWN_AFTER }
    fn raw(&self, b: &[u8]) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        { if let Some(w) = &self.ws { return w.send(b); } match (&self.sock, self.addr) { (Some(s), Some(a)) => s.send_to(b, a).is_ok(), _ => false } }
        #[cfg(target_arch = "wasm32")]
        { self.ws.send(b) }
    }
    /// a packet for one member (`to`) or everyone else in the room (0); false if the relay is down
    pub fn send(&self, to: u32, payload: &[u8], now: f64) -> bool {
        if !self.is_up(now) { return false; }
        let mut w = Vec::with_capacity(payload.len() + 5); w.push(0x02); w.extend_from_slice(&to.to_le_bytes()); w.extend_from_slice(payload); self.raw(&w)
    }
    /// once a frame: join / keep alive, and the packets that arrived (sender id, payload)
    pub fn poll(&mut self, now: f64) -> Vec<(u32, Vec<u8>)> {
        let mut out = Vec::new(); let mut msgs: Vec<Vec<u8>> = Vec::new();
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(r) = &self.resolving { if let Ok(a) = r.try_recv() { self.addr = a; self.resolving = None; self.last_join = -99.0; } }
            if let (Some(s), Some(a)) = (&self.sock, self.addr) { let mut buf = [0u8; 1500]; while let Ok((n, from)) = s.recv_from(&mut buf) { if from == a { msgs.push(buf[..n].to_vec()); } } }
            if let Some(w) = &self.ws { msgs = w.take(); if w.fresh() { self.last_join = -99.0; } }
        }
        #[cfg(target_arch = "wasm32")]
        { self.ws.tick(); msgs = self.ws.take(); if self.ws.fresh() { self.last_join = -99.0; } }
        for m in msgs {
            match m.first() {
                Some(0x81) => { self.last_rx = now; self.members = m.get(9).copied().unwrap_or(0); }
                Some(0x82) if m.len() >= 5 => { self.last_rx = now; out.push((u32::from_le_bytes([m[1], m[2], m[3], m[4]]), m[5..].to_vec())); }
                Some(0x83) => { self.last_rx = now; }
                _ => {}
            }
        }
        if now - self.last_join > JOIN_EVERY { self.last_join = now; let j = join_msg(self.room, self.me); self.raw(&j); }
        out
    }
    pub fn leave(&self) { self.raw(&[0x04]); }
}

/// the browser build: the page holds the WebSocket (window.__fwWs, socket id SOCK); bytes come back through web_api
#[cfg(target_arch = "wasm32")]
pub mod web {
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
    /// socket id of the relay (the MQTT brokers use 0, 1, 2...)
    pub const SOCK: u32 = 1000;
    #[derive(Default)]
    struct St { open: bool, fresh: bool, inbox: Vec<Vec<u8>>, retry_at: f64, backoff: f64, live: bool, room: u32 }
    thread_local! { static ST: RefCell<St> = RefCell::new(St::default()); }
    fn now() -> f64 { js_sys::Date::now() / 1000.0 }
    fn url() -> String { format!("{}/{}", super::URL, ST.with(|s| s.borrow().room)) }
    pub struct Sock;
    impl Sock {
        pub fn open(room: u32) -> Sock { ST.with(|s| { let mut s = s.borrow_mut(); s.live = true; s.backoff = 1.0; s.inbox.clear(); s.room = room; }); ws_open(SOCK, &url()); Sock }
        pub fn send(&self, b: &[u8]) -> bool { if ST.with(|s| s.borrow().open) { ws_send(SOCK, b); true } else { false } }
        pub fn take(&self) -> Vec<Vec<u8>> { ST.with(|s| std::mem::take(&mut s.borrow_mut().inbox)) }
        /// just connected (join right away)
        pub fn fresh(&self) -> bool { ST.with(|s| std::mem::replace(&mut s.borrow_mut().fresh, false)) }
        pub fn tick(&self) {
            let again = ST.with(|s| { let mut s = s.borrow_mut(); if s.live && !s.open && s.retry_at > 0.0 && now() >= s.retry_at { s.retry_at = 0.0; true } else { false } });
            if again { ws_open(SOCK, &url()); }
        }
    }
    impl Drop for Sock { fn drop(&mut self) { ST.with(|s| { let mut s = s.borrow_mut(); s.live = false; s.open = false; }); ws_close(SOCK); } }
    pub fn on_open() { ST.with(|s| { let mut s = s.borrow_mut(); s.open = true; s.fresh = true; s.backoff = 1.0; }); }
    pub fn on_data(b: &[u8]) { ST.with(|s| { let mut s = s.borrow_mut(); if s.inbox.len() < 512 { s.inbox.push(b.to_vec()); } }); }
    pub fn on_close() { ST.with(|s| { let mut s = s.borrow_mut(); if !s.open && s.retry_at > now() { return; } s.open = false; s.retry_at = now() + s.backoff.max(1.0); s.backoff = (s.backoff * 2.0).min(30.0); }); }
}
