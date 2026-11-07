//! A small built-in sample: a name tag with an engraved star, scored rings and cut outline.

use crate::geom::*;
use crate::project::*;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn rounded_rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Vec<Pt> {
    let mut v = Vec::new();
    v.extend(arc_points(Pt::new(x + w - r, y + r), r, -FRAC_PI_2, FRAC_PI_2));
    v.extend(arc_points(Pt::new(x + w - r, y + h - r), r, 0.0, FRAC_PI_2));
    v.extend(arc_points(Pt::new(x + r, y + h - r), r, FRAC_PI_2, FRAC_PI_2));
    v.extend(arc_points(Pt::new(x + r, y + r), r, PI, FRAC_PI_2));
    v
}

fn star(c: Pt, r_out: f64, r_in: f64, points: usize) -> Vec<Pt> {
    (0..points * 2)
        .map(|i| {
            let a = FRAC_PI_2 + i as f64 * PI / points as f64;
            let r = if i % 2 == 0 { r_out } else { r_in };
            Pt::new(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

pub fn project() -> Project {
    let mut p = Project::empty();
    p.name = "Sample tag".into();

    let mut engrave = Layer::new("Engrave", [90, 200, 120]);
    engrave.mode = Mode::Fill;
    engrave.power = 35.0;
    engrave.speed = 2400.0;
    engrave.fill_spacing = 0.8;
    engrave.fill_angle = 0.0;

    let mut score = Layer::new("Score", [80, 150, 245]);
    score.power = 20.0;
    score.speed = 1500.0;

    let mut cut = Layer::new("Cut", [240, 85, 70]);
    cut.power = 90.0;
    cut.speed = 400.0;
    cut.passes = 2;

    p.layers = vec![engrave, score, cut];

    let (w, h) = (150.0, 100.0);
    // cut: outline, hanging holes and a small slot
    p.paths.push(Path::new(rounded_rect(0.0, 0.0, w, h, 10.0), true, 2));
    for (x, y) in [(12.0, 12.0), (w - 12.0, 12.0), (12.0, h - 12.0), (w - 12.0, h - 12.0)] {
        p.paths.push(Path::new(circle_points(Pt::new(x, y), 3.0), true, 2));
    }
    p.paths.push(Path::new(rounded_rect(w / 2.0 - 20.0, 8.0, 40.0, 6.0, 3.0), true, 2));

    // engrave: star and two smaller stars
    let c = Pt::new(w / 2.0, h / 2.0 + 6.0);
    p.paths.push(Path::new(star(c, 26.0, 11.0, 5), true, 0));
    p.paths.push(Path::new(star(Pt::new(24.0, h / 2.0 + 6.0), 9.0, 4.0, 5), true, 0));
    p.paths.push(Path::new(star(Pt::new(w - 24.0, h / 2.0 + 6.0), 9.0, 4.0, 5), true, 0));

    // score: rings around the star and a wave along the top
    p.paths.push(Path::new(circle_points(c, 30.0), true, 1));
    p.paths.push(Path::new(circle_points(c, 33.0), true, 1));
    let wave: Vec<Pt> = (0..=240)
        .map(|i| {
            let t = i as f64 / 240.0;
            Pt::new(22.0 + t * (w - 44.0), h - 5.5 + 1.8 * (t * TAU * 6.0).sin())
        })
        .collect();
    p.paths.push(Path::new(wave, false, 1));
    p
}
