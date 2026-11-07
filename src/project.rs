//! The project: shapes, layers with laser settings, placement and machine settings.

use crate::geom::*;
use crate::import::Drawing;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Follow the outline (cutting or scoring).
    Line,
    /// Fill closed shapes with back-and-forth lines (engraving).
    Fill,
    /// Don't burn this layer.
    Off,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Line => "Line (cut / score)",
            Mode::Fill => "Fill (engrave)",
            Mode::Off => "Off",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Mode::Line => "Line",
            Mode::Fill => "Fill",
            Mode::Off => "Off",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,
    pub color: [u8; 3],
    pub mode: Mode,
    /// Laser power in percent of the machine's maximum.
    pub power: f32,
    /// Feed speed in mm/min.
    pub speed: f32,
    pub passes: u32,
    /// Fill line spacing in mm.
    pub fill_spacing: f32,
    /// Fill line angle in degrees.
    pub fill_angle: f32,
    pub crosshatch: bool,
    /// Shown on the canvas.
    pub visible: bool,
}

impl Layer {
    pub fn new(name: &str, color: [u8; 3]) -> Self {
        Self {
            name: name.to_string(),
            color,
            mode: Mode::Line,
            power: 70.0,
            speed: 600.0,
            passes: 1,
            fill_spacing: 0.2,
            fill_angle: 0.0,
            crosshatch: false,
            visible: true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaserCmd {
    /// GRBL laser mode: M4 with power on each cutting move (power scales with speed).
    M4Dynamic,
    /// M3 on / M5 off around each shape (constant power, works on most machines).
    M3Constant,
}

#[derive(Clone, Debug)]
pub struct Machine {
    pub bed_w: f64,
    pub bed_h: f64,
    /// S value meaning 100 % power (GRBL default 1000, Marlin 255).
    pub max_s: f32,
    pub laser_cmd: LaserCmd,
    pub travel_speed: f32,
    pub air_assist: bool,
    pub return_home: bool,
    pub optimize: bool,
    pub inside_first: bool,
    pub decimals: usize,
    pub header: String,
    pub footer: String,
}

impl Default for Machine {
    fn default() -> Self {
        Self {
            bed_w: 400.0,
            bed_h: 300.0,
            max_s: 1000.0,
            laser_cmd: LaserCmd::M4Dynamic,
            travel_speed: 3000.0,
            air_assist: false,
            return_home: true,
            optimize: true,
            inside_first: true,
            decimals: 3,
            header: String::new(),
            footer: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    /// Percent.
    pub scale: f64,
    pub mirror_x: bool,
    pub mirror_y: bool,
    /// Quarter turns counter-clockwise.
    pub rotate: u8,
}

impl Default for Placement {
    fn default() -> Self {
        Self { x: 10.0, y: 10.0, scale: 100.0, mirror_x: false, mirror_y: false, rotate: 0 }
    }
}

pub struct Project {
    pub name: String,
    /// Shapes in drawing coordinates, with their lower-left corner at 0,0.
    pub paths: Vec<Path>,
    pub layers: Vec<Layer>,
    pub place: Placement,
    pub machine: Machine,
}

impl Project {
    pub fn empty() -> Self {
        Self { name: "Untitled".into(), paths: vec![], layers: vec![], place: Placement::default(), machine: Machine::default() }
    }

    /// Size of the drawing before scaling.
    pub fn raw_bbox(&self) -> BBox {
        bbox_of(&self.paths)
    }

    /// Transform from drawing coordinates to the machine bed.
    pub fn transform(&self) -> Affine {
        let b = self.raw_bbox();
        if b.is_empty() {
            return Affine::IDENTITY;
        }
        let s = self.place.scale / 100.0;
        let c = Pt::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0);
        let mut t = Affine::translate(-c.x, -c.y)
            .then(&Affine::scale(if self.place.mirror_x { -s } else { s }, if self.place.mirror_y { -s } else { s }))
            .then(&Affine::rotate(self.place.rotate as f64 * std::f64::consts::FRAC_PI_2));
        // move so the lower-left corner sits at the placement point
        let mut tb = BBox::empty();
        for p in [b.min, b.max, Pt::new(b.min.x, b.max.y), Pt::new(b.max.x, b.min.y)] {
            tb.add(t.apply(p));
        }
        t = t.then(&Affine::translate(self.place.x - tb.min.x, self.place.y - tb.min.y));
        t
    }

    /// Shapes in machine coordinates.
    pub fn placed_paths(&self) -> Vec<Path> {
        let t = self.transform();
        self.paths
            .iter()
            .map(|p| Path { pts: p.pts.iter().map(|q| t.apply(*q)).collect(), closed: p.closed, layer: p.layer })
            .collect()
    }

    pub fn placed_bbox(&self) -> BBox {
        bbox_of(&self.placed_paths())
    }

    /// Add an imported drawing. If `replace`, the project starts over with it.
    pub fn add_drawing(&mut self, d: Drawing, replace: bool) {
        if replace {
            self.paths.clear();
            self.layers.clear();
            self.place = Placement { x: self.place.x, y: self.place.y, ..Default::default() };
        }
        let b = bbox_of(&d.paths);
        let shift_x = if replace || self.paths.is_empty() { 0.0 } else { self.raw_bbox().max.x + 10.0 - b.min.x };
        let base = if replace || self.paths.is_empty() { Pt::new(-b.min.x, -b.min.y) } else { Pt::new(shift_x, self.raw_bbox().min.y - b.min.y) };
        let map: Vec<usize> = d.layers.iter().map(|(n, c)| self.layer_for(n, *c)).collect();
        for mut p in d.paths {
            for q in p.pts.iter_mut() {
                *q = q.add(base);
            }
            p.layer = map.get(p.layer).copied().unwrap_or(0);
            self.paths.push(p);
        }
        if self.layers.is_empty() {
            self.layers.push(Layer::new("Layer 1", [225, 225, 225]));
        }
        if replace {
            self.sort_layers_outer_last();
        }
    }

    /// Order layers so the one holding the biggest shape (usually the outline) is cut last.
    pub fn sort_layers_outer_last(&mut self) {
        let n = self.layers.len();
        let mut biggest = vec![0.0f64; n];
        for p in &self.paths {
            let b = p.bbox();
            let a = b.width() * b.height();
            if p.layer < n && a > biggest[p.layer] {
                biggest[p.layer] = a;
            }
        }
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|a, b| biggest[*a].partial_cmp(&biggest[*b]).unwrap_or(std::cmp::Ordering::Equal));
        let mut new_index = vec![0; n];
        for (new, old) in order.iter().enumerate() {
            new_index[*old] = new;
        }
        let old_layers = std::mem::take(&mut self.layers);
        let mut slots: Vec<Option<Layer>> = old_layers.into_iter().map(Some).collect();
        self.layers = order.iter().map(|o| slots[*o].take().unwrap()).collect();
        for p in self.paths.iter_mut() {
            if p.layer < n {
                p.layer = new_index[p.layer];
            }
        }
    }

    pub fn layer_for(&mut self, name: &str, color: [u8; 3]) -> usize {
        if let Some(i) = self.layers.iter().position(|l| l.name == name) {
            return i;
        }
        self.layers.push(Layer::new(name, color));
        self.layers.len() - 1
    }

    pub fn add_layer(&mut self) -> usize {
        let n = self.layers.len();
        let color = crate::import::hsv_to_rgb(n as f32 * 67.0 + 20.0, 0.75, 0.95);
        let mut i = n + 1;
        while self.layers.iter().any(|l| l.name == format!("Layer {i}")) {
            i += 1;
        }
        self.layers.push(Layer::new(&format!("Layer {i}"), color));
        n
    }

    /// Remove empty layers that nothing uses, keeping at least one.
    pub fn remove_layer(&mut self, idx: usize) {
        if self.layers.len() <= 1 || idx >= self.layers.len() {
            return;
        }
        let target = if idx == 0 { 1 } else { 0 };
        for p in self.paths.iter_mut() {
            if p.layer == idx {
                p.layer = target;
            }
        }
        self.layers.remove(idx);
        for p in self.paths.iter_mut() {
            if p.layer > idx {
                p.layer -= 1;
            }
        }
    }

    pub fn swap_layers(&mut self, a: usize, b: usize) {
        if a >= self.layers.len() || b >= self.layers.len() {
            return;
        }
        self.layers.swap(a, b);
        for p in self.paths.iter_mut() {
            if p.layer == a {
                p.layer = b;
            } else if p.layer == b {
                p.layer = a;
            }
        }
    }

    pub fn layer_shape_count(&self, idx: usize) -> usize {
        self.paths.iter().filter(|p| p.layer == idx).count()
    }
}
