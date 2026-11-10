//! Value noise that reproduces the JavaScript version bit for bit (including JS int32 wrap-around rules),
//! so a stage generated in Rust is identical to the one the browser build generates.

#[inline]
pub fn to_int32(v: f64) -> i32 {
    // fast path: any |v| < 2^63 truncates to i64 exactly, and the low 32 bits are the JS ToInt32 result
    if v.abs() < 9.2e18 { return (v as i64) as i32; }
    if !v.is_finite() { return 0; }
    let m = v.trunc() % 4294967296.0;
    let m = if m < 0.0 { m + 4294967296.0 } else { m };
    (m as u64 as u32) as i32
}
#[inline]
pub fn to_uint32(v: f64) -> u32 { to_int32(v) as u32 }

/// JS: let h = x*374761393 + y*668265263; h = (h ^ (h >>> 13)) * 1274126177; h ^= h >>> 16; return (h >>> 0) / 4294967295;
#[inline]
pub fn hash2(x: f64, y: f64) -> f64 {
    let h0 = x * 374761393.0 + y * 668265263.0;
    let r1 = to_int32(h0) ^ ((to_uint32(h0) >> 13) as i32);
    let h2 = (r1 as f64) * 1274126177.0;
    let r2 = to_int32(h2) ^ ((to_uint32(h2) >> 16) as i32);
    (r2 as u32) as f64 / 4294967295.0
}

#[inline]
pub fn vnoise(x: f64, y: f64) -> f64 {
    let xi = x.floor(); let yi = y.floor();
    let xf = x - xi; let yf = y - yi;
    let u = xf * xf * (3.0 - 2.0 * xf); let v = yf * yf * (3.0 - 2.0 * yf);
    let a = hash2(xi, yi); let b = hash2(xi + 1.0, yi); let c = hash2(xi, yi + 1.0); let d = hash2(xi + 1.0, yi + 1.0);
    a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v
}

pub fn fbm(x: f64, y: f64, o: u32) -> f64 {
    let (mut s, mut a, mut f) = (0.0, 0.5, 1.0);
    for _ in 0..o { s += a * vnoise(x * f, y * f); f *= 2.03; a *= 0.5; }
    s
}

pub fn ridged(x: f64, z: f64) -> f64 {
    let (mut s, mut a, mut f) = (0.0, 0.5, 1.0);
    for _ in 0..4 { let n = 1.0 - (vnoise(x * f, z * f) * 2.0 - 1.0).abs(); s += a * n * n; f *= 2.1; a *= 0.5; }
    s
}

#[inline] pub fn clamp(v: f64, a: f64, b: f64) -> f64 { if v < a { a } else if v > b { b } else { v } }
#[inline] pub fn smooth(e0: f64, e1: f64, x: f64) -> f64 { let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0); t * t * (3.0 - 2.0 * t) }
/// JS Math.round (halves round towards +infinity)
#[inline] pub fn js_round(v: f64) -> f64 { (v + 0.5).floor() }
/// JS Math.sign (0 stays 0)
#[inline] pub fn sgn(v: f64) -> f64 { if v > 0.0 { 1.0 } else if v < 0.0 { -1.0 } else { 0.0 } }
#[inline] pub fn wrap_a(a: f64) -> f64 { a.sin().atan2(a.cos()) }

#[cfg(test)]
mod tests {
    use super::*;
    fn slow(v: f64) -> i32 { if !v.is_finite() { return 0; } let m = v.trunc() % 4294967296.0; let m = if m < 0.0 { m + 4294967296.0 } else { m }; (m as u64 as u32) as i32 }
    #[test]
    fn int32_matches_js_rules() {
        let mut x = 0.123456789f64;
        for i in 0..200000 { x = (x * 1103515245.0 + 12345.0) % 1e19 - 5e18 * ((i % 3) as f64 - 1.0); for v in [x, -x, x / 7.3, x * 1e-9] { assert_eq!(to_int32(v), slow(v), "{v}"); } }
        assert_eq!(to_int32(2147483648.0), -2147483648); assert_eq!(to_int32(-1.5), -1); assert_eq!(to_int32(4294967297.9), 1);
    }
}
