//! Headless run of the app's own frame path (gfx::rt_frame) with the benchmark recorder:
//!   bench_host <scene> <rt> <savedir> <seconds> [race]
use fogwood_core::gfx;
use fogwood_core::render::loader::Loader;
use std::ffi::{c_char, c_void};
type P = *mut c_void;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let loader = Loader::new(); let egl = loader.egl_handle(); assert!(!egl.is_null(), "no libEGL");
    unsafe {
        extern "C" { fn dlsym(h: P, s: *const c_char) -> P; }
        let sym = |n: &[u8]| { let p = dlsym(egl, n.as_ptr() as *const c_char); assert!(!p.is_null()); p };
        let get_platform_display: extern "C" fn(u32, P, *const isize) -> P = std::mem::transmute(sym(b"eglGetPlatformDisplay\0"));
        let initialize: extern "C" fn(P, *mut i32, *mut i32) -> u32 = std::mem::transmute(sym(b"eglInitialize\0"));
        let bind_api: extern "C" fn(u32) -> u32 = std::mem::transmute(sym(b"eglBindAPI\0"));
        let choose: extern "C" fn(P, *const i32, *mut P, i32, *mut i32) -> u32 = std::mem::transmute(sym(b"eglChooseConfig\0"));
        let create_ctx: extern "C" fn(P, P, P, *const i32) -> P = std::mem::transmute(sym(b"eglCreateContext\0"));
        let make_current: extern "C" fn(P, P, P, P) -> u32 = std::mem::transmute(sym(b"eglMakeCurrent\0"));
        let dpy = get_platform_display(0x31DD, std::ptr::null_mut(), std::ptr::null());
        let (mut ma, mut mi) = (0, 0); assert!(initialize(dpy, &mut ma, &mut mi) != 0); bind_api(0x30A0);
        let attrs = [0x3040, 0x40, 0x3033, 0x1, 0x3038]; let mut cfg: P = std::ptr::null_mut(); let mut n = 0; choose(dpy, attrs.as_ptr(), &mut cfg, 1, &mut n);
        let cattrs = [0x3098, 3, 0x3038]; let ctx = create_ctx(dpy, cfg, std::ptr::null_mut(), cattrs.as_ptr());
        assert!(make_current(dpy, std::ptr::null_mut(), std::ptr::null_mut(), ctx) != 0);
        let _ = &loader;
        let save = format!("{}/fogwood.sav", a[3]); println!("map {}", gfx::rt_init(&save)); gfx::rt_device("desktop test");
        assert!(!gfx::gl_init() || true); gfx::gl_resize(860, 390);
        let rtb = std::fs::read(&a[2]).unwrap(); let rt = fogwood_core::game::rt::parse(&rtb).unwrap();
        let path: Vec<String> = rt.path.iter().flat_map(|p| [p[0].to_string(), p[1].to_string()]).collect(); let roadh: Vec<String> = rt.roadh.iter().map(|v| v.to_string()).collect();
        fogwood_core::bridge::prepare(rt.map_id, &path.join(","), &roadh.join(","), rt.gxh, rt.gzh);
        let obs: Vec<String> = rt.obstacles.iter().flat_map(|o| [o[0].to_string(), o[1].to_string(), o[2].to_string()]).collect(); fogwood_core::bridge::set_obstacles(&obs.join(","));
        fogwood_audio::native::init(48000.0);
        gfx::push_message(std::fs::read(&a[1]).unwrap()); gfx::push_message(rtb);
        let secs: f64 = a[4].parse().unwrap(); let n = (secs * 60.0) as usize;
        for f in 0..n {
            if f == 30 { gfx::rt_touch(0, 1, 430.0, 200.0); gfx::rt_touch(2, 1, 430.0, 200.0); }   // tap: start
            if f == 60 { gfx::rt_bench(); }
            if f > 60 && f % 7 == 0 { gfx::rt_touch(0, 2, 830.0, 330.0); }   // hold the gas pedal
            if f == (n - 2) { gfx::rt_bench(); }
            let ev = gfx::rt_frame(1.0 / 60.0); for l in ev.lines() { if !l.starts_with("h ") { println!("event {}", l); } }
        }
        println!("status {}", gfx::status());
    }
}
