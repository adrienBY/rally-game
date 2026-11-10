//! Finds OpenGL ES entry points: exported symbols of the GLES library first, then eglGetProcAddress.
use std::ffi::{c_char, c_int, c_void, CString};

#[link(name = "dl")]
extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}
const RTLD_LAZY: c_int = 1;

pub struct Loader { gles: Vec<*mut c_void>, egl: *mut c_void, gpa: Option<unsafe extern "C" fn(*const c_char) -> *const c_void> }

impl Loader {
    pub fn new() -> Loader {
        let open = |n: &str| { let c = CString::new(n).unwrap(); unsafe { dlopen(c.as_ptr(), RTLD_LAZY) } };
        let gles: Vec<_> = ["libGLESv3.so", "libGLESv2.so", "libGLESv2.so.2"].iter().map(|n| open(n)).filter(|p| !p.is_null()).collect();
        let mut egl = std::ptr::null_mut();
        for n in ["libEGL.so", "libEGL.so.1"] { let p = open(n); if !p.is_null() { egl = p; break; } }
        let gpa = if egl.is_null() { None } else {
            let s = unsafe { dlsym(egl, b"eglGetProcAddress\0".as_ptr() as *const c_char) };
            if s.is_null() { None } else { Some(unsafe { std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*const c_char) -> *const c_void>(s) }) }
        };
        Loader { gles, egl, gpa }
    }
    pub fn egl_handle(&self) -> *mut c_void { self.egl }
    pub fn get(&self, name: &str) -> *const c_void {
        let c = CString::new(name).unwrap();
        for &h in &self.gles { let p = unsafe { dlsym(h, c.as_ptr()) }; if !p.is_null() { return p; } }
        if let Some(g) = self.gpa { return unsafe { g(c.as_ptr()) }; }
        std::ptr::null()
    }
}
