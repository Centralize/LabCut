//! Basic 2D geometry: points, polylines and curve flattening.

use std::f64::consts::{PI, TAU};

/// How far (in mm) a flattened curve may stray from the true curve.
pub const TOLERANCE: f64 = 0.02;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn dist(self, o: Pt) -> f64 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
    pub fn add(self, o: Pt) -> Pt {
        Pt::new(self.x + o.x, self.y + o.y)
    }
    pub fn sub(self, o: Pt) -> Pt {
        Pt::new(self.x - o.x, self.y - o.y)
    }
    pub fn scale(self, s: f64) -> Pt {
        Pt::new(self.x * s, self.y * s)
    }
    pub fn lerp(self, o: Pt, t: f64) -> Pt {
        Pt::new(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t)
    }
}

/// One drawn shape: a polyline that may be closed, on a named layer.
#[derive(Clone, Debug)]
pub struct Path {
    pub pts: Vec<Pt>,
    pub closed: bool,
    pub layer: usize,
}

impl Path {
    pub fn new(pts: Vec<Pt>, closed: bool, layer: usize) -> Self {
        let mut p = Self { pts, closed, layer };
        p.normalize();
        p
    }

    /// Drop repeated points and mark a polyline closed when its ends meet.
    pub fn normalize(&mut self) {
        self.pts.dedup_by(|a, b| a.dist(*b) < 1e-9);
        if self.pts.len() > 2 {
            let first = self.pts[0];
            let last = *self.pts.last().unwrap();
            if first.dist(last) < 1e-6 {
                self.pts.pop();
                self.closed = true;
            }
        }
    }

    pub fn bbox(&self) -> BBox {
        let mut b = BBox::empty();
        for p in &self.pts {
            b.add(*p);
        }
        b
    }

    #[allow(dead_code)]
    pub fn length(&self) -> f64 {
        let mut l: f64 = self.pts.windows(2).map(|w| w[0].dist(w[1])).sum();
        if self.closed && self.pts.len() > 1 {
            l += self.pts[0].dist(*self.pts.last().unwrap());
        }
        l
    }

    /// Points in cutting order, closing the loop if needed.
    pub fn cut_points(&self) -> Vec<Pt> {
        let mut v = self.pts.clone();
        if self.closed && v.len() > 1 {
            v.push(v[0]);
        }
        v
    }

    /// Smallest distance from `p` to any segment of this path.
    pub fn distance_to(&self, p: Pt) -> f64 {
        let pts = self.cut_points();
        if pts.len() == 1 {
            return pts[0].dist(p);
        }
        pts.windows(2)
            .map(|w| seg_dist(p, w[0], w[1]))
            .fold(f64::INFINITY, f64::min)
    }
}

pub fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let ab = b.sub(a);
    let len2 = ab.x * ab.x + ab.y * ab.y;
    if len2 < 1e-18 {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / len2).clamp(0.0, 1.0);
    p.dist(a.lerp(b, t))
}

#[derive(Clone, Copy, Debug)]
pub struct BBox {
    pub min: Pt,
    pub max: Pt,
}

impl BBox {
    pub fn empty() -> Self {
        Self {
            min: Pt::new(f64::INFINITY, f64::INFINITY),
            max: Pt::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }
    pub fn add(&mut self, p: Pt) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }
    pub fn union(&mut self, o: &BBox) {
        if !o.is_empty() {
            self.add(o.min);
            self.add(o.max);
        }
    }
    pub fn width(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max.x - self.min.x }
    }
    pub fn height(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max.y - self.min.y }
    }
    pub fn contains_box(&self, o: &BBox) -> bool {
        o.min.x >= self.min.x - 1e-6
            && o.min.y >= self.min.y - 1e-6
            && o.max.x <= self.max.x + 1e-6
            && o.max.y <= self.max.y + 1e-6
    }
}

pub fn bbox_of(paths: &[Path]) -> BBox {
    let mut b = BBox::empty();
    for p in paths {
        b.union(&p.bbox());
    }
    b
}

/// Even-odd point-in-polygon test.
pub fn point_in_polygon(p: Pt, poly: &[Pt]) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn arc_segments(radius: f64, sweep: f64) -> usize {
    let r = radius.abs().max(1e-6);
    let step = if TOLERANCE < r {
        2.0 * (1.0 - TOLERANCE / r).acos()
    } else {
        PI / 4.0
    };
    let n = (sweep.abs() / step.max(1e-3)).ceil() as usize;
    let min = ((sweep.abs() / TAU) * 16.0).ceil() as usize;
    n.max(min).clamp(1, 4000)
}

