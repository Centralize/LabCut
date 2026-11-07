//! G-code writing (from a project) and reading (for the simulator).

use crate::geom::*;
use crate::project::*;
use std::fmt::Write as _;

/// One straight machine move, used by the preview and simulator.
#[derive(Clone, Debug)]
pub struct Move {
    pub from: Pt,
    pub to: Pt,
    /// Laser firing during this move.
    pub laser: bool,
    /// 0..1 of the machine's maximum.
    pub power: f32,
    /// mm/min
    #[allow(dead_code)]
    pub feed: f32,
    /// Layer index (from the `; LAYER n` comments), if known.
    pub layer: Option<usize>,
    /// Seconds from the start of the job when this move begins.
    pub t0: f64,
    pub dur: f64,
}

pub struct Job {
    pub moves: Vec<Move>,
    pub total_time: f64,
    pub cut_length: f64,
    pub travel_length: f64,
    pub line_count: usize,
    pub bbox: BBox,
}

impl Job {
    pub fn empty() -> Self {
        Job { moves: vec![], total_time: 0.0, cut_length: 0.0, travel_length: 0.0, line_count: 0, bbox: BBox::empty() }
    }
    /// Index of the move running at time `t`.
    pub fn index_at(&self, t: f64) -> usize {
        match self.moves.binary_search_by(|m| m.t0.partial_cmp(&t).unwrap_or(std::cmp::Ordering::Less)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }
}

struct Writer {
    out: String,
    dec: usize,
    laser_on: bool,
    feed: f32,
    power_s: f32,
    cmd: LaserCmd,
    pos: Option<Pt>,
}

impl Writer {
    fn num(&self, v: f64) -> String {
        let s = format!("{:.*}", self.dec, v);
        let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
        if s == "-0" { "0".into() } else { s }
    }
    fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }
    fn off(&mut self) {
        if self.laser_on && self.cmd == LaserCmd::M3Constant {
            self.line("M5");
        }
        self.laser_on = false;
    }
    fn travel(&mut self, p: Pt) {
        if let Some(cur) = self.pos {
            if cur.dist(p) < 1e-6 {
                return;
            }
        }
        self.off();
        let l = format!("G0 X{} Y{}", self.num(p.x), self.num(p.y));
        self.line(&l);
        self.pos = Some(p);
    }
    fn cut(&mut self, p: Pt, s: f32, feed: f32) {
        let mut l = format!("G1 X{} Y{}", self.num(p.x), self.num(p.y));
        match self.cmd {
            LaserCmd::M3Constant => {
                if !self.laser_on || (s - self.power_s).abs() > 0.01 {
                    let on = format!("M3 S{}", self.num(s as f64));
                    self.line(&on);
                    self.laser_on = true;
                    self.power_s = s;
                }
            }
            LaserCmd::M4Dynamic => {
                if !self.laser_on || (s - self.power_s).abs() > 0.01 {
                    let _ = write!(l, " S{}", self.num(s as f64));
                    self.power_s = s;
                }
                self.laser_on = true;
            }
        }
        if (feed - self.feed).abs() > 0.01 {
            let _ = write!(l, " F{}", self.num(feed as f64));
            self.feed = feed;
        }
        self.line(&l);
        self.pos = Some(p);
    }
}

