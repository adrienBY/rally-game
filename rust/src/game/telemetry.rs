//! Driving telemetry (the TEL button): one CSV row per frame while you drive, written when you press TEL again,
//! leave to the menu, or after 15 minutes. The app copies the file to Downloads/Fogwood.
use super::{Game, State};
use crate::physics::Input;
use std::fmt::Write;

pub const TEL_MAX_S: f64 = 900.0;

pub struct Tel { pub t: f64, rows: String, pub n: usize, header: Vec<String>, last_idx: Option<usize>, prog: i64 }

impl Tel {
    pub fn new(g: &Game) -> Tel {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let header = vec![
            format!("Fogwood Rally telemetry, map {}, {}", g.rt.map, if g.race.on { format!("race, {} laps, {} rivals", g.race.laps, g.race.cars.len().saturating_sub(1)) } else { "time trial".into() }),
            format!("stage length {} m (path index = metres from the start line), unix time {}", g.np, secs),
            format!("grip road {:.2} verge {:.2}; camera mode {}; best lap so far {}", g.tune.mu_road, g.tune.mu_grass, g.cam.mode, g.best.map(|b| format!("{:.3} s", b)).unwrap_or_else(|| "none".into())),
            "columns: steer_in +1 = left button, -1 = right button; lat_m = distance from the road centre line, + = left of it (road edge at 3.4 m); rack = front wheel angle, rad, + = left".into(),
            "notes: lap <s> = time trial lap completed, reset = RST pressed, start = lights out".into(),
        ];
        Tel { t: 0.0, rows: String::with_capacity(1 << 20), n: 0, header, last_idx: None, prog: 0 }
    }

    /// one row, called once per tick while driving
    pub fn row(&mut self, g: &Game, inp: &Input, dt: f64, note: &str, i: usize) {
        self.t += dt;
        let c = &g.car; let np = g.np as i64;
        if let Some(l) = self.last_idx { let mut d = i as i64 - l as i64; if d < -(np / 2) { d += np; } if d > np / 2 { d -= np; } self.prog += d; }
        self.last_idx = Some(i);
        let p = g.rt.path[i]; let tn = g.rt.tan[i]; let lat = (c.x - p[0]) * tn[1] - (c.z - p[1]) * tn[0];
        let fwd = c.h.sin() * tn[0] + c.h.cos() * tn[1];
        let wroad = c.wh.iter().filter(|w| w.road).count();
        let (lap, lap_t, clock) = if g.race.on { let pc = &g.race.cars[g.race.player]; (pc.laps_done + 1, g.race.t - pc.lap_t0, g.race.t) } else { (g.lap as i64, if g.started { g.g_t - g.lap_start } else { 0.0 }, g.g_t) };
        let _ = writeln!(self.rows, "{:.3},{:.3},{},{:.3},{},{},{:.2},{:.2},{:.2},{:.2},{:.1},{:.2},{:.2},{:.3},{:.2},{},{:.0},{:.2},{:.2},{:.2},{},{:.3},{},{},{},{:.2},{:.2},{},{}",
            self.t, clock, lap, lap_t, i, self.prog, lat, c.x, c.z, c.y, c.u * 3.6, c.vl, c.r, fwd, c.slide, c.gear, c.rpm,
            inp.steer, inp.throttle, inp.brake, inp.hb as u8, c.steer, c.on_road as u8, wroad, (c.contacts < 0.5) as u8, c.impact, g.shake,
            if g.race.on { g.race.pos.to_string() } else { String::new() }, note);
        self.n += 1;
    }

    pub fn write(&self, dir: &str, map: &str) -> Option<String> {
        let mut out = String::with_capacity(self.rows.len() + 2048);
        for h in &self.header { let _ = writeln!(out, "# {}", h); }
        let _ = writeln!(out, "# frames {} over {:.1} s", self.n, self.t);
        out.push_str("t_s,clock_s,lap,lap_t_s,idx,prog_m,lat_m,x,z,y,kmh,slip_ms,yaw_rate,heading_dot,slide,gear,rpm,steer_in,throttle,brake,handbrake,rack,on_road,wheels_on_road,airborne,impact,shake,race_pos,note\n");
        out.push_str(&self.rows);
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let path = format!("{}/fogwood-telemetry-{}-{}.csv", dir, map, secs); std::fs::write(&path, out).ok()?; Some(path)
    }
}

impl Game {
    /// TEL pressed, or recording must end (menu, time limit)
    pub fn tel_toggle(&mut self) {
        if let Some(t) = self.tel.take() { self.tel_save(t); }
        else { self.tel = Some(Tel::new(self)); self.flash_msg("RECORDING TELEMETRY", 1.5); }
    }
    pub(crate) fn tel_save(&mut self, t: Tel) {
        if t.n < 30 { self.flash_msg("TELEMETRY: NOTHING RECORDED", 2.0); return; }
        let dir = self.ps.saves.dir();
        match t.write(&dir, &self.rt.map) { Some(p) => { self.events.push(format!("bench {}", p)); self.flash_msg("TELEMETRY SAVED", 2.0); } None => self.flash_msg("TELEMETRY FAILED", 2.0) }
    }
    pub(crate) fn tel_frame(&mut self, inp: &Input, dt: f64, i: usize) {
        if self.tel.is_none() { return; }
        if self.state == State::Drive { let note = std::mem::take(&mut self.tel_note); let mut t = self.tel.take().unwrap(); t.row(self, inp, dt, &note, i); self.tel = Some(t); }
        if self.tel.as_ref().map(|t| t.t > TEL_MAX_S).unwrap_or(false) { let t = self.tel.take().unwrap(); self.tel_save(t); }
    }
}
