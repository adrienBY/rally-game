//! Glue between the app and the renderer: messages arrive on any thread, the game runs and draws on the GL thread.
//! Runtime mode: the page builds a stage, posts its scene ("FWS1") and runtime description ("FRT1"), and the native game takes over.
use crate::game::{Game, Persist};
use crate::render::{loader::Loader, Renderer};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

struct Inbox { scene: Option<Vec<u8>>, rt: Option<Vec<u8>>, frames: Vec<Vec<u8>>, had_scene: bool, size: (i32, i32), status: String,
    touches: Vec<(i32, i32, f32, f32)>, back: bool, insets: [f32; 4], save_path: String }
static INBOX: Mutex<Inbox> = Mutex::new(Inbox { scene: None, rt: None, frames: Vec::new(), had_scene: false, size: (1, 1), status: String::new(),
    touches: Vec::new(), back: false, insets: [0.0; 4], save_path: String::new() });
static DRIVING: AtomicBool = AtomicBool::new(false);
thread_local! {
    static R: RefCell<Option<Renderer>> = const { RefCell::new(None) };
    static G: RefCell<(Option<Game>, Option<Persist>)> = const { RefCell::new((None, None)) };
}
fn inbox() -> std::sync::MutexGuard<'static, Inbox> { INBOX.lock().unwrap_or_else(|e| e.into_inner()) }

/// "FWS1" scene, "FRT1" runtime handoff, anything else a frame packet (page-driven mode)
pub fn push_message(bytes: Vec<u8>) {
    let mut ib = inbox();
    if bytes.len() >= 4 && &bytes[0..4] == b"FWS1" { ib.scene = Some(bytes); ib.frames.clear(); ib.had_scene = true; }
    else if bytes.len() >= 4 && &bytes[0..4] == b"FRT1" { ib.rt = Some(bytes); }
    else { ib.frames.push(bytes); if ib.frames.len() > 3600 { ib.frames.drain(0..1800); } }   // frames are deltas: keep them all unless the GL thread has been gone for a minute
}

fn set_status(s: String) { inbox().status = s; }
pub fn status() -> String { inbox().status.clone() }

/// GL context (re)created. Returns true when the page should resend its scene (the old GPU copy died with the old context).
pub fn gl_init() -> bool {
    // a new context means the old GPU objects are already gone: forget the old renderer instead of deleting
    // its (now meaningless) object names, which could collide with names in the new context
    R.with(|c| { if let Some(old) = c.borrow_mut().take() { std::mem::forget(old); } });
    let loader = Loader::new();
    let gl = unsafe { glow::Context::from_loader_function(|n| loader.get(n)) };
    match unsafe { Renderer::new(gl) } {
        Ok(r) => { R.with(|c| *c.borrow_mut() = Some(r)); set_status("renderer ready".into()); }
        Err(e) => { set_status(format!("renderer init failed: {e}")); R.with(|c| *c.borrow_mut() = None); }
    }
    inbox().had_scene
}
pub fn gl_resize(w: i32, h: i32) { inbox().size = (w.max(1), h.max(1)); }

pub fn gl_draw() {
    let (scene, frames, size) = { let mut ib = inbox(); (ib.scene.take(), std::mem::take(&mut ib.frames), ib.size) };
    R.with(|c| {
        let mut b = c.borrow_mut(); let Some(r) = b.as_mut() else { return };
        unsafe {
            if let Some(s) = scene { match r.load_scene(&s) { Ok(()) => set_status("scene loaded".into()), Err(e) => set_status(format!("scene failed: {e}")) } }
            for f in &frames { if let Err(e) = r.apply_frame(f) { set_status(format!("frame failed: {e}")); } }
            if let Err(e) = r.draw(None, size.0, size.1) { set_status(format!("draw failed: {e}")); }
        }
    });
}

// ---------------- runtime mode ----------------
/// where the records and settings live; returns the stage to open with
pub fn rt_init(save_path: &str) -> String {
    inbox().save_path = save_path.to_string();
    let p = Persist::new(save_path); let m = p.saves.get("fogwood-map").unwrap_or("forest").to_string();
    if crate::game::MAPS.contains(&m.as_str()) { m } else { "forest".into() }
}
/// pointer events in screen pixels: 0 down, 1 move, 2 up / cancel
pub fn rt_touch(action: i32, id: i32, x: f32, y: f32) { inbox().touches.push((action, id, x, y)); }
/// Android back: true when the game uses it (driving -> menu); false lets the app close
pub fn rt_back() -> bool { if DRIVING.load(Ordering::Relaxed) { inbox().back = true; true } else { false } }
/// display cutout / system bar insets in screen pixels (top, right, bottom, left)
pub fn rt_insets(t: f32, r: f32, b: f32, l: f32) { inbox().insets = [t, r, b, l]; }

/// One frame in runtime mode: take in new stages and input, run the game, draw. Returns events for the app, one per line
/// ("h <kind> <strength>" haptics, "map <name>" build that stage).
pub fn rt_frame(dt: f64) -> String {
    let (scene, rtp, size, touches, back, insets, save_path) = { let mut ib = inbox();
        (ib.scene.take(), ib.rt.take(), ib.size, std::mem::take(&mut ib.touches), std::mem::replace(&mut ib.back, false), ib.insets, ib.save_path.clone()) };
    let mut events = String::new();
    R.with(|c| {
        let mut b = c.borrow_mut(); let Some(r) = b.as_mut() else { return };
        G.with(|gc| {
            let mut gs = gc.borrow_mut(); let (game, persist) = &mut *gs;
            let had_scene = scene.is_some();
            if let Some(s) = scene { match unsafe { r.load_scene(&s) } { Ok(()) => set_status("scene loaded".into()), Err(e) => set_status(format!("scene failed: {e}")) } }
            if let Some(p) = rtp {
                match crate::game::rt::parse(&p) {
                    Ok(rt) => { let ps = match game.take() { Some(g) => g.into_persist(), None => persist.take().unwrap_or_else(|| Persist::new(&save_path)) };
                        let mut g = Game::new(rt, ps); g.resize(size.0 as f64, size.1 as f64, [insets[0] as f64, insets[1] as f64, insets[2] as f64, insets[3] as f64]);
                        *game = Some(g); set_status("game ready".into()); }
                    Err(e) => set_status(format!("runtime failed: {e}")),
                }
            } else if had_scene { if let Some(g) = game.as_mut() { if g.loading.is_none() { g.scene_reloaded(); } } }
            if let Some(g) = game.as_mut() {
                if (g.w - size.0 as f64).abs() > 0.5 || (g.h - size.1 as f64).abs() > 0.5 || g.inset_px != [insets[0] as f64, insets[1] as f64, insets[2] as f64, insets[3] as f64] {
                    g.resize(size.0 as f64, size.1 as f64, [insets[0] as f64, insets[1] as f64, insets[2] as f64, insets[3] as f64]); }
                for (a, id, x, y) in touches { g.touch(a, id, x as f64, y as f64); }
                if back { g.back(); }
                g.tick(r, dt.clamp(0.0, 0.05));
                DRIVING.store(g.state == crate::game::State::Drive, Ordering::Relaxed);
                for e in g.events.drain(..) { events.push_str(&e); events.push('\n'); }
            }
            if let Err(e) = unsafe { r.draw(None, size.0, size.1) } { set_status(format!("draw failed: {e}")); }
        });
    });
    events
}
