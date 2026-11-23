//! The relay server's AI cars. When a race has AI cars and the relay can run them, the host's phone uploads the stage
//! (its runtime packet, compressed) and the relay builds the same world the phones have and drives the AI cars there:
//! the same race step, AI and physics as on a phone, at 60 Hz on the host's clock. The cars go out as BOT3 packets from
//! SERVER_ID to everyone (the host included), the pushes they give players go to those players' phones, and players
//! send the pushes they give them here. The relay only feeds this what it is given and sends what it returns, so the
//! Cloudflare Worker (through WebAssembly) and the native relay run exactly the same code.
//!
//! Inputs: `packet` (a game packet from a member of the room, as the relay got it) and `step` (the server clock moved on;
//! call it often, every 20 ms or so). Output: `take_out` (packets to send: to 0 is everyone in the room).
use crate::game::{race, rt, volc, Game, Persist};
use crate::net::proto::{self, CarSnap, Env, Start};
use crate::net::smooth;
use crate::physics::{Car, Obstacle, Obstacles};
use crate::world::{self, Map, World};
use std::collections::HashMap;

/// who the relay's AI cars come from on the wire (never a real player's id: those are random but this one is reserved)
pub const SERVER_ID: u32 = 0x5E4F_F00D;
/// the stage upload: magic, then the runtime packet, deflated
pub const STAGE_MAGIC: &[u8; 4] = b"FWS1";
const DT: f64 = 1.0 / 60.0;
/// at most this much race time is caught up in one call (a stall skips ahead instead of grinding)
const CATCH_UP: f64 = 0.25;

/// The host builds this from its stage's runtime packet.
pub fn stage_blob(rt_packet: &[u8]) -> Vec<u8> {
    let mut b = STAGE_MAGIC.to_vec(); b.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(rt_packet, 6)); b
}

struct PlayerIn { id: u32, track: smooth::Track, seq: u8, heard: f64 }
#[derive(Default)]
struct BotOut { policy: smooth::Policy, seq: u8 }

pub struct Sim {
    room: u32, host: u32, world: World, obs: Obstacles, game: Game, seq: u32,
    /// the host's clock is ours plus this (from pings to the host, the quickest round trip winning)
    offset: f64, best_rtt: f64, pongs: u32, last_ping: f64, pings: HashMap<u32, f64>,
    start: Option<Start>, players: Vec<PlayerIn>, bots: Vec<BotOut>, sim_t: f64, volc_t: f64,
    /// pushes our AI cars gave players, summed per (player, AI car) until the next message to them; when each last went
    hits: Vec<(u32, u8, [f32; 5], f32)>, hit_tx: HashMap<u32, f64>,
    /// pushes players gave our AI cars (from, AI car, push, impact)
    bot_hits: Vec<(u32, u8, [f32; 5], f32)>,
    out: Vec<(u32, Vec<u8>)>, now: f64, last_heard: f64,
    pub log: Vec<String>,
}

