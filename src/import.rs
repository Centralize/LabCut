//! Shared import types and the file-type dispatch.

use crate::geom::Path;
use std::path::Path as FsPath;

/// What an importer returns: shapes plus the layers they sit on.
#[derive(Default)]
pub struct Drawing {
    pub paths: Vec<Path>,
    pub layers: Vec<(String, [u8; 3])>,
    pub notes: Vec<String>,
}

impl Drawing {
    pub fn layer_index(&mut self, name: &str, color: [u8; 3]) -> usize {
        if let Some(i) = self.layers.iter().position(|(n, _)| n == name) {
            return i;
        }
        self.layers.push((name.to_string(), color));
        self.layers.len() - 1
    }
}

pub enum Loaded {
    Drawing(Drawing),
    GCode(String),
}

pub fn load_file(path: &FsPath) -> Result<Loaded, String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't open the file: {e}"))?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    match ext.as_str() {
        "dxf" => crate::dxf::parse(&text).map(Loaded::Drawing),
        "svg" => crate::svg::parse(&text).map(Loaded::Drawing),
        "gcode" | "nc" | "gc" | "ngc" | "tap" | "cnc" | "g" => Ok(Loaded::GCode(text)),
        _ => {
            // Guess from the content.
            let head = text.trim_start();
            if head.starts_with("<?xml") || head.starts_with("<svg") {
                crate::svg::parse(&text).map(Loaded::Drawing)
            } else if head.starts_with('0') {
                crate::dxf::parse(&text).map(Loaded::Drawing)
            } else {
                Err(format!("LabCut can't read .{ext} files. Try DXF, SVG or G-code."))
            }
        }
    }
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8]
}
