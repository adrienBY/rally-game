//! WebRTC data channels: UDP between phones even where only a browser can talk (the web build on iPhones), so a lost
//! packet costs one car state instead of stalling a TCP connection.
//!
//! One unordered, unreliable data channel per pair of phones ("fw", negotiated out of band as stream 0: no extra
//! handshake). Game packets go over it exactly as over the app's own UDP path. The offer, answer and ICE candidates
//! travel through the room (relay or brokers) as K_RTC messages; whoever has the lower id offers.
//!
//! - In the browser the page's RTCPeerConnection does the work (web_boot.js, window.__fwRtc).
//! - In the app, str0m (sans-IO WebRTC in Rust) runs on the session's own UDP socket: its STUN / DTLS packets are told
//!   apart from ours by their first byte (RFC 7983), and the candidates it offers are the ones the app already knows
//!   (the local address and the public one STUN found for that very socket).
//!
//! No TURN server: when the two networks can't reach each other directly, the link never opens and the pair simply
//! keeps using the relay.

/// a negotiation message for the other phone
#[derive(Clone, Debug, PartialEq)]
pub enum Signal { Offer(String), Answer(String), Cand(String) }
impl Signal {
    pub fn kind(&self) -> u8 { match self { Signal::Offer(_) => 0, Signal::Answer(_) => 1, Signal::Cand(_) => 2 } }
    pub fn text(&self) -> &str { match self { Signal::Offer(s) | Signal::Answer(s) | Signal::Cand(s) => s } }
    pub fn from(kind: u8, text: String) -> Option<Signal> { match kind { 0 => Some(Signal::Offer(text)), 1 => Some(Signal::Answer(text)), 2 => Some(Signal::Cand(text)), _ => None } }
}

/// how long the data channel keeps retrying a message (ms): longer than that, a newer car state has replaced it anyway
pub const LIFETIME_MS: u16 = 200;

/// STUN servers the browser gathers its public address from (the app asks the same ones itself, see udp.rs)
pub const ICE_SERVERS: &str = r#"[{"urls":["stun:stun.l.google.com:19302","stun:stun.cloudflare.com:3478"]}]"#;

