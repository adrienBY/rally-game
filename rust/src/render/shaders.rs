//! GLSL ES 3.00 programs equivalent to the three.js materials the game uses (r160, physically-correct lights,
//! no colour management, no tone mapping): MeshBasic, MeshLambert, MeshPhong, Points, plus the game's own
//! sky, dust and post-processing shaders.

pub const ATTRS: [(&str, u32); 9] = [("position", 0), ("normal", 1), ("uv", 2), ("color", 3), ("instanceMatrix", 4), ("instanceColor", 8), ("aA", 9), ("aS", 10), ("vtx", 11)];

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct MeshKey { pub lit: u8 /*0 basic 1 lambert 2 phong*/, pub map: bool, pub vcolor: bool, pub inst: bool, pub icolor: bool,
    pub sway: Option<(u32 /*amp*1000*/, bool, u32 /*fade near*10*/, u32 /*fade far*10*/)>, pub alpha_test: bool, pub double_side: bool, pub fog: bool, pub transparent: bool }

pub fn mesh_vs(k: &MeshKey) -> String {
    let mut d = String::from("#version 300 es\nprecision highp float;\n");
    if k.vcolor { d += "#define USE_VCOLOR\n"; } if k.inst { d += "#define USE_INST\n"; } if k.icolor { d += "#define USE_ICOLOR\n"; }
    if let Some((a, uvk, fne, ffa)) = k.sway { d += &format!("#define SWAY {:.4}\n", a as f32 / 1000.0); if uvk { d += "#define SWAY_UV\n"; }
        if ffa > 0 { d += &format!("#define FADE_NEAR {:.1}\n#define FADE_FAR {:.1}\n", fne as f32 / 10.0, ffa as f32 / 10.0); } }
    d + r#"
layout(location=0) in vec3 position; layout(location=1) in vec3 normal; layout(location=2) in vec2 uv; layout(location=3) in vec4 color;
layout(location=4) in mat4 instanceMatrix; layout(location=8) in vec3 instanceColor;
uniform mat4 uModel; uniform mat4 uView; uniform mat4 uProj; uniform mat3 uNormalM; uniform mat3 uMapM; uniform float uTime; uniform float uWind; uniform vec3 uCamPos;
out vec3 vN; out vec2 vUv; out vec3 vColor; out float vFogDepth; out vec3 vWorld;
void main() {
  vec3 transformed = position;
  vColor = vec3(1.0);
#ifdef USE_VCOLOR
  vColor = color.rgb;
#endif
#ifdef USE_ICOLOR
  vColor *= instanceColor;
#endif
  mat4 im = mat4(1.0);
#ifdef USE_INST
  im = instanceMatrix;
#endif
#ifdef SWAY
  vec4 wpI = im * vec4(0.0, 0.0, 0.0, 1.0); float ph = wpI.x * 0.21 + wpI.z * 0.17;
  float sw = (sin(uTime * 1.4 + ph) * 0.6 + sin(uTime * 3.1 + ph * 2.3) * 0.25) * (0.4 + uWind) * SWAY;
#ifdef SWAY_UV
  float k = uv.y;
#else
  float k = 1.0;
#endif
  transformed.x += sw * k; transformed.z += sw * 0.5 * k;
#ifdef FADE_FAR
  transformed *= smoothstep(FADE_FAR, FADE_NEAR, distance((uModel * wpI).xyz, uCamPos));
#endif
#endif
  vec4 world = uModel * (im * vec4(transformed, 1.0));
  vec4 mv = uView * world;
  gl_Position = uProj * mv;
  vFogDepth = -mv.z; vWorld = world.xyz;
  vec3 n = normal;
#ifdef USE_INST
  mat3 m3 = mat3(im); n /= vec3(dot(m3[0], m3[0]), dot(m3[1], m3[1]), dot(m3[2], m3[2])); n = m3 * n;
#endif
  vN = uNormalM * n;
  vUv = (uMapM * vec3(uv, 1.0)).xy;
}
"#
}

