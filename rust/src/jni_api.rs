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
