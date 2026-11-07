//! A small SVG reader: paths and basic shapes, grouped into layers by stroke colour.

use crate::geom::*;
use crate::import::Drawing;

pub fn parse(text: &str) -> Result<Drawing, String> {
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(text, opts)
        .map_err(|e| format!("This SVG file couldn't be read: {e}"))?;
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        return Err("This file doesn't look like an SVG drawing.".into());
    }

    // Work out how big one SVG unit is in millimetres.
    let px_mm = 25.4 / 96.0;
    let w_attr = root.attribute("width").and_then(length_mm);
    let h_attr = root.attribute("height").and_then(length_mm);
    let vb: Option<Vec<f64>> = root.attribute("viewBox").map(|v| numbers(v)).filter(|v| v.len() == 4);
    let (sx, sy, ox, oy) = match (&vb, w_attr, h_attr) {
        (Some(vb), Some(w), Some(h)) if vb[2] > 0.0 && vb[3] > 0.0 => (w / vb[2], h / vb[3], vb[0], vb[1]),
        (Some(vb), Some(w), None) if vb[2] > 0.0 => (w / vb[2], w / vb[2], vb[0], vb[1]),
        (Some(vb), _, _) => (px_mm, px_mm, vb[0], vb[1]),
        _ => (px_mm, px_mm, 0.0, 0.0),
    };
    // SVG's y axis points down; the machine's points up.
    let base = Affine::translate(-ox, -oy).then(&Affine::scale(sx, -sy));

    let mut drawing = Drawing::default();
    let mut skipped: Vec<String> = Vec::new();
    let style = Style { stroke: None, fill: Some("#000000".into()), hidden: false };
    walk(root, &base, &style, &mut drawing, &mut skipped);
    if !skipped.is_empty() {
        drawing.notes.push(format!("Skipped items LabCut can't cut: {}.", skipped.join(", ")));
    }
    drawing.paths = join_paths(std::mem::take(&mut drawing.paths), 0.01);
    if drawing.paths.is_empty() {
        return Err("No shapes were found in this SVG file. Convert text to paths first.".into());
    }
    Ok(drawing)
}

#[derive(Clone)]
struct Style {
    stroke: Option<String>,
    fill: Option<String>,
    hidden: bool,
}

fn style_of(node: roxmltree::Node, parent: &Style) -> Style {
    let mut s = parent.clone();
    let mut set = |k: &str, v: &str| {
        let v = v.trim();
        match k.trim() {
            "stroke" => s.stroke = if v == "none" { None } else { Some(v.to_string()) },
            "fill" => s.fill = if v == "none" { None } else { Some(v.to_string()) },
            "display" if v == "none" => s.hidden = true,
            "visibility" if v == "hidden" => s.hidden = true,
            _ => {}
        }
    };
    for a in ["stroke", "fill", "display", "visibility"] {
        if let Some(v) = node.attribute(a) {
            set(a, v);
        }
    }
    if let Some(st) = node.attribute("style") {
        for part in st.split(';') {
            if let Some((k, v)) = part.split_once(':') {
                set(k, v);
            }
        }
    }
    s
}

