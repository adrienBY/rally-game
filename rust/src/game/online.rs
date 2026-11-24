//! Online races: the room screen (host, join with a code, lobby), the synced start, and other players' cars.
use super::hud::*;
use super::race::{self, RCar};
use super::{js_round, Game, Rect, State};
use crate::net::{self, proto::{CarSnap, Start}, Link};
use crate::physics::Car;
use crate::world::World;

#[derive(Clone, PartialEq, Debug, Default)]
pub enum OnlineUi { #[default] Closed, Menu, Join(String), Lobby, Stages, Rooms }

pub const KEYS: [&str; 42] = ["on_cprev", "on_cnext", "on_lnext", "on_mode", "on_bots", "on_ai", "on_rooms", "on_r0", "on_r1", "on_r2", "on_r3", "on_r4", "on_r5", "on_host", "on_join", "on_back", "on_start", "on_leave", "on_stage", "k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8", "k9", "kdel", "kok", "on_laps", "online",
    "on_name", "on_st0", "on_st1", "on_st2", "on_st3", "on_st4", "on_st5", "on_st6", "on_st7"];
const MAPS: [(&str, &str); 8] = [("forest", "FOGWOOD"), ("snow", "COL DE GIVRE"), ("canyon", "CANYON ROUGE"), ("lakes", "LAKELAND"), ("touge", "MOMIJI TOUGE"), ("riviera", "RIVIERA CORNICHE"), ("prism", "PRISM SKYWAY"), ("ember", "EMBER PEAK")];
/// the stage list: tile key, short name, surface
const TILES: [(&str, &str, &str); 8] = [("on_st0", "FOREST", "GRAVEL"), ("on_st1", "SNOW", "SNOW"), ("on_st2", "CANYON", "SAND"), ("on_st3", "LAKES", "GRAVEL"), ("on_st4", "TOUGE", "TARMAC"), ("on_st5", "RIVIERA", "TARMAC"), ("on_st6", "PRISM", "LIGHT"), ("on_st7", "EMBER", "ASH")];
fn short_name(m: &str) -> &'static str { MAPS.iter().position(|x| x.0 == m).map(|k| TILES[k].1).unwrap_or("STAGE") }
fn stage_name(m: &str) -> &'static str { MAPS.iter().find(|x| x.0 == m).map(|x| x.1).unwrap_or("STAGE") }

fn snap_of(c: &Car, epoch: u8) -> CarSnap {
    CarSnap { t: 0.0, x: c.x as f32, y: c.y as f32, z: c.z as f32, h: c.h as f32, vx: c.vx as f32, vy: c.vy as f32, vz: c.vz as f32, r: c.r as f32, pitch: c.pitch as f32, roll: c.roll as f32,
        steer: c.steer as f32, u: c.u as f32, gear: c.gear as i8, rpm: c.rpm.clamp(0.0, 65000.0) as u16, d: [c.wh[0].d as f32, c.wh[1].d as f32, c.wh[2].d as f32, c.wh[3].d as f32],
        ang: [c.wh[0].ang as f32, c.wh[1].ang as f32, c.wh[2].ang as f32, c.wh[3].ang as f32],
        // airborne for more than a moment: receivers fly the car on a ballistic arc instead of gluing it to the ground
        flags: net::smooth::with_epoch(if c.contacts == 0.0 && c.air_t > 0.06 { net::smooth::AIR } else { 0 }, epoch), a: 0.0 }
}
fn apply(c: &mut Car, s: &CarSnap, w: &World, dt: f64) {
    c.x = s.x as f64; c.y = s.y as f64; c.z = s.z as f64; c.h = s.h as f64; c.vx = s.vx as f64; c.vy = s.vy as f64; c.vz = s.vz as f64; c.r = s.r as f64;
    c.pitch = s.pitch as f64; c.roll = s.roll as f64; c.steer = s.steer as f64; c.u = s.u as f64; c.gear = s.gear as f64; c.rpm = s.rpm as f64;
    // suspension eases toward the received value (it travels in coarse steps); wheels spin from the speed
    for k in 0..4 { c.wh[k].d += (s.d[k] as f64 - c.wh[k].d) * (dt * 12.0).min(1.0); c.wh[k].ang += s.u as f64 * dt / 0.31; }
    c.contacts = 4.0; c.on_road = true; c.impact = 0.0; let _ = w;
}

impl Game {
    pub fn online_open(&self) -> bool { self.ps.online_ui != OnlineUi::Closed }