impl Sim {
    /// From the host's upload: builds the stage's world (a second or so) and waits for the race start.
    pub fn new(blob: &[u8], room: u32, host: u32, now: f64) -> Result<Sim, String> {
        if blob.len() < 8 || &blob[0..4] != STAGE_MAGIC { return Err("bad stage magic".into()); }
        let pkt = miniz_oxide::inflate::decompress_to_vec_with_limit(&blob[4..], 16 << 20).map_err(|e| format!("inflate: {:?}", e))?;
        let rt = rt::parse(&pkt)?;
        let path: Vec<(f64, f64)> = rt.path.iter().map(|p| (p[0], p[1])).collect();
        if path.len() < 10 || rt.roadh.len() != path.len() { return Err("stage without a road".into()); }
        let world = world::build(Map::from_id(rt.map_id), &path, &rt.roadh, rt.gxh, rt.gzh);
        let obs = Obstacles::new(&rt.obstacles.iter().map(|o| Obstacle { x: o[0], z: o[1], r: o[2], soft: o[3] == 3.0 }).collect::<Vec<_>>());
        volc::init_bridges(&rt, &world);
        let mut game = Game::new(rt, Persist::new(""));
        game.race.online = true;
        // the server has no car of its own: the race's "player" is parked far off the stage, out of everything
        game.car = Car::new(); game.car.x = 1.0e5; game.car.z = 1.0e5; game.car.y = -1.0e4;
        Ok(Sim { room, host, world, obs, game, seq: 0, offset: 0.0, best_rtt: 9.0, pongs: 0, last_ping: -9.0, pings: HashMap::new(), start: None, players: Vec::new(), bots: Vec::new(),
            sim_t: 0.0, volc_t: -1.0, hits: Vec::new(), hit_tx: HashMap::new(), bot_hits: Vec::new(), out: Vec::new(), now, last_heard: now, log: Vec::new() })
    }
    pub fn map(&self) -> &str { &self.game.rt.map }
    fn host_now(&self) -> f64 { self.now + self.offset }
    fn env(&mut self, kind: u8, to: u32) -> Env { self.seq = self.seq.wrapping_add(1); Env { kind, room: self.room, from: SERVER_ID, to, seq: self.seq } }
    /// packets waiting to go out: (to: a member's id, or 0 for everyone)
    pub fn take_out(&mut self) -> Vec<(u32, Vec<u8>)> { std::mem::take(&mut self.out) }
    /// nobody has said anything for a while (the room is done with us)
    pub fn idle(&self, now: f64) -> bool { now - self.last_heard > 30.0 }
    pub fn racing(&self) -> bool { self.start.is_some() }

    /// A game packet from member `from`.
    pub fn packet(&mut self, from: u32, b: &[u8], now: f64) {
        self.now = now; self.last_heard = now;
        if matches!(b.first(), Some(&proto::K_CAR2) | Some(&proto::K_CAR3)) { self.car_in(b); return; }
        if b.first() == Some(&proto::K_BOT3) { return; }
        let Some((e, mut r)) = proto::read_env(b) else { return };
        if e.room != self.room || e.from == SERVER_ID || (e.to != 0 && e.to != SERVER_ID) { return; }
        let _ = from;
        match e.kind {
            proto::K_HELLO => { if let Some(h) = proto::read_hello(&mut r) { if h.host && e.from != self.host { self.host = e.from; self.best_rtt = 9.0; self.pongs = 0; } } }
            proto::K_PONG => { if e.from == self.host { if let (Some(t), Some(ht)) = (r.f64(), r.f64()) { if self.pings.remove(&e.seq).is_some() {
                let rtt = self.now - t; if (0.0..3.0).contains(&rtt) { self.pongs += 1; self.best_rtt += 0.002; if rtt <= self.best_rtt { self.best_rtt = rtt; self.offset = ht - (t + rtt / 2.0); } } } } } }
            proto::K_START => { if e.from == self.host { if let Some(s) = proto::read_start(&mut r) { if s.srv && self.start.as_ref().map(|o| (o.go_at - s.go_at).abs() > 0.01).unwrap_or(true) { self.begin(s); } } } }
            proto::K_BOTHIT => { if let Some((k, d, imp)) = proto::read_bot_hit(&mut r) { if self.bot_hits.len() < 64 { self.bot_hits.push((e.from, k, d, imp)); } } }
            proto::K_BYE => { if let Some(p) = self.players.iter_mut().find(|p| p.id == e.from) { p.heard = -99.0; } }
            _ => {}
        }
    }

    fn car_in(&mut self, b: &[u8]) {
        let Some((sid, seq, ms, mut c)) = proto::read_car2(b) else { return };
        let th = self.host_now(); let now = self.now;
        let Some(p) = self.players.iter_mut().find(|p| p.id as u16 == sid) else { return };
        if p.track.latest.is_some() && (seq.wrapping_sub(p.seq) as i8) <= 0 { return; }
        p.seq = seq; p.heard = now;
        let age = ((th * 1000.0).rem_euclid(65536.0) - ms as f64).rem_euclid(65536.0) / 1000.0; c.t = th - if age < 10.0 { age } else { 0.0 };
        p.track.push_at(c, th);
    }

