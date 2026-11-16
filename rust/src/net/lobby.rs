//! The open-rooms list: hosts announce their room to the relay (see relay-cf/worker.js) and anyone on the ONLINE
//! screen sees them and can join with a tap, no code needed. Asked for every 2 s while the list is on screen.
use std::sync::Mutex;

#[derive(Clone, Debug, PartialEq)]
pub struct Room { pub code: u32, pub name: String, pub map: String, pub players: u32, pub racing: bool }

struct Lobby { t0: web_time::Instant, last_ask: f64, rooms: Vec<Room>, heard: bool,
    #[cfg(not(target_arch = "wasm32"))] ws: super::ws::Ws,
    #[cfg(target_arch = "wasm32")] ws: super::relay::web::Sock }
static LOBBY: Mutex<Option<Lobby>> = Mutex::new(None);

fn url() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    let base = std::env::var("FW_RELAY").ok().filter(|v| v.starts_with("ws")).unwrap_or_else(|| super::relay::URL.to_string());
    #[cfg(target_arch = "wasm32")]
    let base = super::relay::URL.to_string();
    format!("{}/lobby", base.trim_end_matches('/').trim_end_matches("/ws"))
}

/// start listening (the ONLINE screen opened)
pub fn open() {
    let Ok(mut g) = LOBBY.lock() else { return }; if g.is_some() { return; }
    #[cfg(not(target_arch = "wasm32"))]
    let ws = super::ws::Ws::open(url());
    #[cfg(target_arch = "wasm32")]
    let ws = super::relay::web::Sock::open(super::relay::web::LOBBY, url());
    *g = Some(Lobby { t0: web_time::Instant::now(), last_ask: -9.0, rooms: Vec::new(), heard: false, ws });
}
pub fn close() { if let Ok(mut g) = LOBBY.lock() { *g = None; } }
/// once a frame while open: ask again every 2 s, take in the answer
pub fn poll() {
    let Ok(mut g) = LOBBY.lock() else { return }; let Some(l) = g.as_mut() else { return };
    let now = l.t0.elapsed().as_secs_f64();
    #[cfg(target_arch = "wasm32")]
    l.ws.tick();
    let fresh = l.ws.fresh();
    for m in l.ws.take() { if m.first() == Some(&0x86) { l.rooms = parse(&m[1..]); l.heard = true; } }
    if fresh || now - l.last_ask > 2.0 { if l.ws.send(&[0x06]) { l.last_ask = now; } }
}
/// the rooms heard of (None: no answer yet)
pub fn rooms() -> Option<Vec<Room>> { LOBBY.lock().ok()?.as_ref().filter(|l| l.heard).map(|l| l.rooms.clone()) }

fn parse(b: &[u8]) -> Vec<Room> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(b) else { return Vec::new() };
    v.as_array().map(|a| a.iter().filter_map(|r| Some(Room { code: r["c"].as_u64()? as u32, name: r["n"].as_str().unwrap_or("").to_string(), map: r["m"].as_str().unwrap_or("").to_string(),
        players: r["p"].as_u64().unwrap_or(1) as u32, racing: r["r"].as_bool().unwrap_or(false) })).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_the_list() {
        let r = super::parse(br#"[{"c":123456,"n":"ADRIEN","m":"snow","p":2,"r":false},{"c":5,"n":"X","m":"forest","p":1,"r":true}]"#);
        assert_eq!(r.len(), 2); assert_eq!(r[0].code, 123456); assert_eq!(r[0].map, "snow"); assert!(r[1].racing);
    }
}