    /// network work for this frame, the start when it arrives, and the other players' cars (before the race step)
    pub(crate) fn online_tick(&mut self, w: &World, dt: f64) {
        // the open-rooms list is fetched while the room screens are up
        if matches!(self.ps.online_ui, OnlineUi::Menu | OnlineUi::Rooms) { net::lobby::open(); net::lobby::poll(); } else { net::lobby::close(); }
        if !net::active() { return; }
        let map = self.rt.map.clone(); let racing = self.race.online && self.state == State::Drive;
        let name = self.ps.name.clone(); let mycar = self.ps.car as u8; let mypaint = self.ps.paint.get(self.ps.car).copied().unwrap_or(0) as u8;   // name and car reach the room with the next presence message
        let pending = net::with(|s| { s.map = map.clone(); s.racing = racing; s.car = mycar; s.paint = mypaint; if !name.is_empty() { s.name = name; } s.poll(); s.start_in.clone() }).flatten();
        // guests follow the host's stage while waiting, so it is already built when the race starts
        if pending.is_none() && self.state == State::Title && self.loading.is_none() {
            if let Some(Some(hm)) = net::with(|s| if s.is_host { None } else { s.host_id.and_then(|h| s.peers.get(&h)).map(|p| p.map.clone()).filter(|m| !m.is_empty()) }) {
                if hm != self.rt.map && MAPS.iter().any(|m| m.0 == hm) { self.switch_map(&hm); } }
        }
        if let Some(st) = pending {
            if st.map != self.rt.map { if self.loading.is_none() { self.switch_map(&st.map); } }   // the start stays pending until that stage is built
            else if self.loading.is_none() { net::with(|s| s.start_in = None); self.online_begin(w, st); }
        }
        // host: the relay was to drive the AI cars but has said nothing since the lights (or failed): this phone takes them
        if self.race.online && self.state == State::Drive {
            let back = net::with(|s| s.is_host && s.server_bots && (s.sim_state() == Some(2) || (s.host_now() > self.race.go_at + 1.0 && s.server_silence() > 2.0))).unwrap_or(false);
            if back { net::with(|s| s.take_back_bots()); for c in self.race.cars.iter_mut() { if c.bot.is_some() { c.remote = None; } }
                if std::env::var("FW_DEBUG").is_ok() { eprintln!("relay AI cars silent: the host drives them"); } }
        }
        if self.race.online && self.state == State::Drive {
            let mine = snap_of(&self.car, self.race.epoch);
            let ids: Vec<(usize, u32)> = self.race.cars.iter().enumerate().filter_map(|(k, c)| c.remote.map(|id| (k, id))).collect();
            // both phones predict cars over the same terrain (the stage grids are identical everywhere)
            // (tile fall: a flat plane at the arena's top, so a car dropping through a hole stays smooth)
            let flat = if self.tf.on { Some(self.tf.y as f32) } else { None };
            let ground = |x: f32, z: f32| flat.unwrap_or_else(|| w.height(x as f64, z as f64) as f32); let g: net::smooth::Ground = Some(&ground);
            // closer than 40 m a car eases onto the present, fully there by 15 m, and our states go out more precisely
            // within 30 m (contacts happen there)
            let near: Vec<(u32, f32, f32)> = ids.iter().map(|(k, id)| { let c = &self.race.cars[*k].car; let d = (c.x - self.car.x).hypot(c.z - self.car.z);
                let q = ((40.0 - d) / 25.0).clamp(0.0, 1.0); (*id, (q * q * (3.0 - 2.0 * q)) as f32, d as f32) }).collect();
            // the nearest other car (AI cars driven here count too): how precisely and how often our states go out
            let nearest = near.iter().map(|n| n.2).chain(self.race.cars.iter().filter(|c| c.is_ai()).map(|c| (c.car.x - self.car.x).hypot(c.car.z - self.car.z) as f32)).fold(1e9f32, f32::min);
            // bumps: the pushes we gave other cars this phone settled last frame (summed per car) are shown on their cars
            // and sent to their phones
            let hits = std::mem::take(&mut self.race.hits);
            let mut sum: Vec<(u32, u8, [f64; 5], f64)> = Vec::new();
            for (id, by, d, imp) in hits { match sum.iter_mut().find(|x| x.0 == id && x.1 == by) { Some(x) => { for q in 0..5 { x.2[q] += d[q]; } x.3 = x.3.max(imp); } None => sum.push((id, by, d, imp)) } }
            if std::env::var("FW_DEBUG").is_ok() { for (id, by, d, imp) in &sum { eprintln!("hit-out t={:.2} to {:08x} by {} dv=({:.2},{:.2}) dr={:.2} imp={:.2}", self.race.t, id, by, d[0], d[1], d[2], imp); } }
            // host: our AI cars go out right after our own car (closer than 30 m to a player: more precisely)
            let humans: Vec<(f64, f64)> = self.race.cars.iter().enumerate().filter(|(_, c)| c.bot.is_none() && !c.left).map(|(k, c)| if k == self.race.player { (self.car.x, self.car.z) } else { (c.car.x, c.car.z) }).collect();
            let bots_out: Vec<(usize, CarSnap, f32)> = self.race.cars.iter().filter(|c| c.is_ai()).filter_map(|c| c.bot.map(|b| (b as usize, snap_of(&c.car, c.epoch), humans.iter().map(|h| (h.0 - c.car.x).hypot(h.1 - c.car.z) as f32).fold(1e9f32, f32::min)))).collect();
            let (snaps, owned, incoming, bot_in) = net::with(|s| {
                for (id, by, d, imp) in &sum { s.bump(*id, *by, d.map(|v| v as f32), *imp as f32); }
                for (id, n, _) in &near { s.set_near(*id, *n); } s.send_car(mine, dt, g, nearest);
                for (k, sn, nr) in &bots_out { s.send_bot(*k, *sn, dt, g, *nr); }
                s.flush();
                s.advance_view(dt);
                (ids.iter().map(|(k, id)| (*k, s.remote(*id, g))).collect::<Vec<_>>(), ids.iter().map(|(k, id)| (*k, s.owned(*id), s.horizon(*id))).collect::<Vec<_>>(), s.take_hits(), s.take_bot_hits())
            }).unwrap_or_default();
            for (k, sn) in snaps { if let Some(sn) = sn { let mut c = self.race.cars[k].car; apply(&mut c, &sn, w, dt); self.race.cars[k].car = c; } }
            for (k, o, h) in owned { self.race.cars[k].owned = o; self.race.cars[k].horizon = h; }
            // pushes other phones gave our car (by their own car, or by one of the host's AI cars), and on the host the pushes
            // guests gave our AI cars: taken (in the race step, with the other contacts) unless this phone settled that
            // contact itself a moment ago
            let pl = self.race.player;
            let mut todo: Vec<(usize, usize, [f32; 5], f32)> = Vec::new();
            for (from, by, d, imp) in incoming {
                let pusher = if by == net::proto::BY_PLAYER { self.race.cars.iter().position(|c| c.remote == Some(from) && c.bot.is_none()) } else { self.race.cars.iter().position(|c| c.bot == Some(by) && c.remote.is_some()) };
                if let Some(k) = pusher { todo.push((pl, k, d, imp)); } }
            for (from, idx, d, imp) in bot_in {
                let (Some(kb), Some(k)) = (self.race.cars.iter().position(|c| c.bot == Some(idx) && c.is_ai()), self.race.cars.iter().position(|c| c.remote == Some(from) && c.bot.is_none())) else { continue };
                todo.push((kb, k, d, imp)); }
            for (ours, k, d, imp) in todo {
                if std::env::var("FW_DEBUG").is_ok() { eprintln!("hit-in t={:.2} car {} from car {} dv=({:.2},{:.2}) imp={:.2} local={:.2}s ago", self.race.t, ours, k, d[0], d[1], imp, self.race.t - self.race.touched(ours, k)); }
                // either way that car's states from now on carry our own answer to the contact: say so (see net::smooth::epoch)
                self.race.tick_epoch(ours);
                if self.race.t - self.race.touched(ours, k) < 0.35 { continue; }
                self.race.net_hits.push((ours, k, d.map(|v| v as f64), imp as f64));
            }
            // dropped out
            let silence: Vec<(usize, Option<f64>)> = net::with(|s| ids.iter().map(|(k, id)| (*k, s.car_silence(*id))).collect()).unwrap_or_default();
            let names: Vec<(u32, String)> = net::with(|s| s.peers.values().map(|p| (p.id, p.name.clone())).collect()).unwrap_or_default();
            for c in self.race.cars.iter_mut() { if let Some(n) = c.remote.and_then(|id| names.iter().find(|n| n.0 == id)) { if n.1 != "?" && n.1 != c.name { c.name = n.1.clone(); } } }
            // a car silent for a moment is not out yet: it carries on as predicted and the message waits; out after 8 s of
            // silence (or once its player has left the room: their goodbye, or 20 s without a word)
            for (k, q) in silence { let gone = q.map(|t| t > 8.0).unwrap_or(true); let c = &mut self.race.cars[k];
                if gone && !c.left { c.left = true; if !c.finished { let m = format!("{} DROPPED OUT", c.name.to_uppercase()); self.flash_msg(&m, 2.0); } } else if !gone && c.left { c.left = false; } }
        }
    }