    /// The race starts: the same grid as on the phones, our AI cars driven here, the players' cars from their packets.
    fn begin(&mut self, st: Start) {
        let hn = self.host_now(); let g = &mut self.game; let w = &self.world; let np = g.np;
        let rc = &mut g.race; rc.on = true; rc.online = true; rc.phase = race::Phase::Grid; rc.t = 0.0; rc.done_t = 0.0; rc.cars.clear(); rc.pos = 0; rc.order.clear();
        rc.laps = st.laps.max(1) as i64; rc.go_at = st.go_at; rc.me_id = SERVER_ID; rc.hits.clear(); rc.net_hits.clear(); rc.touch_t.clear(); rc.pushed_t.clear();
        let taken: Vec<usize> = st.bots.iter().map(|b| b.0 as usize % race::RIVALS.len()).collect();
        let free: Vec<usize> = (0..race::RIVALS.len()).filter(|c| !taken.contains(c)).chain(taken.iter().copied()).collect();
        for (idx, (ri, k)) in st.bots.iter().enumerate() {
            let (i, lat) = race::slot(np, *k as usize); let ri = *ri as usize % race::RIVALS.len();
            let mut c = race::mk_car(g, Some(ri), i, lat); race::rc_place(&mut c.car, &g.rt, w, i, lat, 0.0); c.bot = Some(idx as u8); g.race.cars.push(c);
        }
        self.players.clear();
        for (n, (id, k)) in st.slots.iter().enumerate() {
            let (i, lat) = race::slot(np, *k as usize);
            let mut c = race::mk_car(g, Some(free[n % free.len()]), i, lat); race::rc_place(&mut c.car, &g.rt, w, i, lat, 0.0); c.remote = Some(*id); c.name = "PLAYER".into();
            g.race.cars.push(c); self.players.push(PlayerIn { id: *id, track: Default::default(), seq: 0, heard: self.now });
        }
        // the parked stand-in for "our" car
        let i = race::slot(np, 0).0; let c = race::mk_car(g, None, i, 0.0); g.race.player = g.race.cars.len(); g.race.cars.push(c);
        g.race.cars.last_mut().unwrap().car = g.car;
        self.bots = (0..st.bots.len()).map(|_| BotOut::default()).collect();
        self.sim_t = hn.min(st.go_at - 0.5); self.volc_t = -1.0;
        self.log.push(format!("race on {} with {} AI cars and {} players, lights out in {:.1}s", g.rt.map, st.bots.len(), st.slots.len(), st.go_at - hn));
        self.start = Some(st);
    }

    /// The server clock is `now`: clock pings to the host, and the race caught up to the present.
    pub fn step(&mut self, now: f64) {
        self.now = now;
        // the host's clock: a few quick pings, then one every 2 s
        if self.host != 0 && now - self.last_ping > if self.pongs < 6 { 0.25 } else { 2.0 } { self.last_ping = now;
            let e = self.env(proto::K_PING, self.host); self.pings.insert(e.seq, now); if self.pings.len() > 32 { self.pings.clear(); }
            let b = proto::time_pair(e, now, 0.0); let h = self.host; self.out.push((h, b)); }
        if self.start.is_none() || self.pongs == 0 { return; }
        let hn = self.host_now();
        if hn - self.sim_t > CATCH_UP { self.sim_t = hn - CATCH_UP; }
        while self.sim_t + DT <= hn { self.sim_t += DT; self.tick(); }
    }

