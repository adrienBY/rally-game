//! Desktop check for the native renderer: headless EGL (Mesa surfaceless), renders a dumped scene + frame packet to a PPM.
//!   render_host <scene> <frame> <out.ppm> [width height]
use fogwood_core::render::{loader::Loader, Renderer};
use glow::HasContext;
use std::ffi::{c_char, c_void};

type P = *mut c_void;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 { eprintln!("usage: render_host scene frame out.ppm [w h]"); std::process::exit(2); }
    let (w, h): (i32, i32) = (a.get(4).and_then(|v| v.parse().ok()).unwrap_or(860), a.get(5).and_then(|v| v.parse().ok()).unwrap_or(390));
    let loader = Loader::new();
    let egl = loader.egl_handle(); assert!(!egl.is_null(), "no libEGL");
    unsafe {
        extern "C" { fn dlsym(h: P, s: *const c_char) -> P; }
        let sym = |n: &[u8]| { let p = dlsym(egl, n.as_ptr() as *const c_char); assert!(!p.is_null(), "missing {:?}", std::str::from_utf8(n)); p };
        let get_platform_display: extern "C" fn(u32, P, *const isize) -> P = std::mem::transmute(sym(b"eglGetPlatformDisplay\0"));
        let initialize: extern "C" fn(P, *mut i32, *mut i32) -> u32 = std::mem::transmute(sym(b"eglInitialize\0"));
        let bind_api: extern "C" fn(u32) -> u32 = std::mem::transmute(sym(b"eglBindAPI\0"));
        let choose: extern "C" fn(P, *const i32, *mut P, i32, *mut i32) -> u32 = std::mem::transmute(sym(b"eglChooseConfig\0"));
        let create_ctx: extern "C" fn(P, P, P, *const i32) -> P = std::mem::transmute(sym(b"eglCreateContext\0"));
        let make_current: extern "C" fn(P, P, P, P) -> u32 = std::mem::transmute(sym(b"eglMakeCurrent\0"));
        let dpy = get_platform_display(0x31DD, std::ptr::null_mut(), std::ptr::null());
        let (mut ma, mut mi) = (0, 0); assert!(initialize(dpy, &mut ma, &mut mi) != 0, "eglInitialize");
        bind_api(0x30A0);
        let attrs = [0x3040, 0x40, 0x3033, 0x1, 0x3038]; let mut cfg: P = std::ptr::null_mut(); let mut n = 0;
        choose(dpy, attrs.as_ptr(), &mut cfg, 1, &mut n); assert!(n > 0, "no config");
        let cattrs = [0x3098, 3, 0x3038]; let ctx = create_ctx(dpy, cfg, std::ptr::null_mut(), cattrs.as_ptr()); assert!(!ctx.is_null(), "context");
        assert!(make_current(dpy, std::ptr::null_mut(), std::ptr::null_mut(), ctx) != 0, "make current");
        let gl = glow::Context::from_loader_function(|n| loader.get(n));
        println!("GL: {} / {}", gl.get_parameter_string(glow::VERSION), gl.get_parameter_string(glow::RENDERER));
        let mut r = Renderer::new(gl).expect("renderer");
        if let Ok(v) = std::env::var("ONLY") { r.only = Some(v.split(',').filter_map(|x| x.parse().ok()).collect()); }
        let t0 = std::time::Instant::now();
        r.load_scene(&std::fs::read(&a[1]).unwrap()).expect("scene");
        println!("scene upload {:?}", t0.elapsed());
        // a frame file, or a stream file: repeated [u32 len][packet] (scene packets inside are loaded too)
        let fb_bytes = std::fs::read(&a[2]).unwrap();
        if a[2].ends_with(".stream") { let mut p = 0; let mut n = 0;
            while p + 4 <= fb_bytes.len() { let l = u32::from_le_bytes([fb_bytes[p], fb_bytes[p + 1], fb_bytes[p + 2], fb_bytes[p + 3]]) as usize; p += 4;
                let pk = &fb_bytes[p..p + l]; p += l; n += 1;
                if &pk[0..4] == b"FWS1" { r.load_scene(pk).expect("scene in stream"); } else { r.apply_frame(pk).expect("frame in stream"); } }
            println!("stream packets {n}");
        } else { r.apply_frame(&fb_bytes).expect("frame"); }
        // offscreen "screen"
        let gl = r.gl();
        let tex = gl.create_texture().unwrap(); gl.bind_texture(glow::TEXTURE_2D, Some(tex));
        gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w, h, 0, glow::RGBA, glow::UNSIGNED_BYTE, None);
        let fb = gl.create_framebuffer().unwrap(); gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
        gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
        let t1 = std::time::Instant::now();
        r.draw(Some(fb), w, h).expect("draw");
        let gl = r.gl(); gl.finish();
        println!("draw {:?} calls {} tris {}", t1.elapsed(), r.stats.0, r.stats.1);
        let gl = r.gl(); gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fb));
        let mut px = vec![0u8; (w * h * 4) as usize];
        gl.read_pixels(0, 0, w, h, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(&mut px));
        let err = gl.get_error(); if err != 0 { println!("GL error 0x{:x}", err); }
        let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
        for y in (0..h).rev() { for x in 0..w { let i = ((y * w + x) * 4) as usize; out.extend_from_slice(&px[i..i + 3]); } }
        std::fs::write(&a[3], out).unwrap();
    }
}