#[cfg(not(target_arch = "wasm32"))]
pub use native::Links;
#[cfg(target_arch = "wasm32")]
pub use web::Links;
/// the clock the links run on (str0m wants std's; a browser has none, so web_time's there)
#[cfg(not(target_arch = "wasm32"))]
pub type Now = std::time::Instant;
#[cfg(target_arch = "wasm32")]
pub type Now = web_time::Instant;
pub fn now() -> Now { Now::now() }

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::Signal;
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::time::Instant;
    use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
    use str0m::channel::{ChannelConfig, ChannelId, Reliability};
    use str0m::net::{Protocol, Receive};
    use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc};

    /// unordered; a message is retried for up to 200 ms then dropped (with no retries at all, a packet that merely
    /// arrives out of order gets written off by the sender and thrown away on arrival)
    fn config() -> ChannelConfig { ChannelConfig { label: "fw".into(), ordered: false, reliability: Reliability::MaxPacketLifetime { lifetime: super::LIFETIME_MS }, negotiated: Some(0), protocol: String::new() } }

    struct Link { rtc: Rtc, cid: Option<ChannelId>, pending: Option<SdpPendingOffer>, open: bool, dead: bool }

    /// the app's WebRTC links, one per phone
    #[derive(Default)]
    pub struct Links {
        links: HashMap<u32, Link>,
        /// our socket's local address (where datagrams arrive) and public one (from STUN)
        local: Option<SocketAddr>, public: Option<SocketAddr>,
        signals: Vec<(u32, Signal)>, inbox: Vec<(u32, Vec<u8>)>, transmit: Vec<(SocketAddr, Vec<u8>)>,
    }

    impl Links {
        pub fn set_addrs(&mut self, local: Option<SocketAddr>, public: Option<SocketAddr>) { self.local = local; self.public = public; }
        fn new_rtc(&self, now: Instant) -> Option<Rtc> {
            let mut rtc = Rtc::builder().build(now);
            let local = self.local?;
            if let Ok(c) = Candidate::host(local, "udp") { rtc.add_local_candidate(c); }
            if let Some(p) = self.public { if p != local { if let Ok(c) = Candidate::server_reflexive(p, local, "udp") { rtc.add_local_candidate(c); } } }
            Some(rtc)
        }
        /// start a link with `id`; the offerer sends its offer (the answerer waits for one)
        pub fn open(&mut self, id: u32, offerer: bool, now: Instant) {
            self.close(id);
            if !offerer { return; }
            let Some(mut rtc) = self.new_rtc(now) else { return };
            let mut api = rtc.sdp_api(); let cid = api.add_channel_with_config(config());
            let Some((offer, pending)) = api.apply() else { return };
            self.signals.push((id, Signal::Offer(offer.to_sdp_string())));
            let mut l = Link { rtc, cid: Some(cid), pending: Some(pending), open: false, dead: false };
            drain(id, &mut l, &mut self.inbox, &mut self.transmit); self.links.insert(id, l);
        }
        pub fn signal(&mut self, id: u32, s: Signal, now: Instant) {
            match s {
                Signal::Offer(sdp) => {
                    self.close(id);
                    let Ok(offer) = SdpOffer::from_sdp_string(&sdp) else { return };
                    let Some(mut rtc) = self.new_rtc(now) else { return };
                    let Ok(answer) = rtc.sdp_api().accept_offer(offer) else { return };
                    let mut l = Link { rtc, cid: None, pending: None, open: false, dead: false };
                    drain(id, &mut l, &mut self.inbox, &mut self.transmit);
                    l.cid = Some(l.rtc.direct_api().create_data_channel(config()));
                    drain(id, &mut l, &mut self.inbox, &mut self.transmit);
                    self.signals.push((id, Signal::Answer(answer.to_sdp_string()))); self.links.insert(id, l);
                }
                Signal::Answer(sdp) => {
                    let Some(l) = self.links.get_mut(&id) else { return }; let Some(p) = l.pending.take() else { return };
                    let Ok(answer) = SdpAnswer::from_sdp_string(&sdp) else { l.dead = true; return };
                    if l.rtc.sdp_api().accept_answer(p, answer).is_err() { l.dead = true; return; }
                    drain(id, l, &mut self.inbox, &mut self.transmit);
                }
                Signal::Cand(c) => {
                    let Some(l) = self.links.get_mut(&id) else { return };
                    // "mid\ncandidate:..." (a browser's local addresses come as .local names we can't resolve: its
                    // public one, and the address its checks arrive from, are what we can use)
                    let line = c.rsplit('\n').next().unwrap_or(&c); let line = line.trim().trim_start_matches("a=");
                    if line.contains(".local ") || line.is_empty() { return; }
                    if let Ok(cand) = Candidate::from_sdp_string(line) { l.rtc.add_remote_candidate(cand); drain(id, l, &mut self.inbox, &mut self.transmit); }
                }
            }
        }
        /// a datagram that arrived on our socket: true if a link took it (STUN / DTLS from a WebRTC peer)
        pub fn datagram(&mut self, source: SocketAddr, b: &[u8], now: Instant) -> bool {
            if self.links.is_empty() { return false; }
            let Some(dest) = self.local else { return false };
            let Ok(r) = Receive::new(Protocol::Udp, source, dest, b) else { return false };
            let input = Input::Receive(now, r);
            let Some(id) = self.links.iter().find(|(_, l)| !l.dead && l.rtc.accepts(&input)).map(|(id, _)| *id) else { return false };
            let l = self.links.get_mut(&id).unwrap();
            if l.rtc.handle_input(input).is_err() { l.dead = true; }
            drain(id, l, &mut self.inbox, &mut self.transmit); true
        }
        /// once a frame: timers (ICE checks, DTLS / SCTP retransmissions, keep-alives)
        pub fn tick(&mut self, now: Instant) {
            for (id, l) in self.links.iter_mut() { if l.dead { continue; } if l.rtc.handle_input(Input::Timeout(now)).is_err() { l.dead = true; } drain(*id, l, &mut self.inbox, &mut self.transmit); }
        }
        pub fn send(&mut self, id: u32, b: &[u8]) -> bool {
            let Some(l) = self.links.get_mut(&id) else { return false }; if !l.open || l.dead { return false; }
            let Some(cid) = l.cid else { return false };
            let ok = match l.rtc.channel(cid) { Some(mut ch) => ch.write(true, b).unwrap_or(false), None => false };
            drain(id, l, &mut self.inbox, &mut self.transmit); ok
        }
        pub fn is_open(&self, id: u32) -> bool { self.links.get(&id).map(|l| l.open && !l.dead).unwrap_or(false) }
        /// a link that failed or was closed by the other side
        pub fn is_dead(&self, id: u32) -> bool { self.links.get(&id).map(|l| l.dead).unwrap_or(false) }
        pub fn exists(&self, id: u32) -> bool { self.links.contains_key(&id) }
        pub fn close(&mut self, id: u32) { if let Some(mut l) = self.links.remove(&id) { l.rtc.disconnect(); } }
        pub fn take_signals(&mut self) -> Vec<(u32, Signal)> { std::mem::take(&mut self.signals) }
        pub fn take_inbox(&mut self) -> Vec<(u32, Vec<u8>)> { std::mem::take(&mut self.inbox) }
        pub fn take_transmit(&mut self) -> Vec<(SocketAddr, Vec<u8>)> { std::mem::take(&mut self.transmit) }
        pub fn ids(&self) -> Vec<u32> { self.links.keys().copied().collect() }
    }

    /// everything the link has to say after a change (str0m wants every change followed by this)
    fn drain(id: u32, l: &mut Link, inbox: &mut Vec<(u32, Vec<u8>)>, transmit: &mut Vec<(SocketAddr, Vec<u8>)>) {
        for _ in 0..512 {
            match l.rtc.poll_output() {
                Ok(Output::Timeout(_)) => return,
                Ok(Output::Transmit(t)) => { if transmit.len() < 4096 { transmit.push((t.destination, t.contents.to_vec())); } }
                Ok(Output::Event(e)) => match e {
                    Event::ChannelOpen(cid, _) => { if l.cid.is_none() || l.cid == Some(cid) { l.cid = Some(cid); l.open = true; } }
                    Event::ChannelData(d) => { if inbox.len() < 1024 { inbox.push((id, d.data)); } }
                    Event::ChannelClose(_) => { l.open = false; l.dead = true; }
                    Event::IceConnectionStateChange(IceConnectionState::Disconnected) => { l.open = false; l.dead = true; }
                    _ => {}
                },
                Err(_) => { l.dead = true; return; }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    //! the browser's RTCPeerConnection, driven from the page (web_boot.js: window.__fwRtc); what it reports comes back
    //! through web_api (web_rtc_*) into the queues here
    use super::Signal;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use web_time::Instant;
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = __fwRtc, js_name = open)]
        fn rtc_open(id: u32, offerer: bool, ice: &str);
        #[wasm_bindgen(js_namespace = __fwRtc, js_name = signal)]
        fn rtc_signal(id: u32, kind: u8, text: &str);
        #[wasm_bindgen(js_namespace = __fwRtc, js_name = send)]
        fn rtc_send(id: u32, b: &[u8]) -> bool;
        #[wasm_bindgen(js_namespace = __fwRtc, js_name = close)]
        fn rtc_close(id: u32);
    }
    #[derive(Default)]
    struct St { open: HashMap<u32, bool>, dead: HashSet<u32>, signals: Vec<(u32, Signal)>, inbox: Vec<(u32, Vec<u8>)> }
    thread_local! { static ST: RefCell<St> = RefCell::new(St::default()); }
    pub fn on_open(id: u32) { ST.with(|s| { s.borrow_mut().open.insert(id, true); }); }
    pub fn on_close(id: u32) { ST.with(|s| { let mut s = s.borrow_mut(); s.open.insert(id, false); s.dead.insert(id); }); }
    pub fn on_data(id: u32, b: &[u8]) { ST.with(|s| { let mut s = s.borrow_mut(); if s.inbox.len() < 1024 { s.inbox.push((id, b.to_vec())); } }); }
    pub fn on_signal(id: u32, kind: u8, text: String) { if let Some(sg) = Signal::from(kind, text) { ST.with(|s| { let mut s = s.borrow_mut(); if s.signals.len() < 256 { s.signals.push((id, sg)); } }); } }

    /// the page's links (same interface as the app's)
    #[derive(Default)]
    pub struct Links { mine: Vec<u32> }
    impl Links {
        pub fn set_addrs(&mut self, _l: Option<std::net::SocketAddr>, _p: Option<std::net::SocketAddr>) {}
        pub fn open(&mut self, id: u32, offerer: bool, _now: Instant) { self.close(id); ST.with(|s| { let mut s = s.borrow_mut(); s.open.insert(id, false); s.dead.remove(&id); }); self.mine.push(id); rtc_open(id, offerer, super::ICE_SERVERS); }
        pub fn signal(&mut self, id: u32, s: Signal, now: Instant) {
            // an offer for a link we don't have yet: open it as the answerer first
            if matches!(s, Signal::Offer(_)) && !self.mine.contains(&id) { self.open(id, false, now); }
            if !self.mine.contains(&id) { return; }
            rtc_signal(id, s.kind(), s.text());
        }
        pub fn datagram(&mut self, _s: std::net::SocketAddr, _b: &[u8], _now: Instant) -> bool { false }
        pub fn tick(&mut self, _now: Instant) {}
        pub fn send(&mut self, id: u32, b: &[u8]) -> bool { self.is_open(id) && rtc_send(id, b) }
        pub fn is_open(&self, id: u32) -> bool { ST.with(|s| s.borrow().open.get(&id).copied().unwrap_or(false)) }
        pub fn is_dead(&self, id: u32) -> bool { self.mine.contains(&id) && ST.with(|s| s.borrow().dead.contains(&id)) }
        pub fn exists(&self, id: u32) -> bool { self.mine.contains(&id) }
        pub fn close(&mut self, id: u32) { if self.mine.contains(&id) { rtc_close(id); self.mine.retain(|x| *x != id); ST.with(|s| { let mut s = s.borrow_mut(); s.open.remove(&id); s.dead.remove(&id); }); } }
        pub fn take_signals(&mut self) -> Vec<(u32, Signal)> { ST.with(|s| std::mem::take(&mut s.borrow_mut().signals)) }
        pub fn take_inbox(&mut self) -> Vec<(u32, Vec<u8>)> { ST.with(|s| std::mem::take(&mut s.borrow_mut().inbox)) }
        pub fn take_transmit(&mut self) -> Vec<(std::net::SocketAddr, Vec<u8>)> { Vec::new() }
        pub fn ids(&self) -> Vec<u32> { self.mine.clone() }
    }
}
#[cfg(target_arch = "wasm32")]
pub use web::{on_close, on_data, on_open, on_signal};