fn walk(node: roxmltree::Node, t: &Affine, parent: &Style, d: &mut Drawing, skipped: &mut Vec<String>) {
    for child in node.children().filter(|n| n.is_element()) {
        let name = child.tag_name().name();
        if matches!(name, "defs" | "clipPath" | "mask" | "symbol" | "metadata" | "title" | "desc" | "style" | "marker" | "pattern" | "linearGradient" | "radialGradient" | "filter") {
            continue;
        }
        let st = style_of(child, parent);
        if st.hidden {
            continue;
        }
        let mut local = *t;
        if let Some(tr) = child.attribute("transform") {
            local = parse_transform(tr).then(t);
        }
        let mut shapes: Vec<(Vec<Pt>, bool)> = Vec::new();
        let num = |k: &str| child.attribute(k).and_then(|v| parse_len(v)).unwrap_or(0.0);
        match name {
            "g" | "svg" | "a" | "switch" => {
                walk(child, &local, &st, d, skipped);
                continue;
            }
            "path" => {
                if let Some(dd) = child.attribute("d") {
                    shapes = path_data(dd);
                }
            }
            "rect" => {
                let (x, y, w, h) = (num("x"), num("y"), num("width"), num("height"));
                let mut rx = child.attribute("rx").and_then(parse_len);
                let mut ry = child.attribute("ry").and_then(parse_len);
                if rx.is_none() {
                    rx = ry;
                }
                if ry.is_none() {
                    ry = rx;
                }
                let rx = rx.unwrap_or(0.0).min(w / 2.0);
                let ry = ry.unwrap_or(0.0).min(h / 2.0);
                if w > 0.0 && h > 0.0 {
                    if rx > 0.0 && ry > 0.0 {
                        let k = 0.5522847498;
                        let mut p = vec![Pt::new(x + rx, y)];
                        let mut cur = p[0];
                        let mut seg = |c1: Pt, c2: Pt, e: Pt, line_to: Pt, p: &mut Vec<Pt>| {
                            p.push(line_to);
                            p.extend(cubic_points(line_to, c1, c2, e));
                            cur = e;
                        };
                        seg(Pt::new(x + w - rx + rx * k, y), Pt::new(x + w, y + ry - ry * k), Pt::new(x + w, y + ry), Pt::new(x + w - rx, y), &mut p);
                        seg(Pt::new(x + w, y + h - ry + ry * k), Pt::new(x + w - rx + rx * k, y + h), Pt::new(x + w - rx, y + h), Pt::new(x + w, y + h - ry), &mut p);
                        seg(Pt::new(x + rx - rx * k, y + h), Pt::new(x, y + h - ry + ry * k), Pt::new(x, y + h - ry), Pt::new(x + rx, y + h), &mut p);
                        seg(Pt::new(x, y + ry - ry * k), Pt::new(x + rx - rx * k, y), Pt::new(x + rx, y), Pt::new(x, y + ry), &mut p);
                        let _ = cur;
                        shapes.push((p, true));
                    } else {
                        shapes.push((vec![Pt::new(x, y), Pt::new(x + w, y), Pt::new(x + w, y + h), Pt::new(x, y + h)], true));
                    }
                }
            }
            "circle" => {
                let r = num("r");
                if r > 0.0 {
                    shapes.push((circle_points(Pt::new(num("cx"), num("cy")), r), true));
                }
            }
            "ellipse" => {
                let (rx, ry) = (num("rx"), num("ry"));
                if rx > 0.0 && ry > 0.0 {
                    let mut p = ellipse_points(Pt::new(num("cx"), num("cy")), Pt::new(rx, 0.0), ry / rx, 0.0, std::f64::consts::TAU);
                    p.pop();
                    shapes.push((p, true));
                }
            }
            "line" => shapes.push((vec![Pt::new(num("x1"), num("y1")), Pt::new(num("x2"), num("y2"))], false)),
            "polyline" | "polygon" => {
                let v = numbers(child.attribute("points").unwrap_or(""));
                let pts: Vec<Pt> = v.chunks_exact(2).map(|c| Pt::new(c[0], c[1])).collect();
                shapes.push((pts, name == "polygon"));
            }
            _ => {
                if !skipped.iter().any(|k| k == name) {
                    skipped.push(name.to_string());
                }
                continue;
            }
        }
        let color_str = st.stroke.clone().or(st.fill.clone()).unwrap_or_else(|| "#000000".into());
        let color = parse_color(&color_str).unwrap_or([0, 0, 0]);
        // very dark colours are hard to see on the dark canvas; show them light but keep the name
        let shown = if color.iter().map(|c| *c as u32).sum::<u32>() < 120 { [225, 225, 225] } else { color };
        let layer_name = format!("#{:02X}{:02X}{:02X}", color[0], color[1], color[2]);
        let layer = d.layer_index(&layer_name, shown);
        for (pts, closed) in shapes {
            let pts: Vec<Pt> = pts.into_iter().map(|p| local.apply(p)).collect();
            let p = Path::new(pts, closed, layer);
            if p.pts.len() >= 2 {
                d.paths.push(p);
            }
        }
    }
}

fn parse_len(v: &str) -> Option<f64> {
    let v = v.trim();
    let end = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E')).unwrap_or(v.len());
    v[..end].parse().ok()
}

