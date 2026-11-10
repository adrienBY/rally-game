//! Text protocol between the game (JavaScript in the WebView) and the native engine.
//! Numbers travel as shortest round-trip decimal strings, so nothing is lost on the way.
use crate::physics::{Car, Input, Obstacle, Obstacles, Tune, NPARAM, NSTATE, NWHEEL};
use crate::world::{self, Map, World};
use std::sync::Mutex;

pub struct Engine { pub world: World, pub obstacles: Obstacles, pub grid: Vec<u8> }
static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

pub fn parse_csv(s: &str) -> Vec<f64> {
    s.split(',').filter(|t| !t.is_empty()).map(|t| match t.trim() {
        "Infinity" => f64::INFINITY, "-Infinity" => f64::NEG_INFINITY, v => v.parse::<f64>().unwrap_or(f64::NAN) }).collect()
}
pub fn fmt_num(v: f64, out: &mut String) {
    use std::fmt::Write;
    if v.is_nan() { out.push_str("NaN"); }
    else if v.is_infinite() { out.push_str(if v > 0.0 { "Infinity" } else { "-Infinity" }); }
    else if v == 0.0 { out.push('0'); }
    else if v.abs() >= 1e-5 && v.abs() < 1e15 { let _ = write!(out, "{}", v); }
    else { let _ = write!(out, "{:e}", v); }
}

/// Build the stage. `path` = "x0,z0,x1,z1,...", `roadh` = "h0,h1,...". Returns "ok <cells> <ms>".
pub fn prepare(map: i32, path: &str, roadh: &str, gxh: i32, gzh: i32) -> String {
    let t0 = std::time::Instant::now();
    let p = parse_csv(path); let pts: Vec<(f64, f64)> = p.chunks_exact(2).map(|c| (c[0], c[1])).collect();
    let rh = parse_csv(roadh);
    if pts.is_empty() || rh.len() != pts.len() { return format!("err path {} roadh {}", pts.len(), rh.len()); }
    let w = world::build(Map::from_id(map), &pts, &rh, gxh, gzh);
    let grid = w.to_bytes(); let cells = w.pd.len();
    *ENGINE.lock().unwrap() = Some(Engine { world: w, obstacles: Obstacles::new(&[]), grid });
    format!("ok {} {}", cells, t0.elapsed().as_millis())
}

pub fn grid_bytes() -> Vec<u8> { ENGINE.lock().unwrap().as_ref().map(|e| e.grid.clone()).unwrap_or_default() }
pub fn with_grid<R>(f: impl FnOnce(&[u8]) -> R) -> Option<R> { ENGINE.lock().unwrap().as_ref().map(|e| f(&e.grid)) }

/// "x,z,r,x,z,r,..."
pub fn set_obstacles(csv: &str) -> i32 {
    let v = parse_csv(csv); let list: Vec<Obstacle> = v.chunks_exact(3).map(|c| Obstacle { x: c[0], z: c[1], r: c[2] }).collect();
    let n = list.len() as i32;
    if let Some(e) = ENGINE.lock().unwrap().as_mut() { e.obstacles = Obstacles::new(&list); n } else { -1 }
}

/// Input: dt, throttle, brake, hb, steer, params[NPARAM], state[NSTATE + 4*NWHEEL].
/// Output: state[...], shifted(0/1).
pub fn step(csv: &str) -> String {
    let v = parse_csv(csv);
    let need = 5 + NPARAM + NSTATE + 4 * NWHEEL;
    if v.len() < need { return format!("err {}", v.len()); }
    let g = ENGINE.lock().unwrap();
    let Some(e) = g.as_ref() else { return "err noworld".into() };
    let input = Input { throttle: v[1], brake: v[2], hb: v[3] != 0.0, steer: v[4] };
    let tune = Tune::from_slice(&v[5..5 + NPARAM]);
    let mut car = Car::from_slice(&v[5 + NPARAM..]);
    let shifted = car.update(&e.world, &e.obstacles, &tune, v[0], &input);
    drop(g);
    let mut out = String::with_capacity(1400);
    for x in car.to_vec() { fmt_num(x, &mut out); out.push(','); }
    out.push(if shifted { '1' } else { '0' });
    out
}