/// Make the G-code text for a project.
pub fn generate(project: &Project) -> String {
    let m = &project.machine;
    let placed = project.placed_paths();
    let bb = bbox_of(&placed);
    let mut w = Writer { out: String::new(), dec: m.decimals, laser_on: false, feed: -1.0, power_s: -1.0, cmd: m.laser_cmd, pos: None };
    w.line("; Made with LabCut");
    w.line(&format!("; Project: {}", project.name));
    if !bb.is_empty() {
        w.line(&format!(
            "; Job area: X{:.2}..{:.2} Y{:.2}..{:.2} mm ({:.1} x {:.1} mm)",
            bb.min.x, bb.max.x, bb.min.y, bb.max.y, bb.width(), bb.height()
        ));
    }
    w.line("G21 ; millimetres");
    w.line("G90 ; absolute positions");
    for l in m.header.lines().filter(|l| !l.trim().is_empty()) {
        w.line(l.trim());
    }
    w.line("M5");
    if m.air_assist {
        w.line("M8 ; air assist on");
    }
    if m.laser_cmd == LaserCmd::M4Dynamic {
        w.line("M4 S0 ; laser in dynamic power mode");
    }
    let mut cursor = Pt::new(0.0, 0.0);
    for (li, layer) in project.layers.iter().enumerate() {
        if layer.mode == Mode::Off {
            continue;
        }
        let paths: Vec<&Path> = placed.iter().filter(|p| p.layer == li).collect();
        if paths.is_empty() {
            continue;
        }
        let s = (layer.power.clamp(0.0, 100.0) / 100.0) * m.max_s;
        let feed = layer.speed.max(1.0);
        let strokes: Vec<Vec<Pt>> = match layer.mode {
            Mode::Line => order_paths(&paths, cursor, m.optimize, m.inside_first),
            Mode::Fill => {
                let mut v = hatch(&paths, layer.fill_spacing.max(0.01) as f64, layer.fill_angle as f64);
                if layer.crosshatch {
                    v.extend(hatch(&paths, layer.fill_spacing.max(0.01) as f64, layer.fill_angle as f64 + 90.0));
                }
                v
            }
            Mode::Off => vec![],
        };
        w.line("");
        w.line(&format!(
            "; LAYER {li}: {} | {} | power {:.0}% | speed {:.0} mm/min | passes {}",
            layer.name,
            layer.mode.short(),
            layer.power,
            layer.speed,
            layer.passes.max(1)
        ));
        for pass in 0..layer.passes.max(1) {
            if layer.passes > 1 {
                w.line(&format!("; pass {}", pass + 1));
            }
            for st in &strokes {
                if st.len() < 2 {
                    continue;
                }
                w.travel(st[0]);
                for p in &st[1..] {
                    w.cut(*p, s, feed);
                }
                cursor = *st.last().unwrap();
            }
            w.off();
        }
    }
    w.line("");
    w.off();
    w.line("M5 ; laser off");
    if m.air_assist {
        w.line("M9 ; air assist off");
    }
    if m.return_home {
        w.line("G0 X0 Y0");
    }
    for l in m.footer.lines().filter(|l| !l.trim().is_empty()) {
        w.line(l.trim());
    }
    w.line("M2 ; end of program");
    w.out
}

/// Choose a cutting order: shapes inside others first, then nearest next shape to cut travel short.
pub fn order_paths(paths: &[&Path], start: Pt, optimize: bool, inside_first: bool) -> Vec<Vec<Pt>> {
    let n = paths.len();
    if !optimize {
        return paths.iter().map(|p| p.cut_points()).collect();
    }
    let boxes: Vec<BBox> = paths.iter().map(|p| p.bbox()).collect();
    // parents[j] = closed shapes that contain shape j
    let mut pending = vec![0usize; n];
    let mut parents: Vec<Vec<usize>> = vec![vec![]; n];
    if inside_first && n < 6000 {
        for i in 0..n {
            if !paths[i].closed {
                continue;
            }
            let area_i = boxes[i].width() * boxes[i].height();
            for j in 0..n {
                if i == j || !boxes[i].contains_box(&boxes[j]) {
                    continue;
                }
                let area_j = boxes[j].width() * boxes[j].height();
                if area_j >= area_i && j < i {
                    continue;
                }
                let probe = paths[j].pts[0];
                if point_in_polygon(probe, &paths[i].pts) {
                    parents[j].push(i);
                    pending[i] += 1;
                }
            }
        }
    }
    let total_pts: usize = paths.iter().map(|p| p.pts.len()).sum();
    let cheap = total_pts > 200_000;
    let mut done = vec![false; n];
    let mut out = Vec::with_capacity(n);
    let mut cur = start;
    while out.len() < n {
        let mut best: Option<(usize, usize, bool, f64)> = None; // (index, start vertex, reversed, dist)
        for i in 0..n {
            if done[i] || pending[i] > 0 {
                continue;
            }
            let p = paths[i];
            if p.closed && !cheap {
                // quick reject using the bounding box
                let b = &boxes[i];
                let dx = (b.min.x - cur.x).max(0.0).max(cur.x - b.max.x);
                let dy = (b.min.y - cur.y).max(0.0).max(cur.y - b.max.y);
                if let Some((_, _, _, bd)) = best {
                    if (dx * dx + dy * dy).sqrt() > bd {
                        continue;
                    }
                }
                for (k, q) in p.pts.iter().enumerate() {
                    let d = q.dist(cur);
                    if best.is_none_or(|b| d < b.3) {
                        best = Some((i, k, false, d));
                    }
                }
            } else {
                let d0 = p.pts[0].dist(cur);
                if best.is_none_or(|b| d0 < b.3) {
                    best = Some((i, 0, false, d0));
                }
                if !p.closed {
                    let d1 = p.pts.last().unwrap().dist(cur);
                    if best.is_none_or(|b| d1 < b.3) {
                        best = Some((i, 0, true, d1));
                    }
                }
            }
        }
        let Some((i, k, rev, _)) = best else {
            // a containment loop; release everything left
            for v in pending.iter_mut() {
                *v = 0;
            }
            continue;
        };
        done[i] = true;
        for &par in &parents[i] {
            pending[par] = pending[par].saturating_sub(1);
        }
        let p = paths[i];
        let mut pts = if p.closed {
            let mut v: Vec<Pt> = p.pts[k..].iter().chain(p.pts[..k].iter()).copied().collect();
            v.push(v[0]);
            v
        } else {
            p.pts.clone()
        };
        if rev {
            pts.reverse();
        }
        cur = *pts.last().unwrap();
        out.push(pts);
    }
    out
}

