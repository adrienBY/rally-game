//! JNI entry points for com.fogwood.rally.NativeCore
use crate::bridge;
use jni::objects::{JClass, JString};
use jni::sys::{jbyteArray, jint, jstring};
use jni::JNIEnv;

fn s(env: &mut JNIEnv, j: &JString) -> String { env.get_string(j).map(|v| v.into()).unwrap_or_default() }
fn out(env: &mut JNIEnv, v: String) -> jstring { env.new_string(v).map(|o| o.into_raw()).unwrap_or(std::ptr::null_mut()) }

#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_prepare<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>, map: jint, path: JString<'l>, roadh: JString<'l>, gxh: jint, gzh: jint) -> jstring {
    let p = s(&mut env, &path); let r = s(&mut env, &roadh);
    let res = bridge::prepare(map, &p, &r, gxh, gzh); out(&mut env, res)
}

#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_gridBytes<'l>(env: JNIEnv<'l>, _c: JClass<'l>) -> jbyteArray {
    bridge::with_grid(|g| env.byte_array_from_slice(g).map(|a| a.into_raw()).unwrap_or(std::ptr::null_mut())).unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_obstacles<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>, csv: JString<'l>) -> jint {
    let v = s(&mut env, &csv); bridge::set_obstacles(&v)
}

#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_step<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>, csv: JString<'l>) -> jstring {
    let v = s(&mut env, &csv); let r = bridge::step(&v); out(&mut env, r)
}

// ---- native renderer (GL thread: glInit/glResize/glDraw; any thread: glMessage) ----
use jni::objects::JByteArray;
use jni::sys::jboolean;

#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_glInit<'l>(_env: JNIEnv<'l>, _c: JClass<'l>) -> jboolean {
    if crate::gfx::gl_init() { 1 } else { 0 }
}
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_glResize<'l>(_env: JNIEnv<'l>, _c: JClass<'l>, w: jint, h: jint) { crate::gfx::gl_resize(w, h); }
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_glDraw<'l>(_env: JNIEnv<'l>, _c: JClass<'l>) { crate::gfx::gl_draw(); }
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_glMessage<'l>(env: JNIEnv<'l>, _c: JClass<'l>, data: JByteArray<'l>) {
    if let Ok(v) = env.convert_byte_array(&data) { crate::gfx::push_message(v); }
}
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_glStatus<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>) -> jstring {
    let s = crate::gfx::status(); out(&mut env, s)
}

// ---- sound engine (rust/audio): AudioTrack thread pulls audioRender, the page posts binary messages ----
use jni::objects::JFloatArray;
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_audioInit<'l>(_env: JNIEnv<'l>, _c: JClass<'l>, sr: jint) { fogwood_audio::native::init(sr as f32); }
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_audioMessage<'l>(env: JNIEnv<'l>, _c: JClass<'l>, data: JByteArray<'l>) {
    if let Ok(v) = env.convert_byte_array(&data) { fogwood_audio::native::message(&v); }
}
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_audioRender<'l>(env: JNIEnv<'l>, _c: JClass<'l>, out: JFloatArray<'l>, frames: jint) {
    thread_local! { static BUF: std::cell::RefCell<Vec<f32>> = const { std::cell::RefCell::new(Vec::new()) }; }
    BUF.with(|b| { let mut b = b.borrow_mut(); let n = (frames.max(0) as usize) * 2; if b.len() < n { b.resize(n, 0.0); }
        fogwood_audio::native::render(&mut b[..n]); let _ = env.set_float_array_region(&out, 0, &b[..n]); });
}
