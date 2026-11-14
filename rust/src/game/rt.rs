//! The runtime handoff the page sends after building a stage ("FRT1" + json): road data, tuning, which scene objects move, the rivals'
//! puppet, culling, boats and the saved records.
use super::math::{from_slice, M};
use serde_json::Value;

pub struct Node { pub parent: i32, pub pre: M, pub p: [f64; 3], pub r: [f64; 3], pub yxz: bool, pub s: [f64; 3], pub name: String }
pub struct Boat { pub x: f64, pub z: f64, pub h: f64, pub v: f64, pub sail: bool, pub ph: f64, pub sc: f64 }
pub struct Cull { pub ids: Vec<usize>, pub c: [f64; 3], pub r: f64, pub d: f64 }
pub struct RgPart { pub obj: usize, pub src: Vec<(usize, M)> }

pub struct Rt {
    pub map: String, pub map_id: i32, pub np: usize, pub gxh: i32, pub gzh: i32,
    pub sun: [f64; 3], pub near: f64, pub far: f64,
    pub path: Vec<[f64; 2]>, pub tan: Vec<[f64; 2]>, pub roadh: Vec<f64>, pub vprof: Vec<f64>, pub vcurve: Vec<f64>, pub jcap: Vec<f64>, pub drop: Vec<i32>,
    pub rline: Vec<f64>, pub rprof: Vec<f64>,
    pub tune: Vec<(String, f64)>, pub susp: [f64; 3], pub iz: f64,
    pub tun: Vec<[i64; 4]>, pub vil: Option<(i64, i64)>,
    pub obstacles: Vec<[f64; 4]>, pub split: [f32; 6],
    pub nodes: Vec<Node>, pub objn: Vec<(usize, usize, M)>,
    pub rg_parts: Vec<RgPart>, pub rg_shadow: Option<usize>,
    pub shadow_obj: Option<usize>, pub glit: Vec<usize>, pub cull: Vec<Cull>,
    pub boats: Vec<Boat>, pub boat_ims: Option<[usize; 4]>,
    pub snow: Option<(usize, usize, Vec<f32>, Vec<f32>)>, pub dust: Option<(usize, usize)>,
    pub saves: Vec<(String, String)>,
    /// stage ground tiles: object, grid width, grid height (vertices), centre
    pub tiles: Vec<(usize, usize, usize, [f64; 3])>,
}

fn f(v: &Value) -> f64 { v.as_f64().unwrap_or(0.0) }
fn arr(v: &Value) -> Vec<f64> { v.as_array().map(|a| a.iter().map(f).collect()).unwrap_or_default() }
fn pairs(v: &Value) -> Vec<[f64; 2]> { arr(v).chunks_exact(2).map(|c| [c[0], c[1]]).collect() }
fn v3(v: &Value) -> [f64; 3] { let a = arr(v); [a.first().copied().unwrap_or(0.0), a.get(1).copied().unwrap_or(0.0), a.get(2).copied().unwrap_or(0.0)] }
fn id(v: &Value) -> Option<usize> { v.as_u64().map(|x| x as usize) }

