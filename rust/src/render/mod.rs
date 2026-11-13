//! Native renderer: draws the scene the game exports (see nglExport in the game) with OpenGL ES 3.
//! Pipeline: scene -> low-resolution colour+depth target -> post (god rays, zoom blur, grade, dither) -> nearest upscale to the screen.
pub mod loader;
pub mod math;
pub mod shaders;

use glow::HasContext;
use math::*;
use serde_json::Value;
use shaders::*;
use std::collections::HashMap;

type Gl = glow::Context;

#[derive(Clone, Copy, PartialEq, Debug)]
enum MatKind { Basic, Lambert, Phong, Points, Sky, Dust }

struct Material {
    kind: MatKind, color: [f32; 3], spec: [f32; 3], shin: f32, emis: [f32; 3], map: i32, alpha_test: f32, transparent: bool, opacity: f32,
    side: i32, vcolor: bool, fog: bool, depth_write: bool, depth_test: bool, poly: Option<(f32, f32)>, blend: i32, sway: Option<(f32, bool)>, size: f32,
    sky: Option<([f32; 3], [f32; 3], [f32; 3], [f32; 3], f32)>, dust: Option<(f32, [f32; 3], [f32; 3])>,
}
struct Texture { tex: glow::Texture, m: [f32; 9] }
struct Attr { name: String, size: i32, buf: glow::Buffer }
struct Geometry { attrs: Vec<Attr>, index: Option<glow::Buffer>, count: i32, dr: (i32, i32), bs: [f32; 4] }
#[derive(PartialEq)]
enum ObjKind { Mesh, Inst, Points }
struct Object {
    kind: ObjKind, geo: usize, mat: usize, mw: M4, ro: i32, cull: bool, vis: bool, opacity: f32,
    count: i32, ibuf: Option<glow::Buffer>, cbuf: Option<glow::Buffer>, ibs: Option<[f32; 4]>, vao: glow::VertexArray, dr: Option<(i32, i32)>,
}
struct GpuScene {
    texs: Vec<Texture>, geos: Vec<Geometry>, mats: Vec<Material>, objs: Vec<Object>,
    fog_c: [f32; 3], fog_d: f32, clear: [f32; 4], hemi: ([f32; 3], [f32; 3], [f32; 3]), dir: ([f32; 3], [f32; 3]),
}
#[derive(Clone, Copy, Default)]
pub struct FrameState { pub lw: i32, pub lh: i32, pub view: M4, pub proj: M4, pub cam: [f32; 3], pub time: f32, pub wind: f32, pub sun: [f32; 2],
    pub sun_vis: f32, pub flash: f32, pub speed: f32, pub dust_scale: f32, pub valid: bool }

struct Program { prog: glow::Program, u: HashMap<&'static str, glow::UniformLocation> }

pub struct Renderer {
    gl: Gl, scene: Option<GpuScene>, mesh_progs: HashMap<MeshKey, Program>, sky: Program, points: Program, dust: Program, post: Program,
    quad_vao: glow::VertexArray, _quad_buf: glow::Buffer,
    tgt: Option<(i32, i32, glow::Framebuffer, glow::Texture, glow::Renderbuffer, glow::Framebuffer, glow::Texture)>,
    pub frame: FrameState, pub stats: (u32, u32), pub only: Option<Vec<usize>>,
}

const UNIFORMS: [&str; 37] = ["uModel", "uView", "uProj", "uNormalM", "uMapM", "uTime", "uWind", "uColor", "uOpacity", "uMap", "uAlphaTest", "uHemiSky",
    "uHemiGround", "uHemiDir", "uDirColor", "uDirDir", "uFogColor", "uFogDensity", "uCamPos", "uSpec", "uShin", "uEmissive", "uSun", "uFog", "uTop", "uGlow",
    "uHalo", "uSize", "uScale", "uDens", "uCol", "tScene", "uSunVis", "uFlash", "uSpeed", "uSunUv", "uDummy"];

unsafe fn compile(gl: &Gl, vs: &str, fs: &str) -> Result<Program, String> {
    let p = gl.create_program()?;
    let mut shs = vec![];
    for (ty, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
        let s = gl.create_shader(ty)?; gl.shader_source(s, src); gl.compile_shader(s);
        if !gl.get_shader_compile_status(s) { return Err(format!("shader: {}\n{}", gl.get_shader_info_log(s), src)); }
        gl.attach_shader(p, s); shs.push(s);
    }
    for (n, l) in ATTRS { gl.bind_attrib_location(p, l, n); }
    gl.link_program(p);
    if !gl.get_program_link_status(p) { return Err(format!("link: {}", gl.get_program_info_log(p))); }
    for s in shs { gl.detach_shader(p, s); gl.delete_shader(s); }
    let mut u = HashMap::new();
    for n in UNIFORMS { if let Some(l) = gl.get_uniform_location(p, n) { u.insert(n, l); } }
    Ok(Program { prog: p, u })
}

fn f(v: &Value) -> f32 { v.as_f64().unwrap_or(0.0) as f32 }
fn v3(v: &Value) -> [f32; 3] { [f(&v[0]), f(&v[1]), f(&v[2])] }
fn bytes_of(v: &Value, blob: &[u8]) -> &'static [u8] {
    let off = v[0].as_u64().unwrap_or(0) as usize; let len = v[1].as_u64().unwrap_or(0) as usize;
    let s = &blob[off.min(blob.len())..(off + len).min(blob.len())];
    // the blob outlives every use inside a single upload call; extend for convenience
    unsafe { std::slice::from_raw_parts(s.as_ptr(), s.len()) }
}
fn as_f32(b: &[u8]) -> Vec<f32> { b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect() }
fn f32_bytes(v: &[f32]) -> &[u8] { unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4) } }

