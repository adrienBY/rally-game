//! smoke_stats rt...: tyre smoke on a stage's tarmac. Drives one lap with the demo driver (analog controls, standing start)
//! (`SMOKE_HB=1`: plus a 1.5 s handbrake slide every 10 s, like a player throwing the car into bends)
//! and reports how much of the lap the tyres slide, how often each rule would have smoked, and the puffs it would throw.
use fogwood_core::game::{dust, Game, Persist};
use fogwood_core::physics::Input;
use fogwood_core::world::ROAD;

fn main() {
    println!("{:8} {:>6} {:>8} {:>8} {:>9} {:>9} {:>9} {:>9}", "map", "lap s", "slip>2%", "slip>5%", "old smk%", "old puf/s", "new smk%", "new puf/s");
    for f in std::env::args().skip(1) {
        let rt = fogwood_core::game::rt::parse(&std::fs::read(&f).unwrap()).expect("rt");
        let path: Vec<String> = rt.path.iter().flat_map(|p| [p[0].to_string(), p[1].to_string()]).collect();
        let roadh: Vec<String> = rt.roadh.iter().map(|v| v.to_string()).collect();
        fogwood_core::bridge::prepare(rt.map_id, &path.join(","), &roadh.join(","), rt.gxh, rt.gzh);
        let obs: Vec<String> = rt.obstacles.iter().flat_map(|o| [o[0].to_string(), o[1].to_string(), o[2].to_string()]).collect();
        fogwood_core::bridge::set_obstacles(&obs.join(","));
        let np = rt.np as i64; let map = rt.map.clone();
        let mut g = Game::new(rt, Persist::new("/tmp/smoke_stats.sav"));
        let lock = fogwood_core::bridge::lock(); let e = lock.as_ref().unwrap();
        g.place_at(&e.world, 0);
        let mut last = fogwood_core::game::path_idx(&e.world, g.car.x, g.car.z) as i64; let mut prog = 0i64; let mut t = 0.0;
        let (mut n, mut s2, mut s5, mut old_on, mut old_p, mut new_on, mut new_p) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let mut heat = [0.0f32; 4]; let dt = 1.0 / 60.0; let hbm = std::env::var("SMOKE_HB").is_ok();
        while t < 400.0 {
            let ai = g.ai_input(&e.world);
            let inp = Input { steer: ai.steer, throttle: if t < 2.0 { 1.0 } else { ai.throttle }, brake: ai.brake, hb: hbm && t > 5.0 && t % 10.0 < 1.5 };
            g.car.update(&e.world, &e.obstacles, &g.tune, dt, &inp);
            let c = &g.car; let sp = c.vx.hypot(c.vz) as f32; n += 1.0;
            let (mut o_any, mut n_any) = (false, false);
            for k in 0..4 { let w = &c.wh[k]; let load = if w.fz > 0.0 { w.fz as f32 } else { 0.0 }; let slip = w.slide as f32;
                heat[k] = dust::tyre_heat(heat[k], slip, load, sp, dt as f32);
                let (sh, ch) = (c.h.sin(), c.h.cos()); let wx = c.x + ch * w.lx + sh * w.lz; let wz = c.z - sh * w.lx + ch * w.lz;
                if load <= 0.0 || e.world.path_dist(wx, wz) >= ROAD { continue; }
                let lk = (load / 2900.0).sqrt().min(1.6);
                if slip > 2.0 { s2 += 0.25; } if slip > 5.0 { s5 += 0.25; }
                if slip > 2.2 { o_any = true; old_p += ((slip - 2.2) * 12.0 * lk).min(70.0) * dt as f32; }
                let hot = heat[k] - 1.0; if hot > 0.0 && slip > 1.0 { n_any = true; new_p += dust::smoke_rate(hot, slip) * lk * dt as f32; } }
            if o_any { old_on += 1.0; } if n_any { new_on += 1.0; }
            let i = fogwood_core::game::path_idx(&e.world, c.x, c.z) as i64; let mut d = i - last; if d < -900 { d += np; } if d > 900 { d -= np; } prog += d; last = i;
            if prog >= np { break; } t += dt;
        }
        println!("{:8} {:6.1} {:8.1} {:8.1} {:9.1} {:9.1} {:9.1} {:9.1}", map, t, s2 / n * 100.0, s5 / n * 100.0, old_on / n * 100.0, old_p / t as f32, new_on / n * 100.0, new_p / t as f32);
    }
}
