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
    d + r#"
#define RECIPROCAL_PI 0.3183098861837907
uniform vec3 uColor; uniform float uOpacity; uniform sampler2D uMap; uniform float uAlphaTest;
uniform vec3 uHemiSky; uniform vec3 uHemiGround; uniform vec3 uHemiDir; uniform vec3 uDirColor; uniform vec3 uDirDir;
uniform vec3 uFogColor; uniform float uFogDensity; uniform vec3 uCamPos; uniform vec3 uSpec; uniform float uShin; uniform vec3 uEmissive; uniform float uDim;
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
#ifdef USE_FOG
  float f = 1.0 - exp(-uFogDensity * uFogDensity * vFogDepth * vFogDepth); outC = mix(outC, uFogColor, f);
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

pub const DUST_VS: &str = r#"#version 300 es
precision highp float;
layout(location=0) in vec3 position; layout(location=9) in float aA; layout(location=10) in float aS;
uniform mat4 uModel; uniform mat4 uView; uniform mat4 uProj; uniform float uScale; uniform float uDens; out float vA; out float vF;
void main() { vec4 mv = uView * uModel * vec4(position, 1.0); gl_Position = uProj * mv; gl_PointSize = aS * uScale / max(0.5, -mv.z); vA = aA;
  float d = -mv.z; vF = 1.0 - exp(-uDens * uDens * d * d); }
"#;
pub const DUST_FS: &str = r#"#version 300 es
precision highp float;
uniform vec3 uFog; uniform vec3 uCol; in float vA; in float vF; out vec4 fragColor;
void main() { vec2 p = gl_PointCoord - 0.5; if (dot(p, p) > 0.25) discard; fragColor = vec4(mix(uCol, uFog, vF), vA); }
"#;

pub const POST_VS: &str = r#"#version 300 es
precision highp float;
layout(location=11) in vec2 vtx; out vec2 vUv;
void main() { vUv = vtx * 0.5 + 0.5; gl_Position = vec4(vtx, 0.0, 1.0); }
"#;
pub const POST_FS: &str = r#"#version 300 es
precision highp float;
uniform sampler2D tScene; uniform vec2 uSun; uniform float uSunVis; uniform float uTime; uniform float uFlash; uniform float uSpeed; uniform float uExpo;
in vec2 vUv; out vec4 fragColor;
const float BAYER[16] = float[16](0., 8., 2., 10., 12., 4., 14., 6., 3., 11., 1., 9., 15., 7., 13., 5.);
float bayer(vec2 p) { vec2 q = mod(floor(p), 4.0); int i = int(q.x + q.y * 4.0); return BAYER[i] / 16.0 - 0.47; }
void main() {
  vec3 c = texture(tScene, vUv).rgb;
  vec2 fromC = vUv - vec2(0.5, 0.52); float edge = smoothstep(0.12, 0.62, length(fromC)) * uSpeed;
  if (edge > 0.002) { vec3 bl = c; for (int i = 1; i <= 5; i++) bl += texture(tScene, vUv - fromC * 0.028 * uSpeed * float(i)).rgb; c = mix(c, bl / 6.0, edge); }
  float acc = 0.0;
  if (uSunVis > 0.01) { vec2 d = uSun - vUv; float L = length(d); if (L > 1.3) d *= 1.3 / L;
    vec2 st = d / 20.0; vec2 p = vUv; float w = 1.0;
    for (int i = 0; i < 20; i++) { p += st; vec4 s = texture(tScene, clamp(p, 0.001, 0.999));
      float l = dot(s.rgb, vec3(0.3, 0.59, 0.11)); acc += smoothstep(0.66, 0.92, l) * (1.0 - s.a) * w; w *= 0.952; }
    acc /= 13.0; }
  c += vec3(1.0, 0.93, 0.74) * acc * 0.85 * uSunVis * (0.9 + 0.1 * sin(uTime * 0.7 + vUv.x * 3.0));
  c *= uExpo; c += vec3(1.0, 0.96, 0.9) * smoothstep(0.75, 1.7, dot(c, vec3(0.3, 0.59, 0.11))) * 0.6 * clamp(uExpo - 1.0, 0.0, 1.0);
  float g = dot(c, vec3(0.3, 0.59, 0.11)); c = mix(vec3(g), c, 0.86);
  c = (c - 0.5) * 1.08 + 0.5; c *= vec3(1.0, 1.0, 0.97);
  vec2 vv = vUv - 0.5; c *= 1.0 - dot(vv, vv) * (0.5 + uSpeed * 0.7);
  c += vec3(0.25, 0.2, 0.15) * uFlash;
  c += bayer(gl_FragCoord.xy) / 26.0;
  c = floor(clamp(c, 0.0, 1.0) * 26.0 + 0.5) / 26.0;
  fragColor = vec4(c, 1.0);
}
"#;