    fn tick(&mut self) {
        let t = self.sim_t; let w = &self.world;
        let ground = |x: f32, z: f32| w.height(x as f64, z as f64) as f32; let gr: smooth::Ground = Some(&ground);
        self.game.race_clock = Some(t);
        // players' cars where they are now (as the phones predict each other)
        for p in self.players.iter_mut() {
            let Some(k) = self.game.race.cars.iter().position(|c| c.remote == Some(p.id)) else { continue };
            let gone = self.now - p.heard > 8.0; let c = &mut self.game.race.cars[k]; c.left = gone;
            if let Some(sn) = p.track.sample(t, gr) { apply(&mut c.car, &sn, DT); c.owned = p.track.owned(); }
        }
        // pushes players gave our AI cars, taken in the race step unless we settled that contact ourselves a moment ago
        for (from, idx, d, imp) in std::mem::take(&mut self.bot_hits) {
            let rc = &mut self.game.race;
            let (Some(kb), Some(k)) = (rc.cars.iter().position(|c| c.bot == Some(idx) && c.is_ai()), rc.cars.iter().position(|c| c.remote == Some(from))) else { continue };
            rc.tick_epoch(kb); if rc.t - rc.touched(kb, k) < 0.35 { continue; }
            rc.net_hits.push((kb, k, d.map(|v| v as f64), imp as f64));
        }
        // Ember Peak: fireballs that landed kick the AI cars (the shared wall clock, like the phones)
        if self.game.rt.volc.is_some() {
            let vt = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| (d.as_millis() % (volc::PERIOD as u128 * 1000)) as f64 / 1000.0).unwrap_or(0.0);
            let tp = if self.volc_t < 0.0 || self.volc_t > vt || vt - self.volc_t > 1.0 { vt - DT } else { self.volc_t }; self.volc_t = vt;
            if vt > tp { for b in volc::landed(&self.game.rt, w, vt, tp) { for c in self.game.race.cars.iter_mut() { if c.is_ai() { volc::kick(&mut c.car, &b); } } } }
        }
        if !self.game.rt.wave_secs.is_empty() {
            w.wave.get_or_init(|| world::Wave::new(&self.game.rt.path, &self.game.rt.tan, &self.game.rt.roadh, &self.game.rt.wave_secs));
            let wt = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| (d.as_millis() % 60_000) as f64 / 1000.0).unwrap_or(0.0); w.set_wave_time(wt);
        }
        self.game.car = Car { x: 1.0e5, z: 1.0e5, y: -1.0e4, ..self.game.car };
        race::step(&mut self.game, w, &self.obs, DT);
        // pushes our AI cars gave players: summed, at most one message per player every 60 ms
        for (id, by, d, imp) in std::mem::take(&mut self.game.race.hits) {
            let d = d.map(|v| v as f32);
            match self.hits.iter_mut().find(|x| x.0 == id && x.1 == by) { Some(x) => { for q in 0..5 { x.2[q] += d[q]; } x.3 = x.3.max(imp as f32); } None => self.hits.push((id, by, d, imp as f32)) } }
        let now = self.now; let due: Vec<u32> = { let mut v: Vec<u32> = self.hits.iter().map(|h| h.0).collect(); v.dedup(); v.into_iter().filter(|id| now - self.hit_tx.get(id).copied().unwrap_or(-9.0) >= 0.06).collect() };
        for id in due { self.hit_tx.insert(id, now); let mine: Vec<(u32, u8, [f32; 5], f32)> = self.hits.iter().filter(|h| h.0 == id).cloned().collect(); self.hits.retain(|h| h.0 != id);
            for (_, by, d, imp) in mine { self.seq = self.seq.wrapping_add(1); let e = Env { kind: proto::K_HIT, room: self.room, from: SERVER_ID, to: id, seq: self.seq }; let b = proto::hit(e, d, imp, by); self.out.push((id, b)); } }
        // our AI cars out: like a phone's own car, when the receivers' prediction has drifted, within the data budget
        let humans: Vec<(f64, f64)> = self.game.race.cars.iter().filter(|c| c.remote.is_some() && !c.left).map(|c| (c.car.x, c.car.z)).collect();
        let n = humans.len().max(1); let (rate, beat) = if n <= 4 { (20.0f32, 0.5f32) } else if n <= 8 { (12.0, 0.6) } else { (8.0, 0.75) };
        let base = smooth::Thresholds::default();
        let list: Vec<(usize, CarSnap, bool)> = self.game.race.cars.iter().filter(|c| c.is_ai()).filter_map(|c| c.bot.map(|b| (b as usize, snap_of(&c.car, c.epoch), humans.iter().any(|h| (h.0 - c.car.x).hypot(h.1 - c.car.z) < 30.0)))).collect();
        for (k, mut c, near) in list {
            if k >= self.bots.len() { continue; }
            c.t = t; c.a = self.bots[k].policy.accel(c.u, DT as f32);
            let thr = if near { smooth::Thresholds { pos: base.pos * 0.4, head: base.head * 0.5, speed: base.speed * 0.5, height: base.height, body: base.body } } else { base };
            let (rate, beat) = if near { ((rate * 1.5).min(30.0), beat.min(0.25)) } else { (rate, beat) };
            let bo = &mut self.bots[k]; if !bo.policy.due(&c, gr, &thr, rate, beat) { continue; }
            bo.seq = bo.seq.wrapping_add(1); let b = proto::bot3(SERVER_ID, bo.seq, k as u8, &c);
            if let Some((_, _, _, mut q)) = proto::read_car2(&b) { q.t = t; bo.policy.sent = Some(q); }
            self.out.push((0, b));
        }
        // the race is over for everyone (or everyone left): stop
        let rc = &self.game.race; let players_done = rc.cars.iter().filter(|c| c.remote.is_some()).all(|c| c.finished || c.left);
        if rc.phase != race::Phase::Grid && players_done && !self.players.is_empty() { self.log.push(format!("race over at {:.1}s", rc.t)); self.start = None; }
    }
    /// where the AI cars are (tests)
    pub fn bots(&self) -> Vec<(f64, f64, f64)> { self.game.race.cars.iter().filter(|c| c.is_ai()).map(|c| (c.car.x, c.car.z, c.car.u)).collect() }
}