pub fn mesh_fs(k: &MeshKey) -> String {
    let mut d = String::from("#version 300 es\nprecision highp float;\n");
    d += &format!("#define LIT {}\n", k.lit);
    if k.map { d += "#define USE_MAP\n"; } if k.vcolor || k.icolor { d += "#define USE_COLOR\n"; } if k.alpha_test { d += "#define ALPHATEST\n"; }
    if k.double_side { d += "#define DOUBLE_SIDE\n"; } if k.fog { d += "#define USE_FOG\n"; } if k.transparent { d += "#define TRANSPARENT\n"; }
    if k.sway.is_some() && k.lit == 0 { d += "#define FOLIAGE\n"; }
    d + r#"
#define RECIPROCAL_PI 0.3183098861837907
uniform vec3 uColor; uniform float uOpacity; uniform sampler2D uMap; uniform float uAlphaTest;
uniform vec3 uHemiSky; uniform vec3 uHemiGround; uniform vec3 uHemiDir; uniform vec3 uDirColor; uniform vec3 uDirDir;
uniform vec3 uFogColor; uniform float uFogDensity; uniform vec3 uCamPos; uniform vec3 uSpec; uniform float uShin; uniform vec3 uEmissive; uniform float uDim;
uniform vec3 uSkySun; uniform vec3 uSkyGlow; uniform float uSkyHalo;
// headlights (Ember Peak): up to 4 cars, each a lamp at its nose (uLampP) aimed along uLampD (its length is the lamp's strength)
uniform int uLampN; uniform vec3 uLampP[4]; uniform vec3 uLampD[4]; uniform vec3 uLampC;
float bayer4(vec2 p) { ivec2 q = ivec2(mod(p, 4.0)); int i = q.x + q.y * 4;
  const float m[16] = float[16](0., 8., 2., 10., 12., 4., 14., 6., 3., 11., 1., 9., 15., 7., 13., 5.); return (m[i] + 0.5) / 16.0; }
// how much of lamp i reaches world point p (a flattened cone, wide and low, that fades out by ~40 m); l = the unit ray from the lamp
float lampAt(int i, vec3 p, out vec3 l) {
  vec3 L = p - uLampP[i]; float d = length(L); l = L / max(d, 1e-3); float k = length(uLampD[i]); vec3 D = uLampD[i] / k;
  float fw = dot(l, D); if (fw < 0.05) return 0.0;
  vec3 q = l - D * fw; float qy = q.y; float qh = length(q.xz);
  float e = length(vec2(qh / 0.62, qy / 0.3)) / fw;
  float cone = 1.0 - smoothstep(0.5, 1.0, e) + 0.18 * (1.0 - smoothstep(1.0, 1.8, e));
  return k * cone / (1.0 + d * d * 0.0045) * (1.0 - smoothstep(26.0, 42.0, d)) * smoothstep(0.4, 1.6, d);
}
in vec3 vN; in vec2 vUv; in vec3 vColor; in float vFogDepth; in vec3 vWorld;
out vec4 fragColor;
void main() {
  vec4 dc = vec4(uColor, uOpacity);
#ifdef USE_MAP
  dc *= texture(uMap, vUv);
#endif
#ifdef USE_COLOR
  dc.rgb *= vColor;
#endif
#ifdef ALPHATEST
  if (dc.a < uAlphaTest) discard;
#endif
  vec3 outC = dc.rgb;
#if LIT > 0
  vec3 N = normalize(vN);
#ifdef DOUBLE_SIDE
  N *= gl_FrontFacing ? 1.0 : -1.0;
#endif
  vec3 irr = clamp(dot(N, uDirDir), 0.0, 1.0) * uDirColor;
  vec3 hemi = mix(uHemiGround, uHemiSky, 0.5 * dot(N, uHemiDir) + 0.5);
  outC = (irr + hemi) * dc.rgb * RECIPROCAL_PI * uDim + uEmissive;
  if (uLampN > 0) {   // headlight pools, banded and Bayer-dithered so they read as pixel art
    float acc = 0.0; vec3 l;
    for (int i = 0; i < 4; i++) { if (i >= uLampN) break; float a = lampAt(i, vWorld, l); if (a > 0.0) acc += a * (0.3 + 0.7 * clamp(dot(N, -l), 0.0, 1.0)); }
    float lv = floor(min(acc, 1.15) * 4.0 + bayer4(gl_FragCoord.xy)) / 4.0;
    outC += dc.rgb * uLampC * lv;
  }
#if LIT == 2
  vec3 V = normalize(uCamPos - vWorld); vec3 H = normalize(uDirDir + V);
  float dNH = clamp(dot(N, H), 0.0, 1.0), dVH = clamp(dot(V, H), 0.0, 1.0);
  float fres = exp2((-5.55473 * dVH - 6.98316) * dVH); vec3 F = uSpec + (vec3(1.0) - uSpec) * fres;
  float D = RECIPROCAL_PI * (uShin * 0.5 + 1.0) * pow(dNH, uShin);
  outC += irr * F * (0.25 * D) * uDim;
#endif
#endif
#ifdef TRANSPARENT
  float a = dc.a;
#else
  float a = 1.0;
#endif
#if defined(USE_FOG) || defined(FOLIAGE)
  vec3 vd = normalize(vWorld - uCamPos); float sunA = max(dot(vd, uSkySun), 0.0);
#endif
#ifdef FOLIAGE
  outC += dc.rgb * uDirColor * 0.2 * pow(sunA, 3.0);   // leaves and grass glow when the sun is behind them
#endif
#ifdef USE_FOG
  // the haze takes the sky's own colour in that direction: towards the sun it glows like the sunset, so far hills melt into it
  vec3 fc = uFogColor + uSkyGlow * (pow(sunA, 6.0) + uSkyHalo * pow(sunA, 2.0) * (1.0 - smoothstep(0.0, 0.5, vd.y)));
  float f = 1.0 - exp(-uFogDensity * uFogDensity * vFogDepth * vFogDepth); outC = mix(outC, fc, f);
  if (uLampN > 0 && uFogDensity > 0.0) {   // the beams lit up in the haze: a short dithered march along the view ray
    vec3 rd = vWorld - uCamPos; float tl = length(rd); rd /= max(tl, 1e-3); float tm = min(tl, 44.0);
    float b = bayer4(gl_FragCoord.xy), sc = 0.0; vec3 l;
    for (int s = 0; s < 6; s++) { vec3 p = uCamPos + rd * ((float(s) + b) / 6.0 * tm); for (int i = 0; i < 4; i++) { if (i >= uLampN) break; sc += lampAt(i, p, l); } }
    sc *= tm / 6.0 * 0.014; float sv = floor(min(sc, 0.8) * 5.0 + bayer4(gl_FragCoord.xy + vec2(2.0, 1.0))) / 5.0;
    outC += uLampC * sv * 0.55;
  }
#endif
  fragColor = vec4(outC, a);
}
"#
}

