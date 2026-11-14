//! Glue between the app and the renderer: messages arrive on any thread, the game runs and draws on the GL thread.
//! Runtime mode: the page builds a stage, posts its scene ("FWS1") and runtime description ("FRT1"), and the native game takes over.
use crate::game::{Game, Persist};
use crate::render::{loader::Loader, Renderer};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

struct Inbox { scene: Option<Vec<u8>>, rt: Option<Vec<u8>>, frames: Vec<Vec<u8>>, had_scene: bool, size: (i32, i32), status: String,
    touches: Vec<(i32, i32, f32, f32)>, back: bool, insets: [f32; 4], save_path: String, thermal: (i32, f32), device: String, bench: bool }
static INBOX: Mutex<Inbox> = Mutex::new(Inbox { scene: None, rt: None, frames: Vec::new(), had_scene: false, size: (1, 1), status: String::new(),
    touches: Vec::new(), back: false, insets: [0.0; 4], save_path: String::new(), thermal: (-1, 0.0), device: String::new(), bench: false });
static DRIVING: AtomicBool = AtomicBool::new(false);
thread_local! {
    static B: RefCell<Option<Bench>> = const { RefCell::new(None) };
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

/// the phone's thermal status (PowerManager, -1 unknown) and battery temperature, for the benchmark log
pub fn rt_thermal(status: i32, batt_c: f32) { inbox().thermal = (status, batt_c); }
/// device description for the benchmark log header
pub fn rt_device(info: &str) { inbox().device = info.to_string(); }
/// start / stop the benchmark recording (same as the REC button)
pub fn rt_bench() { inbox().bench = true; }

// ---------------- benchmark (temporary): one row per frame, written as CSV when it stops ----------------
const BENCH_SECS: f64 = 60.0;
struct Row { id: u64, t: f32, dt: f32, interval: f32, total: f32, tick: f32, prof: [f32; 8], draw: f32, gpu: (f32, f32), calls: u32, tris: u32, state: u8, kmh: f32,
    race: u8, rivals: u8, dust: u16, hud_up: u8, lw: i32, lh: i32, thermal: i32, batt: f32 }
struct Bench { start: std::time::Instant, last: Option<std::time::Instant>, rows: Vec<Row>, header: Vec<String> }
fn pct(v: &mut Vec<f32>, p: f32) -> f32 { if v.is_empty() { return 0.0; } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f32 * p) as usize] }
impl Bench {
    fn write(&self, dir: &str) -> Option<String> {
        use std::fmt::Write;
        let mut out = String::new(); for h in &self.header { let _ = writeln!(out, "# {}", h); }
        let n = self.rows.len().max(1) as f32; let dur = self.rows.last().map(|r| r.t).unwrap_or(0.0) / 1000.0;
        let mut iv: Vec<f32> = self.rows.iter().skip(1).map(|r| r.dt).collect(); let ivc = iv.clone();
        let _ = writeln!(out, "# frames {} over {:.1} s = {:.1} fps; frame time (vsync to vsync, what the game advances by) p50 {:.1} p95 {:.1} p99 {:.1} max {:.1} ms", self.rows.len(), dur, self.rows.len() as f32 / dur.max(0.001),
            pct(&mut iv, 0.5), pct(&mut iv, 0.95), pct(&mut iv, 0.99), ivc.iter().cloned().fold(0.0, f32::max));
        { let mut wk: Vec<f32> = self.rows.iter().skip(1).map(|r| r.interval).collect(); let _ = writeln!(out, "# render thread wake-up interval p50 {:.1} p95 {:.1} p99 {:.1} ms", pct(&mut wk, 0.5), pct(&mut wk, 0.95), pct(&mut wk, 0.99)); }
        let _ = writeln!(out, "# frame times over 20 ms: {} ({:.1}%), over 34 ms: {} ({:.1}%)", ivc.iter().filter(|v| **v > 20.0).count(), ivc.iter().filter(|v| **v > 20.0).count() as f32 / n * 100.0,
            ivc.iter().filter(|v| **v > 34.0).count(), ivc.iter().filter(|v| **v > 34.0).count() as f32 / n * 100.0);
        let avg = |f: &dyn Fn(&Row) -> f32| self.rows.iter().map(f).sum::<f32>() / n;
        let gpu: Vec<&Row> = self.rows.iter().filter(|r| r.gpu.0 >= 0.0).collect();
        let _ = writeln!(out, "# averages ms: game tick {:.2} [physics {:.2} race {:.2} sound {:.2} fx {:.2} camera {:.2} rivals {:.2} world {:.2} hud {:.2}] draw cpu {:.2} gpu scene {} gpu post {}",
            avg(&|r| r.tick), avg(&|r| r.prof[0]), avg(&|r| r.prof[1]), avg(&|r| r.prof[2]), avg(&|r| r.prof[3]), avg(&|r| r.prof[4]), avg(&|r| r.prof[5]), avg(&|r| r.prof[6]), avg(&|r| r.prof[7]), avg(&|r| r.draw),
            if gpu.is_empty() { "n/a".into() } else { format!("{:.2}", gpu.iter().map(|r| r.gpu.0).sum::<f32>() / gpu.len() as f32) },
            if gpu.is_empty() { "n/a".into() } else { format!("{:.2}", gpu.iter().map(|r| r.gpu.1).sum::<f32>() / gpu.len() as f32) });
        let mut worst: Vec<&Row> = self.rows.iter().skip(1).collect(); worst.sort_by(|a, b| b.dt.partial_cmp(&a.dt).unwrap());
        let _ = writeln!(out, "# worst frames (frame: frame time ms): {}", worst.iter().take(12).map(|r| format!("{}: {:.1}", r.id, r.dt)).collect::<Vec<_>>().join(", "));
        let _ = writeln!(out, "frame,t_ms,dt_ms,interval_ms,native_ms,tick_ms,physics_ms,race_ms,sound_ms,fx_ms,camera_ms,rivals_ms,world_ms,hud_ms,draw_cpu_ms,gpu_scene_ms,gpu_post_ms,calls,tris,state,kmh,race,rivals,dust,hud_upload,lw,lh,thermal,battery_c");
        for r in &self.rows {
            let g = |v: f32| if v >= 0.0 { format!("{:.3}", v) } else { String::new() };
            let _ = writeln!(out, "{},{:.1},{:.2},{:.2},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{},{},{},{},{},{:.0},{},{},{},{},{},{},{},{:.1}",
                r.id, r.t, r.dt, r.interval, r.total, r.tick, r.prof[0], r.prof[1], r.prof[2], r.prof[3], r.prof[4], r.prof[5], r.prof[6], r.prof[7], r.draw, g(r.gpu.0), g(r.gpu.1),
                r.calls, r.tris, if r.state == 1 { "drive" } else { "menu" }, r.kmh, r.race, r.rivals, r.dust, r.hud_up, r.lw, r.lh, r.thermal, r.batt);
        }
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let path = format!("{}/fogwood-bench-{}.csv", dir, secs); std::fs::write(&path, out).ok()?; Some(path)
    }
}

