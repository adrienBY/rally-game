//! Play stats: small anonymous notes the app sends to the relay (its /stat page counts them): the game launched, a drive or a
//! race began, a race or a stage ended. Each carries a random id made once per install (no account, nothing from the phone),
//! the driver name, the stage and car. The game only writes the note ("stat {json}" on its event line); the Android and web
//! layers post it.
use super::Game;
pub use self::s as js;

fn esc(s: &str) -> String { s.chars().filter(|c| !c.is_control()).map(|c| if c == '"' || c == '\\' { ' ' } else { c }).take(40).collect() }

impl Game {
    /// this install's random id (made on first use, then kept)
    fn stat_id(&mut self) -> String {
        if let Some(v) = self.ps.saves.get("fogwood-did") { if v.len() >= 8 { return v.to_string(); } }
        let t = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) as u64;
        let mut x = t ^ 0x9E37_79B9_7F4A_7C15 ^ ((self.time * 1e6) as u64).rotate_left(29); x ^= x >> 33; x = x.wrapping_mul(0xff51_afd7_ed55_8ccd); x ^= x >> 33;
        let id = format!("{:012x}", x & 0xffff_ffff_ffff); self.ps.saves.set("fogwood-did", id.clone()); self.ps.saves.flush(); id
    }
    /// note event `e` with extra fields (already JSON values)
    pub fn stat(&mut self, e: &str, extra: &[(&str, String)]) {
        let d = self.stat_id(); let car = super::cars::CARS.get(self.ps.car).map(|c| c.name).unwrap_or("");
        let mut j = format!("{{\"d\":\"{}\",\"e\":\"{}\",\"n\":\"{}\",\"m\":\"{}\",\"c\":\"{}\"", d, esc(e), esc(&self.ps.name), esc(&self.rt.map), esc(car));
        for (k, v) in extra { j.push_str(&format!(",\"{}\":{}", k, v)); }
        j.push('}'); self.events.push(format!("stat {}", j));
    }
}
pub fn s(v: &str) -> String { format!("\"{}\"", esc(v)) }
