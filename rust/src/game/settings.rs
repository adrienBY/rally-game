//! The settings screen: a pixel panel that slides in over the game, with tabs, springy toggles, block sliders you can drag,
//! sliding value pickers and an on-screen keyboard for your online name. Every change applies at once and is saved.
//! Opening it while driving alone pauses the game (an online race keeps running).
use super::hud::*;
use super::{js_round, Game, Persist, Rect, State, PIX};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq)]
enum Kind { Slider(i32), Toggle, Choice(&'static [&'static str]), Name }
struct Item { key: &'static str, label: &'static str, kind: Kind }

const HAPTICS: &[&str] = &["OFF", "LIGHT", "MEDIUM", "STRONG"];
const PIXELS: &[&str] = &["CHUNKY", "CLASSIC", "FINE"];
const UNITS: &[&str] = &["KM/H", "MPH"];
const VIEWS: &[&str] = &["CHASE", "CLOSE", "BONNET"];
const PARTICLES: &[&str] = &["OFF", "LOW", "FULL"];
const RIVALS: &[&str] = &["EASY", "ADAPTIVE", "HARD"];
const TABS: [(&str, &[Item]); 5] = [
    ("SOUND", &[Item { key: "vol", label: "MASTER VOLUME", kind: Kind::Slider(10) }, Item { key: "rvol", label: "RIVAL ENGINES", kind: Kind::Slider(10) }, Item { key: "hap", label: "HAPTICS", kind: Kind::Choice(HAPTICS) }]),
    ("DISPLAY", &[Item { key: "pix", label: "PIXEL SIZE", kind: Kind::Choice(PIXELS) }, Item { key: "rays", label: "SUN RAYS", kind: Kind::Toggle }, Item { key: "parts", label: "DUST AND SMOKE", kind: Kind::Choice(PARTICLES) }, Item { key: "units", label: "SPEED IN", kind: Kind::Choice(UNITS) }]),
    ("DRIVING", &[Item { key: "cam", label: "VIEW", kind: Kind::Choice(VIEWS) }, Item { key: "shake", label: "CAMERA SHAKE", kind: Kind::Toggle }, Item { key: "damage", label: "CRASH DAMAGE", kind: Kind::Toggle }]),
    ("RACE", &[Item { key: "rivals", label: "RIVALS", kind: Kind::Choice(RIVALS) }, Item { key: "rlaps", label: "RACE LAPS", kind: Kind::Slider(5) }, Item { key: "name", label: "ONLINE NAME", kind: Kind::Name }, Item { key: "olaps", label: "LAPS WHEN HOSTING", kind: Kind::Slider(5) }]),
    ("SYSTEM", &[Item { key: "saver", label: "BATTERY SAVER 30 FPS", kind: Kind::Toggle }, Item { key: "mrace", label: "RACE BEHIND THE MENU", kind: Kind::Toggle }, Item { key: "debug", label: "TEL AND REC BUTTONS", kind: Kind::Toggle }]),
];
const KB: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
pub const NAME_MAX: usize = 12;

