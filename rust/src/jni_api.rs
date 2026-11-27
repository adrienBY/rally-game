//! JNI entry points for com.fogwood.rally.NativeCore
use crate::bridge;
use jni::objects::{JClass, JString};
use jni::sys::{jbyteArray, jint, jstring};
use jni::JNIEnv;

fn s(env: &mut JNIEnv, j: &JString) -> String { env.get_string(j).map(|v| v.into()).unwrap_or_default() }
fn out(env: &mut JNIEnv, v: String) -> jstring { env.new_string(v).or_else(|_| env.new_string("")).map(|o| o.into_raw()).unwrap_or(std::ptr::null_mut()) }

#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_prepare<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>, map: jint, path: JString<'l>, roadh: JString<'l>, gxh: jint, gzh: jint) -> jstring { 
    let p = s(&mut env, &path); let r = s(&mut env, &roadh);
    let res = crate::guard::run("prepare", "err panic".to_string(), || bridge::prepare(map, &p, &r, gxh, gzh)); out(&mut env, res)
}

#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_gridBytes<'l>(env: JNIEnv<'l>, _c: JClass<'l>) -> jbyteArray { crate::guard::run("gridBytes", std::ptr::null_mut(), || {
    bridge::with_grid(|g| env.byte_array_from_slice(g).map(|a| a.into_raw()).unwrap_or(std::ptr::null_mut())).unwrap_or(std::ptr::null_mut())
}) }

#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_obstacles<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>, csv: JString<'l>) -> jint { crate::guard::run("obstacles", 0, || {
    let v = s(&mut env, &csv); bridge::set_obstacles(&v)
}) }

#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_step<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>, csv: JString<'l>) -> jstring { 
    let v = s(&mut env, &csv); let r = crate::guard::run("step", String::new(), || bridge::step(&v)); out(&mut env, r)
}

// ---- native renderer (GL thread: glInit/glResize/glDraw; any thread: glMessage) ----
use jni::objects::JByteArray;
use jni::sys::jboolean;

#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_glInit<'l>(_env: JNIEnv<'l>, _c: JClass<'l>) -> jboolean { crate::guard::run("glInit", 0, || {
    if crate::gfx::gl_init() { 1 } else { 0 }
}) }
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_glResize<'l>(_env: JNIEnv<'l>, _c: JClass<'l>, w: jint, h: jint) { crate::guard::run("glResize", (), || { crate::gfx::gl_resize(w, h); }) }
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_glDraw<'l>(_env: JNIEnv<'l>, _c: JClass<'l>) { crate::guard::run("glDraw", (), || { crate::gfx::gl_draw(); }) }
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_glMessage<'l>(env: JNIEnv<'l>, _c: JClass<'l>, data: JByteArray<'l>) { crate::guard::run("glMessage", (), || {
    if let Ok(v) = env.convert_byte_array(&data) { crate::gfx::push_message(v); }
}) }
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_glStatus<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>) -> jstring { 
    let s = crate::guard::run("glStatus", String::new(), crate::gfx::status); out(&mut env, s)
}

// ---- sound engine (rust/audio): AudioTrack thread pulls audioRender, the page posts binary messages ----
use jni::objects::JFloatArray;
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_audioInit<'l>(_env: JNIEnv<'l>, _c: JClass<'l>, sr: jint) { crate::guard::run("audioInit", (), || { fogwood_audio::native::init(sr as f32); }) }
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_audioMessage<'l>(env: JNIEnv<'l>, _c: JClass<'l>, data: JByteArray<'l>) { crate::guard::run("audioMessage", (), || {
    if let Ok(v) = env.convert_byte_array(&data) { fogwood_audio::native::message(&v); }
}) }
/// a recording from the app's assets (snd/<name>): decoded natively and handed to the sound engine
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_audioFile<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>, name: JString<'l>, data: JByteArray<'l>) -> jboolean {
    let n = s(&mut env, &name); let Ok(v) = env.convert_byte_array(&data) else { return 0 };
    crate::guard::run("audioFile", 0, || if crate::stage::sound::load(&n, &v) { 1 } else { 0 })
}
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_audioRender<'l>(env: JNIEnv<'l>, _c: JClass<'l>, out: JFloatArray<'l>, frames: jint) { crate::guard::run("audioRender", (), || {
    thread_local! { static BUF: std::cell::RefCell<Vec<f32>> = const { std::cell::RefCell::new(Vec::new()) }; }
    BUF.with(|b| { let mut b = b.borrow_mut(); let n = (frames.max(0) as usize) * 2; if b.len() < n { b.resize(n, 0.0); }
        fogwood_audio::native::render(&mut b[..n]); let _ = env.set_float_array_region(&out, 0, &b[..n]); });
}) }

// ---- runtime mode: the native game loop ----
use jni::sys::{jfloat, jlong};
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_rtInit<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>, path: JString<'l>) -> jstring { 
    let p = s(&mut env, &path); crate::guard::set_dir(&p); crate::guard::install();
    let m = crate::guard::run("rtInit", "forest".to_string(), || crate::gfx::rt_init(&p)); out(&mut env, m)
}
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_rtTouch<'l>(_env: JNIEnv<'l>, _c: JClass<'l>, action: jint, id: jint, x: jfloat, y: jfloat) { crate::guard::run("rtTouch", (), || { crate::gfx::rt_touch(action, id, x, y); }) }
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_rtBack<'l>(_env: JNIEnv<'l>, _c: JClass<'l>) -> jboolean { crate::guard::run("rtBack", 0, || { if crate::gfx::rt_back() { 1 } else { 0 } }) }
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_rtInsets<'l>(_env: JNIEnv<'l>, _c: JClass<'l>, t: jfloat, r: jfloat, b: jfloat, l: jfloat) { crate::guard::run("rtInsets", (), || { crate::gfx::rt_insets(t, r, b, l); }) }
/// the driver name from the system keyboard dialog
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_rtName<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>, n: JString<'l>) { let v = s(&mut env, &n); crate::guard::run("rtName", (), || crate::gfx::rt_name(&v)) }
/// GL thread: one game frame; dt in nanoseconds. Returns the app events, one per line.
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_rtFrame<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>, dt_ns: jlong) -> jstring { 
    let e = crate::gfx::rt_frame_guarded(dt_ns as f64 * 1e-9); out(&mut env, e)
}
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_rtThermal<'l>(_env: JNIEnv<'l>, _c: JClass<'l>, status: jint, batt: jfloat) { crate::guard::run("rtThermal", (), || { crate::gfx::rt_thermal(status, batt); }) }
#[no_mangle]
pub extern "system" fn Java_com_fogwood_rally_NativeCore_rtDevice<'l>(mut env: JNIEnv<'l>, _c: JClass<'l>, info: JString<'l>) { crate::guard::run("rtDevice", (), || { let i = s(&mut env, &info); crate::gfx::rt_device(&i); }) }