    fn online_begin(&mut self, w: &World, st: Start) {
        let me = net::with(|s| s.me).unwrap_or(0);
        let names: Vec<(u32, String)> = net::with(|s| s.peers.values().map(|p| (p.id, p.name.clone())).collect()).unwrap_or_default();
        let carsof: Vec<(u32, usize, usize)> = net::with(|s| s.peers.values().map(|p| (p.id, p.car as usize, p.paint as usize)).collect()).unwrap_or_default();
        if self.state == State::Drive { self.to_title(); }
        self.gar.open = false;
        self.close_settings();
        self.state = State::Drive; self.lap = 1; self.started = false; self.g_t = 0.0; self.flags = [false; 3]; self.car_audio_on = true;
        let np = self.np; let rc = &mut self.race; rc.on = true; rc.online = true; rc.phase = race::Phase::Grid; rc.t = 0.0; rc.done_t = 0.0; rc.note.clear(); rc.cars.clear(); rc.pos = 0; rc.order.clear();
        rc.laps = st.laps.max(1) as i64; rc.go_at = st.go_at; rc.ai = st.ai.min(race::AI_EXTREME); rc.xprof.clear(); rc.me_id = me; rc.hits.clear(); rc.net_hits.clear(); rc.touch_t.clear(); rc.pushed_t.clear();
        let is_host = net::with(|s| { s.begin_race(st.bots.len(), st.srv); s.is_host }).unwrap_or(false);
        self.race.duos = st.mode == net::proto::MODE_DUOS;
        let team_livery = |t: u8| super::cars::TEAM_LIVERY0 + (t as usize).min(super::cars::TEAMS - 1);
        // players' cars take the rival colours the AI cars leave free first
        let taken: Vec<usize> = st.bots.iter().map(|b| b.0 as usize % race::RIVALS.len()).collect();
        let free: Vec<usize> = (0..race::RIVALS.len()).filter(|c| !taken.contains(c)).chain(taken.iter().copied()).collect();
        let mut colour = 0usize;
        for (idx, (ri, k)) in st.bots.iter().enumerate() {
            let (i, lat) = race::slot(np, *k as usize); let ri = *ri as usize % race::RIVALS.len();
            let mut c: RCar = race::mk_car(self, Some(ri), i, lat); race::rc_place(&mut c.car, &self.rt, w, i, lat, 0.0); c.bot = Some(idx as u8);
            if let Some(t) = st.bot_teams.get(idx).copied().filter(|_| self.race.duos) { c.team = Some(t); c.paint = team_livery(t); }
            // the host's phone (or the relay) drives them; elsewhere they come in like another player's car
            if !is_host || st.srv { c.remote = Some(net::bot_id(idx as u8)); c.name = race::RIVALS[ri].name.to_string(); }
            self.race.cars.push(c);
        }
        for (n, (id, k)) in st.slots.iter().enumerate() {
            let team = st.teams.get(n).copied().filter(|_| self.race.duos);
            let (i, lat) = if std::env::var("FW_OVERLAP").is_ok() { let (i, _) = race::slot(np, 0); (i, -0.6 + *k as f64 * 1.2) } else { race::slot(np, *k as usize) };   // FW_OVERLAP: test hook, cars start touching
            if *id == me {
                self.place_at(w, i); let tn = self.rt.tan[i]; self.car.x += -tn[1] * lat; self.car.z += tn[0] * lat; self.car.y = w.height(self.car.x, self.car.z) + 0.075;
                let mut c = race::mk_car(self, None, i, lat); c.team = team; self.race.player = self.race.cars.len(); self.race.cars.push(c);
            } else {
                let mut c: RCar = race::mk_car(self, Some(free[colour % free.len()]), i, lat); colour += 1; race::rc_place(&mut c.car, &self.rt, w, i, lat, 0.0);
                c.remote = Some(*id); c.name = names.iter().find(|n| n.0 == *id).map(|n| n.1.clone()).unwrap_or_else(|| "PLAYER".into());
                c.model = carsof.iter().find(|n| n.0 == *id).map(|n| n.1).unwrap_or(0) % super::cars::CARS.len(); c.paint = carsof.iter().find(|n| n.0 == *id).map(|n| n.2).unwrap_or(0); if let Some(t) = team { c.team = Some(t); c.paint = team_livery(t); } self.race.cars.push(c);
            }
        }
        if !self.race.cars.iter().any(|c| c.is_player()) { race::stop(self); self.state = State::Title; self.flash_msg("NOT IN THIS RACE", 2.0); return; }
        let mode = ["online", "duos", "tiles"][(st.mode as usize).min(2)]; let np = st.slots.len() + st.bots.len();
        self.stat("play", &[("o", super::stats::js(mode)), ("k", np.to_string()), ("h", if is_host { "1".into() } else { "0".into() })]);
        if st.mode == net::proto::MODE_TILES { super::tilefall::begin(self, w); } else if self.tf.on { super::tilefall::stop(self, w, None); }
        self.last_idx = self.race.cars[self.race.player].last_idx;
        self.go = net::with(|s| (st.go_at - s.host_now() + 0.9).max(0.0)).unwrap_or(4.5);
        self.ps.online_ui = OnlineUi::Lobby;
    }

