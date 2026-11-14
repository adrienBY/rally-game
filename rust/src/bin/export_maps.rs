//! export_maps out.json rt...: stage maps as JSON for the circuit atlas page.
//! Per stage: the road line (every 2 m) with height, planned speed and the demo driver's real lap (speed, gear, brake),
//! corners graded like rally pace notes, obstacles at the road's edge, tunnels, and a few handling numbers.
use fogwood_core::game::{Game, Persist};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut out = String::from("[");
    for (mi, f) in a[2..].iter().enumerate() {
        let rt = fogwood_core::game::rt::parse(&std::fs::read(f).unwrap()).expect("rt");
        let path: Vec<String> = rt.path.iter().flat_map(|p| [p[0].to_string(), p[1].to_string()]).collect();
        let roadh: Vec<String> = rt.roadh.iter().map(|v| v.to_string()).collect();
        fogwood_core::bridge::prepare(rt.map_id, &path.join(","), &roadh.join(","), rt.gxh, rt.gzh);
        let obs: Vec<String> = rt.obstacles.iter().flat_map(|o| [o[0].to_string(), o[1].to_string(), o[2].to_string()]).collect();
        fogwood_core::bridge::set_obstacles(&obs.join(","));
        let np = rt.np;
        // curvature (signed, rad/m) over 12 m
        let curv: Vec<f64> = (0..np).map(|i| { let a = rt.tan[(i + np - 6) % np]; let b = rt.tan[(i + 6) % np]; (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1]) / 12.0 }).collect();
        // corners: runs of the same direction tighter than 220 m radius
        let mut corners = vec![]; let mut i = 0usize;
        // start the scan on a straight-ish point so a corner is not split at index 0
        let s0 = (0..np).find(|&k| curv[k].abs() < 1.0 / 400.0).unwrap_or(0);
        while i < np {
            let k = (s0 + i) % np; let c = curv[k];
            if c.abs() > 1.0 / 220.0 { let sg = c.signum(); let st = i; let mut mx = 0.0f64; let mut ap = k;
                while i < np && curv[(s0 + i) % np].signum() == sg && curv[(s0 + i) % np].abs() > 1.0 / 260.0 { let q = (s0 + i) % np; if curv[q].abs() > mx { mx = curv[q].abs(); ap = q; } i += 1; }
                let len = i - st; if len >= 8 { let r = 1.0 / mx;
                    let grade = if r < 16.0 { "HP".to_string() } else if r < 28.0 { "1".into() } else if r < 45.0 { "2".into() } else if r < 70.0 { "3".into() } else if r < 105.0 { "4".into() } else if r < 160.0 { "5".into() } else { "6".into() };
                    // turning angle over the corner
                    let ta = rt.tan[(s0 + st) % np]; let tb = rt.tan[(s0 + i - 1) % np]; let ang = (ta[0] * tb[1] - ta[1] * tb[0]).atan2(ta[0] * tb[0] + ta[1] * tb[1]).abs().to_degrees();
                    // positive cross (tan x tan') means turning toward +x side; on screen (x right, z down) that reads as a right turn
                    corners.push(format!("{{\"s\":{},\"e\":{},\"a\":{},\"d\":\"{}\",\"r\":{:.0},\"g\":\"{}\",\"ang\":{:.0},\"len\":{}}}", (s0 + st) % np, (s0 + i - 1) % np, ap, if sg > 0.0 { "R" } else { "L" }, r, grade, ang, len)); }
            } else { i += 1; }
        }
        // obstacles near the road (edge at 3.4 m from the line)
        let mut haz = vec![]; let mut trees_near = 0;
        for o in &rt.obstacles { let mut best = 1e9f64; for k in (0..np).step_by(2) { let p = rt.path[k]; let d = (p[0] - o[0]).hypot(p[1] - o[1]); if d < best { best = d; } }
            let edge = best - o[2] - 3.4; if edge < 1.6 { haz.push(format!("[{:.1},{:.1},{}]", o[0], o[1], if o[3] == 2.0 { 1 } else { 0 })); }
            if edge < 5.0 { trees_near += 1; } }
        // the demo driver's lap at 60 Hz
        let mut g = Game::new(fogwood_core::game::rt::parse(&std::fs::read(f).unwrap()).unwrap(), Persist::new("/tmp/export_maps.sav"));
        let lock = fogwood_core::bridge::lock(); let e = lock.as_ref().unwrap();
        g.place_at(&e.world, 0);
        let mut spd = vec![-1.0f64; np]; let mut gear = vec![0i32; np]; let mut brk = vec![0u8; np]; let mut tat = vec![-1.0f64; np]; let mut slide = vec![0u8; np];
        let mut last = fogwood_core::game::path_idx(&e.world, g.car.x, g.car.z) as i64; let mut prog = 0i64; let mut t = 0.0; let mut topk = 0.0f64;
        let (mut sl, mut n) = (0, 0);
        while t < 300.0 {
            let inp = g.ai_input(&e.world); g.car.update(&e.world, &e.obstacles, &g.tune, 1.0 / 60.0, &inp);
            let c = &g.car; let i = fogwood_core::game::path_idx(&e.world, c.x, c.z);
            let mut d = i as i64 - last; if d < -900 { d += np as i64; } if d > 900 { d -= np as i64; } prog += d; last = i as i64;
            if prog >= 0 && prog < np as i64 + 1 && spd[i] < 0.0 { spd[i] = c.u * 3.6; gear[i] = c.gear as i32; brk[i] = (inp.brake > 0.05) as u8; tat[i] = t; slide[i] = (c.vl.abs() > 1.5) as u8; }
            topk = topk.max(c.u * 3.6); n += 1; if c.vl.abs() > 1.5 { sl += 1; }
            if prog >= np as i64 { break; } t += 1.0 / 60.0;
        }
        // fill gaps (indices skipped at speed)
        for i in 0..np { if spd[i] < 0.0 { let mut k = 1; while k < 20 && spd[(i + np - k) % np] < 0.0 { k += 1; } let j = (i + np - k) % np; spd[i] = spd[j].max(0.0); gear[i] = gear[j]; brk[i] = brk[j]; tat[i] = tat[j].max(0.0); slide[i] = slide[j]; } }
        let mut pts = vec![];
        for i in (0..np).step_by(2) { let p = rt.path[i];
            pts.push(format!("[{:.1},{:.1},{:.2},{:.0},{:.0},{},{},{:.2},{}]", p[0], p[1], rt.roadh[i], rt.vprof[i] * 3.6, spd[i], gear[i], brk[i], tat[i], slide[i])); }
        let tun: Vec<String> = rt.tun.iter().map(|t| format!("[{},{}]", t[0], t[1])).collect();
        let tv = |k: &str| rt.tune.iter().find(|(n, _)| n == k).map(|v| v.1).unwrap_or(0.0);
        if mi > 0 { out.push(','); }
        out.push_str(&format!("{{\"map\":\"{}\",\"np\":{},\"lap\":{:.2},\"top\":{:.0},\"slide\":{:.1},\"muR\":{},\"muG\":{},\"near\":{},\"pts\":[{}],\"corners\":[{}],\"haz\":[{}],\"tun\":[{}]}}",
            rt.map, np, t, topk, sl as f64 / n as f64 * 100.0, tv("muRoad"), tv("muGrass"), trees_near, pts.join(","), corners.join(","), haz.join(","), tun.join(",")));
        eprintln!("{} np {} lap {:.1} corners {} haz {}", rt.map, np, t, corners.len(), haz.len());
    }
    out.push(']'); std::fs::write(&a[1], out).unwrap();
}