/// Points along a circular arc from angle `a0` sweeping `sweep` radians (positive = counter-clockwise).
pub fn arc_points(c: Pt, r: f64, a0: f64, sweep: f64) -> Vec<Pt> {
    let n = arc_segments(r, sweep);
    (0..=n)
        .map(|i| {
            let a = a0 + sweep * i as f64 / n as f64;
            Pt::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

pub fn circle_points(c: Pt, r: f64) -> Vec<Pt> {
    let mut v = arc_points(c, r, 0.0, TAU);
    v.pop();
    v
}

/// Ellipse with centre `c`, major-axis vector `major`, minor/major `ratio`, between parameters t0..t1.
pub fn ellipse_points(c: Pt, major: Pt, ratio: f64, t0: f64, t1: f64) -> Vec<Pt> {
    let a = (major.x * major.x + major.y * major.y).sqrt();
    let rot = major.y.atan2(major.x);
    let b = a * ratio;
    let mut sweep = t1 - t0;
    while sweep <= 0.0 {
        sweep += TAU;
    }
    if sweep > TAU + 1e-9 {
        sweep = TAU;
    }
    let n = arc_segments(a.max(b), sweep);
    let (cs, sn) = (rot.cos(), rot.sin());
    (0..=n)
        .map(|i| {
            let t = t0 + sweep * i as f64 / n as f64;
            let (x, y) = (a * t.cos(), b * t.sin());
            Pt::new(c.x + x * cs - y * sn, c.y + x * sn + y * cs)
        })
        .collect()
}

/// Points for a DXF "bulge" segment between p0 and p1 (excluding p0, including p1).
pub fn bulge_points(p0: Pt, p1: Pt, bulge: f64) -> Vec<Pt> {
    if bulge.abs() < 1e-9 {
        return vec![p1];
    }
    let chord = p0.dist(p1);
    if chord < 1e-12 {
        return vec![p1];
    }
    let theta = 4.0 * bulge.atan(); // included angle, signed
    let r = chord / (2.0 * (theta / 2.0).sin()).abs();
    let mid = p0.lerp(p1, 0.5);
    let d = (r * r - (chord / 2.0).powi(2)).max(0.0).sqrt();
    // unit normal to chord, pointing left of p0->p1
    let dir = p1.sub(p0).scale(1.0 / chord);
    let left = Pt::new(-dir.y, dir.x);
    // centre lies left of chord for CCW arcs smaller than half a turn
    let sign = if (bulge > 0.0) == (bulge.abs() < 1.0) { 1.0 } else { -1.0 };
    let c = mid.add(left.scale(d * sign));
    let a0 = (p0.y - c.y).atan2(p0.x - c.x);
    let mut pts = arc_points(c, r, a0, theta);
    pts.remove(0);
    if let Some(last) = pts.last_mut() {
        *last = p1;
    }
    pts
}

pub fn cubic_points(p0: Pt, c1: Pt, c2: Pt, p3: Pt) -> Vec<Pt> {
    let est = p0.dist(c1) + c1.dist(c2) + c2.dist(p3);
    let n = ((est / 0.5).sqrt() * 3.0).ceil().clamp(4.0, 400.0) as usize;
    (1..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let mt = 1.0 - t;
            let a = mt * mt * mt;
            let b = 3.0 * mt * mt * t;
            let c = 3.0 * mt * t * t;
            let d = t * t * t;
            Pt::new(
                a * p0.x + b * c1.x + c * c2.x + d * p3.x,
                a * p0.y + b * c1.y + c * c2.y + d * p3.y,
            )
        })
        .collect()
}

pub fn quad_points(p0: Pt, c: Pt, p2: Pt) -> Vec<Pt> {
    let c1 = p0.lerp(c, 2.0 / 3.0);
    let c2 = p2.lerp(c, 2.0 / 3.0);
    cubic_points(p0, c1, c2, p2)
}

/// Evaluate a (possibly rational) B-spline with de Boor's algorithm.
pub fn bspline_points(degree: usize, ctrl: &[Pt], weights: &[f64], knots: &[f64]) -> Vec<Pt> {
    let n = ctrl.len();
    if n < 2 || degree == 0 {
        return ctrl.to_vec();
    }
    let p = degree.min(n - 1);
    let knots: Vec<f64> = if knots.len() == n + p + 1 {
        knots.to_vec()
    } else {
        // clamped uniform knot vector
        let mut k = vec![0.0; p + 1];
        for i in 1..(n - p) {
            k.push(i as f64);
        }
        k.extend(std::iter::repeat_n((n - p) as f64, p + 1));
        k
    };
    let w: Vec<f64> = if weights.len() == n { weights.to_vec() } else { vec![1.0; n] };
    let t0 = knots[p];
    let t1 = knots[n];
    let poly_len: f64 = ctrl.windows(2).map(|s| s[0].dist(s[1])).sum();
    let steps = ((poly_len / 0.5).ceil() as usize).clamp(n * 8, 5000);
    let mut out = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let t = t0 + (t1 - t0) * i as f64 / steps as f64;
        // find span k with knots[k] <= t < knots[k+1]
        let mut k = p;
        while k < n - 1 && t >= knots[k + 1] {
            k += 1;
        }
        let mut d: Vec<(f64, f64, f64)> = (0..=p)
            .map(|j| {
                let cp = ctrl[j + k - p];
                let wj = w[j + k - p];
                (cp.x * wj, cp.y * wj, wj)
            })
            .collect();
        for r in 1..=p {
            for j in (r..=p).rev() {
                let i0 = j + k - p;
                let denom = knots[i0 + p + 1 - r] - knots[i0];
                let alpha = if denom.abs() < 1e-12 { 0.0 } else { (t - knots[i0]) / denom };
                d[j] = (
                    (1.0 - alpha) * d[j - 1].0 + alpha * d[j].0,
                    (1.0 - alpha) * d[j - 1].1 + alpha * d[j].1,
                    (1.0 - alpha) * d[j - 1].2 + alpha * d[j].2,
                );
            }
        }
        let (x, y, ww) = d[p];
        let ww = if ww.abs() < 1e-12 { 1.0 } else { ww };
        out.push(Pt::new(x / ww, y / ww));
    }
    out
}

/// Join open paths whose end points touch, so a drawing made of loose lines cuts as continuous shapes.
pub fn join_paths(paths: Vec<Path>, tol: f64) -> Vec<Path> {
    let (mut closed, mut open): (Vec<Path>, Vec<Path>) = paths.into_iter().partition(|p| p.closed);
    let mut result = Vec::new();
    while let Some(mut cur) = open.pop() {
        loop {
            let end = *cur.pts.last().unwrap();
            let start = cur.pts[0];
            let mut found = None;
            for (i, o) in open.iter().enumerate() {
                if o.layer != cur.layer {
                    continue;
                }
                let (os, oe) = (o.pts[0], *o.pts.last().unwrap());
                if end.dist(os) < tol {
                    found = Some((i, 0));
                } else if end.dist(oe) < tol {
                    found = Some((i, 1));
                } else if start.dist(oe) < tol {
                    found = Some((i, 2));
                } else if start.dist(os) < tol {
                    found = Some((i, 3));
                }
                if found.is_some() {
                    break;
                }
            }
            let Some((i, how)) = found else { break };
            let mut o = open.swap_remove(i);
            match how {
                0 => cur.pts.extend(o.pts.drain(1..)),
                1 => {
                    o.pts.reverse();
                    cur.pts.extend(o.pts.drain(1..));
                }
                2 => {
                    o.pts.pop();
                    o.pts.extend(cur.pts.drain(..));
                    cur.pts = o.pts;
                }
                _ => {
                    o.pts.reverse();
                    o.pts.pop();
                    o.pts.extend(cur.pts.drain(..));
                    cur.pts = o.pts;
                }
            }
        }
        if cur.pts.len() > 2 && cur.pts[0].dist(*cur.pts.last().unwrap()) < tol {
            cur.pts.pop();
            cur.closed = true;
            closed.push(cur);
        } else {
            result.push(cur);
        }
    }
    closed.extend(result);
    closed
}

/// A 2D affine transform: x' = a*x + c*y + e, y' = b*x + d*y + f.
#[derive(Clone, Copy, Debug)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };
    pub fn translate(x: f64, y: f64) -> Self {
        Affine { e: x, f: y, ..Self::IDENTITY }
    }
    pub fn scale(sx: f64, sy: f64) -> Self {
        Affine { a: sx, d: sy, ..Self::IDENTITY }
    }
    pub fn rotate(rad: f64) -> Self {
        let (s, c) = rad.sin_cos();
        Affine { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }
    /// self applied after `o`.
    pub fn then(&self, o: &Affine) -> Affine {
        // result = o âˆ˜ self  (apply self first, then o)
        Affine {
            a: o.a * self.a + o.c * self.b,
            b: o.b * self.a + o.d * self.b,
            c: o.a * self.c + o.c * self.d,
            d: o.b * self.c + o.d * self.d,
            e: o.a * self.e + o.c * self.f + o.e,
            f: o.b * self.e + o.d * self.f + o.f,
        }
    }
    pub fn apply(&self, p: Pt) -> Pt {
        Pt::new(self.a * p.x + self.c * p.y + self.e, self.b * p.x + self.d * p.y + self.f)
    }
    /// Rough uniform scale factor (for choosing curve resolution).
    #[allow(dead_code)]
    pub fn scale_factor(&self) -> f64 {
        (self.a * self.d - self.b * self.c).abs().sqrt()
    }
}