/// Back-and-forth fill lines inside the closed shapes (even-odd rule).
pub fn hatch(paths: &[&Path], spacing: f64, angle_deg: f64) -> Vec<Vec<Pt>> {
    let rot = Affine::rotate(-angle_deg.to_radians());
    let back = Affine::rotate(angle_deg.to_radians());
    let polys: Vec<Vec<Pt>> = paths
        .iter()
        .filter(|p| p.closed && p.pts.len() >= 3)
        .map(|p| p.pts.iter().map(|q| rot.apply(*q)).collect())
        .collect();
    let mut bb = BBox::empty();
    for p in &polys {
        for q in p {
            bb.add(*q);
        }
    }
    if bb.is_empty() {
        return vec![];
    }
    let mut edges: Vec<(Pt, Pt)> = Vec::new();
    for p in &polys {
        for i in 0..p.len() {
            edges.push((p[i], p[(i + 1) % p.len()]));
        }
    }
    let mut out = Vec::new();
    let mut y = bb.min.y + spacing / 2.0;
    let mut flip = false;
    let max_rows = 200_000;
    let mut rows = 0;
    while y < bb.max.y && rows < max_rows {
        let mut xs: Vec<f64> = edges
            .iter()
            .filter(|(a, b)| (a.y > y) != (b.y > y))
            .map(|(a, b)| a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y))
            .collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut segs: Vec<(f64, f64)> = xs.chunks_exact(2).map(|c| (c[0], c[1])).filter(|(a, b)| b - a > 1e-6).collect();
        if flip {
            segs.reverse();
            for (a, b) in segs {
                out.push(vec![back.apply(Pt::new(b, y)), back.apply(Pt::new(a, y))]);
            }
        } else {
            for (a, b) in segs {
                out.push(vec![back.apply(Pt::new(a, y)), back.apply(Pt::new(b, y))]);
            }
        }
        flip = !flip;
        y += spacing;
        rows += 1;
    }
    out
}

