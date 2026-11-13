//! The native core must reproduce the browser build: identical stage grids, and the same car trajectory.
//! Fixtures were dumped from the JavaScript game (tests/fixtures/<map>.txt).
use fogwood_core::bridge;
use std::sync::Mutex;
static SERIAL: Mutex<()> = Mutex::new(()); // the engine is a process-wide singleton

fn fnv(b: &[u8]) -> u64 { let mut h: u64 = 0xcbf29ce484222325; for &x in b { h = (h ^ x as u64).wrapping_mul(0x100000001b3); } h }

fn run(name: &str) {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let txt = std::fs::read_to_string(format!("{}/tests/fixtures/{}.txt", env!("CARGO_MANIFEST_DIR"), name)).unwrap();
    let mut lines = txt.lines();
    let head: Vec<&str> = lines.next().unwrap().split(' ').collect();
    let (map, gxh, gzh) = (head[0].parse().unwrap(), head[1].parse().unwrap(), head[2].parse().unwrap());
    let (want, len): (u64, usize) = (u64::from_str_radix(head[3], 16).unwrap(), head[4].parse().unwrap());
    let path = lines.next().unwrap(); let roadh = lines.next().unwrap(); let obs = lines.next().unwrap();
    let t0 = std::time::Instant::now();
    let r = bridge::prepare(map, path, roadh, gxh, gzh);
    assert!(r.starts_with("ok"), "{r}");
    let grid = bridge::grid_bytes();
    println!("{name}: grids in {:?}", t0.elapsed());
    assert_eq!(grid.len(), len); assert_eq!(fnv(&grid), want, "{name}: grid differs from the JS build");
    assert!(bridge::set_obstacles(obs) > 0);
    let mut worst = 0.0f64; let mut frames = 0;
    for l in lines {
        let (inp, js) = l.split_once('|').unwrap();
        let out = bridge::step(inp); assert!(!out.starts_with("err"), "{out}");
        let n = bridge::parse_csv(&out); let j = bridge::parse_csv(js);
        for (a, b) in n.iter().zip(j.iter()) { let e = (a - b).abs() / b.abs().max(1.0); if e > worst { worst = e; } }
        frames += 1;
    }
    println!("{name}: {frames} frames, worst relative error {worst:e}");
    assert!(frames > 100 && worst < 1e-7, "{name}: physics drifted from the JS build ({worst:e})");
}
#[test] fn forest() { run("forest"); }
#[test] fn snow() { run("snow"); }
#[test] fn canyon() { run("canyon"); }
#[test] fn lakes() { run("lakes"); }
