//! Online races: the room screen (host, join with a code, lobby), the synced start, and other players' cars.
use super::hud::*;
use super::race::{self, RCar};
use super::{js_round, Game, Rect, State};
use crate::net::{self, proto::{CarSnap, Start}, Link};
use crate::physics::Car;
use crate::world::World;

#[derive(Clone, PartialEq, Debug, Default)]
pub enum OnlineUi { #[default] Closed, Menu, Join(String), Lobby }

pub const KEYS: [&str; 20] = ["on_host", "on_join", "on_back", "on_start", "on_leave", "on_stage", "k0", "k1", "k2", "k3", "k4", "k5", "k6", "k7", "k8", "k9", "kdel", "kok", "on_laps", "online"];
const MAPS: [(&str, &str); 6] = [("forest", "FOGWOOD"), ("snow", "COL DE GIVRE"), ("canyon", "CANYON ROUGE"), ("lakes", "LAKELAND"), ("touge", "MOMIJI TOUGE"), ("riviera", "RIVIERA CORNICHE")];
fn stage_name(m: &str) -> &'static str { MAPS.iter().find(|x| x.0 == m).map(|x| x.1).unwrap_or("STAGE") }

fn snap_of(c: &Car) -> CarSnap {
    CarSnap { t: 0.0, x: c.x as f32, y: c.y as f32, z: c.z as f32, h: c.h as f32, vx: c.vx as f32, vy: c.vy as f32, vz: c.vz as f32, r: c.r as f32, pitch: c.pitch as f32, roll: c.roll as f32,
        steer: c.steer as f32, u: c.u as f32, gear: c.gear as i8, rpm: c.rpm.clamp(0.0, 65000.0) as u16, d: [c.wh[0].d as f32, c.wh[1].d as f32, c.wh[2].d as f32, c.wh[3].d as f32],
        ang: [c.wh[0].ang as f32, c.wh[1].ang as f32, c.wh[2].ang as f32, c.wh[3].ang as f32], flags: 0 }
}
fn apply(c: &mut Car, s: &CarSnap, w: &World) {
    c.x = s.x as f64; c.y = s.y as f64; c.z = s.z as f64; c.h = s.h as f64; c.vx = s.vx as f64; c.vy = s.vy as f64; c.vz = s.vz as f64; c.r = s.r as f64;
    c.pitch = s.pitch as f64; c.roll = s.roll as f64; c.steer = s.steer as f64; c.u = s.u as f64; c.gear = s.gear as f64; c.rpm = s.rpm as f64;
    for k in 0..4 { c.wh[k].d = s.d[k] as f64; c.wh[k].ang = s.ang[k] as f64; }
    c.contacts = 4.0; c.on_road = true; c.impact = 0.0; let _ = w;
}

impl Game {
    pub fn online_open(&self) -> bool { self.ps.online_ui != OnlineUi::Closed }

    /// network work for this frame, the start when it arrives, and the other players' cars (before the race step)
    pub(crate) fn online_tick(&mut self, w: &World) {
        if !net::active() { return; }
        let map = self.rt.map.clone(); let racing = self.race.online && self.state == State::Drive;
        let pending = net::with(|s| { s.map = map.clone(); s.racing = racing; s.poll(); s.start_in.clone() }).flatten();
        // guests follow the host's stage while waiting, so it is already built when the race starts
        if pending.is_none() && self.state == State::Title && self.loading.is_none() {
            if let Some(Some(hm)) = net::with(|s| if s.is_host { None } else { s.host_id.and_then(|h| s.peers.get(&h)).map(|p| p.map.clone()).filter(|m| !m.is_empty()) }) {
                if hm != self.rt.map && MAPS.iter().any(|m| m.0 == hm) { self.switch_map(&hm); } }
        }
        if let Some(st) = pending {
            if st.map != self.rt.map { if self.loading.is_none() { self.switch_map(&st.map); } }   // the start stays pending until that stage is built
            else if self.loading.is_none() { net::with(|s| s.start_in = None); self.online_begin(w, st); }
        }
        if self.race.online && self.state == State::Drive {
            let mine = snap_of(&self.car);
            let ids: Vec<(usize, u32)> = self.race.cars.iter().enumerate().filter_map(|(k, c)| c.remote.map(|id| (k, id))).collect();
            let snaps = net::with(|s| { s.send_car(mine); ids.iter().map(|(k, id)| (*k, s.remote(*id))).collect::<Vec<_>>() }).unwrap_or_default();
            for (k, sn) in snaps { if let Some(sn) = sn { let mut c = self.race.cars[k].car; apply(&mut c, &sn, w); self.race.cars[k].car = c; } }
        }
    }