pub fn parse(pkt: &[u8]) -> Result<Rt, String> {
    if pkt.len() < 8 || &pkt[0..4] != b"FRT1" { return Err("bad runtime magic".into()); }
    let jl = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]) as usize;
    let j: Value = serde_json::from_slice(&pkt[8..(8 + jl).min(pkt.len())]).map_err(|e| e.to_string())?;
    let np = j["NP"].as_u64().unwrap_or(0) as usize;
    let nodes = j["nodes"].as_array().cloned().unwrap_or_default().iter().map(|n| Node {
        parent: n["parent"].as_i64().unwrap_or(-1) as i32, pre: from_slice(&arr(&n["pre"])), p: v3(&n["p"]), r: v3(&n["r"]), yxz: n["o"].as_str() == Some("YXZ"), s: v3(&n["s"]),
        name: n["name"].as_str().unwrap_or("").to_string() }).collect();
    let objn = j["objn"].as_array().cloned().unwrap_or_default().iter().map(|e| { let a = arr(e); (a[0] as usize, a[1] as usize, from_slice(&a[2..])) }).collect();
    let rg_parts = j["rg"]["parts"].as_array().cloned().unwrap_or_default().iter().filter_map(|p| {
        let obj = id(&p["obj"])?;
        let src = p["src"].as_array()?.iter().filter_map(|s| { let a = s.as_array()?; Some((a.first()?.as_u64()? as usize, from_slice(&arr(a.get(1)?)))) }).collect();
        Some(RgPart { obj, src }) }).collect();
    let cull = j["cull"].as_array().cloned().unwrap_or_default().iter().map(|c| { let a = c.as_array().cloned().unwrap_or_default();
        Cull { ids: a.first().and_then(|x| x.as_array()).map(|l| l.iter().filter_map(id).collect()).unwrap_or_default(),
            c: [a.get(1).map(f).unwrap_or(0.0), a.get(2).map(f).unwrap_or(0.0), a.get(3).map(f).unwrap_or(0.0)], r: a.get(4).map(f).unwrap_or(0.0), d: a.get(5).map(f).unwrap_or(0.0) } }).collect();
    let b = &j["boats"];
    let boats = b["list"].as_array().cloned().unwrap_or_default().iter().map(|q| Boat { x: f(&q["x"]), z: f(&q["z"]), h: f(&q["h"]), v: f(&q["v"]), sail: q["sail"].as_bool().unwrap_or(false), ph: f(&q["ph"]), sc: f(&q["sc"]) }).collect();
    let boat_ims = match (id(&b["hull"]), id(&b["sail"]), id(&b["cab"]), id(&b["wake"])) { (Some(a), Some(s), Some(c), Some(w)) => Some([a, s, c, w]), _ => None };
    let snow = if j["snow"].is_object() { let s = &j["snow"]; id(&s["obj"]).map(|o| (o, s["n"].as_u64().unwrap_or(0) as usize, arr(&s["pos"]).iter().map(|&v| v as f32).collect(), arr(&s["seed"]).iter().map(|&v| v as f32).collect())) } else { None };
    let dust = id(&j["dust"]["obj"]).map(|o| (o, j["dust"]["n"].as_u64().unwrap_or(420) as usize));
    let tune = j["tune"].as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), f(v))).collect()).unwrap_or_default();
    let s = &j["susp"];
    let sp = arr(&j["split"]); let mut split = [0f32; 6]; for i in 0..6.min(sp.len()) { split[i] = sp[i] as f32; }
    Ok(Rt {
        map: j["map"].as_str().unwrap_or("forest").to_string(), map_id: j["mapId"].as_i64().unwrap_or(0) as i32, np,
        gxh: j["GXH"].as_i64().unwrap_or(0) as i32, gzh: j["GZH"].as_i64().unwrap_or(0) as i32,
        sun: v3(&j["sun"]), near: j["near"].as_f64().unwrap_or(0.12), far: j["far"].as_f64().unwrap_or(1900.0),
        path: pairs(&j["path"]), tan: pairs(&j["tan"]), roadh: arr(&j["roadh"]), vprof: arr(&j["vprof"]), vcurve: arr(&j["vcurve"]), jcap: arr(&j["jcap"]),
        drop: arr(&j["drop"]).iter().map(|&v| v as i32).collect(), rline: arr(&j["rline"]), rprof: arr(&j["rprof"]),
        tune, susp: [f(&s["K"]), f(&s["C"]), f(&s["KARB"])], iz: f(&j["iz"]),
        tun: j["tun"].as_array().cloned().unwrap_or_default().iter().map(|t| { let a = arr(t); [a[0] as i64, a[1] as i64, a[2] as i64, a[3] as i64] }).collect(),
        vil: j["vil"].as_array().map(|a| (a[0].as_i64().unwrap_or(0), a[1].as_i64().unwrap_or(0))),
        obstacles: j["obstacles"].as_array().cloned().unwrap_or_default().iter().map(|o| { let a = arr(o); [a[0], a[1], a[2], a.get(3).copied().unwrap_or(0.0)] }).collect(),
        split, nodes, objn, rg_parts, rg_shadow: id(&j["rg"]["shadow"]),
        shadow_obj: id(&j["car"]["shadow"]), glit: j["glit"].as_array().map(|a| a.iter().filter_map(id).collect()).unwrap_or_default(), cull,
        boats, boat_ims, snow, dust,
        tiles: j["tiles"].as_array().cloned().unwrap_or_default().iter().map(|t| { let a = arr(t); (a[0] as usize, a[1] as usize, a[2] as usize, [a[3], a[4], a[5]]) }).collect(),
        saves: j["saves"].as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect()).unwrap_or_default(),
    })
}
