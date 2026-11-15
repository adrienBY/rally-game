//! The main menu, arcade style: chrome FOGWOOD lettering over the attract-mode race, a left-aligned list with a blinking
//! cursor (the selected entry shows its setting in a framed box), the stage and its record at the bottom.
//! Feel: the logo slides in and the entries follow one by one; a tapped entry blinks before it acts; going to a stage is
//! a chequered wipe (squares sweep across, the stage starts under cover, they sweep away); building a stage shows the
//! loading screen (stage card, scrolling chevron bar), which wipes in from the menu and the new stage's menu wipes out of.
use super::hud::*;
use super::{fmt, js_round, Game, Rect, State};

/// the stages, in menu order: map key, number, name, surface
pub const STAGES: [(&str, &str, &str, &str); 6] = [
    ("forest", "SS1", "FOGWOOD", "GRAVEL"), ("snow", "SS2", "COL DE GIVRE", "SNOW"), ("canyon", "SS3", "CANYON ROUGE", "SAND"),
    ("lakes", "SS4", "LAKELAND", "GRAVEL"), ("touge", "SS5", "MOMIJI TOUGE", "TARMAC"), ("riviera", "SS6", "RIVIERA CORNICHE", "TARMAC"),
];
pub fn stage_of(map: &str) -> usize { STAGES.iter().position(|s| s.0 == map).unwrap_or(0) }

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Item { Drive, Stage, Race, Online, Garage, Options }
pub const ITEMS: [(Item, &str, &str); 6] = [
    (Item::Drive, "DRIVE", "m_drive"), (Item::Stage, "STAGE", "m_stage"), (Item::Race, "RACE", "m_race"),
    (Item::Online, "ONLINE", "m_online"), (Item::Garage, "GARAGE", "m_garage"), (Item::Options, "OPTIONS", "m_opts"),
];
/// every hit key the menu draws (entries plus the stage arrows)
pub const KEYS: [&str; 8] = ["m_drive", "m_stage", "m_race", "m_online", "m_garage", "m_opts", "m_sprev", "m_snext"];

/// the chrome of the logo, top row to bottom: sky, a dark horizon line, sunset
const CHROME: [Col; 7] = [hex(0xffffff), hex(0xe4f4ff), hex(0xb4e0ff), hex(0x6fa8e8), hex(0xffd84a), hex(0xffa23a), hex(0xff6a1a)];
const RALLY: [Col; 7] = [hex(0xff6a5a), hex(0xff4a3a), hex(0xff3b2f), hex(0xe6322a), hex(0xd02a24), hex(0xb8231f), hex(0xa01e1b)];
const DIM_ITEM: Col = rgba(143, 154, 184, 1.0);
const NIGHT: Col = rgba(4, 6, 20, 1.0);

#[derive(Default)]
pub struct Menu {
    /// selected entry, the cursor's drawn row (eases to it), time on the menu
    pub cur: usize, cur_y: f64, pub t: f64,
    /// a tapped entry blinking before it acts: which, time since
    pub confirm: Option<(usize, f64)>,
    /// the chequered wipe into a stage: time since it began, race or not; done once the stage started
    pub wipe: Option<(f64, bool, bool)>,
    /// reveal of a freshly built stage's menu (from the loading screen), and time since loading began
    pub reveal: f64, pub load_t: f64,
}

/// wipe timing: cover, hold, uncover (s)
const W_IN: f64 = 0.42; const W_HOLD: f64 = 0.12; const W_OUT: f64 = 0.42;

