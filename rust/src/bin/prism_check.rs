//! prism_check <rt.bin>: the Prism Skyway's edge and air control, on the stage's own world. Drives cars off the deck edge at a range of
//! speeds and steering amounts (they must just drop off, not be flicked up or sideways), then throws a car into the air and measures
//! how much steering, throttle and brake move it.
use fogwood_core::physics::{Car, Input, Tune};
use fogwood_core::world::{Wave, PRISM_EDGE, PRISM_KERB};

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
    // saving it: drift towards the edge at a shallow angle, and when a wheel is over the void (at once, or a little late) steer back hard - the car must not sink into the deck
    // (its reference point stays above the deck surface while it is over the deck), and it should catch the edge, not pop up off it
    let (mut runs, mut saved, mut worst_sink, mut worst_pop) = (0, 0, 0.0f64, 0.0f64); let mut by_speed = std::collections::BTreeMap::new(); let mut why = std::collections::BTreeMap::new(); let mut lat_at_fall: Vec<f64> = Vec::new(); let mut sink_where = String::new();
    for &i in &[200usize, 640, 1000, 1300, 1700] { for &side in &[-1.0f64, 1.0] { for &yaw in &[0.06f64, 0.12, 0.2] { for &v in &[12.0f64, 25.0, 40.0] { for &tw in &[0.0f64, 1.3, 2.6] { for &late in &[0usize, 6, 12] {
        w.set_wave_time(tw - 0.6); let mut c = place((i + np - 20) % np, 0.0, 0.0, v, 0.0); let mut back_at: Option<usize> = None; let mut fell = false; let mut was_out = false;
        for f0 in 0..(4.0 / dt) as usize { let settle = f0 < 36; w.set_wave_time(tw + (f0 as f64 - 36.0) * dt); let sp = c.vx.hypot(c.vz);
            let over = c.wh.iter().any(|q| w.road_frame(wv, q.wx, q.wz).1.abs() > PRISM_KERB); if back_at.is_none() && !settle && over { back_at = Some(f0 + late); }
            if f0 == 36 { let a = -side * yaw; c.h += a; let (cs, sn) = (a.cos(), a.sin()); let (vx, vz) = (c.vx, c.vz); c.vx = vx * cs + vz * sn; c.vz = -vx * sn + vz * cs; }   // drifting at the edge at this angle
            // back towards the middle: positive steering turns the car towards positive lat
            let lat_now = w.road_frame(wv, c.x, c.z).1;
            let steer = if settle { 0.0 } else if back_at.map(|b| f0 >= b).unwrap_or(false) { -lat_now.signum() } else { 0.0 };
            let inp = Input { steer, throttle: if settle && sp >= v { 0.0 } else { 0.6 }, brake: 0.0, hb: false };
            let was_falling = c.falling; let pre_vy = c.vy; c.update(w, &e.obstacles, &tune, dt, &inp);
            if !was_falling && c.falling { let l = w.road_frame(wv, c.x, c.z).1; *why.entry(if l.abs() > PRISM_KERB { "centre past the kerb" } else { "other" }).or_insert(0) += 1; lat_at_fall.push(l.abs()); }
            let (k, lat, s) = w.road_frame(wv, c.x, c.z); if c.y < rt.roadh[k] - 6.0 { fell = true; break; }
            if lat.abs() > PRISM_EDGE || c.falling { was_out = true; }
            if !was_out && lat.abs() < PRISM_KERB - 0.3 { let deck = wv.deck(s, lat) + wv.at(s, w.wave_time()); let sink = deck - c.y;
                if sink > worst_sink { worst_sink = sink; sink_where = format!("idx {} side {} steer {} v {} tw {} late {}", i, side, yaw, v, tw, late); } }
            if std::env::var("DBG4").map(|d| d == format!("{}:{}:{}:{}:{}:{}", i, side, yaw, v, tw, late)).unwrap_or(false) && f0 > 36 {
                let deck = wv.deck(s, lat) + wv.at(s, w.wave_time());
                println!("{} lat {:+.2} y-deck {:+.2} vy {:+.1} steer {:+.1} roll {:+.2} hg {:?} d {:?}", f0, lat, c.y - deck, c.vy, steer, c.roll, c.wh.iter().map(|q| (q.hg - deck) as f32).map(|x| (x * 100.0).round() / 100.0).collect::<Vec<_>>(), c.wh.iter().map(|q| (q.d * 100.0).round() / 100.0).collect::<Vec<_>>()); }
            if back_at.is_some() { worst_pop = worst_pop.max(c.vy - pre_vy.max(0.0)); } }
        if back_at.is_some() { runs += 1; let e2 = by_speed.entry(v as i64).or_insert((0, 0)); e2.0 += 1; if !fell { saved += 1; e2.1 += 1; } }
    } } } } } }
    println!("saving it: {} cars got a wheel over the edge and steered back, {} stayed on (by speed m/s: (tries, saved) {:?}); falls: {:?}, centre offset when it went (median) {:.2} m; deepest the car sank below the deck surface while still over it {:.2} m ({}); most upward speed gained in one frame {:+.2} m/s", runs, saved, by_speed, why, { lat_at_fall.sort_by(|a, b| a.partial_cmp(b).unwrap()); lat_at_fall.get(lat_at_fall.len() / 2).copied().unwrap_or(0.0) }, worst_sink, sink_where, worst_pop);
    // a driver drifting wide: follows the road on a line that creeps out towards the edge (0.5-2 m/s), and once a wheel is over the void
    // aims back for the middle. How often is that saved, and how far out was the car's centre when it was?
    let wrap = |a: f64| fogwood_core::noise::wrap_a(a);
    let (mut tries, mut kept) = (0, 0); let mut kept_at: Vec<f64> = Vec::new(); let mut lost_at: Vec<f64> = Vec::new(); let mut sink2 = 0.0f64;
    for &i in &[100usize, 400, 700, 1000, 1300, 1600] { for &side in &[-1.0f64, 1.0] { for &v in &[15.0f64, 25.0, 35.0] { for &creep in &[0.5f64, 1.0, 2.0] { for &tw in &[0.0f64, 2.0] {
        w.set_wave_time(tw); let mut c = place(i, 0.0, 0.0, v.min(rt.rprof[i] * 0.9), 0.0); let mut target = 0.0f64; let mut back = false; let mut max_out = 0.0f64; let mut fell = false; let mut over = false;
        for f in 0..(8.0 / dt) as usize { w.set_wave_time(tw + f as f64 * dt);
            let (k, lat, s) = w.road_frame(wv, c.x, c.z);
            if !back { target = (target + side * creep * dt).clamp(-9.0, 9.0); if f > 5 && c.wh.iter().any(|q| w.road_frame(wv, q.wx, q.wz).1.abs() > PRISM_KERB) { back = true; over = true; } } else { target = 0.0; }
            let u = c.u.max(0.0); let ld = 5.0 + u * 0.35; let kk = (k + ld as usize) % np; let (tx, tz) = wv.tan[kk]; let (px, pz) = (wv.path[kk].0 - tz * target, wv.path[kk].1 + tx * target);
            let vh = if c.vx.hypot(c.vz) > 2.0 { c.vx.atan2(c.vz) } else { c.h }; let al = wrap((px - c.x).atan2(pz - c.z) - vh);
            let lock = tune.lock / (1.0 + u / tune.lock_fall); let delta = wrap(vh - c.h) + (2.0 * 2.5 * al.sin() / ld).atan();
            let vt = v.min(rt.rprof[(k + 8) % np] * 0.9); let sp = c.vx.hypot(c.vz); let inp = Input { steer: (delta / lock).clamp(-1.0, 1.0), throttle: if sp < vt { 0.7 } else { 0.0 }, brake: if sp > vt + 2.0 { 0.5 } else { 0.0 }, hb: false };   // no faster than the road allows
            c.update(w, &e.obstacles, &tune, dt, &inp);
            if back { max_out = max_out.max(lat.abs()); }
            if std::env::var("DBG5").map(|d| d == format!("{}:{}:{}:{}:{}", i, side, v, creep, tw)).unwrap_or(false) && (back || lat.abs() > 4.0) {
                println!("{} lat {:+.2} tgt {:+.2} steer {:+.2} y-deck {:+.2} roll {:+.2} falling {} hg {:?} sp {:.1}", f, lat, target, inp.steer, c.y - (wv.deck(s, lat) + wv.at(s, w.wave_time())), c.roll, c.falling, c.wh.iter().map(|q| if q.hg < -300.0 { 9.0 } else { 0.0 }).collect::<Vec<_>>(), c.vx.hypot(c.vz)); }
            if !c.falling && lat.abs() < PRISM_KERB - 0.3 { sink2 = sink2.max(wv.deck(s, lat) + wv.at(s, w.wave_time()) - c.y); }
            if c.y < rt.roadh[k] - 6.0 { fell = true; break; }
            if back && lat.abs() < 2.0 { break; } }
        if over { tries += 1; if fell { lost_at.push(max_out); if std::env::var("LOST").is_ok() { println!("  lost {}:{}:{}:{}:{}", i, side, v, creep, tw); } } else { kept += 1; kept_at.push(max_out); } }
    } } } } }
    let med = |v: &mut Vec<f64>| { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v.get(v.len() / 2).copied().unwrap_or(0.0) };
    println!("drifting wide: {} times a wheel went over the edge, {} saved by steering back (centre got out to {:.2} m median when saved, {:.2} m when lost; kerb edge at {:.2} m); deepest sink while on the deck {:.2} m",
        tries, kept, med(&mut kept_at), med(&mut lost_at), PRISM_KERB, sink2);
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
