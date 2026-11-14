//! Glue between the app and the renderer: messages arrive on any thread, drawing happens on the GL thread.
use crate::render::{loader::Loader, Renderer};
use std::cell::RefCell;
use std::sync::Mutex;

struct Inbox { scene: Option<Vec<u8>>, frames: Vec<Vec<u8>>, had_scene: bool, size: (i32, i32), status: String }
static INBOX: Mutex<Inbox> = Mutex::new(Inbox { scene: None, frames: Vec::new(), had_scene: false, size: (1, 1), status: String::new() });
thread_local! { static R: RefCell<Option<Renderer>> = const { RefCell::new(None) }; }

/// Scene packets start with "FWS1"; anything else is a frame packet.
pub fn push_message(bytes: Vec<u8>) {
    let mut ib = INBOX.lock().unwrap();
    if bytes.len() >= 4 && &bytes[0..4] == b"FWS1" { ib.scene = Some(bytes); ib.frames.clear(); ib.had_scene = true; }
    else { ib.frames.push(bytes); if ib.frames.len() > 3600 { ib.frames.drain(0..1800); } }   // frames are deltas: keep them all unless the GL thread has been gone for a minute
}

fn set_status(s: String) { INBOX.lock().unwrap().status = s; }
pub fn status() -> String { INBOX.lock().unwrap().status.clone() }

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
    INBOX.lock().unwrap().had_scene
}
pub fn gl_resize(w: i32, h: i32) { INBOX.lock().unwrap().size = (w.max(1), h.max(1)); }

pub fn gl_draw() {
    let (scene, frames, size) = { let mut ib = INBOX.lock().unwrap(); (ib.scene.take(), std::mem::take(&mut ib.frames), ib.size) };
    R.with(|c| {
        let mut b = c.borrow_mut(); let Some(r) = b.as_mut() else { return };
        unsafe {
            if let Some(s) = scene { match r.load_scene(&s) { Ok(()) => set_status("scene loaded".into()), Err(e) => set_status(format!("scene failed: {e}")) } }
            for f in &frames { if let Err(e) = r.apply_frame(f) { set_status(format!("frame failed: {e}")); } }
            if let Err(e) = r.draw(None, size.0, size.1) { set_status(format!("draw failed: {e}")); }
        }
    });
}