impl Renderer {
    /// Must be called on a thread with a current GLES 3 context.
    pub unsafe fn new(gl: Gl) -> Result<Renderer, String> {
        let sky = compile(&gl, SKY_VS, SKY_FS)?; let points = compile(&gl, POINTS_VS, POINTS_FS)?;
        let dust = compile(&gl, DUST_VS, DUST_FS)?; let post = compile(&gl, POST_VS, POST_FS)?;
        let quad_vao = gl.create_vertex_array()?; let qb = gl.create_buffer()?;
        gl.bind_vertex_array(Some(quad_vao)); gl.bind_buffer(glow::ARRAY_BUFFER, Some(qb));
        let tri: [f32; 6] = [-1.0, -1.0, 3.0, -1.0, -1.0, 3.0];
        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, f32_bytes(&tri), glow::STATIC_DRAW);
        gl.enable_vertex_attrib_array(11); gl.vertex_attrib_pointer_f32(11, 2, glow::FLOAT, false, 8, 0);
        gl.bind_vertex_array(None);
        Ok(Renderer { gl, scene: None, mesh_progs: HashMap::new(), sky, points, dust, post, quad_vao, _quad_buf: qb, tgt: None, frame: FrameState::default(), stats: (0, 0), only: None })
    }

    pub fn has_scene(&self) -> bool { self.scene.is_some() }

    unsafe fn free_scene(&mut self) {
        let gl = &self.gl;
        if let Some(s) = self.scene.take() {
            for t in s.texs { gl.delete_texture(t.tex); }
            for g in s.geos { for a in g.attrs { gl.delete_buffer(a.buf); } if let Some(i) = g.index { gl.delete_buffer(i); } }
            for o in s.objs { gl.delete_vertex_array(o.vao); if let Some(b) = o.ibuf { gl.delete_buffer(b); } if let Some(b) = o.cbuf { gl.delete_buffer(b); } }
        }
    }

    /// Scene packet: "FWS1", u32 json length, json, blob.
    pub unsafe fn load_scene(&mut self, pkt: &[u8]) -> Result<(), String> {
        if pkt.len() < 8 || &pkt[0..4] != b"FWS1" { return Err("bad scene magic".into()); }
        let jl = u32::from_le_bytes([pkt[4], pkt[5], pkt[6], pkt[7]]) as usize;
        let head: Value = serde_json::from_slice(&pkt[8..8 + jl]).map_err(|e| e.to_string())?;
        let blob = &pkt[8 + ((jl + 3) & !3)..];
        self.free_scene();
        let gl = &self.gl;
        let mut texs = vec![];
        for t in head["texs"].as_array().cloned().unwrap_or_default() {
            let (w, h) = (t["w"].as_i64().unwrap_or(1) as i32, t["h"].as_i64().unwrap_or(1) as i32);
            let mut data = bytes_of(&t["data"], blob).to_vec();
            if t["flipY"].as_i64() == Some(1) { let row = (w * 4) as usize; let mut flipped = vec![0u8; data.len()];
                for y in 0..h as usize { flipped[y * row..(y + 1) * row].copy_from_slice(&data[(h as usize - 1 - y) * row..(h as usize - y) * row]); } data = flipped; }
            let tex = gl.create_texture()?; gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w, h, 0, glow::RGBA, glow::UNSIGNED_BYTE, Some(&data));
            let filt = |c: i64| match c { 1003 => glow::NEAREST, 1004 => glow::NEAREST_MIPMAP_NEAREST, 1005 => glow::NEAREST_MIPMAP_LINEAR, 1007 => glow::LINEAR_MIPMAP_NEAREST, 1008 => glow::LINEAR_MIPMAP_LINEAR, _ => glow::LINEAR };
            let wrap = |c: i64| match c { 1000 => glow::REPEAT, 1002 => glow::MIRRORED_REPEAT, _ => glow::CLAMP_TO_EDGE };
            let minf = filt(t["min"].as_i64().unwrap_or(1008)); let mip = t["mip"].as_i64() == Some(1) && minf != glow::NEAREST && minf != glow::LINEAR;
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, filt(t["mag"].as_i64().unwrap_or(1006)) as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, if mip { minf } else if minf == glow::NEAREST || minf == glow::NEAREST_MIPMAP_NEAREST || minf == glow::NEAREST_MIPMAP_LINEAR { glow::NEAREST } else { glow::LINEAR } as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, wrap(t["ws"].as_i64().unwrap_or(1001)) as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, wrap(t["wt"].as_i64().unwrap_or(1001)) as i32);
            if mip { gl.generate_mipmap(glow::TEXTURE_2D); }
            let mut m = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
            if let Some(a) = t["m"].as_array() { for (i, x) in a.iter().take(9).enumerate() { m[i] = f(x); } }
            texs.push(Texture { tex, m });
        }
        let mut geos = vec![];
        for g in head["geos"].as_array().cloned().unwrap_or_default() {
            let mut attrs = vec![];
            for a in g["attrs"].as_array().cloned().unwrap_or_default() {
                let b = gl.create_buffer()?; gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
                let dynamic = matches!(a["name"].as_str(), Some("aA") | Some("aS")) ;
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes_of(&a["data"], blob), if dynamic { glow::DYNAMIC_DRAW } else { glow::STATIC_DRAW });
                attrs.push(Attr { name: a["name"].as_str().unwrap_or("").to_string(), size: a["size"].as_i64().unwrap_or(3) as i32, buf: b });
            }
            let index = if g["index"].is_array() { let b = gl.create_buffer()?; gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(b));
                gl.buffer_data_u8_slice(glow::ELEMENT_ARRAY_BUFFER, bytes_of(&g["index"], blob), glow::STATIC_DRAW); Some(b) } else { None };
            let bs = g["bs"].as_array().map(|a| [f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3])]).unwrap_or([0.0, 0.0, 0.0, 1e9]);
            geos.push(Geometry { attrs, index, count: g["count"].as_i64().unwrap_or(0) as i32, dr: (g["dr"][0].as_i64().unwrap_or(0) as i32, g["dr"][1].as_i64().unwrap_or(-1) as i32), bs });
        }
        let mut mats = vec![];
        for m in head["mats"].as_array().cloned().unwrap_or_default() {
            let kind = match m["kind"].as_str().unwrap_or("basic") { "lambert" => MatKind::Lambert, "phong" => MatKind::Phong, "points" => MatKind::Points, "sky" => MatKind::Sky, "dust" => MatKind::Dust, _ => MatKind::Basic };
            let sky = if kind == MatKind::Sky { let u = &m["u"]; Some((v3(&u["sun"]), v3(&u["fog"]), v3(&u["top"]), v3(&u["glow"]), f(&u["halo"]))) } else { None };
            let dust = if kind == MatKind::Dust { let u = &m["u"]; Some((f(&u["dens"]), v3(&u["fog"]), v3(&u["col"]))) } else { None };
            mats.push(Material { kind, color: v3(&m["color"]), spec: v3(&m["spec"]), shin: f(&m["shin"]), emis: v3(&m["emis"]), map: m["map"].as_i64().unwrap_or(-1) as i32,
                alpha_test: f(&m["at"]), transparent: m["tr"].as_i64() == Some(1), opacity: f(&m["op"]), side: m["side"].as_i64().unwrap_or(0) as i32,
                vcolor: m["vc"].as_i64() == Some(1), fog: m["fog"].as_i64() == Some(1), depth_write: m["dw"].as_i64() != Some(0), depth_test: m["dt"].as_i64() != Some(0),
                poly: m["po"].as_array().map(|a| (f(&a[0]), f(&a[1]))), blend: m["blend"].as_i64().unwrap_or(1) as i32,
                sway: m["sway"].as_array().map(|a| (f(&a[0]), a[1].as_i64() == Some(1))), size: f(&m["size"]), sky, dust });
        }
        let mut objs = vec![];
        for o in head["objs"].as_array().cloned().unwrap_or_default() {
            let kind = match o["type"].as_str().unwrap_or("mesh") { "inst" => ObjKind::Inst, "points" => ObjKind::Points, _ => ObjKind::Mesh };
            let gi = o["g"].as_u64().unwrap_or(0) as usize; let geo = &geos[gi.min(geos.len() - 1)];
            let mut mw = [0f32; 16]; if let Some(a) = o["mw"].as_array() { for (i, x) in a.iter().take(16).enumerate() { mw[i] = f(x); } }
            let vao = gl.create_vertex_array()?; gl.bind_vertex_array(Some(vao));
            for a in &geo.attrs {
                let loc = match a.name.as_str() { "position" => 0, "normal" => 1, "uv" => 2, "color" => 3, "aA" => 9, "aS" => 10, _ => continue };
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(a.buf)); gl.enable_vertex_attrib_array(loc); gl.vertex_attrib_pointer_f32(loc, a.size, glow::FLOAT, false, 0, 0);
            }
            if let Some(ib) = geo.index { gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(ib)); }
            let (mut ibuf, mut cbuf) = (None, None);
            if kind == ObjKind::Inst {
                let b = gl.create_buffer()?; gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes_of(&o["im"], blob), glow::DYNAMIC_DRAW);
                for c in 0..4 { gl.enable_vertex_attrib_array(4 + c); gl.vertex_attrib_pointer_f32(4 + c, 4, glow::FLOAT, false, 64, (c * 16) as i32); gl.vertex_attrib_divisor(4 + c, 1); }
                ibuf = Some(b);
                if o["ic"].is_array() { let b = gl.create_buffer()?; gl.bind_buffer(glow::ARRAY_BUFFER, Some(b));
                    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytes_of(&o["ic"], blob), glow::DYNAMIC_DRAW);
                    gl.enable_vertex_attrib_array(8); gl.vertex_attrib_pointer_f32(8, 3, glow::FLOAT, false, 12, 0); gl.vertex_attrib_divisor(8, 1); cbuf = Some(b); }
            }
            gl.bind_vertex_array(None);
            let mi = o["m"].as_u64().unwrap_or(0) as usize;
            objs.push(Object { kind, geo: gi, mat: mi, mw, ro: o["ro"].as_i64().unwrap_or(0) as i32, cull: o["cull"].as_i64() == Some(1), vis: o["vis"].as_i64() != Some(0),
                opacity: mats.get(mi).map(|m| m.opacity).unwrap_or(1.0), count: o["count"].as_i64().unwrap_or(1) as i32, ibuf, cbuf,
                ibs: o["ibs"].as_array().map(|a| [f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3])]), vao,
                dr: o["dr"].as_array().map(|a| (a[0].as_i64().unwrap_or(0) as i32, a[1].as_i64().unwrap_or(0) as i32)) });
        }
        let mut hemi = ([0.0; 3], [0.0; 3], [0.0, 1.0, 0.0]); let mut dir = ([0.0; 3], [0.0, 1.0, 0.0]);
        if let Some(h) = head["lights"]["hemi"].as_array().and_then(|a| a.first()) { hemi = (v3(&h["sky"]), v3(&h["ground"]), v3(&h["d"])); }
        if let Some(d) = head["lights"]["dir"].as_array().and_then(|a| a.first()) { dir = (v3(&d["c"]), v3(&d["d"])); }
        let clear = head["clear"].as_array().map(|a| [f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3])]).unwrap_or([0.5, 0.5, 0.5, 0.0]);
        self.scene = Some(GpuScene { texs, geos, mats, objs, fog_c: v3(&head["fog"]["c"]), fog_d: f(&head["fog"]["d"]), clear, hemi, dir });
        // compile every program the scene needs now, so the first frames don't hitch
        let keys: Vec<MeshKey> = { let s = self.scene.as_ref().unwrap(); s.objs.iter().filter_map(|o| Self::mesh_key(s, o)).collect() };
        for k in keys { self.mesh_prog(&k)?; }
        Ok(())
    }

    fn mesh_key(s: &GpuScene, o: &Object) -> Option<MeshKey> {
        let m = &s.mats[o.mat]; let g = &s.geos[o.geo];
        let lit = match m.kind { MatKind::Basic => 0, MatKind::Lambert => 1, MatKind::Phong => 2, _ => return None };
        let has = |n: &str| g.attrs.iter().any(|a| a.name == n);
        Some(MeshKey { lit, map: m.map >= 0 && (m.map as usize) < s.texs.len() && has("uv"), vcolor: m.vcolor && has("color"), inst: o.kind == ObjKind::Inst, icolor: o.cbuf.is_some(),
            sway: m.sway.map(|(a, u)| ((a * 1000.0).round() as u32, u)), alpha_test: m.alpha_test > 0.0, double_side: m.side == 2, fog: m.fog, transparent: m.transparent })
    }
    unsafe fn mesh_prog(&mut self, k: &MeshKey) -> Result<(), String> {
        if !self.mesh_progs.contains_key(k) { let p = compile(&self.gl, &mesh_vs(k), &mesh_fs(k))?; self.mesh_progs.insert(k.clone(), p); }
        Ok(())
    }

    /// Frame packet (f32 LE): magic 7001, LW, LH, view16, proj16, cam3, time, wind, sunx, suny, sunvis, flash, speed, dustScale,
    /// nObj, {id, vis, opacity, mw16}*, nBuf, {1, geo, attr, len, data} | {2, obj, count, len, mats, lenc, cols}.
    pub unsafe fn apply_frame(&mut self, pkt: &[u8]) -> Result<(), String> {
        let v = as_f32(pkt); if v.len() < 47 || v[0] != 7001.0 { return Err("bad frame".into()); }
        let fr = &mut self.frame; fr.lw = v[1] as i32; fr.lh = v[2] as i32;
        fr.view.copy_from_slice(&v[3..19]); fr.proj.copy_from_slice(&v[19..35]); fr.cam = [v[35], v[36], v[37]];
        fr.time = v[38]; fr.wind = v[39]; fr.sun = [v[40], v[41]]; fr.sun_vis = v[42]; fr.flash = v[43]; fr.speed = v[44]; fr.dust_scale = v[45]; fr.valid = true;
        let Some(s) = self.scene.as_mut() else { return Ok(()) };
        let gl = &self.gl;
        let mut p = 46; let n = v[p] as usize; p += 1;
        for _ in 0..n { if p + 19 > v.len() { return Err("short obj".into()); }
            let id = v[p] as usize; if let Some(o) = s.objs.get_mut(id) { o.vis = v[p + 1] != 0.0; o.opacity = v[p + 2]; o.mw.copy_from_slice(&v[p + 3..p + 19]); } p += 19; }
        if p >= v.len() { return Ok(()); }
        let nb = v[p] as usize; p += 1;
        for _ in 0..nb {
            if p + 4 > v.len() { return Err("short buf".into()); }
            let kind = v[p] as i32;
            if kind == 1 { let (gid, k, len) = (v[p + 1] as usize, v[p + 2] as usize, v[p + 3] as usize); p += 4; if p + len > v.len() { return Err("short attr".into()); }
                if let Some(a) = s.geos.get(gid).and_then(|g| g.attrs.get(k)) { gl.bind_buffer(glow::ARRAY_BUFFER, Some(a.buf)); gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, f32_bytes(&v[p..p + len]), glow::DYNAMIC_DRAW); }
                p += len;
            } else if kind == 2 { let (id, cnt, len) = (v[p + 1] as usize, v[p + 2] as i32, v[p + 3] as usize); p += 4; if p + len >= v.len() { return Err("short inst".into()); }
                let mats = &v[p..p + len]; p += len; let lc = v[p] as usize; p += 1; let cols = &v[p..(p + lc).min(v.len())]; p += lc;
                if let Some(o) = s.objs.get_mut(id) { o.count = cnt;
                    if let Some(b) = o.ibuf { if len > 0 { gl.bind_buffer(glow::ARRAY_BUFFER, Some(b)); gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, f32_bytes(mats)); } }
                    if let Some(b) = o.cbuf { if lc > 0 { gl.bind_buffer(glow::ARRAY_BUFFER, Some(b)); gl.buffer_sub_data_u8_slice(glow::ARRAY_BUFFER, 0, f32_bytes(cols)); } } }
            } else { return Err(format!("buf kind {kind}")); }
        }
        Ok(())
    }

    unsafe fn ensure_targets(&mut self, lw: i32, lh: i32) -> Result<(), String> {
        if let Some(t) = &self.tgt { if t.0 == lw && t.1 == lh { return Ok(()); } }
        let gl = &self.gl;
        if let Some((_, _, f1, t1, rb, f2, t2)) = self.tgt.take() { gl.delete_framebuffer(f1); gl.delete_texture(t1); gl.delete_renderbuffer(rb); gl.delete_framebuffer(f2); gl.delete_texture(t2); }
        let mk_tex = |filter: u32| -> Result<glow::Texture, String> { let t = gl.create_texture()?; gl.bind_texture(glow::TEXTURE_2D, Some(t));
            gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, lw, lh, 0, glow::RGBA, glow::UNSIGNED_BYTE, None);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, filter as i32); gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, filter as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32); gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32); Ok(t) };
        let t1 = mk_tex(glow::LINEAR)?; let rb = gl.create_renderbuffer()?; gl.bind_renderbuffer(glow::RENDERBUFFER, Some(rb));
        gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH_COMPONENT24, lw, lh);
        let f1 = gl.create_framebuffer()?; gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f1));
        gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(t1), 0);
        gl.framebuffer_renderbuffer(glow::FRAMEBUFFER, glow::DEPTH_ATTACHMENT, glow::RENDERBUFFER, Some(rb));
        let t2 = mk_tex(glow::NEAREST)?; let f2 = gl.create_framebuffer()?; gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f2));
        gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(t2), 0);
        if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE { return Err("framebuffer incomplete".into()); }
        self.tgt = Some((lw, lh, f1, t1, rb, f2, t2));
        Ok(())
    }

    /// Draw the current state into `screen` (None = default framebuffer) of size w x h.
    pub unsafe fn draw(&mut self, screen: Option<glow::Framebuffer>, w: i32, h: i32) -> Result<(), String> {
        let fr = self.frame;
        if self.scene.is_none() || !fr.valid || fr.lw < 8 || fr.lh < 8 {
            let gl = &self.gl; gl.bind_framebuffer(glow::FRAMEBUFFER, screen); gl.viewport(0, 0, w, h);
            gl.clear_color(0.043, 0.063, 0.043, 1.0); gl.clear(glow::COLOR_BUFFER_BIT); return Ok(());
        }
        self.ensure_targets(fr.lw, fr.lh)?;
        let (lw, lh, f1, t1, _, f2, _) = self.tgt.unwrap();
        // make sure every program exists (new materials can't appear mid-scene, but be safe)
        let keys: Vec<MeshKey> = { let s = self.scene.as_ref().unwrap(); s.objs.iter().filter_map(|o| Self::mesh_key(s, o)).collect() };
        for k in &keys { self.mesh_prog(k)?; }
        let gl = &self.gl; let s = self.scene.as_ref().unwrap();
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f1)); gl.viewport(0, 0, lw, lh);
        gl.disable(glow::SCISSOR_TEST); gl.depth_mask(true); gl.color_mask(true, true, true, true);
        gl.clear_color(s.clear[0], s.clear[1], s.clear[2], s.clear[3]); gl.clear_depth_f32(1.0);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        gl.enable(glow::DEPTH_TEST); gl.depth_func(glow::LEQUAL);
        let pv = mul(&fr.proj, &fr.view); let planes = frustum(&pv);
        // visibility + sort keys
        let mut opaque: Vec<(i32, f32, usize)> = vec![]; let mut transp: Vec<(i32, f32, usize)> = vec![];
        for (i, o) in s.objs.iter().enumerate() {
            if !o.vis || (o.kind == ObjKind::Inst && o.count <= 0) { continue; }
            if let Some(only) = &self.only { if !only.contains(&i) { continue; } }
            let g = &s.geos[o.geo]; let bs = o.ibs.unwrap_or(g.bs);
            let c = transform_point(&o.mw, [bs[0], bs[1], bs[2]]); let r = bs[3] * max_scale(&o.mw);
            if o.cull && !sphere_visible(&planes, c, r) { continue; }
            let vz = transform_point(&fr.view, c)[2];
            let m = &s.mats[o.mat];
            if m.transparent { transp.push((o.ro, vz, i)); } else { opaque.push((o.ro, -vz, i)); }
        }
        opaque.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)));
        transp.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)));
        let mut calls = 0u32; let mut tris = 0u32;
        for list in [&opaque, &transp] { for &(_, _, i) in list.iter() {
            let o = &s.objs[i]; let m = &s.mats[o.mat]; let g = &s.geos[o.geo];
            // state
            if m.transparent && m.blend != 0 { gl.enable(glow::BLEND); gl.blend_equation(glow::FUNC_ADD);
                gl.blend_func_separate(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA, glow::ONE, glow::ONE_MINUS_SRC_ALPHA); } else { gl.disable(glow::BLEND); }
            if m.depth_test { gl.enable(glow::DEPTH_TEST); } else { gl.disable(glow::DEPTH_TEST); }
            gl.depth_mask(m.depth_write);
            match m.side { 2 => gl.disable(glow::CULL_FACE), 1 => { gl.enable(glow::CULL_FACE); gl.cull_face(glow::FRONT); } _ => { gl.enable(glow::CULL_FACE); gl.cull_face(glow::BACK); } }
            gl.front_face(if det3(&o.mw) < 0.0 { glow::CW } else { glow::CCW });
            if let Some((fa, un)) = m.poly { gl.enable(glow::POLYGON_OFFSET_FILL); gl.polygon_offset(fa, un); } else { gl.disable(glow::POLYGON_OFFSET_FILL); }
            let prog = match m.kind { MatKind::Sky => &self.sky, MatKind::Points => &self.points, MatKind::Dust => &self.dust,
                _ => match Self::mesh_key(s, o).and_then(|k| self.mesh_progs.get(&k)) { Some(p) => p, None => continue } };
            gl.use_program(Some(prog.prog));
            let u = |n: &str| prog.u.get(n);
            gl.uniform_matrix_4_f32_slice(u("uModel"), false, &o.mw); gl.uniform_matrix_4_f32_slice(u("uView"), false, &fr.view); gl.uniform_matrix_4_f32_slice(u("uProj"), false, &fr.proj);
            match m.kind {
                MatKind::Sky => { let (sun, fog, top, glow_, halo) = m.sky.unwrap();
                    gl.uniform_3_f32_slice(u("uSun"), &sun); gl.uniform_3_f32_slice(u("uFog"), &fog); gl.uniform_3_f32_slice(u("uTop"), &top); gl.uniform_3_f32_slice(u("uGlow"), &glow_); gl.uniform_1_f32(u("uHalo"), halo); }
                MatKind::Points => { gl.uniform_1_f32(u("uSize"), m.size); gl.uniform_3_f32_slice(u("uColor"), &m.color); gl.uniform_1_f32(u("uOpacity"), o.opacity);
                    gl.uniform_3_f32_slice(u("uFogColor"), &s.fog_c); gl.uniform_1_f32(u("uFogDensity"), if m.fog { s.fog_d } else { 0.0 }); }
                MatKind::Dust => { let (dens, fog, col) = m.dust.unwrap(); gl.uniform_1_f32(u("uScale"), fr.dust_scale); gl.uniform_1_f32(u("uDens"), dens);
                    gl.uniform_3_f32_slice(u("uFog"), &fog); gl.uniform_3_f32_slice(u("uCol"), &col); }
                _ => {
                    gl.uniform_matrix_3_f32_slice(u("uNormalM"), false, &normal_matrix(&o.mw));
                    let tm = if m.map >= 0 { s.texs.get(m.map as usize).map(|t| t.m).unwrap_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]) } else { [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0] };
                    gl.uniform_matrix_3_f32_slice(u("uMapM"), false, &tm);
                    gl.uniform_1_f32(u("uTime"), fr.time); gl.uniform_1_f32(u("uWind"), fr.wind);
                    gl.uniform_3_f32_slice(u("uColor"), &m.color); gl.uniform_1_f32(u("uOpacity"), o.opacity); gl.uniform_1_f32(u("uAlphaTest"), m.alpha_test);
                    gl.uniform_3_f32_slice(u("uHemiSky"), &s.hemi.0); gl.uniform_3_f32_slice(u("uHemiGround"), &s.hemi.1); gl.uniform_3_f32_slice(u("uHemiDir"), &s.hemi.2);
                    gl.uniform_3_f32_slice(u("uDirColor"), &s.dir.0); gl.uniform_3_f32_slice(u("uDirDir"), &s.dir.1);
                    gl.uniform_3_f32_slice(u("uFogColor"), &s.fog_c); gl.uniform_1_f32(u("uFogDensity"), s.fog_d); gl.uniform_3_f32_slice(u("uCamPos"), &fr.cam);
                    gl.uniform_3_f32_slice(u("uSpec"), &m.spec); gl.uniform_1_f32(u("uShin"), m.shin.max(1e-4)); gl.uniform_3_f32_slice(u("uEmissive"), &m.emis);
                    gl.uniform_1_i32(u("uMap"), 0);
                    if m.map >= 0 { if let Some(t) = s.texs.get(m.map as usize) { gl.active_texture(glow::TEXTURE0); gl.bind_texture(glow::TEXTURE_2D, Some(t.tex)); } }
                }
            }
            gl.bind_vertex_array(Some(o.vao));
            // constant values for attributes a geometry lacks
            if !g.attrs.iter().any(|a| a.name == "normal") { gl.vertex_attrib_4_f32(1, 0.0, 0.0, 1.0, 1.0); }
            if !g.attrs.iter().any(|a| a.name == "uv") { gl.vertex_attrib_4_f32(2, 0.0, 0.0, 0.0, 1.0); }
            if !g.attrs.iter().any(|a| a.name == "color") { gl.vertex_attrib_4_f32(3, 1.0, 1.0, 1.0, 1.0); }
            if o.cbuf.is_none() { gl.vertex_attrib_4_f32(8, 1.0, 1.0, 1.0, 1.0); }
            let dr = o.dr.unwrap_or(g.dr); let start = dr.0.max(0); let full = if dr.1 < 0 { g.count - start } else { dr.1.min(g.count - start) };
            match o.kind {
                ObjKind::Points => gl.draw_arrays(glow::POINTS, start, full),
                ObjKind::Inst => { if g.index.is_some() { gl.draw_elements_instanced(glow::TRIANGLES, full, glow::UNSIGNED_INT, start * 4, o.count); }
                    else { gl.draw_arrays_instanced(glow::TRIANGLES, start, full, o.count); } tris += (full as u32 / 3) * o.count as u32; }
                ObjKind::Mesh => { if g.index.is_some() { gl.draw_elements(glow::TRIANGLES, full, glow::UNSIGNED_INT, start * 4); } else { gl.draw_arrays(glow::TRIANGLES, start, full); } tris += full as u32 / 3; }
            }
            calls += 1;
        } }
        gl.bind_vertex_array(None); gl.disable(glow::BLEND); gl.disable(glow::POLYGON_OFFSET_FILL); gl.disable(glow::CULL_FACE); gl.depth_mask(true);
        // post at render resolution
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(f2)); gl.viewport(0, 0, lw, lh); gl.disable(glow::DEPTH_TEST);
        let p = &self.post; gl.use_program(Some(p.prog));
        gl.active_texture(glow::TEXTURE0); gl.bind_texture(glow::TEXTURE_2D, Some(t1)); gl.uniform_1_i32(p.u.get("tScene"), 0);
        gl.uniform_2_f32(p.u.get("uSun"), fr.sun[0], fr.sun[1]); gl.uniform_1_f32(p.u.get("uSunVis"), fr.sun_vis); gl.uniform_1_f32(p.u.get("uTime"), fr.time);
        gl.uniform_1_f32(p.u.get("uFlash"), fr.flash); gl.uniform_1_f32(p.u.get("uSpeed"), fr.speed);
        gl.bind_vertex_array(Some(self.quad_vao)); gl.draw_arrays(glow::TRIANGLES, 0, 3); gl.bind_vertex_array(None);
        // nearest-neighbour upscale to the screen
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(f2)); gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, screen);
        gl.blit_framebuffer(0, 0, lw, lh, 0, 0, w, h, glow::COLOR_BUFFER_BIT, glow::NEAREST);
        gl.bind_framebuffer(glow::FRAMEBUFFER, screen);
        self.stats = (calls, tris);
        Ok(())
    }

    pub fn gl(&self) -> &Gl { &self.gl }
}

impl Drop for Renderer {
    fn drop(&mut self) { unsafe { self.free_scene(); } }
}
