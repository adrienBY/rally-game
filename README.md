# Fogwood Rally — Android

Pixel-art rally game (Forest, Snow, Canyon, Lakes, Touge and Riviera stages) packaged as a native Android app with a custom Rust engine core.

```
rust/      fogwood_core: stage generation + vehicle physics in Rust (JNI cdylib)
android/   Kotlin app: full-screen landscape WebView renderer, bundled game + three.js, JNI bridge
web/       game source (rally_template.html), audio, build.py
.github/   CI: tests the Rust core, cross-compiles it for arm64/armv7/x86_64, builds and publishes the APK
```

## How the pieces fit
* **Rust core** (`rust/src`)
  * `world.rs` — distance-to-road field, nearest-road index and terrain height on a 1 m grid for every stage
    rows generated on every CPU core.
  * `physics.rs` — four tyres with Pacejka-style curves and a friction circle, spring/damper suspension with bump stops
    and anti-roll bars, AWD drivetrain, torque curve, auto gearbox, ABS/TC, arcade assist layer, obstacle collisions,
    360 Hz sub-stepping.
  * `noise.rs` — value noise that reproduces the JavaScript int32 arithmetic bit for bit.
  * `bridge.rs` / `jni_api.rs` — one call per frame from the game; numbers travel as exact round-trip decimals.
* **App** (`android/`) — loads `assets/index.html` through `WebViewAssetLoader`, exposes `window.FogwoodNative`,
  serves the generated grids as raw bytes at `/native/grids.bin`. The game detects the bridge and hands world
  generation and the physics step to Rust; in a normal browser it runs its JavaScript versions instead.
* **Parity** — `rust/tests/parity.rs` checks the Rust grids are byte-identical to the browser build and that the car
  follows the same trajectory (fixtures dumped from the game).

## Build
Push to `main` (or run the workflow by hand): GitHub Actions builds `FogwoodRally-<n>.apk`, attaches it to a release
named *Fogwood Rally build n* and to the run's artifacts. Install it on the phone (allow installs from your browser).

Locally: `cd rust && cargo test --release`, then `cargo ndk -t arm64-v8a -o ../android/app/src/main/jniLibs build --release --lib`
and `cd android && gradle assembleRelease` (Android SDK + NDK required).

After editing the game: `python3 web/build.py` (rebuilds `web/dist/*.html` and the app's `assets/index.html`).

## Native renderer (Android)
`rust/src/render/` draws the 3D world with OpenGL ES 3 on a `GLSurfaceView` underneath the WebView. At its first frame the
game exports its whole scene (geometries, materials, textures, instances, lights, fog) as one binary packet through
`window.FogwoodGL.postMessage(ArrayBuffer)`, then sends a small packet per frame (camera, objects that moved, buffers that
changed). Rust reproduces three.js's Basic/Lambert/Phong shading, fog, instancing, wind sway, sky, particles and the post
pass (god rays, zoom blur, grade, 4x4 dither) at the game's low resolution and upscales with nearest filtering. The page
itself only draws the HUD. If the WebView can't post binary messages, the game falls back to WebGL.

`cargo run --release --bin render_host <scene> <frame|.stream> out.ppm` renders dumped packets headlessly (Mesa EGL).

## Signing

Every APK is signed with the same permanent key, so a new build installs over the previous one.
The keystore is committed encrypted (`android/signing/fogwood-release.jks.enc`, AES-256, PBKDF2) and CI
unlocks it with the `FOGWOOD_KEYSTORE_PASSWORD` repository secret (Settings → Secrets and variables → Actions).
Without that secret the build still succeeds but falls back to a throwaway debug key.
