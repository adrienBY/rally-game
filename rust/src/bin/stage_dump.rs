//! stage_dump <map> <outdir>: a bare stage for game_host built from a test fixture (no browser needed): the runtime
//! handoff (road, tuning, the speed plans and racing line computed the way the page does) and an empty scene. Enough
//! to run the game logic headless (online races, AI cars); nothing to look at.
use fogwood_core::physics::{NPARAM, PARAM_NAMES};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (map, out) = (a.get(1).cloned().unwrap_or("lakes".into()), a.get(2).cloned().unwrap_or("/tmp/stage".into()));
    let txt = std::fs::read_to_string(format!("{}/tests/fixtures/{}.txt", env!("CARGO_MANIFEST_DIR"), map)).expect("fixture");
    let mut lines = txt.lines(); let head: Vec<&str> = lines.next().unwrap().split(' ').collect();
    let (mid, gxh, gzh): (i64, i64, i64) = (head[0].parse().unwrap(), head[1].parse().unwrap(), head[2].parse().unwrap());
    let pv = fogwood_core::bridge::parse_csv(lines.next().unwrap()); let roadh = fogwood_core::bridge::parse_csv(lines.next().unwrap());
    let obs = fogwood_core::bridge::parse_csv(lines.next().unwrap());
    let first = fogwood_core::bridge::parse_csv(lines.next().unwrap().split_once('|').unwrap().0); let tv = &first[5..5 + NPARAM];
    let path: Vec<[f64; 2]> = pv.chunks(2).map(|c| [c[0], c[1]]).collect(); let np = path.len();
    let tan: Vec<[f64; 2]> = (0..np).map(|i| { let p = path[(i + np - 1) % np]; let q = path[(i + 1) % np]; let (dx, dz) = (q[0] - p[0], q[1] - p[1]); let l = dx.hypot(dz).max(1e-9); [dx / l, dz / l] }).collect();
    let turn = |i: usize, w: usize| { let a = tan[(i + np - w) % np]; let b = tan[(i + w) % np]; (a[0] * b[1] - a[1] * b[0]).atan2(a[0] * b[0] + a[1] * b[1]) };
    let mu_road = tv[PARAM_NAMES.iter().position(|n| *n == "muRoad").unwrap_or(0)];
    let k_surf = match mid { 1 => 0.5, 2 => 0.62, 5 => 0.74, 6 => 0.8, _ => 0.66 };
    let vcurve: Vec<f64> = (0..np).map(|i| (k_surf * 9.81 / (turn(i, 6).abs() / 12.0).max(1e-4)).sqrt().min(44.0)).collect();
    let mut vprof = vcurve.clone(); for _ in 0..3 { for i in (0..np).rev() { vprof[i] = vprof[i].min((vprof[(i + 1) % np].powi(2) + 10.0).sqrt()); } }
    let al = 9.81 * mu_road * match mid { 1 => 0.96, 2 | 3 => 1.25, 0 => 1.3, _ => 1.38 } * 1.15; let ab = 9.81 * mu_road * 0.85 * 1.1;
    let mut rprof: Vec<f64> = (0..np).map(|i| (al / (turn(i, 6).abs() / 12.0).max(1e-4)).sqrt().min(56.0)).collect();
    for _ in 0..3 { for i in (0..np).rev() { rprof[i] = rprof[i].min((rprof[(i + 1) % np].powi(2) + 2.0 * ab).sqrt()); } }
    let mut rline: Vec<f64> = (0..np).map(|i| { let a = tan[(i + np - 10) % np]; let b = tan[(i + 10) % np]; ((a[0] * b[1] - a[1] * b[0]) * 3.0).clamp(-1.8, 1.8) }).collect();
    for _ in 0..3 { let c = rline.clone(); for i in 0..np { rline[i] = (0..25).map(|k| c[(i + k + np - 12) % np]).sum::<f64>() / 25.0; } }
    let tune: serde_json::Map<String, serde_json::Value> = PARAM_NAMES.iter().zip(tv.iter()).map(|(n, v)| (n.to_string(), serde_json::json!(v))).collect();
    let mapname = match mid { 0 => "forest", 1 => "snow", 2 => "canyon", 3 => "lakes", 5 => "touge", 6 => "riviera", _ => "forest" };
    let j = serde_json::json!({ "map": mapname, "mapId": mid, "NP": np, "GXH": gxh, "GZH": gzh, "sun": [0.4, 0.8, 0.3], "near": 0.12, "far": 1900.0,
        "path": path.iter().flat_map(|p| [p[0], p[1]]).collect::<Vec<_>>(), "tan": tan.iter().flat_map(|p| [p[0], p[1]]).collect::<Vec<_>>(), "roadh": roadh,
        "vprof": vprof, "vcurve": vcurve, "jcap": vec![99.0; np], "drop": vec![0; np], "rline": rline, "rprof": rprof, "tune": tune,
        "susp": { "K": tv[29], "C": tv[30], "KARB": tv[31] }, "iz": tv[32], "obstacles": obs.chunks(3).map(|o| vec![o[0], o[1], o[2]]).collect::<Vec<_>>(), "saves": {} });
    let pack = |magic: &[u8; 4], j: &serde_json::Value| { let s = j.to_string(); let mut b = magic.to_vec(); b.extend_from_slice(&(s.len() as u32).to_le_bytes()); b.extend_from_slice(s.as_bytes()); while b.len() % 4 != 0 { b.push(b' '); } b };
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(format!("{}/rt.bin", out), pack(b"FRT1", &j)).unwrap();
    std::fs::write(format!("{}/scene.bin", out), pack(b"FWS1", &serde_json::json!({ "texs": [], "geos": [], "mats": [], "objs": [] }))).unwrap();
    println!("{} np {} -> {}", mapname, np, out);
}