pub const SKY_VS: &str = r#"#version 300 es
precision highp float;
layout(location=0) in vec3 position; uniform mat4 uModel; uniform mat4 uView; uniform mat4 uProj; out vec3 vD;
void main() { vD = normalize(position); gl_Position = uProj * uView * uModel * vec4(position, 1.0); }
"#;
pub const SKY_FS: &str = r#"#version 300 es
precision highp float;
uniform vec3 uSun; uniform vec3 uFog; uniform vec3 uTop; uniform vec3 uGlow; uniform float uHalo; in vec3 vD; out vec4 fragColor;
void main() { vec3 d = normalize(vD); float s = max(dot(d, uSun), 0.0);
  vec3 c = mix(uFog, uTop, smoothstep(0.0, 0.6, vD.y)) + uGlow * pow(s, 6.0) + uGlow * uHalo * pow(s, 2.0) * (1.0 - smoothstep(0.0, 0.5, vD.y));
  fragColor = vec4(c, 0.0); }
"#;

pub const POINTS_VS: &str = r#"#version 300 es
precision highp float;
layout(location=0) in vec3 position; uniform mat4 uModel; uniform mat4 uView; uniform mat4 uProj; uniform float uSize; out float vFogDepth;
void main() { vec4 mv = uView * uModel * vec4(position, 1.0); gl_Position = uProj * mv; gl_PointSize = uSize; vFogDepth = -mv.z; }
"#;
pub const POINTS_FS: &str = r#"#version 300 es
precision highp float;
uniform vec3 uColor; uniform float uOpacity; uniform vec3 uFogColor; uniform float uFogDensity; in float vFogDepth; out vec4 fragColor;
void main() { vec3 c = uColor; float f = 1.0 - exp(-uFogDensity * uFogDensity * vFogDepth * vFogDepth); fragColor = vec4(mix(c, uFogColor, f), uOpacity); }
"#;