/// One frame in runtime mode: take in new stages and input, run the game, draw. Returns events for the app, one per line
/// ("h <kind> <strength>" haptics, "map <name>" build that stage).
pub fn rt_frame(dt: f64) -> String {
    let t_frame = std::time::Instant::now();
    let (scene, rtp, size, touches, back, insets, save_path, thermal, device) = { let mut ib = inbox();
        (ib.scene.take(), ib.rt.take(), ib.size, std::mem::take(&mut ib.touches), std::mem::replace(&mut ib.back, false), ib.insets, ib.save_path.clone(), ib.thermal, ib.device.clone()) }; let bench_tap = std::mem::replace(&mut inbox().bench, false);
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
                        let mut g = Game::new(rt, ps); g.resize(size.0 as f64, size.1 as f64, [insets[0] as f64, insets[1] as f64, insets[2] as f64, insets[3] as f64]); g.attach(r);
                        *game = Some(g); set_status("game ready".into()); }
                    Err(e) => set_status(format!("runtime failed: {e}")),
                }
            } else if had_scene { if let Some(g) = game.as_mut() { if g.loading.is_none() { g.scene_reloaded(); g.attach(r); } } }
            if let Some(g) = game.as_mut() {
                if (g.w - size.0 as f64).abs() > 0.5 || (g.h - size.1 as f64).abs() > 0.5 || g.inset_px != [insets[0] as f64, insets[1] as f64, insets[2] as f64, insets[3] as f64] {
                    g.resize(size.0 as f64, size.1 as f64, [insets[0] as f64, insets[1] as f64, insets[2] as f64, insets[3] as f64]); }
                for (a, id, x, y) in touches { g.touch(a, id, x as f64, y as f64); }
                if back { g.back(); }
                let t_tick = std::time::Instant::now();
                g.tick(r, dt.clamp(0.0, 0.05));
                let tick_ms = t_tick.elapsed().as_secs_f32() * 1000.0;
                DRIVING.store(g.state == crate::game::State::Drive, Ordering::Relaxed);
                // benchmark on / off (the REC button), at most a minute
                let toggle = std::mem::replace(&mut g.bench_req, false) || bench_tap;
                B.with(|bc| { let mut b = bc.borrow_mut();
                    let stop = b.as_ref().map(|x| toggle || x.start.elapsed().as_secs_f64() > BENCH_SECS).unwrap_or(false);
                    if stop { let bench = b.take().unwrap(); r.timing = false; g.bench_secs = None;
                        let dir = std::path::Path::new(&save_path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| ".".into());
                        match bench.write(&dir) { Some(p) => { g.events.push(format!("bench {}", p)); g.flash_msg("BENCHMARK SAVED", 2.0); } None => g.flash_msg("BENCHMARK FAILED", 2.0) } }
                    else if toggle && b.is_none() {
                        let header = vec![format!("Fogwood Rally benchmark, map {}, {}", g.rt.map, if g.race.on { "race" } else { "time trial" }), format!("device: {}", device),
                            format!("gpu: {} (timer queries: {})", r.gl_renderer(), if r.has_timer { "yes" } else { "no" }), format!("surface {}x{}, render {}x{}, pixel setting {}", size.0, size.1, g.lw, g.lh, g.ps.pix_idx)];
                        *b = Some(Bench { start: std::time::Instant::now(), last: None, rows: Vec::with_capacity(4000), header }); r.timing = true; g.flash_msg("RECORDING 60 S", 1.5); }
                    if let Some(x) = b.as_ref() { g.bench_secs = Some((BENCH_SECS - x.start.elapsed().as_secs_f64()).max(0.0).ceil() as u32); }
                });
                for e in g.events.drain(..) { events.push_str(&e); events.push('\n'); }
                if let Err(e) = unsafe { r.draw(None, size.0, size.1) } { set_status(format!("draw failed: {e}")); }
                B.with(|bc| { if let Some(b) = bc.borrow_mut().as_mut() {
                    let now = std::time::Instant::now(); let interval = b.last.map(|l| (t_frame - l).as_secs_f32() * 1000.0).unwrap_or(0.0); b.last = Some(t_frame);
                    let s = &g.car;
                    b.rows.push(Row { id: r.frame_id, t: (t_frame - b.start).as_secs_f32() * 1000.0, dt: (dt * 1000.0) as f32, interval, total: (now - t_frame).as_secs_f32() * 1000.0, tick: tick_ms, prof: g.prof, draw: r.draw_cpu_ms,
                        gpu: (-1.0, -1.0), calls: r.stats.0, tris: r.stats.1, state: (g.state == crate::game::State::Drive) as u8, kmh: (s.u * 3.6) as f32, race: g.race.on as u8,
                        rivals: g.race.cars.len().saturating_sub(1) as u8, dust: g.d_live as u16, hud_up: g.hud_uploaded as u8, lw: g.lw, lh: g.lh, thermal: thermal.0, batt: thermal.1 });
                    for (id, a, c) in r.gpu_results.drain(..) { if let Some(row) = b.rows.iter_mut().rev().take(16).find(|x| x.id == id) { row.gpu = (a, c); } }
                } });
                return;
            }
            if let Err(e) = unsafe { r.draw(None, size.0, size.1) } { set_status(format!("draw failed: {e}")); }
        });
    });
    events
}
