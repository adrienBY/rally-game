//! Desktop check for the native game: headless EGL, a dumped stage (scene + runtime handoff), the game run at a fixed 60 Hz.
//!   game_host <scene> <rt> <outdir> <script>
//! env: DUSTN=6000 (the app's dust buffer size, for scenes dumped from the browser build), NODUST, NOHUD
//! script: comma-separated steps "t:action", e.g. "1:start,1:auto,8:shot,12:shot,20:race,40:shot,60:state"
//!   start (tap to start)  auto (demo driver drives)  shot (write <outdir>/fNNNN.ppm)  state (print the car)  tap:x:y  race (toggle mode on the menu)
//!   mtap:key  hit:cx:cz:r:dx:dz:speed / hitcar:cx:cz:axx:axz:dx:dz:speed (dent the bodywork, car frame)  cam  menu  log (car each second from then on)  garage gnext gprev gclose gpaint[:d] car:k
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
        let mut r = Renderer::new(gl).expect("renderer"); r.no_hud = std::env::var("NOHUD").is_ok(); r.no_dust = std::env::var("NODUST").is_ok();
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
        let mut g = Game::new(rt, Persist::new("/tmp/game_host.sav")); g.wave_sim = std::env::var("FW_REALTIME").is_err(); g.wave_t0 = std::env::var("FW_WAVE_T0").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0); if let Ok(n) = std::env::var("DUSTN") { g.dust = fogwood_core::game::dust::Dust::new(n.parse().unwrap_or(6000), 0x5EED); }   // a scene dumped from the browser build has the browser's small dust buffer
        g.resize(w as f64, h as f64, [0.0; 4]); if std::env::var("NOLOD").is_err() { g.attach(&mut r); }
        let gl = r.gl();
        let tex = gl.create_texture().unwrap(); gl.bind_texture(glow::TEXTURE_2D, Some(tex));
        gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w, h, 0, glow::RGBA, glow::UNSIGNED_BYTE, None);
        let fb = gl.create_framebuffer().unwrap(); gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
        gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
        let mut steps: Vec<(f64, String)> = a[4].split(',').filter_map(|s| { let (t, act) = s.split_once(':')?; Some((t.parse().ok()?, act.to_string())) }).collect();
        steps.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
        let end = steps.last().map(|s| s.0).unwrap_or(1.0) + 0.05; let dt = 1.0 / 60.0; let mut t = 0.0; let wall0 = std::time::Instant::now(); let mut k = 0; let mut log = false; let mut next_log = 0.0; let mut burst: i32 = 0; let mut bfol: Option<(i64, f64, f64, f64)> = None;
        let mut fuzz: Option<u64> = std::env::var("FUZZ").ok().and_then(|v| v.parse().ok()).map(|v: u64| v | 1);
        let mut tick_ms = 0.0f64; let mut ticks = 0; let mut profs: Vec<[f32; 8]> = Vec::new();
        let mut audio = vec![0f32; 800 * 2]; let (mut arms, mut apk, mut anb, mut aclip) = (0f64, 0f32, 0f64, 0f64);
        // replay: a car pose per 60 Hz frame (x y z h vx vy vz) and a camera per frame (eye xyz, target xyz, fov), from FW_REPLAY_T0 on
        let rd = |v: &str| -> Vec<Vec<f64>> { std::env::var(v).ok().map(|f| std::fs::read_to_string(f).unwrap().lines().map(|l| l.split_whitespace().filter_map(|x| x.parse().ok()).collect()).collect()).unwrap_or_default() };
        let (rep, cams) = (rd("FW_REPLAY"), rd("FW_CAMS")); let camin: f64 = std::env::var("FW_CAMMIN").ok().and_then(|v| v.parse().ok()).unwrap_or(0.7);
        let mut adump: Option<std::fs::File> = std::env::var("FW_AUDIO").ok().map(|f| std::fs::File::create(f).unwrap()); let rep_t0: f64 = std::env::var("FW_REPLAY_T0").ok().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        if !rep.is_empty() { g.ps.race_mode = false; }
        if let Some(ph) = std::env::var("FW_PIXH").ok().and_then(|v| v.parse::<f64>().ok()) { g.pix_h = Some(ph); g.resize(w as f64, h as f64, [0.0; 4]); }
        if let Ok(f) = std::env::var("FW_PROBE") { let txt = std::fs::read_to_string(&f).unwrap(); let gd = fogwood_core::bridge::lock(); let e = gd.as_ref().unwrap();
            let out: Vec<String> = txt.lines().map(|l| { let v: Vec<f64> = l.split_whitespace().filter_map(|x| x.parse().ok()).collect(); format!("{:.2}", e.world.height(v[0], v[1])) }).collect(); std::fs::write(format!("{}.out", f), out.join("\n")).unwrap(); println!("probed {}", out.len()); }
        while t < end {
            if t >= rep_t0 - 1e-9 { let f = ((t - rep_t0) * 60.0).round() as usize;
                if let Some(p) = rep.get(f.min(rep.len().saturating_sub(1))) { let c = &mut g.car; c.x = p[0]; c.y = p[1]; c.z = p[2]; c.h = p[3]; c.vx = p[4]; c.vy = p[5]; c.vz = p[6]; g.go = 0.0;
                    if p.len() >= 11 { g.inp_ovr = Some(fogwood_core::physics::Input { throttle: p[7], brake: p[8], hb: p[9] > 0.5, steer: p[10] }); } }
                if let Some(q) = cams.get(f.min(cams.len().saturating_sub(1))) { let gy = fogwood_core::bridge::lock().as_ref().map(|e| e.world.height(q[0], q[2])).unwrap_or(-1e9);
                    g.cam_ovr = Some(([q[0], q[1].max(gy + camin), q[2]], [q[3], q[4], q[5]], q[6])); } }
            while k < steps.len() && steps[k].0 <= t {
                let act = steps[k].1.clone(); k += 1;
                let parts: Vec<&str> = act.split(':').collect();
                match parts[0] {
                    "start" => g.start_game(), "auto" => g.auto = true, "race" => { g.ps.race_mode = true; }
                    "tap" => { let (x, y) = (parts[1].parse::<f64>().unwrap(), parts[2].parse::<f64>().unwrap()); g.touch(0, 7, x, y); g.touch(2, 7, x, y); }
                    "menu" => { g.to_title(); } "log" => log = true, "tel" => g.tel_toggle(),
                    "online" => { let k = parts[1]; let key: &'static str = fogwood_core::game::online::KEYS.iter().find(|x| **x == k).copied().unwrap_or("online"); g.online_tap(key);
                        if key == "on_host" { if let Some(c) = fogwood_core::net::with(|s| s.code) { std::fs::write("/tmp/fw_code", c.to_string()).unwrap(); println!("hosting {}", c); } } }
                    "joincode" => { let c = std::fs::read_to_string("/tmp/fw_code").unwrap(); g.online_tap("online"); g.online_tap("on_join"); for ch in c.trim().chars() { let k: &'static str = ["k0","k1","k2","k3","k4","k5","k6","k7","k8","k9"][ch.to_digit(10).unwrap() as usize]; g.online_tap(k); } g.online_tap("kok"); println!("joined {}", c.trim()); }
                    "bcam" => {   // bcam:n:back:up:side - follow fireball n through the air
                        let f = |i: usize, d: f64| parts.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(d); bfol = Some((f(1, 0.0) as i64, f(2, 14.0), f(3, 3.0), f(4, 5.0))); }
                    "vbench" => { let gd = fogwood_core::bridge::lock(); let e = gd.as_ref().unwrap(); let t0 = std::time::Instant::now(); let mut n = 0usize; let mut cnt = [0i32; 5];
                        for k in 0..2000 { let tv = 50.0 + k as f64 * 0.0167; let f = fogwood_core::game::volc::frame(&g.rt, &e.world, tv, tv - 0.0167, g.cam_pos); n += f.fire.m.len(); cnt = [cnt[0].max(f.fire.n()), cnt[1].max(f.solid.n()), cnt[2].max(f.glow.n()), cnt[3].max(f.ring.n()), cnt[4].max(f.disc.n())]; }
                        println!("vbench {:.3} ms/frame, max instances fire/solid/glow/ring/disc {:?} ({})", t0.elapsed().as_secs_f64() * 1000.0 / 2000.0, cnt, n); }
                    "vcam" => { bfol = None;   // vcam:lead:dist:up:yaw - a still camera on the fireball that lands about `lead` s from now
                        let f = |i: usize, d: f64| parts.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(d); let (lead, dist, up, yaw, look) = (f(1, 1.0), f(2, 22.0), f(3, 5.0), f(4, 0.0), f(5, 3.0));
                        let tv = (g.wave_t0 + g.time).rem_euclid(fogwood_core::game::volc::PERIOD); let n = ((tv + lead - fogwood_core::game::volc::FLIGHT) / fogwood_core::game::volc::SLOT).round() as i64;
                        let gd = fogwood_core::bridge::lock(); let e = gd.as_ref().unwrap();
                        if let Some(b) = fogwood_core::game::volc::ball(&g.rt, &e.world, n) { let p = b.to;
                            // yaw > 6: down the road from the landing, `dist` back along it (and yaw - 7 m to the side)
                            let (ex, ez) = if yaw > 6.0 { let k = (0..g.rt.path.len()).min_by(|&i, &j| { let (a, c) = (g.rt.path[i], g.rt.path[j]); ((a[0] - p[0]).hypot(a[1] - p[2])).partial_cmp(&(c[0] - p[0]).hypot(c[1] - p[2])).unwrap() }).unwrap();
                                let kb = (k + g.rt.path.len() - dist as usize) % g.rt.path.len(); let (q, tn) = (g.rt.path[kb], g.rt.tan[kb]); (q[0] - tn[1] * (yaw - 7.0), q[1] + tn[0] * (yaw - 7.0)) } else { (p[0] + dist * yaw.cos(), p[2] + dist * yaw.sin()) };
                            g.cam_ovr = Some(([ex, e.world.height(ex, ez) + up, ez], [p[0], p[1] + look, p[2]], 50.0)); println!("vcam ball {} lands at t+{:.2} at {:.1},{:.1},{:.1}", n, b.t0 + fogwood_core::game::volc::FLIGHT - tv, p[0], p[1], p[2]); } }
                    "settings" => g.open_settings(),
                    "garage" => g.open_garage(), "gnext" => g.garage_go(1), "gprev" => g.garage_go(-1), "gclose" => g.close_garage(),
                    "dust" => { let d = &g.dust; let mut v: Vec<(f32, f32, f32)> = (0..d.n).filter(|&i| d.a[i] > 0.0).map(|i| (d.a[i], d.s[i], d.col[i * 3])).collect(); v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
                        println!("dust live {} visible {} kinds {:?} top {:?}", d.live, v.len(), d.kind_counts(), &v[..v.len().min(5)]); }
                    "car" => { let k: usize = parts[1].parse().unwrap_or(0); g.choose_car(k); }
                    "mtap" => { g.menu_tap(parts[1]); }   // a main-menu key (m_drive, m_stage, m_online, m_garage, m_opts, m_sprev, m_snext, m_mprev, m_mnext)
                    "gpaint" => { g.garage_paint(parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(1)); }   // next livery for the car in the garage
                    "hit" => { let f = |i: usize| parts.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0); let (dx, dz) = (f(4), f(5)); let l = dx.hypot(dz).max(1e-6);
                        g.crash.hit(fogwood_core::game::crash::Shape::Circle { c: [f(1), f(2)], r: f(3), top: 10.0 }, [dx / l, 0.0, dz / l], f(6)); }   // hit:cx:cz:r:dx:dz:speed: a post of radius r at (cx, cz), car frame
                    "hitcar" => { let f = |i: usize| parts.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0); let (dx, dz) = (f(5), f(6)); let l = dx.hypot(dz).max(1e-6); let (ax, az) = (f(3), f(4)); let la = ax.hypot(az).max(1e-6);
                        g.crash.hit(fogwood_core::game::crash::Shape::Box { c: [f(1), f(2)], ax: [ax / la, az / la], hl: 2.05, hw: 0.9, top: 1.3 }, [dx / l, 0.0, dz / l], f(7)); }   // hitcar:cx:cz:axx:axz:dx:dz:speed: another car
                    "sclose" => g.close_settings(),
                    "stap" => { let f: f64 = parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0.5); let key = parts[1].replace(".", ":"); println!("stap {} {} vol={} hap={} pix={} rays={}", key, g.settings_tap_key(&key, f), g.ps.vol, g.ps.hap, g.ps.pix_idx, g.ps.rays); }
                    "netstat" => { fogwood_core::net::with(|s| println!("traffic t={:.1} bytes {:?} packets {:?}", t, s.traffic.get(), s.packets.get())); }
                    "net" => { fogwood_core::net::with(|s| println!("net t={:.1} host={} up={} peers={:?} host_now={:.2}", t, s.is_host, s.brokers_up(), s.peers.values().map(|p| (p.name.clone(), p.link(), (p.rtt * 1000.0) as i32)).collect::<Vec<_>>(), s.host_now())); }
                    "rstate" => { println!("race t={:.1} on={} online={} phase={:?} go={:.2} race_t={:.2} pos={} cars={:?}", t, g.race.on, g.race.online, g.race.phase, g.go, g.race.t, g.race.pos,
                        g.race.cars.iter().map(|c| (c.tag(), (c.car.x * 10.0).round() / 10.0, (c.car.z * 10.0).round() / 10.0, c.prog as i64)).collect::<Vec<_>>()); }
                    "aisim" => {   // same as the page's AI lap test: placeAt(0), demo driver + physics at 60 Hz until a full lap
                        let lock = fogwood_core::bridge::lock(); let e = lock.as_ref().unwrap(); let np = g.np;
                        g.place_at(&e.world, 0); let (mut tt, mut prog, mut last) = (0.0, 0i64, fogwood_core::game::path_idx(&e.world, g.car.x, g.car.z) as i64);
                        while tt < 260.0 { let inp = g.ai_input(&e.world); g.car.update(&e.world, &e.obstacles, &g.tune, 1.0 / 60.0, &inp);
                            let i = fogwood_core::game::path_idx(&e.world, g.car.x, g.car.z) as i64; let mut d = i - last; if d < -900 { d += np as i64; } if d > 900 { d -= np as i64; } prog += d; last = i;
                            if prog >= np as i64 { break; } tt += 1.0 / 60.0; }
                        println!("aisim t={:.1} prog={} x={:.6} z={:.6}", tt, prog, g.car.x, g.car.z); }
                    "burst" => { burst = parts[1].parse().unwrap_or(30); }
                    "shot" => {
                        r.draw(Some(fb), w, h).expect("draw"); let gl = r.gl(); gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
                        let mut px = vec![0u8; (w * h * 4) as usize]; gl.read_pixels(0, 0, w, h, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(&mut px));
                        let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
                        for y in (0..h).rev() { for x in 0..w { let i = ((y * w + x) * 4) as usize; out.extend_from_slice(&px[i..i + 3]); } }
                        let f = format!("{}/f{:07.3}.ppm", a[3], t); std::fs::write(&f, out).unwrap(); println!("shot {} calls {} tris {}", f, r.stats.0, r.stats.1);
                    }
                    "audio" => { println!("audio rms {:.4} peak {:.3} clipped blocks {:.1}%", (arms / anb.max(1.0)).sqrt(), apk, aclip / anb.max(1.0) * 100.0); arms = 0.0; apk = 0.0; anb = 0.0; aclip = 0.0; }
                    "cars" => { let pl = g.race.player; for (k, c) in g.race.cars.iter().enumerate() { let s = if k == pl { &g.car } else { &c.car };
                        println!("t={:.2} car {} {} x={:.2} z={:.2} u={:.1} prog={:.0} bot={:?} remote={} ai={} left={} fin={}", t, k, if k == pl { "YOU".to_string() } else { c.tag() }, s.x, s.z, s.u, c.prog, c.bot, c.remote.map(|r| format!("{:08x}", r)).unwrap_or("-".into()), c.is_ai(), c.left, c.finished); }
                        if let Some((b, r, np, links)) = fogwood_core::net::with(|s| (s.race_bots, s.car_rate(), s.peers.len(), s.peers.values().map(|p| format!("{:?}", p.link())).collect::<Vec<_>>())) { println!("t={:.2} race_bots={} rate={:?} peers={} {:?} msg={:?}", t, b, r, np, links, g.msg); } }
                    "state" => { let c = &g.car; println!("t={:.2} state={} x={:.3} z={:.3} h={:.4} u={:.2} gear={} rpm={:.0} lap={} best={:?} race={} pos={} phase={:?}", t, if g.state == State::Drive { "drive" } else { "title" }, c.x, c.z, c.h, c.u, c.gear, c.rpm, g.lap, g.best, g.race.on, g.race.pos, g.race.phase); if g.race.on { println!("  rival resets {:?}", g.race.cars.iter().map(|c| c.resets).collect::<Vec<_>>()); }
                        { let gd = fogwood_core::bridge::lock(); if let Some(e) = gd.as_ref() { println!("  car above ground {:+.3} m", g.car.y - e.world.height(g.car.x, g.car.z)); } } }
                    _ => println!("unknown {}", act),
                }
            }
            if let Some((n, back, up, side)) = bfol { let gd = fogwood_core::bridge::lock(); let e = gd.as_ref().unwrap(); use fogwood_core::game::volc as vc;
                if let Some(b) = vc::ball(&g.rt, &e.world, n) { let tv = (g.wave_t0 + g.time).rem_euclid(vc::PERIOD); let u = ((tv - b.t0) / vc::FLIGHT).clamp(0.0, 1.0); let p = b.pos(u); let q = b.pos((u - 0.02).max(0.0));
                    let (dx, dy, dz) = (p[0] - q[0], p[1] - q[1], p[2] - q[2]); let l = (dx * dx + dy * dy + dz * dz).sqrt().max(1e-6); let (dx, dy, dz) = (dx / l, dy / l, dz / l); let (sx, sz) = (-dz, dx);
                    g.cam_ovr = Some(([p[0] - dx * back + sx * side, p[1] - dy * back + up, p[2] - dz * back + sz * side], [p[0] + dx * 4.0, p[1] + dy * 4.0, p[2] + dz * 4.0], 55.0)); } }
            if burst > 0 { burst -= 1; r.draw(Some(fb), w, h).expect("draw"); let gl = r.gl(); gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
                let mut px = vec![0u8; (w * h * 4) as usize]; gl.read_pixels(0, 0, w, h, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(&mut px));
                let mut out = Vec::with_capacity((w * h * 3) as usize); for y in (0..h).rev() { for x in 0..w { let i = ((y * w + x) * 4) as usize; out.extend_from_slice(&px[i..i + 3]); } }
                std::fs::write(format!("{}/b{:06}.rgb", a[3], (t * 60.0).round() as i64), out).unwrap(); }
            if let Some(st) = fuzz.as_mut() {   // FUZZ=seed: random touches, back presses and menu keys every few frames (panic hunting)
                let mut rnd = || { *st ^= *st << 13; *st ^= *st >> 7; *st ^= *st << 17; (*st >> 11) as f64 / (1u64 << 53) as f64 };
                let p = rnd();
                if p < 0.08 { let (x, y, id) = (rnd() * w as f64, rnd() * h as f64, (rnd() * 3.0) as i32); let act = [0, 1, 2, 5, 6][(rnd() * 5.0) as usize]; g.touch(act, id, x, y); }
                else if p < 0.09 { g.back(); }
                else if p < 0.10 { let k = fogwood_core::game::menu::KEYS[(rnd() * fogwood_core::game::menu::KEYS.len() as f64) as usize]; g.menu_tap(k); }
                else if p < 0.105 { g.garage_paint(1); } else if p < 0.107 { g.resize((200.0 + rnd() * 1800.0), (150.0 + rnd() * 900.0), [0.0; 4]); }
                else if p < 0.108 { g.to_title(); } else if p < 0.109 { g.start_game(); g.auto = true; }
            }
            let t0 = std::time::Instant::now(); let m0 = g.msg.clone(); g.tick(&mut r, dt); if g.msg != m0 && !g.msg.is_empty() { println!("t={:.2} msg {}", t, g.msg); } if std::env::var("HUDCHECK").is_ok() && !g.hud.verify() { println!("HUD MISMATCH at t={:.2}", t); } tick_ms += t0.elapsed().as_secs_f64() * 1000.0; ticks += 1; if g.state == fogwood_core::game::State::Drive { profs.push(g.prof); }
            fogwood_audio::native::render(&mut audio); if let Some(fh) = adump.as_mut() { use std::io::Write; let b: Vec<u8> = audio.iter().flat_map(|v| v.to_le_bytes()).collect(); fh.write_all(&b).unwrap(); } { let (mut s2, mut pk) = (0f64, 0f32); for v in &audio { s2 += (*v as f64) * (*v as f64); pk = pk.max(v.abs()); if !v.is_finite() { println!("audio NaN"); break; } } arms += s2 / audio.len() as f64; apk = apk.max(pk); anb += 1.0; if pk >= 0.999 { aclip += 1.0; } }
            if log && t >= next_log { next_log = t + 1.0; r.draw(Some(fb), w, h).expect("draw"); let c = &g.car; println!("log calls={} tris={} t={:.1} x={:.2} z={:.2} kmh={:.0} gear={} sunvis={:.2} race_t={:.1} pos={} cars={:?}", r.stats.0, r.stats.1, t, c.x, c.z, c.u * 3.6, c.gear, g.sun_vis, g.race.t, g.race.pos,
                g.race.cars.iter().map(|q| (q.r.map(|i| fogwood_core::game::race::RIVALS[i].tag).unwrap_or("YOU"), (q.prog as i64), q.resets)).collect::<Vec<_>>()); }
            for e in g.events.drain(..) { if !e.starts_with("h ") { println!("event {}", e); } }
            t += dt;
            if let Ok(tp) = std::env::var("FW_TRACE") { if g.race.online && g.race.on {
                let line = fogwood_core::net::with(|s| { let mut l = format!("{:.4} {:.3} {:.3}", s.host_now(), g.car.x, g.car.z);
                    for c in &g.race.cars { if let Some(id) = c.remote { l += &format!(" {} {:.3} {:.3}", id, c.car.x, c.car.z); } } l += &format!(" me {}", s.me); l });
                if let Some(l) = line { use std::io::Write; let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&tp).unwrap(); writeln!(f, "{}", l).unwrap(); } } }
            if std::env::var("FW_REALTIME").is_ok() { let target = wall0.elapsed().as_secs_f64(); if t > target { std::thread::sleep(std::time::Duration::from_secs_f64(t - target)); } }
        }
        println!("tick avg {:.3} ms over {} frames (incl. hud)", tick_ms / ticks as f64, ticks);
        if !profs.is_empty() { let names = ["physics", "race", "sound+crash", "fx", "camera", "rivals", "world", "hud"];
            for (k, nm) in names.iter().enumerate() { let mut v: Vec<f32> = profs.iter().map(|p| p[k]).collect(); v.sort_by(|a, b| a.partial_cmp(b).unwrap()); let n = v.len();
                println!("  {:12} mean {:.3} p50 {:.3} p95 {:.3} p99 {:.3} max {:.3} ms", nm, v.iter().sum::<f32>() / n as f32, v[n / 2], v[n * 95 / 100], v[n * 99 / 100], v[n - 1]); } }
    }
}