/// Dust, gravel and smoke, first pass: every puff adds its density (alpha) and lit, fogged colour (rgb, weighted by density)
/// into the dust layer, additively, depth-tested against the scene but writing no depth. Order does not matter, so the CPU
/// never sorts. color = (brightness, earth, seed) per particle (all zero: plain white, no noise).
pub const DUST_VS: &str = r#"#version 300 es
precision highp float;
layout(location=0) in vec3 position; layout(location=3) in vec3 color; layout(location=9) in float aA; layout(location=10) in float aS;
uniform mat4 uModel; uniform mat4 uView; uniform mat4 uProj; uniform float uScale; uniform float uDens; uniform vec3 uDirDir;
uniform vec3 uHemiSky; uniform vec3 uHemiGround; uniform vec3 uDirColor; uniform vec3 uCol; uniform vec3 uFog;
out float vA; out vec3 vRgb; out float vSeed;
void main() { vec4 mv = uView * uModel * vec4(position, 1.0); gl_Position = uProj * mv; gl_PointSize = min(aS * uScale / max(0.5, -mv.z), 160.0); vA = aA;
  float d = -mv.z; float fogk = 1.0 - exp(-uDens * uDens * d * d);
  vA *= smoothstep(1.5, 6.5, d);   // dust right at the lens would just wash the screen out
  vec3 c = dot(color, color) < 1e-6 ? vec3(1.0, 0.0, 0.0) : color; vSeed = c.z;
  // lit like a thin scattering medium: sky and ground light from all round, the sun on top, nothing sphere-like
  vec3 base = mix(uCol, vec3(0.38, 0.31, 0.22), c.y) * c.x;
  // airborne dust is paler and greyer than the ground it came from (fine grains scatter light more evenly): wash half the
  // colour out and lift it a little, so a red-earth stage throws a pale tan cloud, not an orange one
  base = mix(base, vec3(dot(base, vec3(0.3, 0.55, 0.15))) * 1.08, 0.5);
  vec3 lit = base * (mix(uHemiGround, uHemiSky, 0.62) * 0.46 + uDirColor * (0.32 + 0.12 * max(uDirDir.y, 0.0)));
  vRgb = mix(lit, uFog, fogk); }
"#;
pub const DUST_FS: &str = r#"#version 300 es
precision highp float;
uniform float uDensScale; in float vA; in vec3 vRgb; in float vSeed; out vec4 fragColor;
float hash(vec2 p) { return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453); }
float vn(vec2 p) { vec2 i = floor(p), f = fract(p); f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), f.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), f.x), f.y); }
void main() {
  vec2 p = (gl_PointCoord - 0.5) * 2.0; float r2 = dot(p, p); if (r2 > 1.0) discard;
  // a ragged, lumpy puff rather than a disc: two octaves of smooth noise (different for every puff) eat into its rim and
  // vary its density, so sparse puffs never read as circles and dense ones merge into a billowing cloud
  float n = vSeed > 0.0 ? vn(gl_PointCoord * 3.0 + vSeed * 97.0) * 0.65 + vn(gl_PointCoord * 6.5 + vSeed * 53.0) * 0.35 : 0.5;
  float e = clamp(1.0 - r2 - (0.5 - n) * 1.1, 0.0, 1.0);
  float d = e * e * (0.5 + 0.9 * n) * vA * uDensScale;
  fragColor = vec4(vRgb * d, d); }
