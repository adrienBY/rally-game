//! Shapes: polygon triangulation (earcut, as three.js bundles it), ShapeGeometry and ExtrudeGeometry (straight-edged shapes, optional
//! one-segment bevel), for the car's wings, mirrors and lips and the tunnel portals.
use super::three::{Geo, V3, v3};

// ---------------------------------------------------------------- earcut (mapbox earcut 2.2, linked-list ear clipping with hole bridging)
#[derive(Clone)]
struct Node { i: usize, x: f64, y: f64, prev: usize, next: usize, z: i64, prev_z: usize, next_z: usize, steiner: bool }
const NIL: usize = usize::MAX;
struct Ear { n: Vec<Node> }
impl Ear {
    fn insert(&mut self, i: usize, x: f64, y: f64, last: usize) -> usize {
        let k = self.n.len(); self.n.push(Node { i, x, y, prev: k, next: k, z: 0, prev_z: NIL, next_z: NIL, steiner: false });
        if last != NIL { let nx = self.n[last].next; self.n[k].next = nx; self.n[k].prev = last; self.n[nx].prev = k; self.n[last].next = k; }
        k
    }
    fn remove(&mut self, p: usize) {
        let (pr, nx) = (self.n[p].prev, self.n[p].next); self.n[nx].prev = pr; self.n[pr].next = nx;
        let (pz, nz) = (self.n[p].prev_z, self.n[p].next_z); if pz != NIL { self.n[pz].next_z = nz; } if nz != NIL { self.n[nz].prev_z = pz; }
    }
    fn eq(&self, a: usize, b: usize) -> bool { self.n[a].x == self.n[b].x && self.n[a].y == self.n[b].y }
    fn area(&self, p: usize, q: usize, r: usize) -> f64 { let (p, q, r) = (&self.n[p], &self.n[q], &self.n[r]); (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y) }
    fn linked_list(&mut self, d: &[f64], start: usize, end: usize, clockwise: bool) -> usize {
        let mut last = NIL;
        if clockwise == (signed_area(d, start, end) > 0.0) { let mut i = start; while i < end { last = self.insert(i, d[i], d[i + 1], last); i += 2; } }
        else { let mut i = end as i64 - 2; while i >= start as i64 { let iu = i as usize; last = self.insert(iu, d[iu], d[iu + 1], last); i -= 2; } }
        if last != NIL && self.eq(last, self.n[last].next) { let nx = self.n[last].next; self.remove(last); last = nx; }
        last
    }
    fn filter_points(&mut self, start: usize, end: usize) -> usize {
        if start == NIL { return start; } let mut end = if end == NIL { start } else { end };
        let mut p = start;
        loop {
            let mut again = false;
            if !self.n[p].steiner && (self.eq(p, self.n[p].next) || self.area(self.n[p].prev, p, self.n[p].next) == 0.0) {
                let pr = self.n[p].prev; self.remove(p); p = pr; end = pr; if p == self.n[p].next { break; } again = true;
            } else { p = self.n[p].next; }
            if !again && p == end { break; }
        }
        end
    }
    fn ear_cut(&mut self, ear0: usize, tris: &mut Vec<usize>, pass: u8) {
        let mut ear = ear0; if ear == NIL { return; }
        let mut stop = ear;
        while self.n[ear].prev != self.n[ear].next {
            let (prev, next) = (self.n[ear].prev, self.n[ear].next);
            if self.is_ear(ear) {
                tris.push(self.n[prev].i / 2); tris.push(self.n[ear].i / 2); tris.push(self.n[next].i / 2);
                self.remove(ear); ear = self.n[next].next; stop = self.n[next].next; continue;
            }
            ear = next;
            if ear == stop {
                if pass == 0 { let e = self.filter_points(ear, NIL); self.ear_cut(e, tris, 1); }
                else if pass == 1 { let e = self.filter_points(ear, NIL); let e = self.cure_local(e, tris); self.ear_cut(e, tris, 2); }
                else if pass == 2 { self.split_ear_cut(ear, tris); }
                break;
            }
        }
    }
    fn is_ear(&self, ear: usize) -> bool {
        let (a, b, c) = (self.n[ear].prev, ear, self.n[ear].next);
        if self.area(a, b, c) >= 0.0 { return false; }
        let (ax, bx, cx, ay, by, cy) = (self.n[a].x, self.n[b].x, self.n[c].x, self.n[a].y, self.n[b].y, self.n[c].y);
        let (x0, y0, x1, y1) = (ax.min(bx).min(cx), ay.min(by).min(cy), ax.max(bx).max(cx), ay.max(by).max(cy));
        let mut p = self.n[c].next;
        while p != a {
            let q = &self.n[p];
            if q.x >= x0 && q.x <= x1 && q.y >= y0 && q.y <= y1 && point_in_tri(ax, ay, bx, by, cx, cy, q.x, q.y) && self.area(q.prev, p, q.next) >= 0.0 { return false; }
            p = q.next;
        }
        true
    }
    fn cure_local(&mut self, start: usize, tris: &mut Vec<usize>) -> usize {
        let mut p = start; let mut start = start;
        loop {
            let a = self.n[p].prev; let b = self.n[self.n[p].next].next;
            if !self.eq(a, b) && self.intersects(a, p, self.n[p].next, b) && self.locally_inside(a, b) && self.locally_inside(b, a) {
                tris.push(self.n[a].i / 2); tris.push(self.n[p].i / 2); tris.push(self.n[b].i / 2);
                let pn = self.n[p].next; self.remove(p); self.remove(pn); p = b; start = b;
            }
            p = self.n[p].next; if p == start { break; }
        }
        self.filter_points(p, NIL)
    }
    fn split_ear_cut(&mut self, start: usize, tris: &mut Vec<usize>) {
        let mut a = start;
        loop {
            let mut b = self.n[self.n[a].next].next;
            while b != self.n[a].prev {
                if self.n[a].i != self.n[b].i && self.is_valid_diagonal(a, b) {
                    let mut c = self.split_polygon(a, b);
                    let an = self.n[a].next; a = self.filter_points(a, an); let cn = self.n[c].next; c = self.filter_points(c, cn);
                    self.ear_cut(a, tris, 0); self.ear_cut(c, tris, 0); return;
                }
                b = self.n[b].next;
            }
            a = self.n[a].next; if a == start { break; }
        }
    }
    fn eliminate_holes(&mut self, d: &[f64], holes: &[usize], outer: usize) -> usize {
        let mut queue = Vec::new();
        for (k, &h) in holes.iter().enumerate() { let start = h * 2; let end = if k + 1 < holes.len() { holes[k + 1] * 2 } else { d.len() };
            let list = self.linked_list(d, start, end, false); if list == self.n[list].next { self.n[list].steiner = true; } queue.push(self.leftmost(list)); }
        queue.sort_by(|&a, &b| self.n[a].x.partial_cmp(&self.n[b].x).unwrap_or(std::cmp::Ordering::Equal));
        let mut outer = outer; for q in queue { outer = self.eliminate_hole(q, outer); }
        outer
    }
    fn eliminate_hole(&mut self, hole: usize, outer: usize) -> usize {
        let bridge = self.find_hole_bridge(hole, outer); if bridge == NIL { return outer; }
        let bridge_rev = self.split_polygon(bridge, hole);
        let brn = self.n[bridge_rev].next; self.filter_points(bridge_rev, brn);
        let bn = self.n[bridge].next; self.filter_points(bridge, bn)
    }
    fn find_hole_bridge(&self, hole: usize, outer: usize) -> usize {
        let mut p = outer; let (hx, hy) = (self.n[hole].x, self.n[hole].y); let mut qx = f64::NEG_INFINITY; let mut m = NIL;
        loop {
            let (px, py, nx, ny) = (self.n[p].x, self.n[p].y, self.n[self.n[p].next].x, self.n[self.n[p].next].y);
            if hy <= py && hy >= ny && ny != py {
                let x = px + (hy - py) * (nx - px) / (ny - py);
                if x <= hx && x > qx { qx = x; m = if px < nx { p } else { self.n[p].next }; if x == hx { return m; } }
            }
            p = self.n[p].next; if p == outer { break; }
        }
        if m == NIL { return NIL; }
        let stop = m; let (mx, my) = (self.n[m].x, self.n[m].y); let mut tan_min = f64::INFINITY; p = m;
        loop {
            let (px, py) = (self.n[p].x, self.n[p].y);
            if hx >= px && px >= mx && hx != px && point_in_tri(if hy < my { hx } else { qx }, hy, mx, my, if hy < my { qx } else { hx }, hy, px, py) {
                let tan = (hy - py).abs() / (hx - px);
                if self.locally_inside(p, hole) && (tan < tan_min || (tan == tan_min && (px > self.n[m].x || (px == self.n[m].x && self.sector_contains(m, p))))) { m = p; tan_min = tan; }
            }
            p = self.n[p].next; if p == stop { break; }
        }
        m
    }
    fn sector_contains(&self, m: usize, p: usize) -> bool { self.area(self.n[m].prev, m, self.n[p].prev) < 0.0 && self.area(self.n[p].next, m, self.n[m].next) < 0.0 }
    fn leftmost(&self, start: usize) -> usize {
        let mut p = start; let mut l = start;
        loop { if self.n[p].x < self.n[l].x || (self.n[p].x == self.n[l].x && self.n[p].y < self.n[l].y) { l = p; } p = self.n[p].next; if p == start { break; } }
        l
    }
    fn is_valid_diagonal(&self, a: usize, b: usize) -> bool {
        let (an, ap, bn, bp) = (self.n[a].next, self.n[a].prev, self.n[b].next, self.n[b].prev);
        self.n[an].i != self.n[b].i && self.n[ap].i != self.n[b].i && !self.intersects_polygon(a, b)
            && ((self.locally_inside(a, b) && self.locally_inside(b, a) && self.middle_inside(a, b) && (self.area(ap, a, bp) != 0.0 || self.area(a, bp, b) != 0.0))
                || (self.eq(a, b) && self.area(ap, a, an) > 0.0 && self.area(bp, b, bn) > 0.0))
    }
    fn intersects(&self, p1: usize, q1: usize, p2: usize, q2: usize) -> bool {
        let o1 = sign(self.area(p1, q1, p2)); let o2 = sign(self.area(p1, q1, q2)); let o3 = sign(self.area(p2, q2, p1)); let o4 = sign(self.area(p2, q2, q1));
        if o1 != o2 && o3 != o4 { return true; }
        if o1 == 0 && self.on_segment(p1, p2, q1) { return true; } if o2 == 0 && self.on_segment(p1, q2, q1) { return true; }
        if o3 == 0 && self.on_segment(p2, p1, q2) { return true; } if o4 == 0 && self.on_segment(p2, q1, q2) { return true; }
        false
    }
    fn on_segment(&self, p: usize, q: usize, r: usize) -> bool {
        let (p, q, r) = (&self.n[p], &self.n[q], &self.n[r]); q.x <= p.x.max(r.x) && q.x >= p.x.min(r.x) && q.y <= p.y.max(r.y) && q.y >= p.y.min(r.y)
    }
    fn intersects_polygon(&self, a: usize, b: usize) -> bool {
        let mut p = a;
        loop { let pn = self.n[p].next;
            if self.n[p].i != self.n[a].i && self.n[pn].i != self.n[a].i && self.n[p].i != self.n[b].i && self.n[pn].i != self.n[b].i && self.intersects(p, pn, a, b) { return true; }
            p = pn; if p == a { break; } }
        false
    }
    fn locally_inside(&self, a: usize, b: usize) -> bool {
        let (ap, an) = (self.n[a].prev, self.n[a].next);
        if self.area(ap, a, an) < 0.0 { self.area(a, b, an) >= 0.0 && self.area(a, ap, b) >= 0.0 } else { self.area(a, b, ap) < 0.0 || self.area(a, an, b) < 0.0 }
    }
    fn middle_inside(&self, a: usize, b: usize) -> bool {
        let mut p = a; let mut inside = false; let (px, py) = ((self.n[a].x + self.n[b].x) / 2.0, (self.n[a].y + self.n[b].y) / 2.0);
        loop { let q = &self.n[p]; let qn = &self.n[q.next];
            if (q.y > py) != (qn.y > py) && qn.y != q.y && px < (qn.x - q.x) * (py - q.y) / (qn.y - q.y) + q.x { inside = !inside; }
            p = q.next; if p == a { break; } }
        inside
    }
    fn split_polygon(&mut self, a: usize, b: usize) -> usize {
        let a2 = self.n.len(); let an = self.n[a].clone(); self.n.push(Node { prev_z: NIL, next_z: NIL, steiner: false, ..an });
        let b2 = self.n.len(); let bn = self.n[b].clone(); self.n.push(Node { prev_z: NIL, next_z: NIL, steiner: false, ..bn });
        let (an, bp) = (self.n[a].next, self.n[b].prev);
        self.n[a].next = b; self.n[b].prev = a;
        self.n[a2].next = an; self.n[an].prev = a2;
        self.n[b2].next = a2; self.n[a2].prev = b2;
        self.n[bp].next = b2; self.n[b2].prev = bp;
        b2
    }
}
fn sign(v: f64) -> i32 { if v > 0.0 { 1 } else if v < 0.0 { -1 } else { 0 } }
fn point_in_tri(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, px: f64, py: f64) -> bool {
    (cx - px) * (ay - py) >= (ax - px) * (cy - py) && (ax - px) * (by - py) >= (bx - px) * (ay - py) && (bx - px) * (cy - py) >= (cx - px) * (by - py)
}
fn signed_area(d: &[f64], start: usize, end: usize) -> f64 {
    let mut s = 0.0; let mut j = end - 2; let mut i = start; while i < end { s += (d[j] - d[i]) * (d[i + 1] + d[j + 1]); j = i; i += 2; } s
}
/// flat [x0, y0, x1, y1, ...] with holes starting at the given vertex indices -> triangle vertex indices
pub fn earcut(d: &[f64], holes: &[usize]) -> Vec<usize> {
    let mut e = Ear { n: Vec::with_capacity(d.len()) }; let mut tris = Vec::new();
    let outer_len = if holes.is_empty() { d.len() } else { holes[0] * 2 };
    let mut outer = e.linked_list(d, 0, outer_len, true);
    if outer == NIL || e.n[outer].next == e.n[outer].prev { return tris; }
    if !holes.is_empty() { outer = e.eliminate_holes(d, holes, outer); }
    // (z-order hashing only kicks in past 80 points: never for these small shapes)
    e.ear_cut(outer, &mut tris, 0);
    tris
}

