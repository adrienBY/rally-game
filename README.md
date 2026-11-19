# Fogwood Rally — Android

Pixel-art rally game (Forest, Snow, Canyon, Lakes, Touge, Riviera and Prism Skyway stages; time trial or races against five AI rivals) packaged as a native Android app with a custom Rust engine core.

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

Cars: five bodies built by the page from `CAR_SPECS` (one lofted shell each, shared wheels) and the garage in
`rust/src/game/garage.rs` / `cars.rs` (names, stats, handling; same order). `node web/tools/car_preview.mjs out.png [k]`
renders the bodies alone in headless Chrome.

Crash damage (`rust/src/game/crash.rs`): a visual-only node-and-beam soft body built from each car's shell (plastic beams, chassis
rails) that the body meshes follow; the real obstacle (tree, rock, other car, wall) is pressed in, so it dents, folds and crumples where
and how it was hit. It never touches the physics. `game_host ... "hit:cx:cz:r:dx:dz:speed"` (a post) or `"hitcar:..."` dents it on demand.

Tyre smoke check: `cargo run --release --bin smoke_stats <stage.rt>...` drives a lap on each stage (`SMOKE_HB=1` adds handbrake
slides) and reports how often the tyres would smoke on the tarmac and how many puffs per second (`.rt` = the page's `__rtPacket()`).

Two headless phones in an online race (no phones, no internet): `cargo run --release --bin stage_dump lakes /tmp/stage`
builds a bare stage from a test fixture, then two `game_host` runs talk through a local relay (`rust/relay`, with
`FW_RELAY=udp:127.0.0.1:7777 FW_BROKERS= FW_LOOPBACK=1 FW_NOSTUN=1 FW_REALTIME=1`); the host script taps
`online:on_host`, `online:on_bots` (AI cars) and `online:on_start`, the guest `joincode`; `cars` prints every car.
`FW_CUT=from,to` loses the direct packets in that window (a dying path), to check nobody drops out.

Prism Skyway check: `cargo run --release --bin prism_check <rt.bin>` (a runtime packet dumped from the page) drives cars off the deck
edge at many speeds, steering amounts and wave phases and measures any flick as they leave (upward and sideways speed gained over
the void), lets cars drift over the edge and steer back (they must never sink into the deck while still over it), then measures
how much steering, throttle and brake move a car in the air.

Online netcode check (no phones needed): `cargo run --release --bin net_sim` drives a car round each stage, sends it
through a simulated link and reports how far off and how smooth the other player's car looks, plus data per hour.
`cargo run --release --bin net_contact` races two phones door to door through the link and reports how contacts come
across: how many real touches each victim's phone felt and how late, pushes with no real touch, and how much the other
car is thrown about on screen around a contact (old netcode vs current).

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
The keystore is committed in the clear (`android/signing/fogwood-public.jks`, alias `fogwood`, password
`fogwood-public`), so no secret is needed. It is public on purpose: fine for sideloaded builds, not for a store
release. To sign with a private key instead, set `FOGWOOD_KEYSTORE` (path) and `FOGWOOD_KEYSTORE_PASSWORD`.
