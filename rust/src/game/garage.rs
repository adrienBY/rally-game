//! The garage: pick your car. The car stands on the stage with the camera orbiting it (drag to spin it, it keeps turning
//! slowly when left alone); a panel on the left shows its name, class and four stat bars that fill block by block; arrows
//! and dots change car (the new body drops onto its springs), and SELECT makes it yours (then DRIVE starts the stage).
use super::cars::{tune_for, CARS};
use super::hud::*;
use super::math::*;
use super::{js_round, Game, Rect, State};
use crate::world::World;
use std::collections::HashMap;

const STAT_NAMES: [&str; 4] = ["TOP SPEED", "ACCELERATION", "HANDLING", "GRIP"];
const SEGS: usize = 10;

#[derive(Default)]
pub struct Garage {
    pub open: bool, anim: f64, pub sel: usize, last_t: f64,
    /// camera angle around the car (0 = straight ahead of it), its spin speed, time since the last touch
    pub yaw: f64, yaw_v: f64, idle: f64,
    /// car change animation: time since, direction, bar values (0..10) shown and where they started
    change_t: f64, dir: f64, bars: [f64; 4], from: [f64; 4], stamp_t: f64,
    flash: HashMap<String, f64>, rects: Vec<(String, Rect)>,
    /// touch: id, start x, last x, start time, started on the panel
    drag: Option<(i32, f64, f64, f64, bool)>, moved: f64, hint: f64,
    /// asks the game loop to park the car / drop the new body (it has the world)
    pub park: bool, pub hop: bool,
}

fn ease_out(t: f64) -> f64 { let t = t.clamp(0.0, 1.0); 1.0 - (1.0 - t).powi(3) }
fn back_out(t: f64) -> f64 { let t = t.clamp(0.0, 1.0); let c = 1.7; 1.0 + (c + 1.0) * (t - 1.0).powi(3) + c * (t - 1.0).powi(2) }
fn fade(c: Col, a: f64) -> Col { Col { a: c.a * a.clamp(0.0, 1.0) as f32, ..c } }
fn mix(a: Col, b: Col, k: f64) -> Col { let k = k.clamp(0.0, 1.0) as f32; Col { r: a.r + (b.r - a.r) * k, g: a.g + (b.g - a.g) * k, b: a.b + (b.b - a.b) * k, a: a.a + (b.a - a.a) * k } }
const DARK: Col = rgba(8, 11, 8, 0.86);
const DIM: Col = rgba(233, 236, 226, 0.55);
const SEG_OFF: Col = rgba(233, 236, 226, 0.12);

impl Game {
    pub fn garage_open(&self) -> bool { self.gar.open }
    pub fn open_garage(&mut self) {
        if self.state != State::Title { return; }
        let g = &mut self.gar; g.open = true; g.sel = self.ps.car.min(CARS.len() - 1); g.yaw = 0.8; g.yaw_v = 0.0; g.idle = 0.0; g.change_t = 1.0; g.dir = 1.0;
        g.bars = [0.0; 4]; g.from = [0.0; 4]; g.change_t = 0.0; g.park = true; g.hop = false; g.drag = None; g.stamp_t = 9.0;
        self.ps.online_ui = super::online::OnlineUi::Closed; self.cam.shot = -1;
    }
    pub fn close_garage(&mut self) { self.gar.open = false; self.gar.drag = None; self.cam.shot = -1; self.cam.init = false; self.touches.clear(); }
    /// make car k yours: saved, and the handling follows
    pub fn choose_car(&mut self, k: usize) {
        let k = k.min(CARS.len() - 1); self.ps.car = k; self.ps.saves.set("fogwood-car", k.to_string()); self.tune = tune_for(&self.base_tune, k);
    }
    pub fn garage_go(&mut self, d: i32) {
        let n = CARS.len() as i32; let g = &mut self.gar; let next = ((g.sel as i32 + d) % n + n) % n;
        g.from = g.bars; g.sel = next as usize; g.change_t = 0.0; g.dir = if d >= 0 { 1.0 } else { -1.0 }; g.hop = true; g.yaw_v += g.dir * 1.6; g.idle = 0.0;
        self.buzz("tap", 0.5);
    }