fn snap_of(c: &Car, epoch: u8) -> CarSnap {
    CarSnap { t: 0.0, x: c.x as f32, y: c.y as f32, z: c.z as f32, h: c.h as f32, vx: c.vx as f32, vy: c.vy as f32, vz: c.vz as f32, r: c.r as f32, pitch: c.pitch as f32, roll: c.roll as f32,
        steer: c.steer as f32, u: c.u as f32, gear: c.gear as i8, rpm: c.rpm.clamp(0.0, 65000.0) as u16, d: [c.wh[0].d as f32, c.wh[1].d as f32, c.wh[2].d as f32, c.wh[3].d as f32],
        ang: [c.wh[0].ang as f32, c.wh[1].ang as f32, c.wh[2].ang as f32, c.wh[3].ang as f32],
        flags: smooth::with_epoch(if c.contacts == 0.0 && c.air_t > 0.06 { smooth::AIR } else { 0 }, epoch), a: 0.0 }
}
fn apply(c: &mut Car, s: &CarSnap, dt: f64) {
    c.x = s.x as f64; c.y = s.y as f64; c.z = s.z as f64; c.h = s.h as f64; c.vx = s.vx as f64; c.vy = s.vy as f64; c.vz = s.vz as f64; c.r = s.r as f64;
    c.pitch = s.pitch as f64; c.roll = s.roll as f64; c.steer = s.steer as f64; c.u = s.u as f64; c.gear = s.gear as f64; c.rpm = s.rpm as f64;
    for k in 0..4 { c.wh[k].d += (s.d[k] as f64 - c.wh[k].d) * (dt * 12.0).min(1.0); c.wh[k].ang += s.u as f64 * dt / 0.31; }
    c.contacts = 4.0; c.on_road = true; c.impact = 0.0;
}
