//! The browser build: the same game as the Android app, compiled to WebAssembly. The page (index.html in runtime mode)
//! builds each stage and posts it here instead of to the app's FogwoodGL bridge; the game draws into a WebGL2 canvas,
//! keeps its saves in localStorage and streams sound messages to the page's AudioWorklet (the same Rust sound engine).
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// the page's sound sender (posts to the AudioWorklet); never throws
    #[wasm_bindgen(js_namespace = window, js_name = __fwAudio)]
    fn fw_audio(b: &[u8]);
}

pub(crate) fn audio(b: &[u8]) { fw_audio(b); }

fn storage() -> Option<web_sys::Storage> { web_sys::window()?.local_storage().ok()? }
pub(crate) fn store_load(key: &str) -> Result<String, ()> { storage().and_then(|s| s.get_item(key).ok()?).ok_or(()) }
pub(crate) fn store_save(key: &str, text: &str) { if let Some(s) = storage() { let _ = s.set_item(key, text); } }

/// the WebGL2 context of `canvas` drives the renderer; false if WebGL2 isn't there
#[wasm_bindgen]
pub fn web_init(canvas: web_sys::HtmlCanvasElement) -> bool {
    crate::guard::install();
    let opts = js_sys::Object::new();
    for (k, v) in [("antialias", false), ("alpha", false), ("depth", true), ("stencil", false), ("premultipliedAlpha", false), ("preserveDrawingBuffer", false)] {
        let _ = js_sys::Reflect::set(&opts, &JsValue::from_str(k), &JsValue::from_bool(v));
    }
    let _ = js_sys::Reflect::set(&opts, &JsValue::from_str("powerPreference"), &JsValue::from_str("high-performance"));
    let Ok(Some(ctx)) = canvas.get_context_with_context_options("webgl2", &opts) else { return false };
    let Ok(ctx) = ctx.dyn_into::<web_sys::WebGl2RenderingContext>() else { return false };
    // half-float render targets for the dust layer (the renderer falls back to 8-bit without them)
    let _ = ctx.get_extension("EXT_color_buffer_float"); let _ = ctx.get_extension("EXT_color_buffer_half_float");
    crate::guard::run("web_init", false, || { crate::gfx::gl_init_ctx(glow::Context::from_webgl2_context(ctx)); true })
}
/// "FWS1" scene, "FRT1" runtime handoff from the page
#[wasm_bindgen]
pub fn web_message(bytes: &[u8]) { crate::gfx::push_message(bytes.to_vec()); }
/// the save's key; returns the stage to open with
#[wasm_bindgen]
pub fn web_start(key: &str) -> String { crate::guard::run("web_start", "forest".to_string(), || crate::gfx::rt_init(key)) }
#[wasm_bindgen]
pub fn web_resize(w: i32, h: i32) { crate::gfx::gl_resize(w, h); }
/// 0 down, 1 move, 2 up / cancel, in canvas pixels
#[wasm_bindgen]
pub fn web_touch(action: i32, id: i32, x: f32, y: f32) { crate::gfx::rt_touch(action, id, x, y); }
#[wasm_bindgen]
pub fn web_back() -> bool { crate::gfx::rt_back() }
#[wasm_bindgen]
pub fn web_insets(t: f32, r: f32, b: f32, l: f32) { crate::gfx::rt_insets(t, r, b, l); }
/// one frame; returns the app events (haptics "h kind strength", "map name", ...)
#[wasm_bindgen]
pub fn web_frame(dt: f64) -> String { crate::gfx::rt_frame_guarded(dt) }
#[wasm_bindgen]
pub fn web_status() -> String { crate::gfx::status() }
// the page's stage builder asks for these, as it asks the app's FogwoodNative
#[wasm_bindgen]
pub fn web_prepare(map: i32, path: &str, roadh: &str, gxh: i32, gzh: i32) -> String { crate::guard::run("prepare", "err panic".into(), || crate::bridge::prepare(map, path, roadh, gxh, gzh)) }
#[wasm_bindgen]
pub fn web_obstacles(csv: &str) -> i32 { crate::guard::run("obstacles", 0, || crate::bridge::set_obstacles(csv)) }
#[wasm_bindgen]
pub fn web_step(csv: &str) -> String { crate::guard::run("step", String::new(), || crate::bridge::step(csv)) }
#[wasm_bindgen]
pub fn web_grid_bytes() -> Vec<u8> { crate::guard::run("grids", Vec::new(), crate::bridge::grid_bytes) }