    /// touches while the garage is open; true when it took them
    pub fn garage_touch(&mut self, action: i32, id: i32, x: f64, y: f64) -> bool {
        if !self.gar.open || self.state != State::Title { return false; }
        let hit = |g: &Garage| g.rects.iter().rev().find(|(_, r)| x >= r.x - 3.0 && x <= r.x + r.w + 3.0 && y >= r.y - 3.0 && y <= r.y + r.h + 3.0).map(|(k, _)| k.clone());
        let now = self.time;
        match action {
            0 => {
                let k = hit(&self.gar);
                if let Some(k) = k.clone() { self.gar.flash.insert(k.clone(), 0.0);
                    match k.as_str() {
                        "g_prev" => { self.garage_go(-1); return true; }
                        "g_next" => { self.garage_go(1); return true; }
                        "g_back" => { self.buzz("tap", 0.5); self.close_garage(); return true; }
                        "g_sel" => { let s = self.gar.sel;
                            if s != self.ps.car { self.choose_car(s); self.gar.stamp_t = 0.0; self.buzz("shift", 0.8); }
                            else { self.buzz("shift", 0.6); self.close_garage(); self.start_game(); }
                            return true; }
                        d if d.starts_with("g_dot") => { if let Ok(n) = d[5..].parse::<usize>() { if n != self.gar.sel { let dd = n as i32 - self.gar.sel as i32; self.garage_go(0); let g = &mut self.gar; g.sel = n; g.dir = dd.signum() as f64; g.yaw_v += g.dir * 1.0; } } return true; }
                        _ => {}
                    } }
                let on_panel = k.as_deref() == Some("g_panel");
                self.gar.drag = Some((id, x, x, now, on_panel)); self.gar.moved = 0.0; self.gar.idle = 0.0; true
            }
            1 => { if let Some((did, x0, lx, t0, panel)) = self.gar.drag { if did == id {
                let dx = x - lx; self.gar.moved = self.gar.moved.max((x - x0).abs());
                if !panel { self.gar.yaw -= dx * 0.024 / self.u; let dt = (now - self.gar.last_t).max(1.0 / 120.0); self.gar.yaw_v = -dx * 0.024 / self.u / dt * 0.6; self.gar.hint = 1.0; }
                self.gar.drag = Some((did, x0, x, t0, panel)); self.gar.idle = 0.0; } } true }
            _ => { if let Some((did, x0, _, t0, panel)) = self.gar.drag { if did == id { self.gar.drag = None;
                // a quick swipe on the panel changes car
                if panel && (x - x0).abs() > 24.0 * self.u && now - t0 < 0.6 { self.garage_go(if x < x0 { 1 } else { -1 }); } } } true }
        }
    }

    /// once a frame (before the camera): park the car, drop a new body, spin with inertia, drift round when idle
    pub(crate) fn garage_step(&mut self, w: &World, dt: f64) {
        if !self.gar.open { return; }
        if self.gar.park { self.gar.park = false; self.place_at(w, 6); self.car.y += 0.4; }
        if self.gar.hop { self.gar.hop = false; self.car.y += 0.32; self.car.vy = 1.2; self.car.pr += 0.5 * self.gar.dir; }
        let g = &mut self.gar; g.idle += dt; g.change_t += dt; g.stamp_t += dt; g.hint = (g.hint - dt * 0.4).max(0.0);
        if g.drag.map(|d| !d.4).unwrap_or(false) { } else {
            g.yaw += g.yaw_v * dt; g.yaw_v *= (-dt * 2.6).exp();
            if g.idle > 2.5 { g.yaw_v += (0.22 - g.yaw_v) * (dt * 0.8).min(1.0); }
        }
    }
    /// the orbiting showroom camera (the car sits right of the panel); returns the field of view
    pub(crate) fn garage_cam(&mut self, w: &World) -> f64 {
        let s = self.car; let ang = s.h + self.gar.yaw; let r = 6.3;
        let mut p = [s.x + ang.sin() * r, s.y + 1.7, s.z + ang.cos() * r]; p[1] = p[1].max(w.height(p[0], p[2]) + 0.6);
        let t = [s.x, s.y + 0.55, s.z]; let f = [t[0] - p[0], t[2] - p[2]]; let fl = f[0].hypot(f[1]).max(1e-6);
        // the camera's right is (-fz, fx): look a little to the right of the car so it stands right of centre
        let side = if self.w > self.h { 1.55 } else { 0.0 };
        let t2 = [t[0] + f[1] / fl * side, t[1], t[2] - f[0] / fl * side];
        self.cam_pos = p; self.cam_rot = look_rot(p, t2); 46.0
    }
}

