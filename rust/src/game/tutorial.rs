//! The first-run tutorial, for players who arrive from itch.io: a card over the menu shows the controls for what they hold
//! (keys on a computer, a sketch of the touch layout on a phone), then their first drive shows one short tip at a time, each
//! one cleared by doing what it says. Nothing waits on a timer it doesn't need, nothing has to be dismissed.
use super::hud::*;
use super::{Game, Rect, State};
use crate::physics::Input;
use crate::world::{World, ROAD};
use std::sync::atomic::{AtomicBool, Ordering};

/// set by the app before the game starts: show the tutorial to a player who has never been through it (the web build on itch.io)
pub static WANT: AtomicBool = AtomicBool::new(false);
/// set by the app: the device's main pointer is a finger (phones, tablets). The Android app is always touch.
pub static TOUCH_DEVICE: AtomicBool = AtomicBool::new(cfg!(not(target_arch = "wasm32")));
/// the saved key that says the tutorial was seen
const SEEN: &str = "fogwood-tut";
/// the mouse's pointer id from the web page (a click is not a finger)
pub const MOUSE_ID: i32 = 0x7fff;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Step { #[default] Off, Gas, Steer, Slide, Finish, Done }

#[derive(Default, Debug)]
pub struct Tut {
    /// the controls card waits over the menu
    pub card: bool,
    /// where the tips are (they run on the next drive once armed)
    pub step: Step,
    /// time in this step, time spent doing what it asks
    t: f64, did: f64,
    /// stuck: how long, how long the hint has shown, how often it has
    stuck: f64, hint: f64, hints: u8,
}
impl Tut {
    pub fn new(seen: bool) -> Tut { Tut { card: WANT.load(Ordering::Relaxed) && !seen, ..Default::default() } }
    fn running(&self) -> bool { matches!(self.step, Step::Gas | Step::Steer | Step::Slide | Step::Finish) }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Cap { Up, Down, Left, Right, Space, R }

/// what the drive's tip shows this frame
pub struct Tip { pub caps: &'static [Cap], pub text: &'static str, pub pulse: &'static [&'static str], pub count: Option<u8> }

impl Game {
    pub fn tut_seen(&self) -> bool { self.ps.saves.get(SEEN).is_some() }
    /// keys or a touch screen: the last thing the player used, else what the device is
    pub fn tut_touch(&self) -> bool { match self.ps.input_kb { Some(kb) => !kb, None => TOUCH_DEVICE.load(Ordering::Relaxed) } }
    /// the card is on screen: the menu is up and free (the first-run name box answered, no other screen, no wipe or loading)
    pub fn tut_card_up(&self) -> bool {
        self.ps.tut.card && self.state == State::Title && self.loading.is_none() && !self.set.open && !self.gar.open
            && self.ps.online_ui == super::online::OnlineUi::Closed && self.menu.wipe.is_none() && self.menu.view == super::menu::View::Main
            && self.menu.reveal > 0.45 && self.ps.saves.get("fogwood-name-set").is_some()
    }
    /// DRIVE from the card: a time trial on this stage, tips on
    pub fn tut_go(&mut self) {
        self.ps.tut.card = false; self.ps.tut.step = Step::Gas; self.ps.tut.t = 0.0; self.ps.tut.did = 0.0;
        self.ps.race_mode = false; self.menu.cur = 0; self.menu_drive(); self.buzz("shift", 0.6);
        self.stat("tut", &[("s", super::stats::js("card"))]);
    }
    /// the card put away without driving: the tips still come on the first drive
    pub fn tut_close_card(&mut self) { self.ps.tut.card = false; if self.ps.tut.step == Step::Off { self.ps.tut.step = Step::Gas; } }
    /// back to the menu: whatever was left of the tutorial is over
    pub fn tut_left(&mut self) { if self.ps.tut.running() { self.tut_done(); } }
    fn tut_done(&mut self) { self.ps.tut.step = Step::Done; self.ps.saves.set(SEEN, "1".into()); }
    fn tut_next(&mut self, to: Step) {
        let name = match self.ps.tut.step { Step::Gas => "gas", Step::Steer => "steer", Step::Slide => "slide", _ => "finish" };
        self.stat("tut", &[("s", super::stats::js(name))]);
        self.ps.tut.step = to; self.ps.tut.t = 0.0; self.ps.tut.did = 0.0;
        if to == Step::Done { self.tut_done(); } else { self.buzz("tap", 0.5); }
    }
    /// once a frame while driving: move the tips on when the player has done what the current one says
    pub fn tut_step(&mut self, w: &World, dt: f64, inp: &Input) {
        if !self.ps.tut.running() || self.go > 0.0 || self.race.on && self.race.phase == super::race::Phase::Grid { return; }
        let u = self.car.u.abs(); let off = self.road_dist(w, self.car.x, self.car.z) > ROAD + 5.0;
        let t = &mut self.ps.tut; t.t += dt;
        // stuck (stopped, or crawling off the road): the reset hint, a few times at most, once they have driven a little
        if t.hint > 0.0 { t.hint -= dt; if u > 6.0 && !off { t.hint = t.hint.min(0.6); } return; }
        if t.step != Step::Gas && t.hints < 3 && (u < 1.2 || off && u < 6.0) { t.stuck += dt; } else { t.stuck = (t.stuck - dt * 2.0).max(0.0); }
        if t.stuck > 2.5 { t.stuck = 0.0; t.hint = 4.5; t.hints += 1; return; }
        let doing = match t.step {
            Step::Gas => u > 7.0,
            Step::Steer => inp.steer != 0.0 && u > 3.0,
            Step::Slide => (inp.hb || inp.brake > 0.0) && u > 3.0,
            _ => false };
        if doing { t.did += dt; }
        let (step, did, tt) = (t.step, t.did, t.t);
        match step {
            Step::Gas if did > 0.3 => self.tut_next(Step::Steer),
            Step::Steer if did > 1.0 => self.tut_next(Step::Slide),
            Step::Slide if did > 0.25 || tt > 15.0 => self.tut_next(Step::Finish),
            Step::Finish if tt > 4.0 => self.tut_next(Step::Done),
            _ => {}
        }
    }
    /// the tip to show now, if any
    pub fn tut_tip(&self) -> Option<Tip> {
        let t = &self.ps.tut; if !t.running() || self.state != State::Drive || self.go > 0.0 { return None; }
        let touch = self.tut_touch();
        if t.hint > 0.0 { return Some(if touch { Tip { caps: &[], text: "STUCK? TAP RST", pulse: &["rst"], count: None } }
            else { Tip { caps: &[Cap::R], text: "BACK ON THE ROAD", pulse: &[], count: None } }); }
        Some(match (t.step, touch) {
            (Step::Gas, false) => Tip { caps: &[Cap::Up], text: "HOLD TO ACCELERATE", pulse: &[], count: Some(1) },
            (Step::Gas, true) => Tip { caps: &[], text: "HOLD GAS TO ACCELERATE", pulse: &["gas"], count: Some(1) },
            (Step::Steer, false) => Tip { caps: &[Cap::Left, Cap::Right], text: "STEER", pulse: &[], count: Some(2) },
            (Step::Steer, true) => Tip { caps: &[], text: "STEER WITH THE ARROWS", pulse: &["left", "right"], count: Some(2) },
            (Step::Slide, false) => Tip { caps: &[Cap::Space], text: "HANDBRAKE FOR HAIRPINS", pulse: &[], count: Some(3) },
            (Step::Slide, true) => Tip { caps: &[], text: "H-BRAKE FOR HAIRPINS", pulse: &["hb"], count: Some(3) },
            _ => Tip { caps: &[], text: "REACH THE FINISH TO SET A TIME", pulse: &[], count: None },
        })
    }
}

// ---------------- drawing ----------------
const CAP_H: f64 = 11.0;
fn cap_w(c: Cap) -> f64 { if c == Cap::Space { 35.0 } else { 11.0 } }
const CARD_FILL: Col = rgba(8, 12, 8, 0.9);
const DIM: Col = rgba(233, 236, 226, 0.55);

/// a key cap: a raised key with its symbol, `lit` while held; returns its width
fn cap(g: &mut Canvas, c: Cap, x: f64, y: f64, u: f64, lit: bool) -> f64 {
    let (w, h) = (cap_w(c) * u, CAP_H * u);
    g.rect_c(x + u, y + h, w - 2.0 * u, u, rgba(0, 0, 0, 0.6));   // the key's depth
    g.panel(x, y, w, h, u, if lit { rgba(243, 207, 98, 0.28) } else { rgba(233, 236, 226, 0.1) }, Some(if lit { AMBER } else { rgba(233, 236, 226, 0.8) }));
    let ink = if lit { AMBER } else { INK };
    // arrows: four rows of 1, 3, 5, 7 pixels about the key's middle pixel (column / row 5 of 11)
    for r in 0..4 { let r = r as f64; let len = (2.0 * r + 1.0) * u;
        match c {
            Cap::Up => g.rect_c(x + (5.0 - r) * u, y + (3.0 + r) * u, len, u, ink),
            Cap::Down => g.rect_c(x + (5.0 - r) * u, y + (6.0 - r) * u, len, u, ink),
            Cap::Left => g.rect_c(x + (3.0 + r) * u, y + (5.0 - r) * u, u, len, ink),
            Cap::Right => g.rect_c(x + (6.0 - r) * u, y + (5.0 - r) * u, u, len, ink),
            _ => {} } }
    let label = match c { Cap::Space => "SPACE", Cap::R => "R", _ => "" };
    if !label.is_empty() { g.text(label, x + (w - text_w(label, u)) / 2.0, y + 2.0 * u, u, ink, None); }
    w
}
fn caps_w(cs: &[Cap], u: f64) -> f64 { cs.iter().map(|c| cap_w(*c) * u).sum::<f64>() + (cs.len().max(1) - 1) as f64 * 2.0 * u }
fn blink(now: f64) -> bool { ((now / 520.0) as i64) % 2 == 0 }

/// the controls card over the menu (the menu dims behind it)
pub fn card(gm: &Game, g: &mut Canvas, lw: f64, lh: f64, u: f64, now: f64) {
    g.rect_c(0.0, 0.0, lw, lh, rgba(4, 7, 4, 0.5));
    let touch = gm.tut_touch();
    let title = "HOW TO DRIVE";
    if !touch {
        let rows: [(&[Cap], &str); 5] = [(&[Cap::Up], "GAS"), (&[Cap::Down], "BRAKE, REVERSE"), (&[Cap::Left, Cap::Right], "STEER"), (&[Cap::Space], "HANDBRAKE"), (&[Cap::R], "BACK ON THE ROAD")];
        let (cw, ch) = (166.0 * u, 134.0 * u); let (x0, y0) = (((lw - cw) / 2.0).round(), ((lh - ch) / 2.0).round());
        g.panel(x0, y0, cw, ch, u, CARD_FILL, Some(AMBER));
        g.tx(title, x0 + (cw - text_w(title, u)) / 2.0, y0 + 7.0 * u, u, AMBER);
        g.rect_c(x0 + 10.0 * u, y0 + 19.0 * u, cw - 20.0 * u, u, rgba(233, 236, 226, 0.18));
        let kx = x0 + 50.0 * u;   // keys end here, words start after
        for (n, (cs, word)) in rows.iter().enumerate() { let y = y0 + (26.0 + n as f64 * 15.0) * u;
            let mut x = kx - caps_w(cs, u); for c in cs.iter() { x += cap(g, *c, x, y, u, false) + 2.0 * u; }
            g.tx(word, kx + 8.0 * u, y + 2.0 * u, u, INK); }
        let alt = "WASD WORKS TOO"; g.text(alt, x0 + (cw - text_w(alt, u)) / 2.0, y0 + ch - 29.0 * u, u, DIM, None);
        let go = "PRESS ENTER TO DRIVE"; if blink(now) { g.tx(go, x0 + (cw - text_w(go, u)) / 2.0, y0 + ch - 14.0 * u, u, AMBER); }
    } else {
        // a sketch of this screen with its controls where they really are (the drive's layout at the same proportions)
        let (lu, hu) = (gm.lw as f64 / u, gm.lh as f64 / u); let sw = 196.0; let k = sw / lu; let sh = (hu * k).round();
        let (cw, ch) = ((sw + 24.0) * u, (sh + 52.0) * u); let (x0, y0) = (((lw - cw) / 2.0).round(), ((lh - ch) / 2.0).round());
        g.panel(x0, y0, cw, ch, u, CARD_FILL, Some(AMBER));
        g.tx(title, x0 + (cw - text_w(title, u)) / 2.0, y0 + 7.0 * u, u, AMBER);
        let (sx, sy) = (x0 + 12.0 * u, y0 + 20.0 * u);
        g.panel(sx, sy, sw * u, sh * u, u, rgba(233, 236, 226, 0.05), Some(rgba(233, 236, 226, 0.35)));
        // the road running away to the horizon, and the car on it
        let (hz, mid) = ((sh * 0.42).round(), sx + (sw * u / 2.0).round());
        let mut r = hz; while r < sh - 1.0 { let half = ((2.0 + (r - hz) / (sh - hz) * sw * 0.2) * u).round(); g.rect_c(mid - half, sy + r * u, half * 2.0, u, rgba(233, 236, 226, 0.07)); r += 1.0; }
        let cb = sy + (sh - 16.0) * u;   // the car's wheels stand here
        g.rect_c(mid - 9.0 * u, cb - 3.0 * u, 3.0 * u, 3.0 * u, rgba(10, 10, 10, 0.9)); g.rect_c(mid + 6.0 * u, cb - 3.0 * u, 3.0 * u, 3.0 * u, rgba(10, 10, 10, 0.9));
        g.rect_c(mid - 10.0 * u, cb - 9.0 * u, 20.0 * u, 6.0 * u, hex(0x2c3f86)); g.rect_c(mid - 6.0 * u, cb - 13.0 * u, 12.0 * u, 4.0 * u, hex(0x1f2d63));
        g.rect_c(mid - 5.0 * u, cb - 12.0 * u, 10.0 * u, 2.0 * u, rgba(170, 190, 210, 0.7));
        g.rect_c(mid - 9.0 * u, cb - 7.0 * u, 3.0 * u, u, RED); g.rect_c(mid + 6.0 * u, cb - 7.0 * u, 3.0 * u, u, RED);
        // the drive's rects in HUD units (draw.rs), then onto the sketch
        let (pb, pr, pl) = (5.0, 5.0, 5.0); let (gw, gh, bw, bh) = (28.0, 46.0, 46.0, 28.0);
        let gx = lu - pr - gw; let brx = gx - bw - 5.0;
        let rects = [(pl, hu - pb - 32.0, 38.0, 32.0), (pl + 42.0, hu - pb - 32.0, 38.0, 32.0), (gx, hu - pb - gh, gw, gh), (brx, hu - pb - bh, bw, bh), (brx, hu - pb - 2.0 * bh - 4.0, bw, bh)];
        let m = |r: (f64, f64, f64, f64)| Rect { x: (sx + r.0 * k * u).round(), y: (sy + r.1 * k * u).round(), w: (r.2 * k * u).round(), h: (r.3 * k * u).round() };
        let [l, r, gas, brake, hb] = rects.map(m);
        for b in [l, r, gas, brake, hb] { g.panel(b.x, b.y, b.w, b.h, u, rgba(8, 12, 8, 0.75), Some(rgba(233, 236, 226, 0.6))); }
        g.arrow(l.x + l.w / 2.0, l.y + l.h / 2.0, -1.0, 4.0 * u, u, INK); g.arrow(r.x + r.w / 2.0, r.y + r.h / 2.0, 1.0, 4.0 * u, u, INK);
        // the words beside the controls, in the empty parts of the screen
        let lab = |g: &mut Canvas, s: &str, x: f64, y: f64| g.tx(s, x.round(), y.round(), u, AMBER);
        let steer = "STEER"; lab(g, steer, l.x + (r.x + r.w - l.x - text_w(steer, u)) / 2.0, l.y - 10.0 * u);
        let gs = "GAS"; lab(g, gs, (gas.x + (gas.w - text_w(gs, u)) / 2.0).min(sx + sw * u - text_w(gs, u) - 3.0 * u), gas.y - 10.0 * u);
        let hbs = "H-BRAKE"; lab(g, hbs, hb.x + hb.w - text_w(hbs, u) - 2.0 * u, hb.y - 12.0 * u);
        let bk = "BRAKE"; lab(g, bk, brake.x - text_w(bk, u) - 4.0 * u, brake.y + (brake.h - 7.0 * u) / 2.0);
        let go = "TAP TO DRIVE"; if blink(now) { g.tx(go, x0 + (cw - text_w(go, u)) / 2.0, y0 + ch - 14.0 * u, u, AMBER); }
    }
}

/// the drive's tip: one line under the stage panel, and the on-screen control it talks about pulsing
pub fn tip(g: &mut Canvas, t: &Tip, ui: &[(&'static str, Rect)], inp: &Input, lw: f64, y: f64, u: f64, now: f64) {
    let held = |c: Cap| match c { Cap::Up => inp.throttle > 0.0, Cap::Down => inp.brake > 0.0, Cap::Left => inp.steer > 0.0, Cap::Right => inp.steer < 0.0, Cap::Space => inp.hb, Cap::R => false };
    let cw = if t.caps.is_empty() { 0.0 } else { caps_w(t.caps, u) + 6.0 * u };
    let count = t.count.map(|n| format!("{}/3", n)); let nw = count.as_ref().map(|s| text_w(s, u) + 10.0 * u).unwrap_or(0.0);
    let w = (cw + text_w(t.text, u) + nw + 12.0 * u).round(); let h = if t.caps.is_empty() { 15.0 * u } else { 17.0 * u };
    let x0 = ((lw - w) / 2.0).round();
    g.panel(x0, y, w, h, u, rgba(8, 12, 8, 0.8), Some(AMBER));
    let mut x = x0 + 6.0 * u; let cy = y + 3.0 * u;
    for c in t.caps { x += cap(g, *c, x, cy, u, held(*c)) + 2.0 * u; }
    if !t.caps.is_empty() { x += 4.0 * u; }
    g.tx(t.text, x, y + (h - 7.0 * u) / 2.0, u, INK);
    if let Some(s) = count { g.text(&s, x0 + w - 6.0 * u - text_w(&s, u), y + (h - 7.0 * u) / 2.0, u, DIM, None); }
    // a ring round the control (on and off, so it reads as "this one" without hiding the button)
    // (one ring round all of them: the two steering pads read as one control)
    let mut ring: Option<(f64, f64, f64, f64)> = None;
    for key in t.pulse { if let Some((_, b)) = ui.iter().find(|(k, _)| k == key) {
        ring = Some(match ring { None => (b.x, b.y, b.x + b.w, b.y + b.h), Some((a, c, d, e)) => (a.min(b.x), c.min(b.y), d.max(b.x + b.w), e.max(b.y + b.h)) }); } }
    if let (true, Some((x0, y0, x1, y1))) = (blink(now), ring) {
        let e = 2.0 * u; let (bx, by, bw, bh) = (x0 - e, y0 - e, x1 - x0 + 2.0 * e, y1 - y0 + 2.0 * e);
        g.rect_c(bx, by, bw, u, AMBER); g.rect_c(bx, by + bh - u, bw, u, AMBER); g.rect_c(bx, by, u, bh, AMBER); g.rect_c(bx + bw - u, by, u, bh, AMBER); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn card_only_for_new_players_where_asked() {
        WANT.store(false, Ordering::Relaxed); assert!(!Tut::new(false).card);
        WANT.store(true, Ordering::Relaxed); assert!(Tut::new(false).card); assert!(!Tut::new(true).card);
        WANT.store(false, Ordering::Relaxed);
        assert_eq!(Tut::new(false).step, Step::Off);
    }
    #[test]
    fn caps_line_up() { assert_eq!(caps_w(&[Cap::Left, Cap::Right], 1.0), 24.0); assert_eq!(caps_w(&[Cap::Space], 2.0), 70.0); }
}
