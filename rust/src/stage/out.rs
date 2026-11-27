//! The last steps: static one-off meshes merged per material look, the handling tune, the stage's sound emitters, and the runtime
//! handoff ("FRT1" + json) the game starts from.
use super::three::*;
use super::St;
use crate::noise::clamp;
use serde_json::{json, Value};

pub fn merge_static(st: &mut St) {
    let hex = |c: Option<Col>| c.map(|c| format!("{:02x}{:02x}{:02x}", (c.r * 255.0).round() as i64, (c.g * 255.0).round() as i64, (c.b * 255.0).round() as i64)).unwrap_or_default();
    let mut keys: Vec<String> = Vec::new(); let mut groups: std::collections::HashMap<String, Vec<ObjId>> = Default::default();
    for &o in &st.sc.root.clone() { let ob = &st.sc.objs[o]; if ob.kind != OKind::Mesh || !ob.visible || ob.dyn_ { continue; } let m = &st.sc.mats[ob.mat];
        if m.transparent || m.sway.is_some() || matches!(m.kind, Kind::Sky | Kind::Dust) || st.sc.geos[ob.geo].count() > 3000 { continue; }
        let k = format!("{:?}|{}|{:?}|{}|{}|{:?}|{}|{}|{}", m.kind, hex(m.color), m.map, m.vc, m.side, m.po, if m.kind == Kind::Basic { String::new() } else { hex(m.emis) }, m.fog, m.alpha_test);
        groups.entry(k.clone()).or_insert_with(|| { keys.push(k); Vec::new() }).push(o); }
    for k in keys { let list = groups.remove(&k).unwrap(); if list.len() < 2 { continue; }
        let m = st.sc.objs[list[0]].mat; let (use_uv, use_col) = (st.sc.mats[m].map.is_some(), st.sc.mats[m].vc);
        let (mut p, mut n, mut uv, mut c) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for &o in &list { let wm = st.sc.world(o); let src = &st.sc.geos[st.sc.objs[o].geo]; let mut g = if src.index.is_some() { src.to_non_indexed() } else { src.clone() }; g.bs = None; g.apply(&wm); if g.nrm.is_none() { g.compute_normals(); }
            let cnt = g.count(); p.extend_from_slice(&g.pos); n.extend_from_slice(g.nrm.as_ref().unwrap());
            if use_uv { match &g.uv { Some(u) => uv.extend_from_slice(u), None => uv.extend(std::iter::repeat(0.0).take(cnt * 2)) } }
            if use_col { match &g.col { Some(cc) => c.extend_from_slice(cc), None => c.extend(std::iter::repeat(1.0).take(cnt * 3)) } }
            st.sc.remove_root(o); }
        let mut g = Geo::new(p).with_nrm(n); if use_uv { g.uv = Some(uv); } if use_col { g.col = Some(c); }
        let o = st.mesh(g, m); st.sc.objs[o].culled = false; st.add(o); }
}