"#;
/// Specks: gravel, snow grains and clumps thrown by the tyres, too small and solid to be part of the dust cloud. Each is a crisp
/// square of whole game pixels (1-6, by size and distance), lit like the scene, fogged, opaque; partial cover (a speck smaller
/// than a pixel, or fading out) is a 4x4 ordered dither, so they stay sharp. aP = position + size (m), aC = colour + alpha.
pub const SPECK_VS: &str = r#"#version 300 es
precision highp float;
layout(location=0) in vec4 aP; layout(location=3) in vec4 aC;
uniform mat4 uView; uniform mat4 uProj; uniform float uScale; uniform vec3 uHemiSky; uniform vec3 uHemiGround; uniform vec3 uDirColor; uniform vec3 uDirDir;
uniform vec3 uFogColor; uniform float uFogDensity;
out vec3 vRgb; out float vA;
void main() { vec4 mv = uView * vec4(aP.xyz, 1.0); gl_Position = uProj * mv; float d = max(0.3, -mv.z);
  float px = aP.w * uScale / d; gl_PointSize = clamp(floor(px + 0.5), 1.0, 6.0); vA = aC.a * clamp(px / 0.6, 0.0, 1.0) * smoothstep(0.8, 2.5, d);
  vec3 lit = aC.rgb * (mix(uHemiGround, uHemiSky, 0.7) * 0.55 + uDirColor * (0.3 + 0.25 * max(uDirDir.y, 0.0)));
  float f = 1.0 - exp(-uFogDensity * uFogDensity * d * d); vRgb = mix(lit, uFogColor, f); }
"#;
pub const SPECK_FS: &str = r#"#version 300 es
precision highp float;
in vec3 vRgb; in float vA; out vec4 fragColor;
float bayer(vec2 c) { vec2 q = mod(floor(c), 4.0); int i = int(q.x) + int(q.y) * 4;
  int m[16] = int[16](0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5); return (float(m[i]) + 0.5) / 16.0; }
void main() { if (vA < bayer(gl_FragCoord.xy)) discard; fragColor = vec4(vRgb, 1.0); }
"#;
/// separable 9-tap gaussian in 5 bilinear fetches, along uSrc (1,0 or 0,1), uTexel = 1 / source size
pub const BLUR_FS: &str = r#"#version 300 es
precision highp float;
uniform sampler2D tScene; uniform vec2 uTexel; uniform vec2 uSrc; in vec2 vUv; out vec4 fragColor;
void main() { vec2 o1 = uSrc * uTexel * 1.3846154, o2 = uSrc * uTexel * 3.2307692;
  fragColor = texture(tScene, vUv) * 0.2270270 + (texture(tScene, vUv + o1) + texture(tScene, vUv - o1)) * 0.3162162 + (texture(tScene, vUv + o2) + texture(tScene, vUv - o2)) * 0.0702703; }
"#;
/// Dust, last pass: the blurred layer becomes one continuous cloud over the scene, drawn in chunky pixels to match the game.
/// The cloud is sampled once per block of B game pixels (B grows with the resolution), so its edges and curls step in big
/// square pixels; opacity comes from the optical depth (1 - e^-density), capped so the cloud stays see-through, and is put
/// on a few levels with a 4x4 ordered dither (the old-console look); shading (self-shadow, sunlit edges, back glow) is
/// stepped too. Only the cut-off by things in front is per pixel (from the sharp, depth-tested layer), so cars and trees
/// keep crisp outlines through the dust.
pub const DCOMP_FS: &str = r#"#version 300 es
precision highp float;
uniform sampler2D tDust; uniform sampler2D tSharp; uniform vec2 uTexel; uniform vec2 uSun; uniform float uSunVis; uniform vec3 uDirColor; uniform float uDensScale; uniform vec2 uSrc;
in vec2 vUv; out vec4 fragColor;
float bayer(vec2 c) { vec2 q = mod(c, 4.0); int i = int(q.x) + int(q.y) * 4;
  int m[16] = int[16](0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5); return (float(m[i]) + 0.5) / 16.0; }
