//! prism_check <rt.bin>: the Prism Skyway's barriers and air control, on the stage's own world. Drives a car into each barrier at a
//! range of speeds and angles (it must stay on the deck), then throws a car into the air and checks steering and throttle move it.
use fogwood_core::physics::{Car, Input, Tune};
use fogwood_core::world::{Wave, PRISM_RAIL};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let rt = fogwood_core::game::rt::parse(&std::fs::read(a.get(1).map(|s| s.as_str()).unwrap_or("/tmp/prism_stage/rt.bin")).unwrap()).expect("rt");
    let path: Vec<String> = rt.path.iter().flat_map(|p| [p[0].to_string(), p[1].to_string()]).collect();
    let roadh: Vec<String> = rt.roadh.iter().map(|v| v.to_string()).collect();
    println!("prepare {}", fogwood_core::bridge::prepare(rt.map_id, &path.join(","), &roadh.join(","), rt.gxh, rt.gzh));
    let mut tv = [0f64; 33];
    for (i, n) in fogwood_core::physics::PARAM_NAMES.iter().enumerate() { if let Some((_, v)) = rt.tune.iter().find(|(k, _)| k == n) { tv[i] = *v; } }
    tv[29] = rt.susp[0]; tv[30] = rt.susp[1]; tv[31] = rt.susp[2]; tv[32] = rt.iz;
    let tune = Tune::from_slice(&tv);
    let g = fogwood_core::bridge::lock(); let e = g.as_ref().unwrap(); let w = &e.world;
    let wv = w.wave.get_or_init(|| Wave { path: rt.path.iter().map(|p| (p[0], p[1])).collect(), tan: rt.tan.iter().map(|t| (t[0], t[1])).collect(), secs: rt.wave_secs.clone() });
    let np = rt.np; let dt = 1.0 / 60.0;
    let place = |i: usize, lat: f64, yaw: f64, v: f64, up: f64| { let mut c = Car::new(); let (tx, tz) = (rt.tan[i][0], rt.tan[i][1]);
        c.x = rt.path[i][0] - tz * lat; c.z = rt.path[i][1] + tx * lat; c.h = tx.atan2(tz) + yaw; c.vx = c.h.sin() * v; c.vz = c.h.cos() * v;
        c.gear = 3.0; c.rpm = 5000.0; c.y = w.height(c.x, c.z) + 0.075 + up; for wh in c.wh.iter_mut() { wh.comp = 0.135; wh.d = 0.075; wh.fz = 2900.0; wh.w = v / 0.32; } c };
    // barriers
    let (mut worst, mut fails, mut n) = (0.0f64, 0, 0);
    for &i in &[200usize, 640, 1000, 1300, 1700] { for &side in &[-1.0f64, 1.0] { for &yaw in &[0.15f64, 0.35, 0.6, 1.0] { for &v in &[12.0f64, 25.0, 40.0] { for &tw in &[0.0f64, 1.3, 2.6] {
        w.set_wave_time(tw);
        let mut c = place(i, 0.0, -side * yaw, v, 0.0); let mut maxlat = 0.0f64; let mut fell = false;
        for _ in 0..(2.5 / dt) as usize { c.update(w, &e.obstacles, &tune, dt, &Input { steer: 0.0, throttle: 1.0, brake: 0.0, hb: false });
            let (k, lat, _) = w.road_frame(wv, c.x, c.z); maxlat = maxlat.max(lat.abs()); if c.y < rt.roadh[k] - 6.0 { fell = true; break; } }
        n += 1; worst = worst.max(maxlat); if fell || maxlat > PRISM_RAIL { fails += 1; if fails <= 12 { println!("  through/over: idx {} side {} yaw {} v {} -> max lat {:.2} fell {}", i, side, yaw, v, maxlat, fell); } }
    } } } } }
    println!("barriers: {} runs, {} lost, worst centre offset {:.2} m (rail at {:.2})", n, fails, worst, PRISM_RAIL);
    // air control: 6 m up at 30 m/s on the straightest stretch, 0.5 s of input
    w.set_wave_time(0.0);
    for (name, inp) in [("none", Input::default()), ("steer right", Input { steer: 1.0, ..Default::default() }), ("steer left", Input { steer: -1.0, ..Default::default() }), ("throttle", Input { throttle: 1.0, ..Default::default() }), ("brake", Input { brake: 1.0, ..Default::default() })] {
        let st = (0..np).max_by(|&a, &b| { let f = |i: usize| rt.tan[i][0] * rt.tan[(i + 30) % np][0] + rt.tan[i][1] * rt.tan[(i + 30) % np][1]; f(a).partial_cmp(&f(b)).unwrap() }).unwrap();
        let mut c = place(st, 0.0, 0.0, 30.0, 6.0); let (h0, c0) = (c.h, c.vx.atan2(c.vz));
        for _ in 0..(0.5 / dt) as usize { c.update(w, &e.obstacles, &tune, dt, &inp); }
        let wr = |a: f64| fogwood_core::noise::wrap_a(a).to_degrees(); println!("air {:12}: nose turned {:+.1} deg, course turned {:+.1} deg, speed {:.1} m/s, contacts {}", name, wr(c.h - h0), wr(c.vx.atan2(c.vz) - c0), c.vx.hypot(c.vz), c.contacts);
    }
    if let Ok(dbg) = std::env::var("DBG") { let v: Vec<f64> = dbg.split(',').map(|x| x.parse().unwrap()).collect();
        w.set_wave_time(v[4]); let mut c = place(v[0] as usize, 0.0, -v[1] * v[2], v[3], 0.0);
        for f in 0..(2.5 / dt) as usize { c.update(w, &e.obstacles, &tune, dt, &Input { steer: 0.0, throttle: 1.0, brake: 0.0, hb: false });
            let (k, lat, _) = w.road_frame(wv, c.x, c.z); let lc = lat.clamp(-3.4, 3.4); let (px, pz) = wv.path[k]; let (tx, tz) = wv.tan[k];
            let deck = w.height(px - tz * lc, pz + tx * lc);
            if f % 3 == 0 { println!("{:.2} k {} lat {:+.2} y-deck {:+.2} vy {:+.1} sp {:.1} contacts {} pitch {:+.2} roll {:+.2}", f as f64 * dt, k, lat, c.y - deck, c.vy, c.vx.hypot(c.vz), c.contacts, c.pitch, c.roll); } } }
    let _ = np;
}