impl Game {
    /// once a frame, before the stage lock is taken: menu timers, and the actions they trigger
    pub fn menu_step(&mut self, dt: f64) {
        let m = &mut self.menu; m.t += dt; m.reveal += dt;
        if self.loading.is_some() { m.load_t += dt; } else { m.load_t = 0.0; }
        if self.state != State::Title { m.t = 0.0; m.confirm = None; }
        if let Some((k, t)) = self.menu.confirm { let t = t + dt;
            if t < 0.3 { self.menu.confirm = Some((k, t)); } else { self.menu.confirm = None; self.menu_act(ITEMS[k].0); } }
        if let Some((t, race, started)) = self.menu.wipe { let t = t + dt;
            if !started && t >= W_IN + W_HOLD * 0.5 { self.ps.race_mode = race; self.menu.wipe = Some((t, race, true)); self.start_game(); }
            else if t >= W_IN + W_HOLD + W_OUT { self.menu.wipe = None; }
            else { self.menu.wipe = Some((t, race, started)); } }
    }
    fn menu_act(&mut self, it: Item) {
        match it {
            Item::Drive => { self.menu.wipe = Some((0.0, false, false)); self.buzz("shift", 0.6); }
            Item::Race => { self.menu.wipe = Some((0.0, true, false)); self.buzz("shift", 0.6); }
            Item::Stage => { self.menu_stage(1); }
            Item::Online => { self.online_tap("online"); }
            Item::Garage => { self.open_garage(); }
            Item::Options => { self.open_settings(); }
        }
    }
    /// the next / previous stage: builds it (loading screen)
    pub fn menu_stage(&mut self, d: i32) {
        let n = STAGES.len() as i32; let k = ((stage_of(&self.rt.map) as i32 + d) % n + n) % n; let want = STAGES[k as usize].0;
        if want != self.rt.map { self.switch_map(want); }
    }
    /// a tap on a menu key; true when it was one
    pub fn menu_tap(&mut self, key: &str) -> bool {
        if self.menu.wipe.is_some() { return true; }
        match key {
            "m_sprev" => { self.menu.cur = 1; self.menu_stage(-1); true }
            "m_snext" => { self.menu.cur = 1; self.menu_stage(1); true }
            k => match ITEMS.iter().position(|i| i.2 == k) { Some(i) => { self.menu.cur = i; if self.menu.confirm.is_none() { self.menu.confirm = Some((i, 0.0)); } true } None => false },
        }
    }
}

fn ease_out(t: f64) -> f64 { let t = t.clamp(0.0, 1.0); 1.0 - (1.0 - t).powi(3) }

/// the waving chequered flag on its pole (one pixel per `p`)
fn flag(g: &mut Canvas, x: f64, y: f64, p: f64, time: f64) {
    g.rect_c(x + p, y + p, p, 15.0 * p, rgba(0, 0, 0, 0.55)); g.rect_c(x, y, p, 15.0 * p, hex(0x8a7356));
    let wave = ((time * 3.0) as i64) % 2;
    for cx in 0..4 { let dy = if (cx as i64 + wave) % 2 == 0 { 0.0 } else { p };
        for cy in 0..3 { let c = if (cx + cy) % 2 == 0 { hex(0xffffff) } else { hex(0x141414) };
            g.rect_c(x + p + cx as f64 * 4.0 * p + p, y + cy as f64 * 3.0 * p + dy + p, 4.0 * p, 3.0 * p, rgba(0, 0, 0, 0.45));
            g.rect_c(x + p + cx as f64 * 4.0 * p, y + cy as f64 * 3.0 * p + dy, 4.0 * p, 3.0 * p, c); } }
}
/// a small gold trophy (9 x 9 pixels)
fn trophy(g: &mut Canvas, x: f64, y: f64, p: f64) {
    let gold = hex(0xffd84a); let hi = hex(0xfff4b0); let sh = rgba(0, 0, 0, 0.55);
    let parts: [(f64, f64, f64, f64); 8] = [(2.0, 0.0, 5.0, 4.0), (3.0, 4.0, 3.0, 1.0), (0.0, 1.0, 2.0, 1.0), (0.0, 2.0, 1.0, 2.0), (7.0, 1.0, 2.0, 1.0), (8.0, 2.0, 1.0, 2.0), (4.0, 5.0, 1.0, 2.0), (2.0, 7.0, 5.0, 2.0)];
    for (a, b, w, h) in parts { g.rect_c(x + (a + 1.0) * p, y + (b + 1.0) * p, w * p, h * p, sh); }
    for (a, b, w, h) in parts { g.rect_c(x + a * p, y + b * p, w * p, h * p, gold); }
    g.rect_c(x + 3.0 * p, y + p, p, 2.0 * p, hi);
}
/// a four-point sparkle, size in pixels `p`
fn sparkle(g: &mut Canvas, cx: f64, cy: f64, p: f64, big: bool) {
    let c = hex(0xffffff); let r = if big { 4.0 } else { 2.0 };
    g.rect_c(cx - p / 2.0, cy - r * p, p, (2.0 * r + 1.0) * p, c); g.rect_c(cx - r * p, cy - p / 2.0, (2.0 * r + 1.0) * p, p, c);
    if big { g.rect_c(cx - 1.5 * p, cy - 1.5 * p, 3.0 * p, 3.0 * p, c); }
}

