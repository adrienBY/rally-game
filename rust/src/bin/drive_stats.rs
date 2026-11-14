//! drive_stats rt...: per-map road and handling numbers. Drives one lap with the demo driver's line through a
//! button-style controller (left/right/gas/brake on or off, like the touch controls) and reports what the stage asks of the player.
use fogwood_core::game::{Game, Persist};
use fogwood_core::physics::Input;

fn main() {
    println!("{:8} {:>5} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "map", "km", "fast%", "mid%", "slow%", "hpin%", "Sbnd", "climb", "crest", "vplan", "kmh", "lap", "taps", "corr", "slide%", "offrd%", "air%", "bump", "pitch", "roll", "muR/G");
    for f in std::env::args().skip(1) {
        let rt = fogwood_core::game::rt::parse(&std::fs::read(&f).unwrap()).expect("rt");
        let path: Vec<String> = rt.path.iter().flat_map(|p| [p[0].to_string(), p[1].to_string()]).collect();
        let roadh: Vec<String> = rt.roadh.iter().map(|v| v.to_string()).collect();
        fogwood_core::bridge::prepare(rt.map_id, &path.join(","), &roadh.join(","), rt.gxh, rt.gzh);
        let obs: Vec<String> = rt.obstacles.iter().flat_map(|o| [o[0].to_string(), o[1].to_string(), o[2].to_string()]).collect();
        fogwood_core::bridge::set_obstacles(&obs.join(","));
        let np = rt.np; let km = np as f64 / 1000.0; let map = rt.map.clone();
        // ---- road geometry: curvature over 12 m, road height crests/dips
        let (mut fast, mut mid, mut slow, mut hp) = (0, 0, 0, 0); let mut sb = 0; let mut last_sign = 0i32; let mut climb = 0.0; let mut crest = 0.0;
        let mut vplan = 0.0;
        for i in 0..np {
            let a = rt.tan[(i + np - 6) % np]; let b = rt.tan[(i + 6) % np];
            let d = (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1]) / 12.0;   // rad per m
            let r = 1.0 / d.abs().max(1e-6);
            if r > 150.0 { fast += 1 } else if r > 60.0 { mid += 1 } else if r > 25.0 { slow += 1 } else { hp += 1 }
            if r < 120.0 { let s = if d > 0.0 { 1 } else { -1 }; if last_sign != 0 && s != last_sign { sb += 1; } last_sign = s; }
            let dh = rt.roadh[(i + 1) % np] - rt.roadh[i]; if dh > 0.0 { climb += dh; }
            let c = rt.roadh[(i + 5) % np] - 2.0 * rt.roadh[i] + rt.roadh[(i + np - 5) % np]; crest += c * c;   // height curvature over 5 m
            vplan += rt.vprof[i];
        }
        // obstacles (trunks, rocks, posts) close to the road line
        let mut near = [0usize; 3];
        for o in &rt.obstacles { let mut best = 1e9f64; for k in (0..np).step_by(2) { let p = rt.path[k]; let d = (p[0] - o[0]).hypot(p[1] - o[1]); if d < best { best = d; } }
            let d = best - o[2]; for (q, lim) in [5.0, 8.0, 12.0].iter().enumerate() { if d < *lim { near[q] += 1; } } }
        eprintln!("{:8} obstacles {} near 5/8/12 m per km {:.0}/{:.0}/{:.0}", rt.map, rt.obstacles.len(), near[0] as f64 / km, near[1] as f64 / km, near[2] as f64 / km);
        let pc = |n: i32| n as f64 / np as f64 * 100.0;
        // ---- one lap: demo line through button controls
        let mut g = Game::new(rt, Persist::new("/tmp/drive_stats.sav"));
        let lock = fogwood_core::bridge::lock(); let e = lock.as_ref().unwrap();
        g.place_at(&e.world, 0);
        let mut last = fogwood_core::game::path_idx(&e.world, g.car.x, g.car.z) as i64; let mut prog = 0i64; let mut t = 0.0;
        let (mut taps, mut corr, mut slide, mut off, mut air, mut bump, mut pr, mut rr, mut n, mut su) = (0, 0, 0, 0, 0, 0.0, 0.0, 0.0, 0, 0.0);
        let mut held = 0.0f64; let mut lastvy = g.car.vy; let mut last_dir = 0.0;
        while t < 300.0 {
            let ai = g.ai_input(&e.world); let lk = g.steer_lock(g.car.u);
            // button steering: hold a side while the rack is short of where the line wants it, let go once it gets there
            let want = ai.steer * lk; let err = want - g.car.steer;
            let dir = if held != 0.0 { if err * held > 0.01 || (want.abs() > 0.95 * lk && want * held > 0.0) { held } else { 0.0 } } else if err.abs() > 0.05 { err.signum() } else { 0.0 };
            if dir != 0.0 && held == 0.0 { taps += 1; if last_dir != 0.0 && dir != last_dir { corr += 1; } last_dir = dir; }
            held = dir;
            let inp = Input { steer: dir, throttle: if ai.throttle > 0.4 { 1.0 } else { 0.0 }, brake: if ai.brake > 0.25 { 1.0 } else { 0.0 }, hb: false };
            g.car.update(&e.world, &e.obstacles, &g.tune, 1.0 / 60.0, &inp);
            let c = &g.car; n += 1; su += c.u;
            if c.vl.abs() > 1.5 { slide += 1; } if !c.on_road { off += 1; } if c.contacts < 0.5 { air += 1; }
            let ay = (c.vy - lastvy) * 60.0; lastvy = c.vy; bump += ay * ay; pr += c.pr * c.pr; rr += c.rr * c.rr;
            let i = fogwood_core::game::path_idx(&e.world, c.x, c.z) as i64; let mut d = i - last; if d < -900 { d += np as i64; } if d > 900 { d -= np as i64; } prog += d; last = i;
            if prog >= np as i64 { break; } t += 1.0 / 60.0;
        }
        let nf = n as f64;
        println!("{:8} {:5.2} {:6.1} {:6.1} {:6.1} {:6.1} {:6.1} {:6.1} {:6.3} {:6.1} {:6.1} {:6.1} {:6.1} {:6.1} {:6.1} {:6.1} {:6.2} {:6.2} {:6.3} {:6.3} {:.2}/{:.2}",
            map, km, pc(fast), pc(mid), pc(slow), pc(hp), sb as f64 / km, climb / km, (crest / np as f64).sqrt() * 100.0, vplan / np as f64 * 3.6, su / nf * 3.6, t,
            taps as f64 / km, corr as f64 / km, slide as f64 / nf * 100.0, off as f64 / nf * 100.0, air as f64 / nf * 100.0, (bump / nf).sqrt(), (pr / nf).sqrt(), (rr / nf).sqrt(), g.tune.mu_road, g.tune.mu_grass);
    }
}
