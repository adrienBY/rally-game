//! Wire format, shared by the UDP direct path and the broker relay. Little endian, versioned by the magic.
//! Envelope: "FW1" kind:u8 room:u32 from:u32 to:u32 (0 = everyone) seq:u32, then the body.

pub const MAGIC: &[u8; 3] = b"FW1";
pub const K_HELLO: u8 = 1; pub const K_PROBE: u8 = 2; pub const K_PROBE_ACK: u8 = 3; pub const K_PING: u8 = 4; pub const K_PONG: u8 = 5;
pub const K_START: u8 = 6; pub const K_CAR: u8 = 7; pub const K_BYE: u8 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Env { pub kind: u8, pub room: u32, pub from: u32, pub to: u32, pub seq: u32 }

/// One car, as the owner sees it. `t` is on the host's clock.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CarSnap {
    pub t: f64, pub x: f32, pub y: f32, pub z: f32, pub h: f32, pub vx: f32, pub vy: f32, pub vz: f32, pub r: f32, pub pitch: f32, pub roll: f32, pub steer: f32,
    pub u: f32, pub gear: i8, pub rpm: u16, pub d: [f32; 4], pub ang: [f32; 4], pub flags: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hello { pub host: bool, pub racing: bool, pub name: String, pub map: String, pub cands: Vec<([u8; 4], u16)> }
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Start { pub map: String, pub laps: u8, pub go_at: f64, pub slots: Vec<(u32, u8)> }

#[derive(Default)]
pub struct W(pub Vec<u8>);
impl W {
    pub fn env(e: Env) -> W { let mut w = W(Vec::with_capacity(96)); w.0.extend_from_slice(MAGIC); w.u8(e.kind); w.u32(e.room); w.u32(e.from); w.u32(e.to); w.u32(e.seq); w }
    pub fn u8(&mut self, v: u8) { self.0.push(v); }
    pub fn u16(&mut self, v: u16) { self.0.extend_from_slice(&v.to_le_bytes()); }
    pub fn u32(&mut self, v: u32) { self.0.extend_from_slice(&v.to_le_bytes()); }
    pub fn f32(&mut self, v: f32) { self.0.extend_from_slice(&v.to_le_bytes()); }
    pub fn f64(&mut self, v: f64) { self.0.extend_from_slice(&v.to_le_bytes()); }
    pub fn str(&mut self, s: &str) { let b = s.as_bytes(); let n = b.len().min(32); self.u8(n as u8); self.0.extend_from_slice(&b[..n]); }
}
pub struct R<'a> { b: &'a [u8], i: usize }
impl<'a> R<'a> {
    pub fn new(b: &'a [u8]) -> R<'a> { R { b, i: 0 } }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> { if self.i + n > self.b.len() { return None; } let s = &self.b[self.i..self.i + n]; self.i += n; Some(s) }
    pub fn u8(&mut self) -> Option<u8> { Some(self.take(1)?[0]) }
    pub fn u16(&mut self) -> Option<u16> { Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?)) }
    pub fn u32(&mut self) -> Option<u32> { Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?)) }
    pub fn f32(&mut self) -> Option<f32> { Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?)) }
    pub fn f64(&mut self) -> Option<f64> { Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?)) }
    pub fn str(&mut self) -> Option<String> { let n = self.u8()? as usize; Some(String::from_utf8_lossy(self.take(n)?).to_string()) }
}

pub fn read_env(b: &[u8]) -> Option<(Env, R<'_>)> {
    if b.len() < 20 || &b[..3] != MAGIC { return None; }
    let mut r = R::new(&b[3..]); let e = Env { kind: r.u8()?, room: r.u32()?, from: r.u32()?, to: r.u32()?, seq: r.u32()? }; Some((e, r))
}

