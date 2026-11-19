//! prism_check <rt.bin>: the Prism Skyway's edge and air control, on the stage's own world. Drives cars off the deck edge at a range of
//! speeds and steering amounts (they must just drop off, not be flicked up or sideways), then throws a car into the air and measures
//! how much steering, throttle and brake move it.
use fogwood_core::physics::{Car, Input, Tune};
use fogwood_core::world::{Wave, PRISM_EDGE};

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
    let wv = w.wave.get_or_init(|| Wave::new(&rt.path, &rt.tan, &rt.roadh, &rt.wave_secs));
    let np = rt.np; let dt = 1.0 / 60.0;
    let place = |i: usize, lat: f64, yaw: f64, v: f64, up: f64| { let mut c = Car::new(); let (tx, tz) = (rt.tan[i][0], rt.tan[i][1]);
        c.x = rt.path[i][0] - tz * lat; c.z = rt.path[i][1] + tx * lat; c.h = tx.atan2(tz) + yaw; c.vx = c.h.sin() * v; c.vz = c.h.cos() * v;
        c.gear = 3.0; c.rpm = 5000.0; c.y = w.height(c.x, c.z) + 0.075 + up; for wh in c.wh.iter_mut() { wh.comp = 0.135; wh.d = 0.075; wh.fz = 2900.0; wh.w = v / 0.32; } c };
    // the edge: drive off it at a range of speeds and steering amounts and look for a flick - the car thrown up or sideways as it goes
    // over (it should just drop off: no more upward speed than it had, no sideways shove beyond what it carried)
    let (mut n, mut off, mut worst_up, mut worst_side) = (0, 0, 0.0f64, 0.0f64); let mut worst_where = String::new();
    for &i in &[200usize, 640, 1000, 1300, 1700] { for &side in &[-1.0f64, 1.0] { for &yaw in &[0.15f64, 0.35, 0.6, 1.0] { for &v in &[12.0f64, 25.0, 40.0] { for &tw in &[0.0f64, 1.3, 2.6] {
        w.set_wave_time(tw - 0.6); let mut c = place((i + np - 20) % np, 0.0, 0.0, v, 0.0); let steer = -side * yaw.min(1.0); let mut left: Option<(f64, f64, f64, usize)> = None;
        for f0 in 0..(3.1 / dt) as usize { let settle = f0 < 36; w.set_wave_time(tw + (f0 as f64 - 36.0) * dt); let sp = c.vx.hypot(c.vz);
            let inp = if settle { Input { steer: 0.0, throttle: if sp < v { 1.0 } else { 0.0 }, brake: 0.0, hb: false } } else { Input { steer, throttle: 1.0, brake: 0.0, hb: false } };
            let pre = c; c.update(w, &e.obstacles, &tune, dt, &inp);
            let (k, lat, _) = w.road_frame(wv, c.x, c.z);
            if left.is_none() && lat.abs() > PRISM_EDGE && c.contacts == 0.0 { left = Some((pre.vy.max(0.0), pre.vx, pre.vz, f0)); }
            if let Some((vy0, vx0, vz0, f1)) = left { if f0 < f1 + 30 && lat.abs() > PRISM_EDGE { let dv = (c.vx - vx0).hypot(c.vz - vz0); worst_up = worst_up.max(c.vy - vy0);
                if dv > worst_side { worst_side = dv; worst_where = format!("idx {} side {} steer {} v {}", i, side, yaw, v); } } }
            if std::env::var("DBG3").map(|d| d == format!("{}:{}:{}:{}:{}", i, side, yaw, v, tw)).unwrap_or(false) && f0 > 36 {
                println!("{} lat {:+.2} y-deck {:+.2} v ({:+.1},{:+.1}) vy {:+.1} contacts {} roll {:+.2} pitch {:+.2} r {:+.2} fz {:?}", f0, lat, c.y - w.height(wv.path[k].0, wv.path[k].1), c.vx, c.vz, c.vy, c.contacts, c.roll, c.pitch, c.r, c.wh.iter().map(|q| q.fz as i64).collect::<Vec<_>>()); }
            if c.y < rt.roadh[k] - 6.0 { break; } }
        n += 1; if left.is_some() { off += 1; }
    } } } } }
    println!("edge: {} runs, {} went off; over the void, in the half second after the last wheel leaves the deck: most upward speed gained {:+.2} m/s, largest change of horizontal velocity {:.2} m/s ({})", n, off, worst_up, worst_side, worst_where);
    // air control: 6 m up at 30 m/s on the straightest stretch, 0.5 s of input
    w.set_wave_time(0.0);
    for (name, inp) in [("none", Input::default()), ("steer right", Input { steer: 1.0, ..Default::default() }), ("steer left", Input { steer: -1.0, ..Default::default() }), ("throttle", Input { throttle: 1.0, ..Default::default() }), ("brake", Input { brake: 1.0, ..Default::default() })] {
        let st = (0..np).max_by(|&a, &b| { let f = |i: usize| rt.tan[i][0] * rt.tan[(i + 30) % np][0] + rt.tan[i][1] * rt.tan[(i + 30) % np][1]; f(a).partial_cmp(&f(b)).unwrap() }).unwrap();
        let mut c = place(st, 0.0, 0.0, 30.0, 6.0); let (h0, c0) = (c.h, c.vx.atan2(c.vz));
        for _ in 0..(0.5 / dt) as usize { c.update(w, &e.obstacles, &tune, dt, &inp); }
        let wr = |a: f64| fogwood_core::noise::wrap_a(a).to_degrees(); println!("air {:12}: nose turned {:+.1} deg, course turned {:+.1} deg, speed {:.1} m/s, contacts {}", name, wr(c.h - h0), wr(c.vx.atan2(c.vz) - c0), c.vx.hypot(c.vz), c.contacts);
    }
    let _ = np;
}