void main() {
  float B = max(2.0, floor(uSrc.y / 100.0));
  vec2 cell = floor(vUv * uSrc / B); vec2 uv = (cell + 0.5) * B / uSrc; vec2 tb = uTexel * B;
  vec4 b = texture(tDust, uv) / uDensScale, sc = texture(tSharp, uv) / uDensScale, sh = texture(tSharp, vUv) / uDensScale;
  float D = mix(b.a, sc.a, 0.15);
  // something in front of the dust here (per pixel): the sharp layer has nothing, so the cloud stops at its outline
  D = min(D, sh.a * 4.0 + 0.002); if (D < 0.006) discard;
  vec3 col = (b.a > 1e-4 ? b.rgb / b.a : sc.rgb / max(sc.a, 1e-4));
  float gx = texture(tDust, uv + vec2(tb.x, 0.0)).a - texture(tDust, uv - vec2(tb.x, 0.0)).a;
  float gy = texture(tDust, uv + vec2(0.0, tb.y)).a - texture(tDust, uv - vec2(0.0, tb.y)).a;
  vec2 grad = vec2(gx, gy) / uDensScale; vec2 sd = normalize(uSun - uv + vec2(0.0, 0.6));
  float own = clamp(sc.a / max(b.a, 1e-3), 0.0, 1.0);
  float rim = clamp(dot(-grad, sd) * 2.0, -0.5, 0.8) * own * own;
  float thick = (1.0 - exp(-D * 2.8)) * 0.94;
  // stepped shading: three tones from the shadowed core to the lit edge
  float shade = mix(1.08, 0.72, smoothstep(0.15, 1.3, D)) * (0.92 + 0.22 * rim); shade = floor(shade * 6.0 + 0.5) / 6.0;
  col *= shade;
  vec2 asp = vec2(uSrc.x / uSrc.y, 1.0); float near = uSunVis * smoothstep(0.75, 0.0, length((uv - uSun) * asp));
  col += uDirColor * floor(0.22 * near * (1.0 - thick) * 12.0) / 12.0;
  // few opacity levels, dithered per block
  // the faintest haze is dropped instead of dithered into a screen-door pattern; above it the dither only blends neighbouring levels
  if (thick < 0.1) discard;
  float a = floor(thick * 6.0 + 0.25 + 0.5 * bayer(cell)) / 6.0; if (a <= 0.0) discard;
  fragColor = vec4(col, a); }
"#;

pub const POST_VS: &str = r#"#version 300 es
precision highp float;
layout(location=11) in vec2 vtx; out vec2 vUv;
void main() { vUv = vtx * 0.5 + 0.5; gl_Position = vec4(vtx, 0.0, 1.0); }
"#;
pub const POST_FS: &str = r#"#version 300 es
precision highp float;
uniform sampler2D tScene; uniform vec2 uSun; uniform float uSunVis; uniform float uTime; uniform float uFlash; uniform float uSpeed; uniform float uExpo; uniform vec3 uSplitS; uniform vec3 uSplitH; uniform vec2 uTexel;
uniform sampler2D tHud; uniform float uHudOn; uniform sampler2D tRay; uniform float uRayOn;
in vec2 vUv; out vec4 fragColor;
const float BAYER[16] = float[16](0., 8., 2., 10., 12., 4., 14., 6., 3., 11., 1., 9., 15., 7., 13., 5.);
float bayer(vec2 p) { vec2 q = mod(floor(p), 4.0); int i = int(q.x + q.y * 4.0); return BAYER[i] / 16.0 - 0.47; }
void main() {
  vec3 c = texture(tScene, vUv).rgb;
  vec2 fromC = vUv - vec2(0.5, 0.52); float edge = smoothstep(0.12, 0.62, length(fromC)) * uSpeed;
  if (edge > 0.002) { vec3 bl = c; for (int i = 1; i <= 5; i++) bl += texture(tScene, vUv - fromC * 0.028 * uSpeed * float(i)).rgb; c = mix(c, bl / 6.0, edge); }
  float acc = uRayOn > 0.5 ? texture(tRay, vUv).r : 0.0;   // god rays: marched at quarter resolution over a pre-blurred bright mask, upsampled bilinearly
  c += vec3(1.0, 0.93, 0.74) * acc * 0.85 * uSunVis * (0.9 + 0.1 * sin(uTime * 0.7 + vUv.x * 3.0));
  c *= uExpo; c += vec3(1.0, 0.96, 0.9) * smoothstep(0.75, 1.7, dot(c, vec3(0.3, 0.59, 0.11))) * 0.6 * clamp(uExpo - 1.0, 0.0, 1.0);
  { float l = dot(c, vec3(0.3, 0.59, 0.11)); c += uSplitS * (1.0 - l) * (1.0 - l) + uSplitH * l * l; }
  float g = dot(c, vec3(0.3, 0.59, 0.11)); c = mix(vec3(g), c, 0.86);
  c = (c - 0.5) * 1.08 + 0.5; c *= vec3(1.0, 1.0, 0.97);
  vec2 vv = vUv - 0.5; c *= 1.0 - dot(vv, vv) * (0.5 + uSpeed * 0.7);
  c += vec3(0.25, 0.2, 0.15) * uFlash;
  if (uHudOn > 0.5) { vec4 h = texture(tHud, vec2(vUv.x, 1.0 - vUv.y)); c = mix(c, h.rgb, h.a); }   // the HUD, drawn at the same resolution, dithered with the scene
  c += bayer(gl_FragCoord.xy) / 26.0;
  c = floor(clamp(c, 0.0, 1.0) * 26.0 + 0.5) / 26.0;
  fragColor = vec4(c, 1.0);
}
"#;