fn length_mm(v: &str) -> Option<f64> {
    let v = v.trim();
    let n = parse_len(v)?;
    let unit = v.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E').trim();
    Some(match unit {
        "mm" => n,
        "cm" => n * 10.0,
        "in" => n * 25.4,
        "pt" => n * 25.4 / 72.0,
        "pc" => n * 25.4 / 6.0,
        "%" => return None,
        _ => n * 25.4 / 96.0,
    })
}

fn numbers(s: &str) -> Vec<f64> {
    let mut lx = Lexer::new(s);
    let mut v = Vec::new();
    while let Some(n) = lx.number() {
        v.push(n);
    }
    v
}

fn parse_color(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().to_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        let h = |i: usize, n: usize| u8::from_str_radix(&hex[i..i + n], 16).ok();
        return match hex.len() {
            3 => Some([h(0, 1)? * 17, h(1, 1)? * 17, h(2, 1)? * 17]),
            6 | 8 => Some([h(0, 2)?, h(2, 2)?, h(4, 2)?]),
            _ => None,
        };
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|r| r.strip_suffix(')')) {
        let parts: Vec<f64> = inner
            .split(',')
            .map(|p| {
                let p = p.trim();
                if let Some(pc) = p.strip_suffix('%') {
                    pc.parse::<f64>().unwrap_or(0.0) * 2.55
                } else {
                    p.parse().unwrap_or(0.0)
                }
            })
            .collect();
        if parts.len() >= 3 {
            return Some([parts[0] as u8, parts[1] as u8, parts[2] as u8]);
        }
    }
    Some(match s.as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "lime" => [0, 255, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "cyan" | "aqua" => [0, 255, 255],
        "magenta" | "fuchsia" => [255, 0, 255],
        "orange" => [255, 165, 0],
        "purple" => [128, 0, 128],
        "gray" | "grey" => [128, 128, 128],
        _ => return None,
    })
}

fn parse_transform(s: &str) -> Affine {
    let mut result = Affine::IDENTITY;
    let mut rest = s;
    // transforms listed left-to-right apply right-to-left
    let mut list = Vec::new();
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(',').trim().to_string();
        let Some(close) = rest[open..].find(')') else { break };
        let args = numbers(&rest[open + 1..open + close]);
        rest = &rest[open + close + 1..];
        let a = |i: usize, d: f64| args.get(i).copied().unwrap_or(d);
        let m = match name.as_str() {
            "matrix" if args.len() == 6 => Affine { a: args[0], b: args[1], c: args[2], d: args[3], e: args[4], f: args[5] },
            "translate" => Affine::translate(a(0, 0.0), a(1, 0.0)),
            "scale" => Affine::scale(a(0, 1.0), a(1, a(0, 1.0))),
            "rotate" => {
                let r = Affine::rotate(a(0, 0.0).to_radians());
                if args.len() == 3 {
                    Affine::translate(-args[1], -args[2]).then(&r).then(&Affine::translate(args[1], args[2]))
                } else {
                    r
                }
            }
            "skewX" => Affine { c: a(0, 0.0).to_radians().tan(), ..Affine::IDENTITY },
            "skewY" => Affine { b: a(0, 0.0).to_radians().tan(), ..Affine::IDENTITY },
            _ => Affine::IDENTITY,
        };
        list.push(m);
    }
    for m in list.iter().rev() {
        result = result.then(m);
    }
    result
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn new(s: &'a str) -> Self {
        Self { s: s.as_bytes(), i: 0 }
    }
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',') {
            self.i += 1;
        }
    }
    fn peek_cmd(&mut self) -> Option<u8> {
        self.skip_ws();
        let c = *self.s.get(self.i)?;
        if c.is_ascii_alphabetic() && c != b'e' && c != b'E' {
            Some(c)
        } else {
            None
        }
    }
    fn at_end(&mut self) -> bool {
        self.skip_ws();
        self.i >= self.s.len()
    }
    fn flag(&mut self) -> Option<bool> {
        self.skip_ws();
        let c = *self.s.get(self.i)?;
        if c == b'0' || c == b'1' {
            self.i += 1;
            Some(c == b'1')
        } else {
            None
        }
    }
    fn number(&mut self) -> Option<f64> {
        self.skip_ws();
        let start = self.i;
        let s = self.s;
        let mut j = self.i;
        if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
            j += 1;
        }
        let mut digits = false;
        while j < s.len() && s[j].is_ascii_digit() {
            j += 1;
            digits = true;
        }
        if j < s.len() && s[j] == b'.' {
            j += 1;
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
                digits = true;
            }
        }
        if !digits {
            return None;
        }
        if j < s.len() && (s[j] == b'e' || s[j] == b'E') {
            let mut k = j + 1;
            if k < s.len() && (s[k] == b'+' || s[k] == b'-') {
                k += 1;
            }
            if k < s.len() && s[k].is_ascii_digit() {
                while k < s.len() && s[k].is_ascii_digit() {
                    k += 1;
                }
                j = k;
            }
        }
        self.i = j;
        std::str::from_utf8(&s[start..j]).ok()?.parse().ok()
    }
}

