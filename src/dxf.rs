//! A small DXF reader: turns the common 2D entities into polylines grouped by layer.

use crate::geom::*;
use crate::import::Drawing;
use std::collections::HashMap;

type Rec = Vec<(i32, String)>;

struct Entity {
    kind: String,
    codes: Rec,
    /// VERTEX records belonging to an old-style POLYLINE.
    vertices: Vec<Rec>,
}

impl Entity {
    fn get(&self, code: i32) -> Option<&str> {
        self.codes.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
    }
    fn f(&self, code: i32) -> f64 {
        self.get(code).and_then(|v| v.trim().parse().ok()).unwrap_or(0.0)
    }
    fn fo(&self, code: i32, default: f64) -> f64 {
        self.get(code).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
    }
    fn i(&self, code: i32) -> i64 {
        self.get(code).and_then(|v| v.trim().parse().ok()).unwrap_or(0)
    }
    fn all(&self, code: i32) -> Vec<f64> {
        self.codes
            .iter()
            .filter(|(c, _)| *c == code)
            .filter_map(|(_, v)| v.trim().parse().ok())
            .collect()
    }
}

struct Block {
    base: Pt,
    entities: Vec<Entity>,
}

pub fn parse(text: &str) -> Result<Drawing, String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 4 {
        return Err("The file is empty or isn't a text DXF file.".into());
    }
    let mut pairs: Vec<(i32, String)> = Vec::with_capacity(lines.len() / 2);
    let mut i = 0;
    while i + 1 < lines.len() {
        let code = lines[i].trim().parse::<i32>();
        match code {
            Ok(c) => pairs.push((c, lines[i + 1].trim_end().trim_start().to_string())),
            Err(_) => {
                if i == 0 && lines[0].starts_with("AutoCAD Binary DXF") {
                    return Err("Binary DXF files aren't supported. Save as ASCII DXF instead.".into());
                }
                // Try to resync by skipping one line.
                i += 1;
                continue;
            }
        }
        i += 2;
    }

    let mut drawing = Drawing::default();
    let mut layer_colors: HashMap<String, i64> = HashMap::new();
    let mut units_scale = 1.0;
    let mut blocks: HashMap<String, Block> = HashMap::new();
    let mut entities: Vec<Entity> = Vec::new();

    let mut idx = 0;
    while idx < pairs.len() {
        let (c, v) = &pairs[idx];
        if *c == 0 && v == "SECTION" {
            let name = pairs.get(idx + 1).map(|p| p.1.clone()).unwrap_or_default();
            idx += 2;
            let start = idx;
            while idx < pairs.len() && !(pairs[idx].0 == 0 && pairs[idx].1 == "ENDSEC") {
                idx += 1;
            }
            let sec = &pairs[start..idx.min(pairs.len())];
            match name.as_str() {
                "HEADER" => {
                    for w in 0..sec.len() {
                        if sec[w].0 == 9 && sec[w].1 == "$INSUNITS" {
                            if let Some((70, u)) = sec.get(w + 1).map(|p| (p.0, p.1.clone())) {
                                units_scale = match u.trim().parse::<i32>().unwrap_or(4) {
                                    1 => 25.4,
                                    2 => 304.8,
                                    5 => 10.0,
                                    6 => 1000.0,
                                    8 => 0.0000254,
                                    9 => 0.0254,
                                    10 => 914.4,
                                    13 => 0.001,
                                    14 => 100.0,
                                    _ => 1.0,
                                };
                            }
                        }
                    }
                }
                "TABLES" => {
                    // LAYER table: name (2) and colour (62)
                    let recs = split_records(sec);
                    for r in recs {
                        if r.kind == "LAYER" {
                            if let Some(n) = r.get(2) {
                                layer_colors.insert(n.to_string(), r.i(62).abs());
                            }
                        }
                    }
                }
                "BLOCKS" => {
                    let recs = split_records(sec);
                    let mut cur: Option<(String, Block)> = None;
                    for r in recs {
                        match r.kind.as_str() {
                            "BLOCK" => {
                                let name = r.get(2).unwrap_or("").to_string();
                                cur = Some((name, Block { base: Pt::new(r.f(10), r.f(20)), entities: vec![] }));
                            }
                            "ENDBLK" => {
                                if let Some((n, b)) = cur.take() {
                                    blocks.insert(n, b);
                                }
                            }
                            _ => {
                                if let Some((_, b)) = cur.as_mut() {
                                    b.entities.push(r);
                                }
                            }
                        }
                    }
                }
                "ENTITIES" => {
                    entities.extend(split_records(sec));
                }
                _ => {}
            }
        }
        idx += 1;
    }

    let mut ctx = Ctx { drawing: &mut drawing, layer_colors: &layer_colors, blocks: &blocks, skipped: vec![] };
    let base = Affine::scale(units_scale, units_scale);
    for e in &entities {
        ctx.entity(e, &base, None, 0);
    }
    let skipped = ctx.skipped;
    if !skipped.is_empty() {
        drawing.notes.push(format!("Skipped items LabCut can't cut: {}.", skipped.join(", ")));
    }
    drawing.paths = join_paths(std::mem::take(&mut drawing.paths), 0.01);
    if drawing.paths.is_empty() {
        return Err("No lines, arcs, circles or polylines were found in this DXF file.".into());
    }
    Ok(drawing)
}