/// the chequered squares of a wipe: k 0..1 covering (squares appear along a diagonal sweep), then 1..2 uncovering
pub fn wipe(g: &mut Canvas, lw: f64, lh: f64, u: f64, k: f64) {
    if k <= 0.0 || k >= 2.0 { return; }
    let s = (12.0 * u).max(6.0); let cols = (lw / s).ceil() as i32 + 1; let rows = (lh / s).ceil() as i32 + 1; let span = cols as f64 + rows as f64 * 0.6 + 2.0;
    for j in 0..rows { for i in 0..cols {
        let d = i as f64 + j as f64 * 0.6;
        let f = if k <= 1.0 { (k * span - d).clamp(0.0, 2.0) / 2.0 } else { 1.0 - ((k - 1.0) * span - d).clamp(0.0, 2.0) / 2.0 };
        if f <= 0.0 { continue; } let sz = js_round(s * f / u) * u; if sz <= 0.0 { continue; }
        let c = if (i + j) % 2 == 0 { hex(0x0b0d16) } else { hex(0xe9ece2) };
        let (x, y) = (i as f64 * s + (s - sz) / 2.0, j as f64 * s + (s - sz) / 2.0); g.rect_c(js_round(x), js_round(y), sz, sz, c);
    } }
}

/// the main menu over the attract race; records hit areas into `ui`
pub fn draw(gm: &mut Game, g: &mut Canvas, ui: &mut Vec<(&'static str, Rect)>, lw: f64, lh: f64, u: f64, pl: f64, pt: f64, pb: f64) {
    let m = &mut gm.menu; let t = m.t; let time = gm.time;
    // shade the left third so the list reads over the race
    let bands = 24; for b in 0..bands { let xa = js_round(lw * 0.6 * b as f64 / bands as f64); let xb = js_round(lw * 0.6 * (b + 1) as f64 / bands as f64); let a = 0.82 * (1.0 - b as f64 / bands as f64).powf(1.3);
        g.rect_c(xa, 0.0, xb - xa, lh, Col { a: a as f32, ..NIGHT }); }
    let x0 = js_round(pl + 10.0 * u);
    // logo: slides in, chrome FOGWOOD, red RALLY, flag, sparkles
    let sc = js_round(3.0 * u).max(2.0); let slide = (1.0 - ease_out(t / 0.35)) * 60.0 * u;
    let ly = js_round(pt + 8.0 * u); let lx = js_round(x0 - slide);
    g.text_italic("FOGWOOD", lx, ly, sc, &CHROME, 0.0, Some((rgba(0, 0, 0, 0.85), sc)));
    let fw = Canvas::italic_w("FOGWOOD", sc, 0.0);
    let sc2 = js_round(1.5 * u).max(1.0); let ry = ly + 7.0 * sc + 5.0 * u;
    g.text_italic("RALLY", lx + 2.0 * u, ry, sc2, &RALLY, 2.0, Some((rgba(0, 0, 0, 0.85), sc2)));
    let rw = Canvas::italic_w("RALLY", sc2, 2.0);
    flag(g, lx + 2.0 * u + rw + 6.0 * u, ry - 3.0 * u, (u * 0.75).max(1.0).round(), time);
    let tw = time % 2.6; if tw > 1.9 && tw < 2.2 { sparkle(g, lx + 2.0 * sc + 2.0 * sc, ly + sc, u.max(1.0), true); }
    let tw2 = (time + 1.3) % 2.6; if tw2 > 1.9 && tw2 < 2.1 { sparkle(g, lx + fw - 4.0 * sc, ly + 6.0 * sc, u.max(1.0), false); }

    // the list
    let top = ry + 7.0 * sc2 + 14.0 * u; let bottom_line = lh - pb - 10.0 * u;
    let pitch = ((bottom_line - 4.0 * u - top) / ITEMS.len() as f64).clamp(10.0 * u, 13.0 * u); let isc = u;
    let ty = top + m.cur as f64 * pitch; if m.cur_y == 0.0 { m.cur_y = ty; } m.cur_y += (ty - m.cur_y) * 0.45; if (ty - m.cur_y).abs() < u { m.cur_y = ty; }
    let blink_on = ((time / 0.35) as i64) % 2 == 0;
    let stage = stage_of(&gm.rt.map); let st = STAGES[stage];
    let car = super::cars::CARS[gm.ps.car.min(super::cars::CARS.len() - 1)].name;
    let room = crate::net::with(|s| s.code);
    for (k, (it, label, key)) in ITEMS.iter().enumerate() {
        let appear = ease_out((t - 0.18 - k as f64 * 0.06) / 0.25); if appear <= 0.0 { continue; }
        let y = js_round(top + k as f64 * pitch); let ix = js_round(x0 + 9.0 * u - (1.0 - appear) * 40.0 * u);
        let on = k == m.cur; let confirming = m.confirm.map(|c| c.0 == k).unwrap_or(false);
        let col = if confirming { if ((m.confirm.unwrap().1 / 0.06) as i64) % 2 == 0 { AMBER } else { INK } } else if on { INK } else { DIM_ITEM };
        let lab_w = text_w(label, isc);
        ui.push((*key, Rect { x: x0, y: y - 3.0 * u, w: (lab_w + 16.0 * u).max(60.0 * u), h: pitch.max(14.0 * u) }));
        g.text(label, ix, y, isc, col, Some(rgba(0, 0, 0, 0.8)));
        if on {
            let val = match it { Item::Drive => "TIME TRIAL".to_string(), Item::Stage => format!("{} {}", st.1, st.2), Item::Race => format!("5 RIVALS - {} LAPS", gm.race.laps),
                Item::Online => match room { Some(c) => format!("ROOM {:06}", c), None => "HOST OR JOIN".into() }, Item::Garage => car.to_string(), Item::Options => String::new() };
            if !val.is_empty() {
                let bx = js_round(ix + lab_w + 10.0 * u); let arrows = *it == Item::Stage; let aw = if arrows { 10.0 * u } else { 0.0 };
                let bw = text_w(&val, u) + 8.0 * u + 2.0 * aw; let bh = 11.0 * u; let by = js_round(y + 7.0 * isc / 2.0 - bh / 2.0);
                g.rect_c(bx + u, by + u, bw, bh, rgba(0, 0, 0, 0.5)); g.panel(bx, by, bw, bh, u, rgba(0, 0, 0, 0.55), Some(AMBER));
                g.text(&val, bx + 4.0 * u + aw, by + 2.0 * u, u, AMBER, None);
                if arrows { g.arrow(bx + 5.0 * u, by + bh / 2.0, -1.0, 3.0 * u, u, AMBER); g.arrow(bx + bw - 5.0 * u, by + bh / 2.0, 1.0, 3.0 * u, u, AMBER);
                    ui.push(("m_sprev", Rect { x: bx - 4.0 * u, y: by - 4.0 * u, w: aw + 6.0 * u, h: bh + 8.0 * u }));
                    ui.push(("m_snext", Rect { x: bx + bw - aw - 2.0 * u, y: by - 4.0 * u, w: aw + 6.0 * u, h: bh + 8.0 * u })); }
            }
        }
    }
    if t > 0.18 && blink_on { g.text(">", x0, js_round(m.cur_y), isc, AMBER, Some(rgba(0, 0, 0, 0.8))); }

    // bottom: the stage and its record
    if t > 0.5 {
        let line = format!("{} {}", st.1, st.2); let by = js_round(bottom_line); g.text(&line, x0, by, u, rgba(200, 208, 232, 1.0), Some(rgba(0, 0, 0, 0.8)));
        if gm.best.is_some() { let bx = x0 + text_w(&line, u) + 6.0 * u; trophy(g, bx, by - u, u); let rec = fmt(gm.best); g.text(&rec, bx + 12.0 * u, by, u, AMBER, Some(rgba(0, 0, 0, 0.8))); }
    }
    // the chequered wipe into a stage, and the reveal of a fresh menu
    if let Some((wt, _, _)) = gm.menu.wipe { let k = if wt < W_IN { wt / W_IN } else if wt < W_IN + W_HOLD { 1.0 } else { 1.0 + (wt - W_IN - W_HOLD) / W_OUT }; wipe(g, lw, lh, u, k); }
}

/// wipe overlays outside the menu itself: the second half of a stage-start wipe while driving, and a fresh stage's reveal
pub fn overlays(gm: &Game, g: &mut Canvas, lw: f64, lh: f64, u: f64) {
    if gm.state == State::Drive { if let Some((wt, _, _)) = gm.menu.wipe { let k = if wt < W_IN + W_HOLD { 1.0 } else { 1.0 + (wt - W_IN - W_HOLD) / W_OUT }; wipe(g, lw, lh, u, k); } }
    if gm.loading.is_none() && gm.menu.reveal < W_OUT { wipe(g, lw, lh, u, 1.0 + gm.menu.reveal / W_OUT); }
}

/// the loading screen while a stage is being built: wipes in over the menu, then the stage card
pub fn loading(gm: &Game, g: &mut Canvas, lw: f64, lh: f64, u: f64, pl: f64, pb: f64) {
    let lt = gm.menu.load_t; let time = lt;   // the game clock stands still while a stage is built
    if lt < W_IN { wipe(g, lw, lh, u, lt / W_IN); return; }
    let want = gm.loading.clone().unwrap_or_default(); let st = STAGES[stage_of(&want)];
    g.rect_c(0.0, 0.0, lw, lh, NIGHT);
    // speed lines streaming left
    let n = 18; for k in 0..n { let y = js_round(lh * (k as f64 + 0.5) / n as f64); let len = (40.0 + ((k * 37) % 50) as f64) * u; let sp = (90.0 + ((k * 53) % 70) as f64) * u;
        let x = lw - ((time * sp + k as f64 * 97.0 * u) % (lw + len)); g.rect_c(js_round(x), y, js_round(len), u, rgba(29, 79, 160, 0.35)); }
    // stage card
    let x0 = js_round(pl + 14.0 * u); let k = ease_out((lt - W_IN) / 0.3); let slide = (1.0 - k) * 50.0 * u;
    let sc = js_round(2.0 * u).max(2.0); let y0 = js_round(lh * 0.24);
    g.text_italic(st.1, x0 - slide, y0, sc, &RALLY, 1.0, Some((rgba(0, 0, 0, 0.9), sc)));
    let sc_n = js_round(3.0 * u).max(2.0); let name_w = Canvas::italic_w(st.2, sc_n, 0.0);
    let sc_n = if name_w > lw - x0 - 10.0 * u { js_round(2.0 * u).max(2.0) } else { sc_n };
    g.text_italic(st.2, x0 - slide * 1.4, y0 + 7.0 * sc + 6.0 * u, sc_n, &CHROME, 0.0, Some((rgba(0, 0, 0, 0.9), sc_n)));
    let info = format!("{} - STAGE {} OF 6", st.3, stage_of(&want) + 1);
    g.text(&info, x0 - slide * 1.8, y0 + 7.0 * sc + 6.0 * u + 7.0 * sc_n + 8.0 * u, u, rgba(200, 208, 232, k as f32), None);
    // the chevron bar, scrolling
    let (bx, bw, bh) = (x0, (lw - x0 * 2.0).min(220.0 * u), 9.0 * u); let by = js_round(lh - pb - 24.0 * u);
    g.rect_c(bx + u, by + u, bw, bh, rgba(0, 0, 0, 0.6)); g.panel(bx, by, bw, bh, u, rgba(10, 12, 24, 1.0), Some(EDGE));
    let cw = 8.0 * u; let off = (time * 36.0 * u) % (cw * 2.0);
    let mut cx = bx + 2.0 * u - cw * 2.0 + off; while cx < bx + bw - 2.0 * u { // chevrons pointing right, clipped to the bar
        for r in 0..((bh - 4.0 * u) / u) as i32 { let yy = by + 2.0 * u + r as f64 * u; let mid = (bh - 4.0 * u) / 2.0; let dx = (mid - (r as f64 * u - mid + u / 2.0).abs()).max(0.0);
            let xa = (cx + dx).max(bx + 2.0 * u); let xb = (cx + dx + 4.0 * u).min(bx + bw - 2.0 * u); if xb > xa { g.rect_c(js_round(xa), yy, js_round(xb - xa), u, AMBER); } }
        cx += cw * 2.0; }
    if ((time / 0.4) as i64) % 2 == 0 { g.text("LOADING", bx, by - 10.0 * u, u, INK, Some(rgba(0, 0, 0, 0.8))); }
}

/// a fresh menu starts with the wipe uncovering it (the loading screen's squares sweep away)
pub fn fresh() -> Menu { Menu { reveal: 0.0, ..Default::default() } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stages_line_up() { for (k, s) in STAGES.iter().enumerate() { assert_eq!(stage_of(s.0), k); } assert_eq!(stage_of("nowhere"), 0); }
    #[test]
    fn wipe_covers_then_clears() {
        let (w, h) = (200, 120); let mut g = Canvas::new(w, h);
        let cover = |g: &mut Canvas, k: f64| { g.clear(); wipe(g, w as f64, h as f64, 1.0, k); g.finish(); g.bytes.chunks(4).filter(|p| p[3] > 0).count() as f64 / (w * h) as f64 };
        assert!(cover(&mut g, 0.02) < 0.2); assert!(cover(&mut g, 1.0) > 0.97, "fully covered at the peak"); assert!(cover(&mut g, 1.98) < 0.2);
    }
}