/// Parse SVG path data into polylines.
fn path_data(d: &str) -> Vec<(Vec<Pt>, bool)> {
    let mut out = Vec::new();
    let mut lx = Lexer::new(d);
    let mut cur = Pt::default();
    let mut start = Pt::default();
    let mut pts: Vec<Pt> = Vec::new();
    let mut last_ctrl: Option<Pt> = None;
    let mut last_qctrl: Option<Pt> = None;
    let mut cmd = b'M';
    let flush = |pts: &mut Vec<Pt>, closed: bool, out: &mut Vec<(Vec<Pt>, bool)>| {
        if pts.len() >= 2 {
            out.push((std::mem::take(pts), closed));
        } else {
            pts.clear();
        }
    };
    while !lx.at_end() {
        if let Some(c) = lx.peek_cmd() {
            cmd = c;
            lx.i += 1;
        }
        let rel = cmd.is_ascii_lowercase();
        let base = if rel { cur } else { Pt::default() };
        let up = cmd.to_ascii_uppercase();
        let mut was_cubic = false;
        let mut was_quad = false;
        match up {
            b'M' => {
                let (Some(x), Some(y)) = (lx.number(), lx.number()) else { break };
                flush(&mut pts, false, &mut out);
                cur = base.add(Pt::new(x, y));
                start = cur;
                pts.push(cur);
                cmd = if rel { b'l' } else { b'L' };
            }
            b'L' => {
                let (Some(x), Some(y)) = (lx.number(), lx.number()) else { break };
                cur = base.add(Pt::new(x, y));
                pts.push(cur);
            }
            b'H' => {
                let Some(x) = lx.number() else { break };
                cur = Pt::new(if rel { cur.x + x } else { x }, cur.y);
                pts.push(cur);
            }
            b'V' => {
                let Some(y) = lx.number() else { break };
                cur = Pt::new(cur.x, if rel { cur.y + y } else { y });
                pts.push(cur);
            }
            b'C' => {
                let v: Vec<f64> = (0..6).filter_map(|_| lx.number()).collect();
                if v.len() < 6 {
                    break;
                }
                let c1 = base.add(Pt::new(v[0], v[1]));
                let c2 = base.add(Pt::new(v[2], v[3]));
                let e = base.add(Pt::new(v[4], v[5]));
                if pts.is_empty() {
                    pts.push(cur);
                }
                pts.extend(cubic_points(cur, c1, c2, e));
                last_ctrl = Some(c2);
                was_cubic = true;
                cur = e;
            }
            b'S' => {
                let v: Vec<f64> = (0..4).filter_map(|_| lx.number()).collect();
                if v.len() < 4 {
                    break;
                }
                let c1 = match last_ctrl {
                    Some(lc) => cur.add(cur.sub(lc)),
                    None => cur,
                };
                let c2 = base.add(Pt::new(v[0], v[1]));
                let e = base.add(Pt::new(v[2], v[3]));
                if pts.is_empty() {
                    pts.push(cur);
                }
                pts.extend(cubic_points(cur, c1, c2, e));
                last_ctrl = Some(c2);
                was_cubic = true;
                cur = e;
            }
            b'Q' => {
                let v: Vec<f64> = (0..4).filter_map(|_| lx.number()).collect();
                if v.len() < 4 {
                    break;
                }
                let c = base.add(Pt::new(v[0], v[1]));
                let e = base.add(Pt::new(v[2], v[3]));
                if pts.is_empty() {
                    pts.push(cur);
                }
                pts.extend(quad_points(cur, c, e));
                last_qctrl = Some(c);
                was_quad = true;
                cur = e;
            }
            b'T' => {
                let v: Vec<f64> = (0..2).filter_map(|_| lx.number()).collect();
                if v.len() < 2 {
                    break;
                }
                let c = match last_qctrl {
                    Some(lc) => cur.add(cur.sub(lc)),
                    None => cur,
                };
                let e = base.add(Pt::new(v[0], v[1]));
                if pts.is_empty() {
                    pts.push(cur);
                }
                pts.extend(quad_points(cur, c, e));
                last_qctrl = Some(c);
                was_quad = true;
                cur = e;
            }
            b'A' => {
                let (Some(rx), Some(ry), Some(rot)) = (lx.number(), lx.number(), lx.number()) else { break };
                let (Some(large), Some(sweep)) = (lx.flag(), lx.flag()) else { break };
                let (Some(x), Some(y)) = (lx.number(), lx.number()) else { break };
                let e = base.add(Pt::new(x, y));
                if pts.is_empty() {
                    pts.push(cur);
                }
                pts.extend(svg_arc(cur, rx, ry, rot, large, sweep, e));
                cur = e;
            }
            b'Z' => {
                cur = start;
                flush(&mut pts, true, &mut out);
                pts.push(cur);
                // After Z a new subpath implicitly starts at `start`; skip numbers that would loop.
                if lx.peek_cmd().is_none() && !lx.at_end() {
                    break;
                }
            }
            _ => {
                // unknown command: stop parsing this path
                break;
            }
        }
        if !was_cubic {
            last_ctrl = None;
        }
        if !was_quad {
            last_qctrl = None;
        }
    }
    flush(&mut pts, false, &mut out);
    out
}