/// Group code pairs into records that each start with a code-0 marker.
fn split_records(sec: &[(i32, String)]) -> Vec<Entity> {
    let mut out: Vec<Entity> = Vec::new();
    let mut in_polyline = false;
    for (c, v) in sec {
        if *c == 0 {
            if v == "VERTEX" && in_polyline {
                if let Some(last) = out.last_mut() {
                    last.vertices.push(vec![]);
                }
                continue;
            }
            if v == "SEQEND" && in_polyline {
                in_polyline = false;
                // swallow the SEQEND record's codes
                out.push(Entity { kind: "SEQEND".into(), codes: vec![], vertices: vec![] });
                continue;
            }
            in_polyline = v == "POLYLINE";
            out.push(Entity { kind: v.clone(), codes: vec![], vertices: vec![] });
        } else if let Some(last) = out.last_mut() {
            if in_polyline && !last.vertices.is_empty() {
                last.vertices.last_mut().unwrap().push((*c, v.clone()));
            } else {
                last.codes.push((*c, v.clone()));
            }
        }
    }
    out
}

struct Ctx<'a> {
    drawing: &'a mut Drawing,
    layer_colors: &'a HashMap<String, i64>,
    blocks: &'a HashMap<String, Block>,
    skipped: Vec<String>,
}

