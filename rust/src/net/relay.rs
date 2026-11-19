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

pub struct Client {
    room: u32, me: u32, last_join: f64, last_rx: f64, pub members: u8, last_kick: f64,
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
                return Client { room, me, last_join: -99.0, last_rx: -99.0, members: 0, last_kick: 0.0, ws: None, sock, addr: None, resolving: Some(r) };
            }
            Client { room, me, last_join: -99.0, last_rx: -99.0, members: 0, last_kick: 0.0, ws: Some(super::ws::Ws::open(format!("{}/{}", target.trim_end_matches('/'), room))), sock: None, addr: None, resolving: None }
        }
        #[cfg(target_arch = "wasm32")]
        { Client { room, me, last_join: -99.0, last_rx: -99.0, members: 0, last_kick: 0.0, ws: web::Sock::open(web::SOCK, format!("{}/{}", URL, room)) } }
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
        // connected but no answer to our joins: the connection died without saying so (network switch, sleeping radio):
        // open a new one rather than sending into it for good
        if self.last_rx > -99.0 && now - self.last_rx > DOWN_AFTER && now - self.last_kick > DOWN_AFTER {
            self.last_kick = now; if std::env::var("FW_WSDEBUG").is_ok() { eprintln!("relay silent {:.1}s: reconnect", now - self.last_rx); }
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(w) = &self.ws { w.reconnect(); }
            #[cfg(target_arch = "wasm32")]
            self.ws.reconnect();
        }
        out
    }
    pub fn leave(&self) { self.raw(&[0x04]); }
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