#[derive(Default)]
pub struct Settings {
    pub open: bool, anim: f64, tab: usize, tab_y: f64, content_t: f64, content_dir: f64,
    springs: HashMap<&'static str, (f64, f64)>, flash: HashMap<String, f64>, slide: HashMap<&'static str, (f64, f64)>,
    drag: Option<(i32, &'static str, Rect, i32)>, pub kb: Option<String>, last_t: f64, rects: Vec<(String, Rect)>,
    /// opened just to type the name (from the online screen): DONE or CANCEL closes the whole screen again
    kb_only: bool,
}
impl Settings {
    /// still on screen (open, or sliding away)
    pub fn visible(&self) -> bool { self.open || self.anim > 0.001 }
}

fn ease_out(t: f64) -> f64 { let t = t.clamp(0.0, 1.0); 1.0 - (1.0 - t).powi(3) }
fn mix(a: Col, b: Col, k: f64) -> Col { let k = k.clamp(0.0, 1.0) as f32; Col { r: a.r + (b.r - a.r) * k, g: a.g + (b.g - a.g) * k, b: a.b + (b.b - a.b) * k, a: a.a + (b.a - a.a) * k } }
fn fade(c: Col, a: f64) -> Col { Col { a: c.a * a.clamp(0.0, 1.0) as f32, ..c } }
const DARK: Col = rgba(10, 14, 10, 0.95);
const DIM: Col = rgba(233, 236, 226, 0.55);

/// current value of a setting (slider steps, choice index, toggle 0/1)
fn value(ps: &Persist, cam: u8, key: &str) -> i32 {
    match key { "vol" => ps.vol, "rvol" => ps.rvol, "hap" => ps.hap, "pix" => ps.pix_idx as i32, "rays" => ps.rays as i32, "units" => ps.mph as i32,
        "cam" => cam as i32, "shake" => ps.shake as i32, "olaps" => ps.online_laps as i32, "saver" => ps.saver as i32, "debug" => ps.debug as i32,
        "parts" => ps.particles, "damage" => ps.damage as i32, "rivals" => ps.rivals, "rlaps" => ps.race_laps, "mrace" => ps.menu_race as i32, _ => 0 }
}

impl Game {
    pub fn settings_paused(&self) -> bool { self.set.open && self.state == State::Drive && !self.race.online }
    pub fn open_settings(&mut self) { self.touches.clear(); self.set.open = true; self.set.kb = None; self.set.drag = None; self.set.kb_only = false; self.set.tab_y = self.set.tab as f64; }
    pub fn close_settings(&mut self) { if self.set.open && self.wipe_close(super::menu::Go::CloseSettings) { return; } self.close_settings_now(); }
    pub fn close_settings_now(&mut self) { self.set.open = false; self.set.kb = None; self.set.drag = None; self.set.kb_only = false; self.touches.clear(); }
    /// the name keyboard on its own (online screen): the settings screen opens on PLAYER with the keyboard up
    /// the driver name, typed on the phone's own keyboard (the app or page shows a text box and answers with set_name)
    pub fn edit_name(&mut self) { self.ask_name(false); }
    /// `first`: the first run, the box can't be dismissed without a name
    pub fn ask_name(&mut self, first: bool) { let n: String = self.ps.name.chars().filter(|c| *c != '\n').collect(); self.events.push(format!("name {} {}", if first { 1 } else { 0 }, n)); }
    /// a name came back from the keyboard: letters, digits, spaces and a few marks the pixel font has, up to NAME_MAX
    pub fn set_name(&mut self, raw: &str) {
        let n: String = raw.to_uppercase().chars().filter(|c| c.is_ascii_alphanumeric() || " .-'!".contains(*c)).take(NAME_MAX).collect(); let n = n.trim().to_string();
        if n.is_empty() { return; }
        self.ps.name = n.clone(); self.ps.saves.set("fogwood-name", n); self.ps.saves.set("fogwood-name-set", "1".into()); self.ps.saves.flush(); self.buzz("shift", 0.6);
        let name = self.ps.name.clone(); crate::net::with(|s| s.name = name);
    }

    fn set_value(&mut self, key: &'static str, v: i32) {
        let ps = &mut self.ps;
        match key {
            "vol" => { ps.vol = v.clamp(0, 10); ps.sound_on = ps.vol > 0; }
            "rvol" => ps.rvol = v.clamp(0, 10),
            "hap" => { ps.hap = v.clamp(0, 3); ps.vib_on = ps.hap > 0; }
            "pix" => { ps.pix_idx = v.clamp(0, PIX.len() as i32 - 1) as usize; ps.downgraded = true; let (w, h, i) = (self.w, self.h, self.inset_px); self.resize(w, h, i); }
            "rays" => ps.rays = v != 0, "units" => ps.mph = v != 0, "shake" => ps.shake = v != 0, "saver" => ps.saver = v != 0, "debug" => ps.debug = v != 0,
            "olaps" => ps.online_laps = v.clamp(1, 5) as u8,
            "parts" => ps.particles = v.clamp(0, 2), "damage" => ps.damage = v != 0, "rivals" => ps.rivals = v.clamp(0, 2), "rlaps" => ps.race_laps = v.clamp(1, 5), "mrace" => ps.menu_race = v != 0,
            "cam" => { self.cam.mode = v.rem_euclid(3); self.cam.init = false; }
            _ => {}
        }
        self.ps.cam_mode = self.cam.mode as u8; self.ps.store();
    }