/// Read G-code into moves for the simulator.
pub fn parse(text: &str, max_s: f32, travel_speed: f32) -> Job {
    let mut moves = Vec::new();
    let mut pos = Pt::new(0.0, 0.0);
    let mut absolute = true;
    let mut unit = 1.0;
    let mut motion = 0; // 0,1,2,3
    let mut feed = 1000.0f32;
    let mut s = 0.0f32;
    let mut laser_on = false;
    let mut layer: Option<usize> = None;
    let mut t = 0.0f64;
    let mut cut_len = 0.0;
    let mut travel_len = 0.0;
    let mut bbox = BBox::empty();
    let mut lines = 0;
    // find the largest S in the file when it is larger than the machine setting
    let max_s = max_s.max(1.0);

    for raw in text.lines() {
        lines += 1;
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("; LAYER ") {
            layer = rest.split(':').next().and_then(|n| n.trim().parse().ok());
            continue;
        }
        // strip comments
        let mut code = String::with_capacity(line.len());
        let mut in_paren = false;
        for ch in line.chars() {
            match ch {
                ';' if !in_paren => break,
                '(' => in_paren = true,
                ')' => in_paren = false,
                c if !in_paren => code.push(c.to_ascii_uppercase()),
                _ => {}
            }
        }
        if code.trim().is_empty() {
            continue;
        }
        // tokenise into letter/value words
        let mut words: Vec<(char, f64)> = Vec::new();
        let b = code.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let c = b[i] as char;
            if c.is_ascii_alphabetic() {
                let mut j = i + 1;
                while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
                    j += 1;
                }
                let st = j;
                while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.' || b[j] == b'-' || b[j] == b'+') {
                    j += 1;
                }
                if let Ok(v) = code[st..j].parse::<f64>() {
                    words.push((c, v));
                }
                i = j.max(i + 1);
            } else {
                i += 1;
            }
        }
        let (mut x, mut y, mut ii, mut jj, mut rr) = (None, None, None, None, None);
        let mut has_motion_word = false;
        for (c, v) in &words {
            match c {
                'G' => match (*v * 10.0).round() as i64 {
                    0 => { motion = 0; has_motion_word = true; }
                    10 => { motion = 1; has_motion_word = true; }
                    20 => { motion = 2; has_motion_word = true; }
                    30 => { motion = 3; has_motion_word = true; }
                    200 => unit = 25.4,
                    210 => unit = 1.0,
                    900 => absolute = true,
                    910 => absolute = false,
                    _ => {}
                },
                'M' => match *v as i64 {
                    3 | 4 => laser_on = true,
                    5 | 2 | 30 => laser_on = false,
                    _ => {}
                },
                'S' => s = *v as f32,
                'F' => feed = (*v * unit) as f32,
                'X' => x = Some(*v * unit),
                'Y' => y = Some(*v * unit),
                'I' => ii = Some(*v * unit),
                'J' => jj = Some(*v * unit),
                'R' => rr = Some(*v * unit),
                _ => {}
            }
        }
        let _ = has_motion_word;
        if x.is_none() && y.is_none() {
            continue;
        }
        let target = if absolute {
            Pt::new(x.unwrap_or(pos.x), y.unwrap_or(pos.y))
        } else {
            Pt::new(pos.x + x.unwrap_or(0.0), pos.y + y.unwrap_or(0.0))
        };
        let firing = motion != 0 && laser_on && s > 0.0;
        let speed = if motion == 0 { travel_speed } else { feed.max(1.0) };
        let pts: Vec<Pt> = if motion == 2 || motion == 3 {
            let cw = motion == 2;
            let centre = if let (Some(i), Some(j)) = (ii, jj) {
                Some(pos.add(Pt::new(i, j)))
            } else if ii.is_some() || jj.is_some() {
                Some(pos.add(Pt::new(ii.unwrap_or(0.0), jj.unwrap_or(0.0))))
            } else if let Some(r) = rr {
                arc_centre_from_radius(pos, target, r, cw)
            } else {
                None
            };
            match centre {
                Some(c) => {
                    let r = c.dist(pos);
                    let a0 = (pos.y - c.y).atan2(pos.x - c.x);
                    let a1 = (target.y - c.y).atan2(target.x - c.x);
                    let mut sweep = a1 - a0;
                    if cw {
                        while sweep >= -1e-9 { sweep -= std::f64::consts::TAU; }
                    } else {
                        while sweep <= 1e-9 { sweep += std::f64::consts::TAU; }
                    }
                    if pos.dist(target) > 1e-9 && (sweep.abs() - std::f64::consts::TAU).abs() < 1e-6 {
                        sweep = 0.0;
                    }
                    let mut v = arc_points(c, r, a0, sweep);
                    v.remove(0);
                    if let Some(l) = v.last_mut() { *l = target; }
                    v
                }
                None => vec![target],
            }
        } else {
            vec![target]
        };
        for p in pts {
            let len = pos.dist(p);
            if len < 1e-9 {
                continue;
            }
            let dur = len / speed as f64 * 60.0;
            if firing {
                cut_len += len;
                bbox.add(pos);
                bbox.add(p);
            } else {
                travel_len += len;
            }
            moves.push(Move {
                from: pos,
                to: p,
                laser: firing,
                power: (s / max_s).clamp(0.0, 1.0),
                feed: speed,
                layer,
                t0: t,
                dur,
            });
            t += dur;
            pos = p;
        }
    }
    Job { moves, total_time: t, cut_length: cut_len, travel_length: travel_len, line_count: lines, bbox }
}

fn arc_centre_from_radius(a: Pt, b: Pt, r: f64, cw: bool) -> Option<Pt> {
    let d = a.dist(b);
    if d < 1e-9 || d > 2.0 * r.abs() + 1e-6 {
        return None;
    }
    let h = (r * r - (d / 2.0).powi(2)).max(0.0).sqrt();
    let mid = a.lerp(b, 0.5);
    let dir = b.sub(a).scale(1.0 / d);
    let left = Pt::new(-dir.y, dir.x);
    // positive R = the shorter arc
    let side = if cw == (r > 0.0) { -1.0 } else { 1.0 };
    Some(mid.add(left.scale(h * side)))
}

pub fn format_time(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}
