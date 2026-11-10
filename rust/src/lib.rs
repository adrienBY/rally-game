//! Fogwood Rally native core.
//! * `world`   – stage generation (road distance field, nearest-road index, terrain height)
//! * `physics` – four-tyre vehicle model with suspension, gearbox and collisions
//! * `bridge`  – a tiny text protocol so the WebView front end can drive it through one JNI call per frame
pub mod noise;
pub mod world;
pub mod physics;
pub mod bridge;
pub mod render;
pub mod gfx;
#[cfg(any(target_os = "android", feature = "jni-host"))]
mod jni_api;