    fn online_begin(&mut self, w: &World, st: Start) {
        let me = net::with(|s| s.me).unwrap_or(0);
        let names: Vec<(u32, String)> = net::with(|s| s.peers.values().map(|p| (p.id, p.name.clone())).collect()).unwrap_or_default();
        if self.state == State::Drive { self.to_title(); }
        self.state = State::Drive; self.lap = 1; self.started = false; self.g_t = 0.0; self.flags = [false; 3]; self.car_audio_on = true;
        let np = self.np; let rc = &mut self.race; rc.on = true; rc.online = true; rc.phase = race::Phase::Grid; rc.t = 0.0; rc.done_t = 0.0; rc.note.clear(); rc.cars.clear(); rc.pos = 0; rc.order.clear();
        rc.laps = st.laps.max(1) as i64; rc.go_at = st.go_at;
        let mut colour = 0usize;
        for (id, k) in &st.slots {
            let (i, lat) = race::slot(np, *k as usize);
            if *id == me {
                self.place_at(w, i); let tn = self.rt.tan[i]; self.car.x += -tn[1] * lat; self.car.z += tn[0] * lat; self.car.y = w.height(self.car.x, self.car.z) + 0.075;
                let c = race::mk_car(self, None, i, lat); self.race.player = self.race.cars.len(); self.race.cars.push(c);
            } else {
                let mut c: RCar = race::mk_car(self, Some(colour % 5), i, lat); colour += 1; race::rc_place(&mut c.car, &self.rt, w, i, lat, 0.0);
                c.remote = Some(*id); c.name = names.iter().find(|n| n.0 == *id).map(|n| n.1.clone()).unwrap_or_else(|| "PLAYER".into()); self.race.cars.push(c);
            }
        }
        if !self.race.cars.iter().any(|c| c.is_player()) { race::stop(self); self.state = State::Title; self.flash_msg("NOT IN THIS RACE", 2.0); return; }
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
            (k, OnlineUi::Join(mut code)) if k.starts_with('k') => {
                if k == "kdel" { code.pop(); } else if k == "kok" { if code.len() == 6 { let name = self.ps.name.clone(); net::join(code.parse().unwrap_or(0), &name, &self.rt.map); self.ps.online_ui = OnlineUi::Lobby; return true; } }
                else if code.len() < 6 { code.push_str(&k[1..]); }
                self.ps.online_ui = OnlineUi::Join(code); }
            ("on_leave", OnlineUi::Lobby) => { net::leave(); self.ps.online_ui = OnlineUi::Menu; }
            ("on_stage", OnlineUi::Lobby) => { let k = MAPS.iter().position(|m| m.0 == self.rt.map).unwrap_or(0); let next = MAPS[(k + 1) % MAPS.len()].0; self.switch_map(next); }
            ("on_laps", OnlineUi::Lobby) => { self.ps.online_laps = self.ps.online_laps % 5 + 1; }
            ("on_start", OnlineUi::Lobby) => {
                if self.loading.is_some() { return true; }
                let (map, laps) = (self.rt.map.clone(), self.ps.online_laps);
                if let Some(Some(st)) = net::with(|s| if s.peers.is_empty() { None } else { s.start_race(&map, laps, 7.0) }) { net::with(|s| s.start_in = Some(st)); }
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
            button(g, ui, "on_host", "HOST A ROOM", bx, y0, bw, true);
            button(g, ui, "on_join", "JOIN WITH A CODE", bx, y0 + 17.0 * u, bw, false);
            button(g, ui, "on_back", "BACK", bx, y0 + 34.0 * u, bw, false);
            centre(g, &format!("YOUR NAME: {}", ps.name), y0 + 52.0 * u, u, rgba(233, 236, 226, 0.6));
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
            let row = |g: &mut Canvas, y: f64, name: &str, right: &str, me: bool| { if me { g.rect_c(bx, y - u, bw, 9.0 * u, rgba(243, 207, 98, 0.16)); } g.tx(name, bx + 3.0 * u, y, u, if me { AMBER } else { INK }); g.text(right, bx + bw - 3.0 * u - text_w(right, u), y, u, rgba(233, 236, 226, 0.7), None); };
            row(g, y, &format!("{}{}", ps.name, if host { " (HOST)" } else { "" }), "YOU", true); y += 10.0 * u;
            for (name, link, rtt, ph, _, _) in peers.iter().take(5) {
                let r = match link { Link::Direct => format!("DIRECT{}", if *rtt > 0.0 { format!(" {:.0}MS", rtt * 1000.0) } else { String::new() }), Link::Relay => format!("RELAY{}", if *rtt > 0.0 { format!(" {:.0}MS", rtt * 1000.0) } else { String::new() }), Link::Connecting => "...".into() };
                row(g, y, &format!("{}{}", name, if *ph { " (HOST)" } else { "" }), &r, false); y += 10.0 * u; }
            if peers.len() > 5 { row(g, y, &format!("+{} MORE", peers.len() - 5), "", false); }
            let yb = py + ph - 32.0 * u;
            if host {
                let half = ((bw - 4.0 * u) / 2.0).floor();
                button(g, ui, "on_stage", &format!("STAGE: {}", stage_name(map)), bx, yb - 17.0 * u, half, false);
                button(g, ui, "on_laps", &format!("LAPS: {}", ps.online_laps), bx + half + 4.0 * u, yb - 17.0 * u, half, false);
                button(g, ui, "on_start", "START RACE", bx, yb, half, !peers.is_empty());
                button(g, ui, "on_leave", "CLOSE ROOM", bx + half + 4.0 * u, yb, half, false);
            } else {
                centre(g, &format!("STAGE: {}", stage_name(map)), yb - 24.0 * u, u, INK); centre(g, "THE HOST STARTS THE RACE", yb - 13.0 * u, u, rgba(233, 236, 226, 0.75));
                let half = ((bw - 4.0 * u) / 2.0).floor();
                button(g, ui, "on_back", "HIDE", bx, yb, half, false); button(g, ui, "on_leave", "LEAVE ROOM", bx + half + 4.0 * u, yb, half, false);
            }
            if host { button(g, ui, "on_back", "HIDE", bx, yb + 16.0 * u, bw, false); }
        }
        OnlineUi::Closed => {}
    }
}