/// Endpoint-parameterised SVG arc to points (excluding the start).
fn svg_arc(p0: Pt, rx: f64, ry: f64, rot_deg: f64, large: bool, sweep: bool, p1: Pt) -> Vec<Pt> {
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-9 || ry < 1e-9 || p0.dist(p1) < 1e-12 {
        return vec![p1];
    }
    let phi = rot_deg.to_radians();
    let (sp, cp) = phi.sin_cos();
    let dx = (p0.x - p1.x) / 2.0;
    let dy = (p0.y - p1.y) / 2.0;
    let x1 = cp * dx + sp * dy;
    let y1 = -sp * dx + cp * dy;
    let lam = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lam > 1.0 {
        let s = lam.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut co = (num / den).max(0.0).sqrt();
    if large == sweep {
        co = -co;
    }
    let cxp = co * rx * y1 / ry;
    let cyp = -co * ry * x1 / rx;
    let cx = cp * cxp - sp * cyp + (p0.x + p1.x) / 2.0;
    let cy = sp * cxp + cp * cyp + (p0.y + p1.y) / 2.0;
    let ang = |ux: f64, uy: f64, vx: f64, vy: f64| {
        let dot = ux * vx + uy * vy;
        let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
        let mut a = (dot / len).clamp(-1.0, 1.0).acos();
        if ux * vy - uy * vx < 0.0 {
            a = -a;
        }
        a
    };
    let t1 = ang(1.0, 0.0, (x1 - cxp) / rx, (y1 - cyp) / ry);
    let mut dt = ang((x1 - cxp) / rx, (y1 - cyp) / ry, (-x1 - cxp) / rx, (-y1 - cyp) / ry);
    if !sweep && dt > 0.0 {
        dt -= std::f64::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f64::consts::TAU;
    }
    let r = rx.max(ry);
    let n = ((dt.abs() / (2.0 * (1.0 - (TOLERANCE / r).min(1.0)).acos()).max(0.01)).ceil() as usize).clamp(4, 2000);
    let mut v: Vec<Pt> = (1..=n)
        .map(|i| {
            let t = t1 + dt * i as f64 / n as f64;
            let (x, y) = (rx * t.cos(), ry * t.sin());
            Pt::new(cp * x - sp * y + cx, sp * x + cp * y + cy)
        })
        .collect();
    if let Some(l) = v.last_mut() {
        *l = p1;
    }
    v
}