/// draw the garage over the stage; records its hit areas
pub fn draw(gar: &mut Garage, ps: &super::Persist, time: f64, g: &mut Canvas, lw: f64, lh: f64, u: f64, pl: f64, pr: f64, pt: f64, pb: f64) {
    let dt = (time - gar.last_t).clamp(0.0, 0.05); gar.last_t = time;
    gar.anim = (gar.anim + dt / 0.3).min(1.0); let a = ease_out(gar.anim);
    for v in gar.flash.values_mut() { *v += dt; } gar.flash.retain(|_, v| *v < 0.35);
    let fl = |gar: &Garage, k: &str| gar.flash.get(k).map(|t| 1.0 - t / 0.35).unwrap_or(0.0);
    gar.rects.clear();
    let car = &CARS[gar.sel];
    // bars fill towards the new car's numbers, one after another
    for i in 0..4 { let k = ease_out((gar.change_t - i as f64 * 0.06) / 0.35); gar.bars[i] = gar.from[i] + (car.stats[i] - gar.from[i]) * k; }
    // vignette: darker on the left behind the panel, letterbox bars top and bottom
    for i in 0..8 { let x = lw * 0.06 * i as f64; g.rect_c(x, 0.0, lw * 0.06 + 1.0, lh, rgba(4, 6, 4, ((0.42 - i as f64 * 0.05) * a).max(0.0) as f32)); }
    let bar_h = js_round(14.0 * u * a);
    g.rect_c(0.0, 0.0, lw, bar_h + pt - 4.0 * u, rgba(4, 6, 4, 0.75)); g.rect_c(0.0, lh - bar_h - pb + 4.0 * u, lw, bar_h + pb, rgba(4, 6, 4, 0.75));
    // header: back, title with hazard stripes, counter
    let (bx, by, bw, bh) = (pl, pt, 22.0 * u, 13.0 * u); let f = fl(gar, "g_back");
    gar.rects.push(("g_back".into(), Rect { x: bx, y: by, w: bw, h: bh }));
    g.panel(bx, by + f * u, bw, bh, u, mix(PANEL, rgba(243, 207, 98, 0.5), f), Some(EDGE)); g.arrow(bx + bw / 2.0 + u, by + bh / 2.0 + f * u, -1.0, 4.0 * u, u, INK);
    g.tx("GARAGE", bx + bw + 6.0 * u, by + 3.0 * u, u * 2.0 - 0.0, AMBER);
    let hx = bx + bw + 6.0 * u + text_w("GARAGE", 2.0 * u) + 6.0 * u; let hw = (lw * 0.42 - hx).max(0.0);
    for i in 0..((hw / (6.0 * u)) as i32) { let x = hx + i as f64 * 6.0 * u; let c = if ((i as f64 + time * 4.0) as i32) % 2 == 0 { rgba(243, 207, 98, 0.55) } else { rgba(20, 20, 20, 0.6) }; g.rect_c(x, by + 5.0 * u, 3.0 * u, 3.0 * u, c); }
    let cnt = format!("{}/{}", gar.sel + 1, CARS.len()); g.tx(&cnt, lw - pr - text_w(&cnt, u), by + 3.0 * u, u, DIM);

    // left panel: name (slides in), class, drive, stats
    let pw = js_round((lw * 0.38).clamp(124.0 * u, 176.0 * u)); let px = js_round(pl - (1.0 - a) * (pw + 20.0 * u)); let py = js_round(by + bh + 10.0 * u);
    let ph = js_round((lh - py - pb - 20.0 * u).max(96.0 * u).min(120.0 * u));
    gar.rects.push(("g_panel".into(), Rect { x: px, y: py, w: pw, h: ph }));
    g.panel(px, py, pw, ph, u, DARK, Some(rgba(243, 207, 98, 0.7)));
    g.rect_c(px + u, py + u, 3.0 * u, ph - 2.0 * u, AMBER);
    let ct = ease_out(gar.change_t / 0.28); let slide = (1.0 - ct) * 26.0 * u * gar.dir;
    let ix = px + 9.0 * u;
    let name_sc = if text_w(car.name, 2.0 * u) <= pw - 16.0 * u { 2.0 * u } else { u };
    g.text(car.name, ix + slide, py + 7.0 * u, name_sc, fade(AMBER, ct), Some(fade(SHADOW, ct)));
    g.text(car.class, ix + slide * 0.6, py + 7.0 * u + 8.0 * name_sc + 3.0 * u, u, fade(DIM, ct), None);
    // drive pill and the SELECTED tag
    let dy = py + 7.0 * u + 8.0 * name_sc + 13.0 * u; let dw = text_w(car.drive, u) + 8.0 * u;
    g.panel(ix, dy, dw, 10.0 * u, u, rgba(233, 236, 226, 0.12), Some(EDGE)); g.text(car.drive, ix + 4.0 * u, dy + 2.0 * u, u, INK, None);
    if gar.sel == ps.car { let st = back_out(gar.stamp_t / 0.3); let tw = text_w("SELECTED", u) + 8.0 * u; let tx = ix + dw + 4.0 * u;
        g.panel(tx, dy - (1.0 - st) * 6.0 * u, tw, 10.0 * u, u, rgba(120, 200, 110, (0.28 * st.clamp(0.0, 1.0)) as f32), Some(fade(rgba(150, 230, 130, 1.0), st)));
        g.text("SELECTED", tx + 4.0 * u, dy + 2.0 * u - (1.0 - st) * 6.0 * u, u, fade(rgba(170, 240, 150, 1.0), st), None); }
    // stats: label, segmented bar, value
    let sy0 = dy + 16.0 * u; let segw = ((pw - 18.0 * u - 22.0 * u) / SEGS as f64 - u).floor().max(2.0 * u); let row = ((py + ph - sy0 - 6.0 * u) / 4.0).max(14.0 * u).min(17.0 * u);
    for i in 0..4 {
        let y = sy0 + i as f64 * row; g.text(STAT_NAMES[i], ix, y, u, DIM, None);
        let v = gar.bars[i]; let target = car.stats[i]; let yb = y + 8.0 * u;
        for s in 0..SEGS { let x = ix + s as f64 * (segw + u); let fill = (v - s as f64).clamp(0.0, 1.0);
            g.rect_c(x, yb, segw, 4.0 * u, SEG_OFF);
            if fill > 0.0 { let hot = s as f64 + 1.0 > target - 0.01 && s as f64 + 1.0 <= target + 0.01 && gar.change_t < 0.8;
                let c = if hot { mix(AMBER, INK, ((time * 10.0).sin() * 0.5 + 0.5) * 0.6) } else { AMBER };
                g.rect_c(x, yb, js_round(segw * fill), 4.0 * u, c); g.rect_c(x, yb, js_round(segw * fill), u, rgba(255, 245, 200, 0.5)); } }
        let vs = format!("{:.0}", target); g.text(&vs, px + pw - 8.0 * u - text_w(&vs, u), y + 6.0 * u, u, INK, None);
    }

    // arrows either side of the car, bobbing outwards
    let cy = lh * 0.5; let ax0 = px + pw + 10.0 * u + (1.0 - a) * 30.0 * u; let ax1 = lw - pr - 18.0 * u - (1.0 - a) * 30.0 * u;
    for (k, x, d) in [("g_prev", ax0, -1.0), ("g_next", ax1, 1.0)] {
        let f = fl(gar, k); let bob = (time * 3.4).sin().max(0.0) * 2.0 * u * d + f * 3.0 * u * d; let (w, h) = (18.0 * u, 22.0 * u);
        gar.rects.push((k.into(), Rect { x, y: cy - h / 2.0, w, h }));
        g.panel(x, cy - h / 2.0, w, h, u, mix(rgba(8, 11, 8, 0.55), rgba(243, 207, 98, 0.45), f), Some(rgba(243, 207, 98, 0.75)));
        for q in 0..2 { g.arrow(x + w / 2.0 + bob + (q as f64 - 0.5) * 5.0 * u * d + u, cy, d, 5.0 * u, u, if q == 1 { AMBER } else { fade(AMBER, 0.45) }); }
    }

    // dots under the car
    let n = CARS.len(); let dwid = 9.0 * u; let dx0 = js_round((ax0 + 18.0 * u + ax1) / 2.0 - n as f64 * dwid / 2.0); let dyy = lh - pb - 26.0 * u;
    for k in 0..n { let on = k == gar.sel; let x = dx0 + k as f64 * dwid; let s = if on { 5.0 * u } else { 3.0 * u };
        gar.rects.push((format!("g_dot{}", k), Rect { x: x - 2.0 * u, y: dyy - 4.0 * u, w: dwid, h: 12.0 * u }));
        g.rect_c(x + (5.0 * u - s) / 2.0, dyy + (5.0 * u - s) / 2.0, s, s, if on { AMBER } else if k == ps.car { rgba(170, 240, 150, 0.8) } else { DIM }); }
    if gar.hint < 0.99 && gar.anim >= 1.0 { let h = "DRAG TO SPIN"; let al = 0.5 + 0.3 * (time * 2.0).sin(); g.text(h, (ax0 + 18.0 * u + ax1) / 2.0 - text_w(h, u) / 2.0, dyy - 12.0 * u, u, fade(DIM, al * (1.0 - gar.hint)), None); }

    // the big button: SELECT this car, or DRIVE once it is yours
    let mine = gar.sel == ps.car; let label = if mine { "DRIVE" } else { "SELECT" };
    let (sw, sh) = (js_round(100.0 * u), js_round(20.0 * u)); let sx = js_round(lw - pr - sw + (1.0 - a) * 60.0 * u); let sy = js_round(lh - pb - sh - 2.0 * u);
    let f = fl(gar, "g_sel"); let pulse = 0.5 + 0.5 * (time * 3.0).sin();
    gar.rects.push(("g_sel".into(), Rect { x: sx, y: sy, w: sw, h: sh }));
    let base = if mine { rgba(120, 200, 110, 0.85) } else { rgba(243, 207, 98, 0.9) };
    g.rect_c(sx + 2.0 * u, sy + 2.0 * u + f * u, sw, sh, rgba(0, 0, 0, 0.45));
    g.panel(sx, sy + f * 2.0 * u, sw, sh, u, mix(base, INK, f * 0.5 + pulse * 0.12), Some(rgba(255, 250, 225, 0.9)));
    let lx = sx + (sw - text_w(label, 2.0 * u) - 10.0 * u) / 2.0; g.text(label, lx, sy + 3.0 * u + f * 2.0 * u, 2.0 * u, rgba(20, 22, 18, 1.0), None);
    let chev = lx + text_w(label, 2.0 * u) + 4.0 * u + pulse * 2.0 * u; for q in 0..2 { g.arrow(chev + q as f64 * 4.0 * u, sy + sh / 2.0 + f * 2.0 * u, 1.0, 4.0 * u, u, rgba(20, 22, 18, if q == 0 { 1.0 } else { 0.5 })); }
}