pub fn tune(st: &mut St) {
    let mut t: Vec<(&'static str, f64)> = vec![("lock", 0.9), ("lockFall", 70.0), ("steerRate", 5.5), ("returnRate", 6.0), ("rateFall", 45.0), ("assist", 0.3), ("csExtra", 0.18), ("csRate", 1.8), ("latKeep", 0.6),
        ("gripF", 1.15), ("gripR", 1.12), ("By", 7.4), ("Cy", 1.35), ("muRoad", 0.95), ("muGrass", 0.72), ("split", 0.5), ("power", 1.0), ("brake", 2300.0), ("brakeRear", 0.565), ("abs", 0.13), ("hb", 2600.0),
        ("camFollow", 16.0), ("arcade", 0.75), ("aGrip", 2.6), ("aYaw", 10.2), ("aBrake", 6.0), ("aAccel", 3.2), ("tc", 1.0), ("aHbYaw", 5.0), ("aYawCap", 1.68)];
    let set = |t: &mut Vec<(&'static str, f64)>, k: &str, v: f64| { for e in t.iter_mut() { if e.0 == k { e.1 = v; } } };
    let (mr, mg) = if st.snow { (0.74, 0.5) } else if st.can { (0.88, 0.6) } else if st.tou { (1.02, 0.55) } else if st.riv { (1.08, 0.6) } else if st.pri { (1.04, 0.6) } else { (0.95, 0.72) };
    set(&mut t, "muRoad", mr); set(&mut t, "muGrass", mg); st.tune = t;
}

fn f32le(v: &[f64]) -> Vec<u8> { let mut b = Vec::with_capacity(v.len() * 4); for x in v { b.extend_from_slice(&(*x as f32).to_le_bytes()); } b }
/// the stage's sound preset and emitters (water, lanterns, the village, the town), as the page sent them to the sound engine
pub fn audio(st: &St) -> Vec<Vec<u8>> {
    let mut e: Vec<f64> = Vec::new(); let (gxh, gzh) = (st.r.gxh as f64, st.r.gzh as f64); let wl = crate::world::WL; let np = st.np();
    if st.lak { let mut z = -gzh; while z <= gzh && e.len() < 5 * 70 { let mut x = -gxh; while x <= gxh { let h = st.height(x, z); if h < wl + 0.5 && h > wl - 1.2 && st.road_dist(x, z) < 70.0 { e.extend_from_slice(&[4.0, x, wl, z, 1.0]); } x += 30.0; } z += 30.0; } }
    if st.riv { let mut z = -gzh; while z <= gzh && e.len() < 5 * 80 { let mut x = -gxh; while x <= gxh { let h = st.height(x, z); if h < 1.2 && h > -3.0 && st.road_dist(x, z) < 80.0 { e.extend_from_slice(&[4.0, x, 0.0, z, 1.1]); } x += 28.0; } z += 28.0; }
        if let Some((p, n)) = st.r.vil { let k = (p + n / 2) % np; let pp = st.p(k); e.extend_from_slice(&[6.0, pp[0], st.r.roadh[k] + 4.0, pp[1], 0.5]); } }
    if st.tou { for l in &st.lan { e.extend_from_slice(&[5.0, l.x, l.y + 0.3, l.z, 1.0]); }
        for n in 0..40usize { let k = (n * 397) % np; let sd = if n % 2 == 1 { 1.0 } else { -1.0 }; let off = st.road() + 8.0 + ((n * 13) % 20) as f64; let (x, z) = st.side(k, off * sd); e.extend_from_slice(&[5.0, x, st.height(x, z) + 0.3, z, 0.8]); }
        if let Some(t) = st.town { e.extend_from_slice(&[6.0, t.x, t.y, t.z, 1.0]); } }
    let mut em = vec![3.0, (e.len() / 5) as f64]; em.extend(e);
    vec![f32le(&[5.0, st.m.id() as f64]), f32le(&em)]
}

pub fn rt_packet(st: &St, ids: &[Option<usize>], saves: &[(String, String)]) -> Vec<u8> {
    let sc = &st.sc; let np = st.np(); let idv = |o: ObjId| -> Vec<usize> { ids.get(o).copied().flatten().into_iter().collect() };
    let id1 = |o: ObjId| -> Value { ids.get(o).copied().flatten().map(|v| json!(v)).unwrap_or(Value::Null) };
    // movable nodes and every object's matrix relative to its node
    let mut nodes: Vec<(ObjId, Value)> = Vec::new();
    let node_of = |nodes: &Vec<(ObjId, Value)>, o: ObjId| -> Option<usize> { let mut q = Some(o); while let Some(i) = q { if let Some(k) = nodes.iter().position(|n| n.0 == i) { return Some(k); } q = sc.objs[i].parent; } None };
    let mut add_node = |nodes: &mut Vec<(ObjId, Value)>, o: ObjId, name: String| {
        if nodes.iter().any(|n| n.0 == o) { return; } let ob = &sc.objs[o];
        let pnode = ob.parent.and_then(|p| node_of(nodes, p));
        let pre = match (pnode, ob.parent) { (Some(k), Some(p)) => sc.world(nodes[k].0).invert().mul(&sc.world(p)), (None, Some(p)) => sc.world(p), _ => M4::ID };
        nodes.push((o, json!({ "name": name, "parent": pnode.map(|k| k as i64).unwrap_or(-1), "pre": pre.0.to_vec(), "p": [ob.pos.x, ob.pos.y, ob.pos.z], "r": [ob.rot.x, ob.rot.y, ob.rot.z], "o": if ob.yxz { "YXZ" } else { "XYZ" }, "s": [ob.scl.x, ob.scl.y, ob.scl.z] }))); };
    add_node(&mut nodes, st.car_root, "car".into());
    for (i, w) in st.wheels.iter().enumerate() { add_node(&mut nodes, w.pivot, format!("pivot{}", i)); add_node(&mut nodes, w.spin, format!("spin{}", i)); }
    add_node(&mut nodes, st.car_shadow, "shadow".into()); add_node(&mut nodes, st.sky, "sky".into()); add_node(&mut nodes, st.peaks, "peaks".into()); add_node(&mut nodes, st.star_g, "stars".into());
    for (i, &m) in st.water_l.iter().enumerate() { add_node(&mut nodes, m, format!("water{}", i)); } for (i, &m) in st.glit.iter().enumerate() { add_node(&mut nodes, m, format!("glit{}", i)); }
    for (i, &m) in st.clouds.iter().enumerate() { add_node(&mut nodes, m, format!("clouds{}", i)); } for (i, &m) in st.sunb.iter().enumerate() { add_node(&mut nodes, m, format!("sunb{}", i)); }
    add_node(&mut nodes, st.snowfall, "snowfall".into()); add_node(&mut nodes, st.dust, "dust".into());
    let local = |o: ObjId| -> Option<(usize, M4)> { let k = node_of(&nodes, o)?; Some((k, sc.world(nodes[k].0).invert().mul(&sc.world(o)))) };
    let mut objn: Vec<(usize, Value)> = Vec::new();
    for o in 0..sc.objs.len() { if let Some(id) = ids[o] { if let Some((k, m)) = local(o) { let mut v = vec![json!(id), json!(k)]; v.extend(m.0.iter().map(|x| json!(x))); objn.push((id, Value::Array(v))); } } }
    objn.sort_by_key(|e| e.0);
    let rg_parts: Vec<Value> = st.rg_parts.iter().map(|p| json!({ "obj": id1(p.im), "src": p.src.iter().map(|&s| local(s).map(|(k, m)| json!([k, m.0.to_vec()])).unwrap_or(Value::Null)).collect::<Vec<_>>(),
        "model": p.model, "tint": p.tint as i32, "liv": p.liv, "paint": p.paint as i32 })).collect();
    let car_models: Vec<Value> = st.car_models.iter().map(|&b| { let mut l = Vec::new(); sc.walk(b, &mut l); json!(l.into_iter().flat_map(|o| idv(o)).collect::<Vec<_>>()) }).collect();
    let car_livs: Vec<Value> = st.car_models.iter().map(|&b| { let mut l = Vec::new(); sc.walk(b, &mut l);
        let paint: Vec<usize> = l.iter().filter(|&&o| sc.objs[o].kind == OKind::Mesh && sc.mats[sc.objs[o].mat].paint).flat_map(|&o| idv(o)).collect();
        json!({ "paint": paint, "alt": sc.objs[b].liveries.iter().map(|&m| id1(m)).collect::<Vec<_>>() }) }).collect();
    let mut rline = vec![0.0f64; np]; { let t = &st.r.tan; for i in 0..np { let a = t[(i + np - 10) % np]; let b = t[(i + 10) % np]; rline[i] = clamp((a[0] * b[1] - a[1] * b[0]) * 3.0, -1.8, 1.8) as f32 as f64; }
        for _ in 0..3 { let c = rline.clone(); for i in 0..np { let mut s = 0.0; for k in -12i64..=12 { s += c[st.r.wrap(i as i64 + k)]; } rline[i] = (s / 25.0) as f32 as f64; } } }
    let mu = st.tune.iter().find(|e| e.0 == "muRoad").map(|e| e.1).unwrap_or(0.95);
    let al = 9.81 * mu * (if st.snow { 0.96 } else if st.pri || st.vol { 1.12 } else if st.can || st.lak { 1.25 } else if st.forest { 1.3 } else { 1.38 }) * 1.15; let ab = 9.81 * mu * 0.85 * 1.1;
    let mut rprof = vec![0.0f64; np];
    for i in 0..np { let a = st.r.tan[(i + np - 6) % np]; let b = st.r.tan[(i + 6) % np]; let d = libm::fabs(libm::atan2(a[0] * b[1] - a[1] * b[0], a[0] * b[0] + a[1] * b[1])) / 12.0;
        let mut v = (56f64).min(libm::sqrt(al / d.max(1e-4))).min(st.r.jcap[i] * 1.5); v = v as f32 as f64;
        if st.vol { v = (v * (1.0 - clamp((st.r.roadh[i] - st.r.roadh[(i + 6) % np]) / 6.0 * 2.2, 0.0, 0.3))) as f32 as f64; } rprof[i] = v; }
    for _ in 0..3 { for i in (0..np).rev() { rprof[i] = rprof[i].min(libm::sqrt(rprof[(i + 1) % np].powi(2) + 2.0 * ab)) as f32 as f64; } }
    let flat = |v: &Vec<[f64; 2]>| v.iter().flat_map(|p| [p[0], p[1]]).collect::<Vec<f64>>();
    let tune: serde_json::Map<String, Value> = st.tune.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    let shade: Vec<i64> = (0..np * 13).map(|q| { let i = q / 13; let l = (q % 13) as f64 - 6.0; let (x, z) = st.side(i, l); super::jr(st.sun_shadow_at(x, z) * 99.0) as i64 }).collect();
    let wave = if st.pri { json!({ "objs": sc.root.iter().filter(|&&o| sc.objs[o].wave).flat_map(|&o| idv(o)).collect::<Vec<_>>(), "secs": st.r.waves }) } else { Value::Null };
    let tiles: Vec<Value> = sc.root.iter().filter_map(|&o| { let (tw, th) = sc.objs[o].grid?; let b = sc.geos[sc.objs[o].geo].bs?; Some(json!([id1(o), tw, th, b[0], b[1], b[2]])) }).collect();
    let mut sv = serde_json::Map::new(); for (k, v) in saves { if k.starts_with("fogwood-") { sv.insert(k.clone(), json!(v)); } }
    let pk = json!({ "v": 1, "map": st.m.name(), "mapId": st.m.id(), "NP": np, "GXH": st.r.gxh, "GZH": st.r.gzh, "sun": [st.sun.x, st.sun.y, st.sun.z], "fogD": st.fog_d, "far": st.far_cam, "near": 0.12,
        "path": flat(&st.r.path), "tan": flat(&st.r.tan), "roadh": st.r.roadh, "vprof": st.r.vprof, "vcurve": st.r.vcurve, "jcap": st.r.jcap, "drop": st.drop, "rline": rline, "rprof": rprof,
        "tune": tune, "susp": { "K": 21000, "C": 1450, "KARB": 11000 }, "iz": 1750,
        "tun": st.r.tun.iter().map(|t| json!([t.m, t.half, t.i0, t.i1])).collect::<Vec<_>>(), "vil": st.r.vil.map(|(p, n)| json!([p, n])).unwrap_or(Value::Null),
        "obstacles": st.obstacles, "split": st.split, "nodes": nodes.iter().map(|n| n.1.clone()).collect::<Vec<_>>(), "objn": objn.into_iter().map(|e| e.1).collect::<Vec<_>>(),
        "rg": { "parts": rg_parts, "shadow": id1(st.rg_shadow) },
        "car": { "liveries": car_livs, "shadow": id1(st.car_shadow), "models": car_models }, "glit": st.glit.iter().map(|&m| id1(m)).collect::<Vec<_>>(),
        "cull": st.cull.iter().map(|c| json!([idv(c.o), c.c.x, c.c.y, c.c.z, c.r, c.d])).collect::<Vec<_>>(),
        "boats": match st.boat_ims { Some([h, s, c, w]) => json!({ "list": st.boats.iter().map(|b| json!({ "x": b.x, "z": b.z, "h": b.h, "v": b.v, "sail": b.sail, "ph": b.ph, "sc": b.sc })).collect::<Vec<_>>(), "hull": id1(h), "sail": id1(s), "cab": id1(c), "wake": id1(w) }), None => Value::Null },
        "snow": if st.snow || st.tou || st.vol { json!({ "obj": id1(st.snowfall), "n": st.sfn, "pos": st.sf_pos, "seed": st.sf_seed }) } else { Value::Null },
        "dust": { "obj": id1(st.dust), "n": st.dn }, "shade": shade, "shadeTint": st.shadow_tint,
        "volc": match st.volc_ims { Some(v) => json!({ "fire": id1(v[0]), "solid": id1(v[1]), "glow": id1(v[2]), "ring": id1(v[3]), "disc": id1(v[4]), "crater": st.volc_crater,
            "rivers": st.volc_rivers.iter().map(|r| json!({ "pts": r.pts, "hw": r.hw, "kc": r.kc, "sg": r.sg })).collect::<Vec<_>>(), "bridges": st.r.bridges.iter().map(|b| json!([b.0, b.1])).collect::<Vec<_>>() }), None => Value::Null },
        "tilefall": id1(st.tilefall), "wave": wave, "tiles": tiles, "saves": sv });
    let js = serde_json::to_vec(&pk).unwrap_or_default(); let mut out = Vec::with_capacity(8 + js.len()); out.extend_from_slice(b"FRT1"); out.extend_from_slice(&(js.len() as u32).to_le_bytes()); out.extend_from_slice(&js); out
}
