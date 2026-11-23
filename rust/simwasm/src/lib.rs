//! The Cloudflare relay's AI cars: fogwood_core::server::Sim for JavaScript (relay-cf/worker.js). One `Room` per online
//! room that has uploaded a stage; the worker feeds it the room's packets and the clock and sends what it returns.
use fogwood_core::server::{Sim, SERVER_ID};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Room { sim: Sim }

#[wasm_bindgen]
impl Room {
    /// from the host's STAGE upload (server::stage_blob); throws with the reason if it can't be used
    #[wasm_bindgen(constructor)]
    pub fn new(blob: &[u8], room: u32, host: u32, now: f64) -> Result<Room, JsValue> { Sim::new(blob, room, host, now).map(|sim| Room { sim }).map_err(|e| JsValue::from_str(&e)) }
    /// a game packet a member (`from`) sent to everyone or to SERVER_ID
    pub fn packet(&mut self, from: u32, b: &[u8], now: f64) { self.sim.packet(from, b, now); }
    /// the clock moved on (seconds): the race caught up to it
    pub fn step(&mut self, now: f64) { self.sim.step(now); }
    /// what to send, packed: [to u32 LE (0: everyone)][length u16 LE][payload] ...
    pub fn take_out(&mut self) -> Vec<u8> {
        let mut w = Vec::new(); for (to, p) in self.sim.take_out() { w.extend_from_slice(&to.to_le_bytes()); w.extend_from_slice(&(p.len() as u16).to_le_bytes()); w.extend_from_slice(&p); } w
    }
    /// lines for the worker's log
    pub fn log(&mut self) -> String { let l = self.sim.log.join("\n"); self.sim.log.clear(); l }
    /// nobody has said anything for a while: the worker can drop it
    pub fn idle(&self, now: f64) -> bool { self.sim.idle(now) }
    pub fn map(&self) -> String { self.sim.map().to_string() }
}
/// the id the relay's AI cars come from on the wire
#[wasm_bindgen]
pub fn server_id() -> u32 { SERVER_ID }