    /// touches while the screen is open; true when the settings took them
    pub fn settings_touch(&mut self, action: i32, id: i32, x: f64, y: f64) -> bool {
        if !self.set.open { return false; }
        let hit = |s: &Settings, x: f64, y: f64| -> Option<String> { s.rects.iter().rev().find(|(_, r)| x >= r.x - 2.0 && x <= r.x + r.w + 2.0 && y >= r.y - 2.0 && y <= r.y + r.h + 2.0).map(|(k, _)| k.clone()) };
        match action {
            0 => {
                let Some(k) = hit(&self.set, x, y) else { return true };
                self.set.flash.insert(k.clone(), 0.0);
                if k == "bg" || k == "close" { self.close_settings(); self.buzz("tap", 0.5); return true; }
                if k == "panel" { return true; }
                if let Some(t) = k.strip_prefix("tab").and_then(|t| t.parse::<usize>().ok()) {
                    if t != self.set.tab { self.set.content_dir = if t > self.set.tab { 1.0 } else { -1.0 }; self.set.tab = t; self.set.content_t = 0.0; self.set.kb = None; self.buzz("tap", 0.45); } return true; }
                if let Some(mut name) = self.set.kb.clone() {
                    match k.as_str() { "kb:del" => { name.pop(); } "kb:ok" => { let n = name.trim().to_string(); if !n.is_empty() { self.ps.name = n.clone(); self.ps.saves.set("fogwood-name", n); } self.set.kb = None; self.buzz("shift", 0.6); if self.set.kb_only { self.close_settings(); } return true; }
                        "kb:cancel" => { self.set.kb = None; self.buzz("tap", 0.4); if self.set.kb_only { self.close_settings(); } return true; }
                        s if s.starts_with("kb:") => { if name.len() < NAME_MAX { name.push_str(&s[3..]); } }
                        _ => return true }
                    self.set.kb = Some(name); self.buzz("tap", 0.35); return true;
                }
                for (_, items) in TABS.iter() { for it in items.iter() {
                    let cur = value(&self.ps, self.cam.mode as u8, it.key);
                    match it.kind {
                        Kind::Toggle if k == it.key => { self.set_value(it.key, 1 - cur); self.buzz("shift", if cur == 0 { 0.6 } else { 0.4 }); return true; }
                        Kind::Name if k == it.key => { self.set.kb = Some(self.ps.name.clone()); self.buzz("tap", 0.45); return true; }
                        Kind::Choice(opts) => { let n = opts.len() as i32;
                            let d = if k == format!("{}:-", it.key) { -1 } else if k == format!("{}:+", it.key) || k == it.key { 1 } else { 0 };
                            if d != 0 { self.set_value(it.key, (cur + d).rem_euclid(n)); self.set.slide.insert(it.key, (0.0, d as f64)); self.buzz("tap", 0.5);
                                if it.key == "hap" && self.ps.hap > 0 { self.buzz("impact", 0.7); } return true; } }
                        Kind::Slider(max) if k == format!("{}:s", it.key) => {
                            let r = self.set.rects.iter().find(|(q, _)| *q == k).map(|q| q.1).unwrap(); self.set.drag = Some((id, it.key, r, max)); self.slider_at(x); return true; }
                        _ => {}
                    } } }
                true
            }
            1 => { if self.set.drag.map(|d| d.0 == id).unwrap_or(false) { self.slider_at(x); } true }
            _ => { if self.set.drag.map(|d| d.0 == id).unwrap_or(false) { self.set.drag = None; self.buzz("tap", 0.3); } true }
        }
    }
    /// test hook: press the control named `key` (as drawn last frame); `frac` picks a point along it (sliders)
    pub fn settings_tap_key(&mut self, key: &str, frac: f64) -> bool {
        let Some(r) = self.set.rects.iter().find(|(k, _)| k == key).map(|q| q.1) else { return false };
        let (x, y) = (r.x + r.w * frac, r.y + r.h / 2.0); self.settings_touch(0, 99, x, y); self.settings_touch(2, 99, x, y); true
    }
    fn slider_at(&mut self, x: f64) {
        let Some((_, key, r, max)) = self.set.drag else { return };
        let min = if key == "olaps" { 1 } else { 0 };
        let v = (((x - r.x) / r.w) * max as f64).ceil().clamp(min as f64, max as f64) as i32;
        if v != value(&self.ps, self.cam.mode as u8, key) { self.set_value(key, v); self.buzz("tap", 0.25 + 0.04 * v as f64); }
    }
}

/// Draw the screen (and advance its animations). Records hit areas for `settings_touch`.
pub fn draw(set: &mut Settings, ps: &Persist, cam: u8, paused: bool, g: &mut Canvas, lw: f64, lh: f64, u: f64, time: f64) {
    let dt = (time - set.last_t).clamp(0.0, 0.05); set.last_t = time;
    set.anim = (set.anim + if set.open { dt / 0.24 } else { -dt / 0.18 }).clamp(0.0, 1.0);
    set.rects.clear();
    if set.anim <= 0.001 { return; }
    let a = ease_out(set.anim);
    set.tab_y += (set.tab as f64 - set.tab_y) * (1.0 - (-dt * 20.0).exp());
    set.content_t = (set.content_t + dt / 0.2).min(1.0);
    for v in set.flash.values_mut() { *v += dt; } set.flash.retain(|_, v| *v < 0.4);
    for v in set.slide.values_mut() { v.0 = (v.0 + dt / 0.18).min(1.0); }
    // springs follow each toggle and slider to its value with a little overshoot
    for (_, items) in TABS.iter() { for it in items.iter() {
        let target = match it.kind { Kind::Toggle => value(ps, cam, it.key) as f64, Kind::Slider(m) => value(ps, cam, it.key) as f64 / m as f64, _ => continue };
        let s = set.springs.entry(it.key).or_insert((target, 0.0)); let (k, c) = (260.0, 15.0); let sub = 4;
        for _ in 0..sub { let h = dt / sub as f64; s.1 += ((target - s.0) * k - s.1 * c) * h; s.0 += s.1 * h; } } }
    let fl = |set: &Settings, k: &str| set.flash.get(k).map(|t| 1.0 - t / 0.4).unwrap_or(0.0);

    // backdrop and panel (slides in from the right)
    g.rect_c(0.0, 0.0, lw, lh, rgba(4, 6, 4, (0.6 * a) as f32));
    set.rects.push(("bg".into(), Rect { x: 0.0, y: 0.0, w: lw, h: lh }));
    let pw = js_round((lw - 12.0 * u).min(300.0 * u)); let ph = js_round((lh - 12.0 * u).min(178.0 * u));
    let px0 = js_round((lw - pw) / 2.0); let py = js_round((lh - ph) / 2.0);
    let px = js_round(px0 + (1.0 - a) * (lw - px0 + 6.0 * u));
    set.rects.push(("panel".into(), Rect { x: px, y: py, w: pw, h: ph }));
    g.panel(px, py, pw, ph, u, DARK, Some(AMBER));
    // header with rally hazard stripes and a glint running along them
    g.tx("SETTINGS", px + 8.0 * u, py + 6.0 * u, 2.0 * u, AMBER);
    if paused { let l = "PAUSED"; let w = text_w(l, u) + 6.0 * u; let x = px + 8.0 * u + text_w("SETTINGS", 2.0 * u) + 8.0 * u; g.panel(x, py + 7.0 * u, w, 10.0 * u, u, rgba(216, 72, 58, 0.35), Some(RED)); g.tx(l, x + 3.0 * u, py + 9.0 * u, u, INK); }
    let cw_ = 13.0 * u; let cx_ = px + pw - cw_ - 6.0 * u; let cy_ = py + 5.0 * u; let cf = fl(set, "close");
    set.rects.push(("close".into(), Rect { x: cx_, y: cy_, w: cw_, h: cw_ }));
    g.panel(cx_, cy_ + cf * u, cw_, cw_, u, mix(PANEL, rgba(243, 207, 98, 0.5), cf), Some(EDGE)); g.tx("X", cx_ + 4.0 * u, cy_ + 3.0 * u + cf * u, u, INK);
    let sy = py + 22.0 * u; let sw = 6.0 * u; let mut sx = px + u; let glint = (time * 0.6).fract() * (pw + 40.0 * u) - 20.0 * u;
    while sx < px + pw - u { let k = ((sx - px) / sw) as i64; let w = sw.min(px + pw - u - sx);
        let base = if k % 2 == 0 { rgba(243, 207, 98, 0.85) } else { rgba(20, 24, 18, 0.9) };
        let gl = (1.0 - ((sx - px - glint).abs() / (18.0 * u))).max(0.0); g.rect_c(sx, sy, w, 2.0 * u, mix(base, INK, gl * 0.6)); sx += sw; }
    // tabs
    let (tx, ty0, tw, th, tg) = (px + 6.0 * u, sy + 6.0 * u, 64.0 * u, 16.0 * u, 3.0 * u);
    let iy = js_round(ty0 + set.tab_y * (th + tg));
    g.panel(tx, iy, tw, th, u, rgba(243, 207, 98, 0.92), None);
    for (i, (name, _)) in TABS.iter().enumerate() {
        let y = ty0 + i as f64 * (th + tg); let on = (set.tab_y - i as f64).abs() < 0.5; let k = format!("tab{}", i); let f = fl(set, &k);
        set.rects.push((k, Rect { x: tx, y, w: tw, h: th }));
        if !on { g.panel(tx, y, tw, th, u, mix(rgba(233, 236, 226, 0.05), rgba(243, 207, 98, 0.35), f), None); }
        let c = if on { rgba(14, 16, 10, 1.0) } else { INK };
        if on { g.arrow(tx + 6.0 * u, y + th / 2.0 - 0.5 * u, 1.0, 3.0 * u, u, c); }
        g.text(name, tx + 12.0 * u, y + 5.0 * u, u, c, if on { None } else { Some(SHADOW) });
    }
    // content (slides and fades in when the tab changes)
    let cx0 = tx + tw + 9.0 * u; let cw = px + pw - 7.0 * u - cx0; let ct = ease_out(set.content_t);
    let cx = js_round(cx0 + (1.0 - ct) * 14.0 * u * set.content_dir); let al = 0.25 + 0.75 * ct;
    let ry0 = sy + 6.0 * u; let rh = 24.0 * u;
    if let Some(name) = set.kb.clone() {
        // keyboard for the online name
        g.tx("ONLINE NAME", cx, ry0 + 2.0 * u, u, fade(DIM, al));
        let blink = if (time * 2.2).fract() < 0.55 { "-" } else { " " };
        let shown = format!("{}{}", name, if name.len() < NAME_MAX { blink } else { "" }); g.tx(&shown, cx, ry0 + 11.0 * u, 2.0 * u, fade(AMBER, al));
        let cols = 9.0; let kg = 2.0 * u; let kw = ((cw - (cols - 1.0) * kg) / cols).floor(); let kh = 12.0 * u; let ky0 = ry0 + 30.0 * u;
        for (i, ch) in KB.chars().enumerate() { let (c, r) = ((i % 9) as f64, (i / 9) as f64); let x = cx + c * (kw + kg); let y = ky0 + r * (kh + kg);
            let k = format!("kb:{}", ch); let f = fl(set, &k); set.rects.push((k, Rect { x, y, w: kw, h: kh }));
            g.panel(x, y + f * u, kw, kh, u, mix(rgba(233, 236, 226, 0.08), rgba(243, 207, 98, 0.7), f), Some(fade(EDGE, al)));
            let s = ch.to_string(); g.text(&s, x + (kw - text_w(&s, u)) / 2.0, y + 3.0 * u + f * u, u, fade(INK, al), None); }
        let y = ky0 + 4.0 * (kh + kg); let bw3 = ((cw - 2.0 * kg) / 3.0).floor();
        for (j, (k, l)) in [("kb:cancel", "CANCEL"), ("kb:del", "DELETE"), ("kb:ok", "DONE")].iter().enumerate() { let x = cx + j as f64 * (bw3 + kg); let f = fl(set, k);
            set.rects.push((k.to_string(), Rect { x, y, w: bw3, h: kh }));
            let hot = *k == "kb:ok"; g.panel(x, y + f * u, bw3, kh, u, mix(if hot { rgba(243, 207, 98, 0.3) } else { PANEL }, rgba(243, 207, 98, 0.7), f), Some(if hot { AMBER } else { EDGE }));
            g.text(l, x + (bw3 - text_w(l, u)) / 2.0, y + 3.0 * u + f * u, u, if hot { AMBER } else { INK }, None); }
        return;
    }
    let items = TABS[set.tab].1; let ww = (cw * 0.52).min(118.0 * u).floor(); let wx = cx + cw - ww;
    for (n, it) in items.iter().enumerate() {
        let y = ry0 + n as f64 * rh; let cur = value(ps, cam, it.key);
        let rf = [it.key.to_string(), format!("{}:-", it.key), format!("{}:+", it.key), format!("{}:s", it.key)].iter().map(|k| fl(set, k)).fold(0.0, f64::max);
        if rf > 0.0 { g.rect_c(cx - 3.0 * u, y - 2.0 * u, cw + 6.0 * u, rh - 4.0 * u, rgba(243, 207, 98, (0.14 * rf * al) as f32)); }
        g.text(it.label, cx, y + 5.0 * u, u, fade(INK, al), Some(SHADOW));
        let wy = y + 2.0 * u; let wh = 13.0 * u;
        match it.kind {
            Kind::Slider(max) => {
                let sp = set.springs.get(it.key).map(|s| s.0).unwrap_or(0.0); let drag = set.drag.map(|d| d.1 == it.key).unwrap_or(false);
                let lab = if it.key == "olaps" || it.key == "rlaps" { format!("{} {}", cur, if cur == 1 { "LAP" } else { "LAPS" }) } else if cur == 0 { "MUTED".to_string() } else { format!("{}/10", cur) }; g.text(&lab, cx, y + 14.0 * u, u, fade(AMBER, al), None);
                let gp = u; let bwd = ((ww - (max as f64 - 1.0) * gp) / max as f64).floor(); let bh = if drag { 11.0 * u } else { 8.0 * u }; let by = wy + (wh - bh) / 2.0;
                set.rects.push((format!("{}:s", it.key), Rect { x: wx, y: wy - 3.0 * u, w: (bwd + gp) * max as f64, h: wh + 6.0 * u }));
                for b in 0..max { let bx = wx + b as f64 * (bwd + gp); let fill = (sp * max as f64 - b as f64).clamp(0.0, 1.0);
                    g.rect_c(bx, by, bwd, bh, fade(rgba(233, 236, 226, 0.12), al));
                    if fill > 0.0 { let hgt = js_round(bh * fill.min(1.0)); let col = mix(rgba(243, 207, 98, 0.9), INK, if drag { 0.25 } else { 0.0 }); g.rect_c(bx, by + bh - hgt, bwd, hgt, fade(col, al)); } }
                // grip at the current end
                let gx = js_round(wx + sp.clamp(0.0, 1.0) * ((bwd + gp) * max as f64 - gp) - u); g.rect_c(gx, by - 2.0 * u, 2.0 * u, bh + 4.0 * u, fade(INK, al));
            }
            Kind::Toggle => {
                let sp = set.springs.get(it.key).map(|s| s.0).unwrap_or(0.0); let tw2 = 30.0 * u; let tx2 = wx + ww - tw2;
                set.rects.push((it.key.to_string(), Rect { x: wx, y: wy - 2.0 * u, w: ww, h: wh + 4.0 * u }));
                g.panel(tx2, wy, tw2, wh, u, fade(mix(rgba(233, 236, 226, 0.1), rgba(243, 207, 98, 0.85), sp), al), Some(fade(mix(EDGE, AMBER, sp), al)));
                let kw2 = 11.0 * u; let kx = js_round(tx2 + 1.0 * u + sp * (tw2 - kw2 - 2.0 * u)); g.rect_c(kx, wy + u, kw2, wh - 2.0 * u, fade(INK, al)); g.rect_c(kx + 2.0 * u, wy + 3.0 * u, kw2 - 4.0 * u, wh - 6.0 * u, fade(mix(rgba(60, 64, 56, 1.0), rgba(243, 207, 98, 1.0), sp), al));
                let st = if cur != 0 { "ON" } else { "OFF" }; g.text(st, tx2 - text_w(st, u) - 5.0 * u, wy + 3.0 * u, u, fade(if cur != 0 { AMBER } else { DIM }, al), None);
            }
            Kind::Choice(opts) => {
                let aw = 13.0 * u;
                for (s, xa, dir) in [(":-", wx, -1.0), (":+", wx + ww - aw, 1.0)] { let k = format!("{}{}", it.key, s); let f = fl(set, &k);
                    set.rects.push((k, Rect { x: xa, y: wy - 2.0 * u, w: aw, h: wh + 4.0 * u }));
                    g.panel(xa, wy + f * u, aw, wh, u, mix(rgba(233, 236, 226, 0.08), rgba(243, 207, 98, 0.7), f), Some(fade(EDGE, al))); g.arrow(xa + aw / 2.0 + dir * 0.5 * u, wy + wh / 2.0 + f * u - 0.5 * u, dir, 3.0 * u, u, fade(INK, al)); }
                set.rects.push((it.key.to_string(), Rect { x: wx + aw, y: wy, w: ww - 2.0 * aw, h: wh }));
                let (st, dir) = set.slide.get(it.key).copied().unwrap_or((1.0, 0.0)); let e = ease_out(st);
                let lab = opts[cur.clamp(0, opts.len() as i32 - 1) as usize]; let lx = wx + (ww - text_w(lab, u)) / 2.0 + (1.0 - e) * 10.0 * u * dir;
                g.text(lab, js_round(lx), wy + 3.0 * u, u, fade(AMBER, al * (0.3 + 0.7 * e)), Some(SHADOW));
                // position dots
                let n = opts.len() as f64; let dw = 2.0 * u; let dx0 = wx + (ww - (n * dw + (n - 1.0) * dw)) / 2.0;
                for i in 0..opts.len() { g.rect_c(dx0 + i as f64 * 2.0 * dw, wy + wh + 2.0 * u, dw, u, fade(if i as i32 == cur { AMBER } else { rgba(233, 236, 226, 0.25) }, al)); }
            }
            Kind::Name => {
                set.rects.push((it.key.to_string(), Rect { x: wx, y: wy, w: ww, h: wh }));
                let f = fl(set, it.key); g.panel(wx, wy + f * u, ww, wh, u, mix(PANEL, rgba(243, 207, 98, 0.5), f), Some(fade(AMBER, al)));
                g.text(&ps.name, wx + 5.0 * u, wy + 3.0 * u + f * u, u, fade(AMBER, al), None); let e = "EDIT"; g.text(e, wx + ww - text_w(e, u) - 5.0 * u, wy + 3.0 * u + f * u, u, fade(DIM, al), None);
            }
        }
    }
    let hint = "CHANGES SAVE AUTOMATICALLY"; g.text(hint, cx, py + ph - 11.0 * u, u, rgba(233, 236, 226, (0.35 * al) as f32), None);
}
