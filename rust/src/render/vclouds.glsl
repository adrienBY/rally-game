// volumetric cloud slab. uMap: 256x256 noise (G = R shifted by 37,17); uCol = (bottom, top, extinction); uSun = sun dir; uGlow = sun colour; uTop = sky colour; uFog/uDens = exp2 fog
float vcN(vec3 x) { vec3 p = floor(x); vec3 f = fract(x); f = f * f * (3.0 - 2.0 * f);
  vec2 uv = p.xz + vec2(37.0, 17.0) * p.y + f.xz; vec2 rg = textureLod(uMap, (uv + 0.5) / 256.0, 0.0).yx; return mix(rg.x, rg.y, f.y); }
float vcCov(vec2 xz) { float c = vcN(vec3(xz * 0.0021, 3.5)) * 0.65 + vcN(vec3(xz * 0.0063, 9.5)) * 0.35; return 0.5 + smoothstep(0.28, 0.72, c) * 0.62; }
float vcD(vec3 p, float cov, bool hi) {
  float h = (p.y - uCol.x) / (uCol.y - uCol.x); if (h < 0.0 || h > 1.0) return 0.0;
  vec3 q = (p + vec3(uTime * 1.1, 0.0, uTime * 0.45)) * 0.0135;
  float b = vcN(q) * 0.6 + vcN(q * 2.17 + vec3(0.0, uTime * 0.012, 0.0)) * 0.4;
  float d = clamp((b * cov * 1.25 - h) * 5.0, 0.0, 1.0) * smoothstep(0.0, 0.12, h);
  if (hi && d > 0.0 && d < 0.95) { vec3 r = q * 5.3 + vec3(uTime * 0.03, 0.0, 0.0); float e = vcN(r) * 0.65 + vcN(r * 2.3) * 0.35; d = clamp(d - (1.0 - e) * 0.38 * (1.0 - d), 0.0, 1.0); }
  return d; }
float vcHG(float c, float g) { float g2 = g * g; return (1.0 - g2) / pow(1.0 + g2 - 2.0 * g * c, 1.5); }
vec4 vcMarch(vec3 ro, vec3 pw, vec2 fc) {
  vec3 rd = normalize(pw - ro);
  float t0 = ro.y > uCol.y ? length(pw - ro) : 0.0;
  float tb = rd.y < -1e-4 ? (uCol.x - ro.y) / rd.y : t0 + 700.0;
  float t1 = min(tb, t0 + 700.0); if (t1 <= t0) return vec4(0.0);
  vec3 L = normalize(uSun); float cs = dot(rd, L);
  float ph = min(mix(1.0, vcHG(cs, 0.6), 0.5) + 0.3 * vcHG(cs, -0.3), 4.0);
  float jit = fract(52.9829189 * fract(dot(fc, vec2(0.06711056, 0.00583715))));
  vec3 col = vec3(0.0); float T = 1.0, tHit = -1.0, span = t1 - t0, sig = uCol.z;
  for (int i = 0; i < 26; i++) {
    float u = (float(i) + jit) / 26.0, tn = t0 + span * u * u;
    float dt = span * 2.0 * max(u, 0.02) / 26.0;
    vec3 p = ro + rd * tn; float cov = vcCov(p.xz); float d = vcD(p, cov, true);
    if (d > 0.002) {
      if (tHit < 0.0) tHit = tn;
      float od = (vcD(p + L * 4.0, cov, false) * 4.0 + vcD(p + L * 12.0, cov, false) * 8.0 + vcD(p + L * 26.0, cov, false) * 14.0 + d * 2.0) * sig * 1.6;
      float h = clamp((p.y - uCol.x) / (uCol.y - uCol.x), 0.0, 1.0);
      float sun = exp(-od) + 0.18 * exp(-od * 0.25);
      vec3 amb = mix(uFog * vec3(0.4, 0.44, 0.56), uFog * vec3(0.66, 0.69, 0.78) + uTop * 0.08, h * h);
      vec3 S = uGlow * sun * ph + amb;
      float a = 1.0 - exp(-d * sig * max(dt, 0.35));
      col += T * a * S; T *= 1.0 - a;
      if (T < 0.025) { T = 0.0; break; }
    }
  }
  float A = 1.0 - T; if (A < 0.004) return vec4(0.0);
  vec3 c = col / A; float fd = tHit < 0.0 ? t0 : tHit; float ff = 1.0 - exp(-uDens * uDens * fd * fd);
  return vec4(mix(c, uFog, ff), A); }