impl Ctx<'_> {
    fn skip(&mut self, kind: &str) {
        if !self.skipped.iter().any(|k| k == kind) {
            self.skipped.push(kind.to_string());
        }
    }

    fn layer_for(&mut self, e: &Entity, parent_layer: Option<&str>) -> usize {
        let mut name = e.get(8).unwrap_or("0").to_string();
        if name == "0" {
            if let Some(p) = parent_layer {
                name = p.to_string();
            }
        }
        let mut aci = e.i(62);
        if aci == 0 || aci == 256 || e.get(62).is_none() {
            aci = *self.layer_colors.get(&name).unwrap_or(&7);
        }
        let color = if let Some(tc) = e.get(420).and_then(|v| v.trim().parse::<i64>().ok()) {
            [((tc >> 16) & 255) as u8, ((tc >> 8) & 255) as u8, (tc & 255) as u8]
        } else {
            aci_color(aci)
        };
        self.drawing.layer_index(&name, color)
    }

    fn push(&mut self, pts: Vec<Pt>, closed: bool, layer: usize, t: &Affine, ocs_flip: bool) {
        if pts.is_empty() {
            return;
        }
        let pts: Vec<Pt> = pts
            .into_iter()
            .map(|p| t.apply(if ocs_flip { Pt::new(-p.x, p.y) } else { p }))
            .collect();
        let path = Path::new(pts, closed, layer);
        if path.pts.len() >= 2 {
            self.drawing.paths.push(path);
        }
    }

    fn entity(&mut self, e: &Entity, t: &Affine, parent_layer: Option<&str>, depth: usize) {
        let flip = e.fo(230, 1.0) < 0.0;
        match e.kind.as_str() {
            "LINE" => {
                let l = self.layer_for(e, parent_layer);
                self.push(vec![Pt::new(e.f(10), e.f(20)), Pt::new(e.f(11), e.f(21))], false, l, t, false);
            }
            "CIRCLE" => {
                let l = self.layer_for(e, parent_layer);
                self.push(circle_points(Pt::new(e.f(10), e.f(20)), e.f(40)), true, l, t, flip);
            }
            "ARC" => {
                let l = self.layer_for(e, parent_layer);
                let a0 = e.f(50).to_radians();
                let mut a1 = e.f(51).to_radians();
                while a1 <= a0 {
                    a1 += std::f64::consts::TAU;
                }
                self.push(arc_points(Pt::new(e.f(10), e.f(20)), e.f(40), a0, a1 - a0), false, l, t, flip);
            }
            "ELLIPSE" => {
                let l = self.layer_for(e, parent_layer);
                let c = Pt::new(e.f(10), e.f(20));
                let major = Pt::new(e.f(11), e.f(21));
                let t0 = e.fo(41, 0.0);
                let t1 = e.fo(42, std::f64::consts::TAU);
                let full = (t1 - t0 - std::f64::consts::TAU).abs() < 1e-6 || (t1 - t0).abs() < 1e-9;
                let mut pts = ellipse_points(c, major, e.fo(40, 1.0), t0, if full { t0 + std::f64::consts::TAU } else { t1 });
                if full {
                    pts.pop();
                }
                self.push(pts, full, l, t, flip);
            }
            "LWPOLYLINE" => {
                let l = self.layer_for(e, parent_layer);
                let closed = e.i(70) & 1 == 1;
                // vertices: each 10 starts a vertex; 42 bulge applies to the vertex before it
                let mut verts: Vec<(Pt, f64)> = Vec::new();
                let mut cur_x = None;
                for (c, v) in &e.codes {
                    let val: f64 = v.trim().parse().unwrap_or(0.0);
                    match c {
                        10 => cur_x = Some(val),
                        20 => {
                            if let Some(x) = cur_x.take() {
                                verts.push((Pt::new(x, val), 0.0));
                            }
                        }
                        42 => {
                            if let Some(last) = verts.last_mut() {
                                last.1 = val;
                            }
                        }
                        _ => {}
                    }
                }
                let pts = bulge_polyline(&verts, closed);
                self.push(pts, closed, l, t, flip);
            }
            "POLYLINE" => {
                let l = self.layer_for(e, parent_layer);
                let flags = e.i(70);
                if flags & (16 | 64) != 0 {
                    self.skip("3D mesh"); // 3D mesh / polyface
                    return;
                }
                let closed = flags & 1 == 1;
                let verts: Vec<(Pt, f64)> = e
                    .vertices
                    .iter()
                    .map(|r| {
                        let v = Entity { kind: String::new(), codes: r.clone(), vertices: vec![] };
                        (Pt::new(v.f(10), v.f(20)), v.f(42))
                    })
                    .collect();
                let pts = bulge_polyline(&verts, closed);
                self.push(pts, closed, l, t, flip);
            }
            "SPLINE" => {
                let l = self.layer_for(e, parent_layer);
                let degree = e.fo(71, 3.0) as usize;
                let closed = e.i(70) & 1 == 1;
                let knots = e.all(40);
                let weights = e.all(41);
                let cx = e.all(10);
                let cy = e.all(20);
                let ctrl: Vec<Pt> = cx.iter().zip(cy.iter()).map(|(x, y)| Pt::new(*x, *y)).collect();
                let pts = if ctrl.len() >= 2 {
                    bspline_points(degree, &ctrl, &weights, &knots)
                } else {
                    let fx = e.all(11);
                    let fy = e.all(21);
                    fx.iter().zip(fy.iter()).map(|(x, y)| Pt::new(*x, *y)).collect()
                };
                self.push(pts, closed, l, t, false);
            }
            "INSERT" => {
                if depth > 8 {
                    return;
                }
                let name = e.get(2).unwrap_or("").to_string();
                let Some(block) = self.blocks.get(&name) else { return };
                let ins_layer = e.get(8).unwrap_or("0").to_string();
                let sx = e.fo(41, 1.0);
                let sy = e.fo(42, 1.0);
                let rot = e.f(50).to_radians();
                let cols = e.fo(70, 1.0).max(1.0) as usize;
                let rows = e.fo(71, 1.0).max(1.0) as usize;
                let (dc, dr) = (e.f(44), e.f(45));
                for r in 0..rows {
                    for c in 0..cols {
                        let local = Affine::translate(-block.base.x, -block.base.y)
                            .then(&Affine::scale(sx, sy))
                            .then(&Affine::rotate(rot))
                            .then(&Affine::translate(e.f(10), e.f(20)))
                            .then(&Affine::translate(c as f64 * dc, r as f64 * dr));
                        let full = local.then(t);
                        for be in &block.entities {
                            self.entity(be, &full, Some(&ins_layer), depth + 1);
                        }
                    }
                }
            }
            "SEQEND" | "VERTEX" | "ATTDEF" | "ATTRIB" | "VIEWPORT" | "POINT" | "EOF" | "ENDSEC" => {}
            other => self.skip(other),
        }
    }
}

fn bulge_polyline(verts: &[(Pt, f64)], closed: bool) -> Vec<Pt> {
    if verts.is_empty() {
        return vec![];
    }
    let mut pts = vec![verts[0].0];
    let n = verts.len();
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs {
        let (p0, b) = verts[i];
        let p1 = verts[(i + 1) % n].0;
        pts.extend(bulge_points(p0, p1, b));
    }
    if closed && pts.len() > 1 {
        pts.pop();
    }
    pts
}

/// AutoCAD colour index to RGB (main colours; others get a spread of hues).
pub fn aci_color(aci: i64) -> [u8; 3] {
    match aci {
        1 => [230, 50, 50],
        2 => [230, 210, 40],
        3 => [60, 200, 70],
        4 => [40, 200, 210],
        5 => [60, 110, 240],
        6 => [210, 70, 210],
        7 | 0 => [225, 225, 225],
        8 => [128, 128, 128],
        9 => [190, 190, 190],
        n => {
            let h = ((n - 10).rem_euclid(240) / 10) as f32 * 15.0;
            crate::import::hsv_to_rgb(h, 0.7, 0.95)
        }
    }
}