    /// taps on the online screen; true when handled
    pub fn online_tap(&mut self, key: &str) -> bool {
        match (key, self.ps.online_ui.clone()) {
            ("online", OnlineUi::Closed) => { self.ps.online_ui = if net::active() { OnlineUi::Lobby } else { OnlineUi::Menu }; }
            ("on_back", OnlineUi::Menu) => { self.ps.online_ui = OnlineUi::Closed; }
            ("on_back", OnlineUi::Join(_)) => { self.ps.online_ui = OnlineUi::Menu; }
            ("on_back", OnlineUi::Lobby) => { self.ps.online_ui = OnlineUi::Closed; }   // stays in the room; ONLINE brings it back
            ("on_host", OnlineUi::Menu) => { let name = self.ps.name.clone(); net::host(&name, &self.rt.map); self.ps.online_ui = OnlineUi::Lobby; }
            ("on_join", OnlineUi::Menu) => { self.ps.online_ui = OnlineUi::Join(String::new()); }
            ("on_rooms", OnlineUi::Menu) => { self.ps.online_ui = OnlineUi::Rooms; }
            ("on_back", OnlineUi::Rooms) => { self.ps.online_ui = OnlineUi::Menu; }
            (k, OnlineUi::Rooms) if k.starts_with("on_r") => {
                let n: usize = k[4..].parse().unwrap_or(99);
                if let Some(r) = net::lobby::rooms().and_then(|l| l.into_iter().nth(n)) { let name = self.ps.name.clone(); net::join(r.code, &name, &self.rt.map); self.ps.online_ui = OnlineUi::Lobby; } }
            (k, OnlineUi::Join(mut code)) if k.starts_with('k') => {
                if k == "kdel" { code.pop(); } else if k == "kok" { if code.len() == 6 { let name = self.ps.name.clone(); net::join(code.parse().unwrap_or(0), &name, &self.rt.map); self.ps.online_ui = OnlineUi::Lobby; return true; } }
                else if code.len() < 6 { code.push_str(&k[1..]); }
                self.ps.online_ui = OnlineUi::Join(code); }
            ("on_leave", OnlineUi::Lobby) => { net::leave(); self.ps.online_ui = OnlineUi::Menu; }
            ("on_name", _) => { self.edit_name(); }
            // your car and its livery, from the room screen (the others see them with your next presence message)
            ("on_cprev", OnlineUi::Lobby) | ("on_cnext", OnlineUi::Lobby) => { let n = super::cars::CARS.len(); let k = if key == "on_cnext" { (self.ps.car + 1) % n } else { (self.ps.car + n - 1) % n }; self.choose_car(k); self.buzz("tap", 0.5); }
            ("on_lnext", OnlineUi::Lobby) => { let n = super::cars::LIVERIES.len(); let c = self.ps.car; if let Some(p) = self.ps.paint.get_mut(c) { *p = (*p + 1) % n; }
                let v: Vec<String> = self.ps.paint.iter().map(|p| p.to_string()).collect(); self.ps.saves.set("fogwood-paint", v.join(",")); self.buzz("tap", 0.5); }
            ("on_stage", OnlineUi::Lobby) => { self.ps.online_ui = OnlineUi::Stages; }
            ("on_back", OnlineUi::Stages) => { self.ps.online_ui = OnlineUi::Lobby; }
            (k, OnlineUi::Stages) if k.starts_with("on_st") => {
                if let Some(n) = k[5..].parse::<usize>().ok().filter(|n| *n < MAPS.len()) { let m = MAPS[n].0; if m != self.rt.map { self.switch_map(m); } }
                self.ps.online_ui = OnlineUi::Lobby; }
            ("on_laps", OnlineUi::Lobby) => { self.ps.online_laps = self.ps.online_laps % 5 + 1; }
            ("on_mode", OnlineUi::Lobby) => { self.ps.online_mode = (self.ps.online_mode + 1) % 3; self.ps.saves.set("fogwood-omode", self.ps.online_mode.to_string()); }
            ("on_bots", OnlineUi::Lobby) => { self.ps.online_bots = (self.ps.online_bots + 1) % 6; }
            ("on_ai", OnlineUi::Lobby) => { self.ps.online_ai = (self.ps.online_ai + 1) % 4; }
            ("on_start", OnlineUi::Lobby) => {
                if self.loading.is_some() { return true; }
                let (map, laps, mode) = (self.rt.map.clone(), self.ps.online_laps, self.ps.online_mode);
                // the AI cars: the rivals, fastest first, as many as picked
                let mut order: Vec<u8> = (0..race::RIVALS.len() as u8).collect(); order.sort_by(|a, b| race::RIVALS[*b as usize].pace.partial_cmp(&race::RIVALS[*a as usize].pace).unwrap());
                let nb = if mode == net::proto::MODE_TILES { 0 } else { self.ps.online_bots as usize };   // tile fall: players only
                let bots: Vec<u8> = order.into_iter().take(nb).collect();
                // the relay drives the AI cars when it can (the stage goes up first; the host takes them back if it never answers)
                let blob = if bots.is_empty() || std::env::var("FW_NOSIM").is_ok() { None } else { Some(crate::server::stage_blob(&self.rt.raw)) };
                if let Some(Some(st)) = net::with(|s| if s.peers.is_empty() { None } else {
                    let srv = blob.as_ref().map(|b| s.sim_capable() && s.upload_stage(b)).unwrap_or(false);
                    s.start_race(&map, laps, 7.0, &bots, srv, mode, self.ps.online_ai) }) { net::with(|s| s.start_in = Some(st)); }
                else { self.flash_msg("WAITING FOR PLAYERS", 1.5); } }
            _ => return false,
        }
        true
    }
}

/// the online screen, drawn over the title (replaces the stage and mode rows while open)
pub fn draw(ps: &super::Persist, map: &str, g: &mut Canvas, ui: &mut Vec<(&'static str, Rect)>, lw: f64, lh: f64, u: f64, top: f64) {
    let pw = (lw - 16.0 * u).min(210.0 * u); let px = js_round((lw - pw) / 2.0); let py = top; let ph = (lh - py - 6.0 * u).max(60.0 * u);
    g.panel(px, py, pw, ph, u, rgba(8, 12, 8, 0.88), Some(AMBER));
    let mut button = |g: &mut Canvas, ui: &mut Vec<(&'static str, Rect)>, k: &'static str, label: &str, x: f64, y: f64, w: f64, on: bool| {
        let h = 13.0 * u; ui.push((k, Rect { x, y, w, h }));
        g.panel(x, y, w, h, u, if on { rgba(243, 207, 98, 0.3) } else { PANEL }, Some(if on { AMBER } else { EDGE }));
        g.tx(label, x + (w - text_w(label, u)) / 2.0, y + 3.0 * u, u, if on { AMBER } else { INK }); };
    let centre = |g: &mut Canvas, s: &str, y: f64, sc: f64, c: Col| g.tx(s, px + (pw - text_w(s, sc)) / 2.0, y, sc, c);
    let (bx, bw) = (px + 10.0 * u, pw - 20.0 * u);
    match &ps.online_ui {
        OnlineUi::Menu => {
            centre(g, "ONLINE RACE", py + 6.0 * u, 2.0 * u, AMBER);
            centre(g, "RACE FRIENDS OVER THE INTERNET", py + 22.0 * u, u, rgba(233, 236, 226, 0.75));
            let y0 = py + 34.0 * u;
            let open = net::lobby::rooms().map(|r| r.len()).unwrap_or(0);
            let half = ((bw - 4.0 * u) / 2.0).floor();
            button(g, ui, "on_rooms", &if open > 0 { format!("OPEN ROOMS ({})", open) } else { "OPEN ROOMS".to_string() }, bx, y0, bw, open > 0);
            button(g, ui, "on_host", "HOST A ROOM", bx, y0 + 17.0 * u, half, open == 0);
            button(g, ui, "on_join", "JOIN BY CODE", bx + half + 4.0 * u, y0 + 17.0 * u, half, false);
            button(g, ui, "on_back", "BACK", bx, y0 + 34.0 * u, bw, false);
            let nm = if ps.name.is_empty() { "SET YOUR NAME".to_string() } else { format!("NAME: {}  - EDIT", ps.name) };
            button(g, ui, "on_name", &nm, bx, y0 + 51.0 * u, bw, ps.name.is_empty());
        }
        OnlineUi::Join(code) => {
            centre(g, "ENTER ROOM CODE", py + 5.0 * u, u, AMBER);
            let shown: String = (0..6).map(|i| code.chars().nth(i).unwrap_or('-')).map(|c| format!("{} ", c)).collect(); centre(g, shown.trim_end(), py + 15.0 * u, 2.0 * u, INK);
            let kw = ((bw - 8.0 * u) / 3.0).floor(); let y0 = py + 33.0 * u;
            let keys: [(&'static str, &str); 12] = [("k1", "1"), ("k2", "2"), ("k3", "3"), ("k4", "4"), ("k5", "5"), ("k6", "6"), ("k7", "7"), ("k8", "8"), ("k9", "9"), ("kdel", "DEL"), ("k0", "0"), ("kok", "JOIN")];
            for (n, (k, l)) in keys.iter().enumerate() { let (cx, cy) = ((n % 3) as f64, (n / 3) as f64); button(g, ui, k, l, bx + cx * (kw + 4.0 * u), y0 + cy * 16.0 * u, kw, *k == "kok" && code.len() == 6); }
            button(g, ui, "on_back", "BACK", bx, y0 + 4.0 * 16.0 * u, bw, false);
        }
        OnlineUi::Lobby => {
            let info = net::with(|s| (s.code, s.is_host, s.brokers_up(), s.peers.values().map(|p| (p.name.clone(), p.link(), p.rtt, p.host, p.map.clone(), p.first_seen)).collect::<Vec<_>>(), s.host_id.is_some()));
            let Some((code, host, up, mut peers, have_host)) = info else { centre(g, "NOT CONNECTED", py + 10.0 * u, u, RED); return };
            peers.sort_by(|a, b| a.5.partial_cmp(&b.5).unwrap());
            let cs = format!("{:06}", code); centre(g, &format!("ROOM {} {}", &cs[..3], &cs[3..]), py + 5.0 * u, 2.0 * u, AMBER);
            let status = if up == 0 { "CONNECTING TO THE MEETING POINT...".to_string() } else if !host && !have_host { "LOOKING FOR THE HOST...".to_string() } else if host { "SHARE THE CODE - FRIENDS TAP JOIN".to_string() } else { "CONNECTED".to_string() };
            centre(g, &status, py + 20.0 * u, u, if up == 0 { RED } else { rgba(233, 236, 226, 0.75) });
            let mut y = py + 31.0 * u;
            ui.push(("on_name", Rect { x: bx, y: y - u, w: bw, h: 9.0 * u }));   // your own row: tap to rename
            let row = |g: &mut Canvas, y: f64, name: &str, right: &str, me: bool| { if me { g.rect_c(bx, y - u, bw, 9.0 * u, rgba(243, 207, 98, 0.16)); } g.tx(name, bx + 3.0 * u, y, u, if me { AMBER } else { INK }); g.text(right, bx + bw - 3.0 * u - text_w(right, u), y, u, rgba(233, 236, 226, 0.7), None); };
            row(g, y, &format!("{}{}", ps.name, if host { " (HOST)" } else { "" }), "YOU - EDIT NAME", true); y += 10.0 * u;
            // your car: arrows change it, the middle changes its livery
            { let aw = 16.0 * u; let cn = super::cars::CARS[ps.car.min(super::cars::CARS.len() - 1)].name; let lv = super::cars::LIVERIES[ps.paint.get(ps.car).copied().unwrap_or(0).min(super::cars::LIVERIES.len() - 1)];
              ui.push(("on_cprev", Rect { x: bx, y: y - 2.0 * u, w: aw, h: 11.0 * u })); ui.push(("on_cnext", Rect { x: bx + bw - aw, y: y - 2.0 * u, w: aw, h: 11.0 * u })); ui.push(("on_lnext", Rect { x: bx + aw, y: y - 2.0 * u, w: bw - 2.0 * aw, h: 11.0 * u }));
              g.panel(bx, y - 2.0 * u, bw, 11.0 * u, u, PANEL, Some(EDGE)); g.tx("<", bx + 5.0 * u, y, u, AMBER); g.tx(">", bx + bw - 10.0 * u, y, u, AMBER);
              let t = format!("{}  {}", cn, lv); g.tx(&t, bx + (bw - text_w(&t, u)) / 2.0, y, u, INK); }
            y += 13.0 * u;
            for (name, link, rtt, ph, _, _) in peers.iter().take(4) {
                let r = match link { Link::Direct => format!("DIRECT{}", if *rtt > 0.0 { format!(" {:.0}MS", rtt * 1000.0) } else { String::new() }), Link::Relay => format!("RELAY{}", if *rtt > 0.0 { format!(" {:.0}MS", rtt * 1000.0) } else { String::new() }), Link::Connecting => "...".into() };
                row(g, y, &format!("{}{}", name, if *ph { " (HOST)" } else { "" }), &r, false); y += 10.0 * u; }
            if peers.len() > 4 { row(g, y, &format!("+{} MORE", peers.len() - 4), "", false); }
            let yb = py + ph - 32.0 * u;
            if host {
                let half = ((bw - 4.0 * u) / 2.0).floor();
                let q = ((half - 4.0 * u) / 2.0).floor();
                button(g, ui, "on_stage", &format!("STAGE: {} >", short_name(map)), bx, yb - 17.0 * u, half, false);
                button(g, ui, "on_laps", &format!("LAPS {}", ps.online_laps), bx + half + 4.0 * u, yb - 17.0 * u, q, false);
                button(g, ui, "on_bots", &if ps.online_bots == 0 { "AI OFF".to_string() } else { format!("AI {}", ps.online_bots) }, bx + half + 8.0 * u + q, yb - 17.0 * u, q, ps.online_bots > 0);
                button(g, ui, "on_start", "START RACE", bx, yb, half, !peers.is_empty());
                button(g, ui, "on_leave", "CLOSE ROOM", bx + half + 4.0 * u, yb, half, false);
            } else {
                centre(g, &format!("STAGE: {}", stage_name(map)), yb - 24.0 * u, u, INK); centre(g, "THE HOST STARTS THE RACE", yb - 13.0 * u, u, rgba(233, 236, 226, 0.75));
                let half = ((bw - 4.0 * u) / 2.0).floor();
                button(g, ui, "on_back", "HIDE", bx, yb, half, false); button(g, ui, "on_leave", "LEAVE ROOM", bx + half + 4.0 * u, yb, half, false);
            }
            if host { let half = ((bw - 4.0 * u) / 2.0).floor(); let q = ((half - 4.0 * u) / 2.0).floor(); let md = ["MODE: RACE", "MODE: DUOS", "MODE: TILES"][(ps.online_mode as usize).min(2)];
                button(g, ui, "on_mode", md, bx, yb + 16.0 * u, half, ps.online_mode != 0); button(g, ui, "on_back", "HIDE", bx + half + 4.0 * u, yb + 16.0 * u, q, false);
                // how hard the AI cars drive (under their number)
                button(g, ui, "on_ai", race::AI_NAMES[(ps.online_ai as usize).min(3)], bx + half + 8.0 * u + q, yb + 16.0 * u, q, ps.online_ai == race::AI_EXTREME); }
        }
        OnlineUi::Rooms => {
            centre(g, "OPEN ROOMS", py + 5.0 * u, u, AMBER);
            let yb = py + ph - 17.0 * u;
            match net::lobby::rooms() {
                None => centre(g, "LOOKING FOR ROOMS...", py + 30.0 * u, u, rgba(233, 236, 226, 0.75)),
                Some(list) if list.is_empty() => { centre(g, "NO OPEN ROOMS RIGHT NOW", py + 30.0 * u, u, INK); centre(g, "GO BACK AND HOST ONE", py + 42.0 * u, u, rgba(233, 236, 226, 0.75)); }
                Some(list) => {
                    const K: [&str; 6] = ["on_r0", "on_r1", "on_r2", "on_r3", "on_r4", "on_r5"];
                    let rows = (((yb - 4.0 * u) - (py + 17.0 * u)) / (16.0 * u)).floor().clamp(1.0, 6.0) as usize;
                    for (n, r) in list.iter().take(rows).enumerate() {
                        let y = py + 17.0 * u + n as f64 * 16.0 * u; let h = 13.0 * u; ui.push((K[n], Rect { x: bx, y, w: bw, h }));
                        g.panel(bx, y, bw, h, u, PANEL, Some(EDGE));
                        let left = if r.name.is_empty() { "ROOM".to_string() } else { r.name.to_uppercase() };
                        g.tx(&left, bx + 4.0 * u, y + 3.0 * u, u, INK);
                        let right = format!("{}  {}P{}", short_name(&r.map), r.players, if r.racing { "  RACING" } else { "" });
                        g.tx(&right, bx + bw - 4.0 * u - text_w(&right, u), y + 3.0 * u, u, if r.racing { rgba(233, 236, 226, 0.55) } else { AMBER });
                    }
                }
            }
            button(g, ui, "on_back", "BACK", bx, yb, bw, false);
        }
        OnlineUi::Stages => {
            centre(g, "PICK THE STAGE", py + 5.0 * u, u, AMBER);
            let gp = 4.0 * u; let tw = ((bw - 3.0 * gp) / 4.0).floor(); let th = 25.0 * u; let y0 = py + 17.0 * u;
            for (n, (k, name, surf)) in TILES.iter().enumerate() {
                let (x, y) = (bx + (n % 4) as f64 * (tw + gp), y0 + (n / 4) as f64 * (th + gp)); let on = MAPS[n].0 == map;
                ui.push((k, Rect { x, y, w: tw, h: th }));
                g.panel(x, y, tw, th, u, if on { rgba(243, 207, 98, 0.3) } else { PANEL }, Some(if on { AMBER } else { EDGE }));
                g.tx(name, x + (tw - text_w(name, u)) / 2.0, y + 5.0 * u, u, if on { AMBER } else { INK });
                g.tx(surf, x + (tw - text_w(surf, u)) / 2.0, y + 15.0 * u, u, rgba(233, 236, 226, 0.55));
            }
            let yb = py + ph - 17.0 * u;
            button(g, ui, "on_back", "BACK", bx, yb, bw, false);
        }
        OnlineUi::Closed => {}
    }
}