pub fn hello(e: Env, h: &Hello) -> Vec<u8> {
    let mut w = W::env(e); w.u8(h.host as u8 | (h.racing as u8) << 1); w.str(&h.name); w.str(&h.map); w.u8(h.cands.len().min(4) as u8);
    for (ip, port) in h.cands.iter().take(4) { w.0.extend_from_slice(ip); w.u16(*port); } w.0
}
pub fn read_hello(r: &mut R) -> Option<Hello> {
    let f = r.u8()?; let name = r.str()?; let map = r.str()?; let n = r.u8()? as usize; let mut cands = Vec::new();
    for _ in 0..n.min(4) { let ip = r.take(4)?; let port = r.u16()?; cands.push(([ip[0], ip[1], ip[2], ip[3]], port)); }
    Some(Hello { host: f & 1 != 0, racing: f & 2 != 0, name, map, cands })
}
pub fn start(e: Env, s: &Start) -> Vec<u8> {
    let mut w = W::env(e); w.str(&s.map); w.u8(s.laps); w.f64(s.go_at); w.u8(s.slots.len().min(12) as u8); for (id, k) in s.slots.iter().take(12) { w.u32(*id); w.u8(*k); } w.0
}
pub fn read_start(r: &mut R) -> Option<Start> {
    let map = r.str()?; let laps = r.u8()?; let go_at = r.f64()?; let n = r.u8()? as usize; let mut slots = Vec::new(); for _ in 0..n { slots.push((r.u32()?, r.u8()?)); }
    Some(Start { map, laps, go_at, slots })
}
pub fn car(e: Env, c: &CarSnap) -> Vec<u8> {
    let mut w = W::env(e); w.f64(c.t);
    for v in [c.x, c.y, c.z, c.h, c.vx, c.vy, c.vz, c.r, c.pitch, c.roll, c.steer, c.u] { w.f32(v); }
    w.u8(c.gear as u8); w.u16(c.rpm); for v in c.d { w.f32(v); } for v in c.ang { w.f32(v); } w.u8(c.flags); w.0
}
pub fn read_car(r: &mut R) -> Option<CarSnap> {
    let t = r.f64()?; let mut f = [0f32; 12]; for v in f.iter_mut() { *v = r.f32()?; }
    let gear = r.u8()? as i8; let rpm = r.u16()?; let mut d = [0f32; 4]; for v in d.iter_mut() { *v = r.f32()?; } let mut ang = [0f32; 4]; for v in ang.iter_mut() { *v = r.f32()?; }
    Some(CarSnap { t, x: f[0], y: f[1], z: f[2], h: f[3], vx: f[4], vy: f[5], vz: f[6], r: f[7], pitch: f[8], roll: f[9], steer: f[10], u: f[11], gear, rpm, d, ang, flags: r.u8()? })
}
pub fn time_pair(e: Env, a: f64, b: f64) -> Vec<u8> { let mut w = W::env(e); w.f64(a); w.f64(b); w.0 }
pub fn bare(e: Env) -> Vec<u8> { W::env(e).0 }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrips() {
        let e = Env { kind: K_HELLO, room: 123456, from: 0xdeadbeef, to: 0, seq: 9 };
        let h = Hello { host: true, racing: false, name: "ADRIEN".into(), map: "lakes".into(), cands: vec![([192, 168, 1, 20], 40000), ([203, 0, 113, 9], 51234)] };
        let b = hello(e, &h); let (e2, mut r) = read_env(&b).unwrap(); assert_eq!(e2, e); assert_eq!(read_hello(&mut r).unwrap(), h);
        let s = Start { map: "snow".into(), laps: 3, go_at: 1234.5, slots: vec![(1, 0), (2, 1)] };
        let b = start(Env { kind: K_START, ..e }, &s); let (_, mut r) = read_env(&b).unwrap(); assert_eq!(read_start(&mut r).unwrap(), s);
        let c = CarSnap { t: 5.25, x: 1.0, y: 2.0, z: -3.0, h: 0.5, u: 20.0, gear: 3, rpm: 6100, d: [0.07; 4], ang: [1.0, 2.0, 3.0, 4.0], flags: 1, ..Default::default() };
        let b = car(Env { kind: K_CAR, ..e }, &c); let (_, mut r) = read_env(&b).unwrap(); assert_eq!(read_car(&mut r).unwrap(), c);
        assert!(read_env(b"XX1aaaaaaaaaaaaaaaaaaaa").is_none());
    }
}