/// God-ray mask at quarter resolution: each texel averages a 4x4 block of the scene (4 bilinear taps), keeping only bright sky.
pub const MASK_FS: &str = r#"#version 300 es
precision highp float;
uniform sampler2D tScene; uniform vec2 uTexel;
in vec2 vUv; out vec4 fragColor;
float m(vec2 p) { vec4 s = texture(tScene, p); return smoothstep(0.66, 0.92, dot(s.rgb, vec3(0.3, 0.59, 0.11))) * (1.0 - s.a); }
void main() {
  vec2 o = uTexel;
  fragColor = vec4((m(vUv + vec2(-o.x, -o.y)) + m(vUv + vec2(o.x, -o.y)) + m(vUv + vec2(-o.x, o.y)) + m(vUv + vec2(o.x, o.y))) * 0.25, 0.0, 0.0, 1.0);
}
"#;
/// Radial march toward the sun over the smooth mask, at quarter resolution: fixed sample positions (no per-pixel jitter), so it holds still frame to frame.
pub const RAY_FS: &str = r#"#version 300 es
precision highp float;
uniform sampler2D tMask; uniform vec2 uSun;
in vec2 vUv; out vec4 fragColor;
void main() {
  vec2 d = uSun - vUv; float L = length(d); if (L > 1.3) d *= 1.3 / L;
  vec2 st = d / 16.0; vec2 p = vUv; float w = 1.0, acc = 0.0;
  for (int i = 0; i < 16; i++) { p += st; float near = 1.0 - smoothstep(0.04, 0.42, length((p - uSun) * vec2(1.6, 1.0)));
    acc += texture(tMask, clamp(p, 0.0, 1.0)).r * w * (0.25 + 0.75 * near); w *= 0.94; }
  fragColor = vec4(acc / 9.6, 0.0, 0.0, 1.0);
}
"#;
/// Final upscale to the screen ("sharp bilinear"): each low-res pixel stays a flat block, and only the one screen pixel on its border blends with
/// the neighbour. With a non-integer scale (756 -> 2800 is x3.7), plain nearest makes pixels 3 or 4 wide by turns, so edges jump as things move.
pub const UPSCALE_FS: &str = r#"#version 300 es
precision highp float;
uniform sampler2D tScene; uniform vec2 uSrc; uniform vec2 uZoom;
in vec2 vUv; out vec4 fragColor;
void main() {
  vec2 px = vUv * uSrc, i = floor(px), f = px - i - 0.5, reg = 0.5 - 0.5 / uZoom;
  vec2 g = (f - clamp(f, -reg, reg)) * uZoom + 0.5;
  fragColor = vec4(texture(tScene, (i + g) / uSrc).rgb, 1.0);
}
"#;