// ---------------------------------------------------------------- shapes
pub type P2 = (f64, f64);
pub fn area(c: &[P2]) -> f64 { let n = c.len(); let mut a = 0.0; let mut p = n - 1; for q in 0..n { a += c[p].0 * c[q].1 - c[q].0 * c[p].1; p = q; } a * 0.5 }
pub fn is_cw(c: &[P2]) -> bool { area(c) < 0.0 }
/// a shape's outline as Shape.extractPoints gives it for straight edges: the points, consecutive repeats dropped, closed back to the start
pub fn outline(pts: &[P2]) -> Vec<P2> {
    let mut o: Vec<P2> = Vec::new(); for &p in pts { if o.last() != Some(&p) { o.push(p); } }
    if o.len() > 1 && o.last() != o.first() { o.push(o[0]); }
    o
}
fn remove_dup_end(c: &mut Vec<P2>) { if c.len() > 2 && c.last() == c.first() { c.pop(); } }
pub fn triangulate(contour: &mut Vec<P2>, holes: &mut [Vec<P2>]) -> Vec<[usize; 3]> {
    remove_dup_end(contour); let mut d: Vec<f64> = contour.iter().flat_map(|p| [p.0, p.1]).collect(); let mut hi = Vec::new(); let mut ix = contour.len();
    for h in holes.iter_mut() { remove_dup_end(h); hi.push(ix); ix += h.len(); d.extend(h.iter().flat_map(|p| [p.0, p.1])); }
    earcut(&d, &hi).chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect()
}
/// ShapeGeometry: the shape's points (outline, then holes) in the xy plane, uv = position
pub fn shape_geo(shape: &[P2], holes: &[Vec<P2>]) -> Geo {
    let mut v = outline(shape); if !is_cw(&v) { v.reverse(); }
    let mut hs: Vec<Vec<P2>> = holes.iter().map(|h| { let mut h = outline(h); if is_cw(&h) { h.reverse(); } h }).collect();
    let faces = triangulate(&mut v, &mut hs);
    for h in &hs { v.extend_from_slice(h); }
    let pos: Vec<f32> = v.iter().flat_map(|p| [p.0 as f32, p.1 as f32, 0.0]).collect(); let nrm: Vec<f32> = v.iter().flat_map(|_| [0.0, 0.0, 1.0]).collect();
    let uv: Vec<f32> = v.iter().flat_map(|p| [p.0 as f32, p.1 as f32]).collect();
    Geo::new(pos).with_nrm(nrm).with_uv(uv).with_index(faces.iter().flat_map(|f| [f[0] as u32, f[1] as u32, f[2] as u32]).collect())
}
fn bevel_vec(pt: P2, prev: P2, next: P2) -> P2 {
    let (vpx, vpy) = (pt.0 - prev.0, pt.1 - prev.1); let (vnx, vny) = (next.0 - pt.0, next.1 - pt.1); let vpl2 = vpx * vpx + vpy * vpy;
    let col = vpx * vny - vpy * vnx; let (tx, ty, shrink);
    if col.abs() > f64::EPSILON {
        let vpl = vpl2.sqrt(); let vnl = (vnx * vnx + vny * vny).sqrt();
        let (psx, psy) = (prev.0 - vpy / vpl, prev.1 + vpx / vpl); let (nsx, nsy) = (next.0 - vny / vnl, next.1 + vnx / vnl);
        let sf = ((nsx - psx) * vny - (nsy - psy) * vnx) / (vpx * vny - vpy * vnx);
        tx = psx + vpx * sf - pt.0; ty = psy + vpy * sf - pt.1; let tl2 = tx * tx + ty * ty;
        if tl2 <= 2.0 { return (tx, ty); } shrink = (tl2 / 2.0).sqrt();
    } else {
        let eq = if vpx > f64::EPSILON { vnx > f64::EPSILON } else if vpx < -f64::EPSILON { vnx < -f64::EPSILON } else { vpy.signum() == vny.signum() };
        if eq { tx = -vpy; ty = vpx; shrink = vpl2.sqrt(); } else { tx = vpx; ty = vpy; shrink = (vpl2 / 2.0).sqrt(); }
    }
    (tx / shrink, ty / shrink)
}
/// ExtrudeGeometry of a straight-edged shape: depth along +z from 0, `bevel` (thickness = size) with one segment, or none. Non-indexed,
/// vertex normals computed (the uv the page never used are left at 0).
pub fn extrude(shape: &[P2], depth: f64, bevel: f64) -> Geo {
    let (bevel_on, bt, bsz, segs) = if bevel > 0.0 { (true, bevel, bevel, 1usize) } else { (false, 0.0, 0.0, 0usize) };
    let steps = 1usize;
    let mut verts = outline(shape); if !is_cw(&verts) { verts.reverse(); }
    let faces = triangulate(&mut verts, &mut []);
    let contour = verts.clone(); let vlen = verts.len();
    let mv: Vec<P2> = (0..contour.len()).map(|i| { let j = (i + contour.len() - 1) % contour.len(); let k = (i + 1) % contour.len(); bevel_vec(contour[i], contour[j], contour[k]) }).collect();
    let sc = |p: P2, v: P2, s: f64| (p.0 + v.0 * s, p.1 + v.1 * s);
    let mut ph: Vec<V3> = Vec::new();
    for b in 0..segs { let t = b as f64 / segs as f64; let z = bt * (t * std::f64::consts::PI / 2.0).cos(); let bs = bsz * (t * std::f64::consts::PI / 2.0).sin();
        for i in 0..contour.len() { let q = sc(contour[i], mv[i], bs); ph.push(v3(q.0, q.1, -z)); } }
    let bs = bsz;
    for i in 0..vlen { let q = if bevel_on { sc(verts[i], mv[i], bs) } else { verts[i] }; ph.push(v3(q.0, q.1, 0.0)); }
    for s in 1..=steps { for i in 0..vlen { let q = if bevel_on { sc(verts[i], mv[i], bs) } else { verts[i] }; ph.push(v3(q.0, q.1, depth / steps as f64 * s as f64)); } }
    for b in (0..segs).rev() { let t = b as f64 / segs as f64; let z = bt * (t * std::f64::consts::PI / 2.0).cos(); let bs = bsz * (t * std::f64::consts::PI / 2.0).sin();
        for i in 0..contour.len() { let q = sc(contour[i], mv[i], bs); ph.push(v3(q.0, q.1, depth + z)); } }
    let mut out: Vec<V3> = Vec::new();
    // lids
    if bevel_on { let top = vlen * (steps + segs * 2);
        for f in &faces { out.push(ph[f[2]]); out.push(ph[f[1]]); out.push(ph[f[0]]); }
        for f in &faces { out.push(ph[f[0] + top]); out.push(ph[f[1] + top]); out.push(ph[f[2] + top]); }
    } else {
        for f in &faces { out.push(ph[f[2]]); out.push(ph[f[1]]); out.push(ph[f[0]]); }
        for f in &faces { out.push(ph[f[0] + vlen * steps]); out.push(ph[f[1] + vlen * steps]); out.push(ph[f[2] + vlen * steps]); }
    }
    // side walls
    let mut i = contour.len() as i64;
    while { i -= 1; i >= 0 } { let j = i as usize; let k = if i - 1 < 0 { contour.len() - 1 } else { (i - 1) as usize };
        for s in 0..(steps + segs * 2) { let (l1, l2) = (vlen * s, vlen * (s + 1)); let (a, b, c, d) = (j + l1, k + l1, k + l2, j + l2);
            for q in [a, b, d, b, c, d] { out.push(ph[q]); } } }
    let pos: Vec<f32> = out.iter().flat_map(|p| [p.x as f32, p.y as f32, p.z as f32]).collect(); let n = out.len();
    let mut g = Geo::new(pos).with_uv(vec![0.0; n * 2]); g.compute_normals(); g
}
