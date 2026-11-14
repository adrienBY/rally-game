//! Desktop check for the native game: headless EGL, a dumped stage (scene + runtime handoff), the game run at a fixed 60 Hz.
//!   game_host <scene> <rt> <outdir> <script>
//! script: comma-separated steps "t:action", e.g. "1:start,1:auto,8:shot,12:shot,20:race,40:shot,60:state"
//!   start (tap to start)  auto (demo driver drives)  shot (write <outdir>/fNNNN.ppm)  state (print the car)  tap:x:y  race (toggle mode on the menu)
//!   cam  menu  log (car each second from then on)
use fogwood_core::game::{Game, Persist, State};
use fogwood_core::render::{loader::Loader, Renderer};
use glow::HasContext;
use std::ffi::{c_char, c_void};

type P = *mut c_void;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 { eprintln!("usage: game_host scene rt outdir script [w h]"); std::process::exit(2); }
    let (w, h): (i32, i32) = (a.get(5).and_then(|v| v.parse().ok()).unwrap_or(860), a.get(6).and_then(|v| v.parse().ok()).unwrap_or(390));
    let loader = Loader::new(); let egl = loader.egl_handle(); assert!(!egl.is_null(), "no libEGL");
    unsafe {
        extern "C" { fn dlsym(h: P, s: *const c_char) -> P; }
        let sym = |n: &[u8]| { let p = dlsym(egl, n.as_ptr() as *const c_char); assert!(!p.is_null()); p };
        let get_platform_display: extern "C" fn(u32, P, *const isize) -> P = std::mem::transmute(sym(b"eglGetPlatformDisplay\0"));
        let initialize: extern "C" fn(P, *mut i32, *mut i32) -> u32 = std::mem::transmute(sym(b"eglInitialize\0"));
        let bind_api: extern "C" fn(u32) -> u32 = std::mem::transmute(sym(b"eglBindAPI\0"));
        let choose: extern "C" fn(P, *const i32, *mut P, i32, *mut i32) -> u32 = std::mem::transmute(sym(b"eglChooseConfig\0"));
        let create_ctx: extern "C" fn(P, P, P, *const i32) -> P = std::mem::transmute(sym(b"eglCreateContext\0"));
        let make_current: extern "C" fn(P, P, P, P) -> u32 = std::mem::transmute(sym(b"eglMakeCurrent\0"));
        let dpy = get_platform_display(0x31DD, std::ptr::null_mut(), std::ptr::null());
        let (mut ma, mut mi) = (0, 0); assert!(initialize(dpy, &mut ma, &mut mi) != 0); bind_api(0x30A0);
        let attrs = [0x3040, 0x40, 0x3033, 0x1, 0x3038]; let mut cfg: P = std::ptr::null_mut(); let mut n = 0; choose(dpy, attrs.as_ptr(), &mut cfg, 1, &mut n);
        let cattrs = [0x3098, 3, 0x3038]; let ctx = create_ctx(dpy, cfg, std::ptr::null_mut(), cattrs.as_ptr());
        assert!(make_current(dpy, std::ptr::null_mut(), std::ptr::null_mut(), ctx) != 0);
        let gl = glow::Context::from_loader_function(|n| loader.get(n));
        let mut r = Renderer::new(gl).expect("renderer");
        r.load_scene(&std::fs::read(&a[1]).unwrap()).expect("scene");
        let rtb = std::fs::read(&a[2]).unwrap(); let rt = fogwood_core::game::rt::parse(&rtb).expect("rt");
        // the stage's world, as the page would have built it through the bridge
        let path: Vec<String> = rt.path.iter().flat_map(|p| [p[0].to_string(), p[1].to_string()]).collect();
        let roadh: Vec<String> = rt.roadh.iter().map(|v| v.to_string()).collect();
        println!("prepare {}", fogwood_core::bridge::prepare(rt.map_id, &path.join(","), &roadh.join(","), rt.gxh, rt.gzh));
        let obs: Vec<String> = rt.obstacles.iter().flat_map(|o| [o[0].to_string(), o[1].to_string(), o[2].to_string()]).collect();
        println!("obstacles {}", fogwood_core::bridge::set_obstacles(&obs.join(",")));
        fogwood_audio::native::init(48000.0);
        // the recordings the page would send (16-bit PCM wavs): engine 2, skid 3, impact 4
        for (id, f, looped) in [(2.0f32, "/home/claude/forest/engine.wav", 1.0f32), (3.0, "/home/claude/forest/skid.wav", 1.0), (4.0, "/home/claude/forest/impact.wav", 0.0)] {
            let Ok(b) = std::fs::read(f) else { continue }; let mut p = 12; let (mut ch, mut rate, mut data) = (1u16, 44100u32, &b[0..0]);
            while p + 8 <= b.len() { let id4 = &b[p..p + 4]; let n = u32::from_le_bytes([b[p + 4], b[p + 5], b[p + 6], b[p + 7]]) as usize;
                if id4 == b"fmt " { ch = u16::from_le_bytes([b[p + 10], b[p + 11]]); rate = u32::from_le_bytes([b[p + 12], b[p + 13], b[p + 14], b[p + 15]]); }
                if id4 == b"data" { data = &b[p + 8..(p + 8 + n).min(b.len())]; } p += 8 + n + (n & 1); }
            let frames = data.len() / 2 / ch as usize; let chs = ch.min(2) as usize;
            let mut m: Vec<u8> = Vec::new(); for v in [4.0f32, id, chs as f32, rate as f32, looped, frames as f32] { m.extend_from_slice(&v.to_le_bytes()); }
            for c in 0..chs { for i in 0..frames { let q = (i * ch as usize + c) * 2; m.extend_from_slice(&data[q..q + 2]); } }
            fogwood_audio::native::message(&m); println!("loaded {} {} ch {} Hz {} frames", f, chs, rate, frames);
        }
        let mut g = Game::new(rt, Persist::new("/tmp/game_host.sav")); g.resize(w as f64, h as f64, [0.0; 4]); if std::env::var("NOLOD").is_err() { g.attach(&mut r); }
        let gl = r.gl();
        let tex = gl.create_texture().unwrap(); gl.bind_texture(glow::TEXTURE_2D, Some(tex));
        gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w, h, 0, glow::RGBA, glow::UNSIGNED_BYTE, None);
        let fb = gl.create_framebuffer().unwrap(); gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
        gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
        let mut steps: Vec<(f64, String)> = a[4].split(',').filter_map(|s| { let (t, act) = s.split_once(':')?; Some((t.parse().ok()?, act.to_string())) }).collect();
        steps.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        let end = steps.last().map(|s| s.0).unwrap_or(1.0) + 0.05; let dt = 1.0 / 60.0; let mut t = 0.0; let mut k = 0; let mut log = false; let mut next_log = 0.0;
        let mut tick_ms = 0.0f64; let mut ticks = 0;
        let mut audio = vec![0f32; 800 * 2]; let (mut arms, mut apk, mut anb, mut aclip) = (0f64, 0f32, 0f64, 0f64);
        while t < end {
            while k < steps.len() && steps[k].0 <= t {
                let act = steps[k].1.clone(); k += 1;
                let parts: Vec<&str> = act.split(':').collect();
                match parts[0] {
                    "start" => g.start_game(), "auto" => g.auto = true, "race" => { g.ps.race_mode = true; }
                    "tap" => { let (x, y) = (parts[1].parse::<f64>().unwrap(), parts[2].parse::<f64>().unwrap()); g.touch(0, 7, x, y); g.touch(2, 7, x, y); }
                    "menu" => { g.to_title(); } "log" => log = true,
                    "aisim" => {   // same as the page's AI lap test: placeAt(0), demo driver + physics at 60 Hz until a full lap
                        let lock = fogwood_core::bridge::lock(); let e = lock.as_ref().unwrap(); let np = g.np;
                        g.place_at(&e.world, 0); let (mut tt, mut prog, mut last) = (0.0, 0i64, fogwood_core::game::path_idx(&e.world, g.car.x, g.car.z) as i64);
                        while tt < 260.0 { let inp = g.ai_input(&e.world); g.car.update(&e.world, &e.obstacles, &g.tune, 1.0 / 60.0, &inp);
                            let i = fogwood_core::game::path_idx(&e.world, g.car.x, g.car.z) as i64; let mut d = i - last; if d < -900 { d += np as i64; } if d > 900 { d -= np as i64; } prog += d; last = i;
                            if prog >= np as i64 { break; } tt += 1.0 / 60.0; }
                        println!("aisim t={:.1} prog={} x={:.6} z={:.6}", tt, prog, g.car.x, g.car.z); }
                    "shot" => {
                        r.draw(Some(fb), w, h).expect("draw"); let gl = r.gl(); gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
                        let mut px = vec![0u8; (w * h * 4) as usize]; gl.read_pixels(0, 0, w, h, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(&mut px));
                        let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
                        for y in (0..h).rev() { for x in 0..w { let i = ((y * w + x) * 4) as usize; out.extend_from_slice(&px[i..i + 3]); } }
                        let f = format!("{}/f{:05.1}.ppm", a[3], t); std::fs::write(&f, out).unwrap(); println!("shot {} calls {} tris {}", f, r.stats.0, r.stats.1);
                    }
                    "audio" => { println!("audio rms {:.4} peak {:.3} clipped blocks {:.1}%", (arms / anb.max(1.0)).sqrt(), apk, aclip / anb.max(1.0) * 100.0); arms = 0.0; apk = 0.0; anb = 0.0; aclip = 0.0; }
                    "state" => { let c = &g.car; println!("t={:.2} state={} x={:.3} z={:.3} h={:.4} u={:.2} gear={} rpm={:.0} lap={} best={:?} race={} pos={} phase={:?}", t, if g.state == State::Drive { "drive" } else { "title" }, c.x, c.z, c.h, c.u, c.gear, c.rpm, g.lap, g.best, g.race.on, g.race.pos, g.race.phase); }
                    _ => println!("unknown {}", act),
                }
            }
            let t0 = std::time::Instant::now(); g.tick(&mut r, dt); tick_ms += t0.elapsed().as_secs_f64() * 1000.0; ticks += 1;
            fogwood_audio::native::render(&mut audio); { let (mut s2, mut pk) = (0f64, 0f32); for v in &audio { s2 += (*v as f64) * (*v as f64); pk = pk.max(v.abs()); if !v.is_finite() { println!("audio NaN"); break; } } arms += s2 / audio.len() as f64; apk = apk.max(pk); anb += 1.0; if pk >= 0.999 { aclip += 1.0; } }
            if log && t >= next_log { next_log = t + 1.0; let c = &g.car; println!("log t={:.1} x={:.2} z={:.2} kmh={:.0} gear={} race_t={:.1} pos={} cars={:?}", t, c.x, c.z, c.u * 3.6, c.gear, g.race.t, g.race.pos,
                g.race.cars.iter().map(|q| (q.r.map(|i| fogwood_core::game::race::RIVALS[i].tag).unwrap_or("YOU"), (q.prog as i64), q.resets)).collect::<Vec<_>>()); }
            for e in g.events.drain(..) { if !e.starts_with("h ") { println!("event {}", e); } }
            t += dt;
        }
        println!("tick avg {:.3} ms over {} frames (incl. hud)", tick_ms / ticks as f64, ticks);
    }
}
