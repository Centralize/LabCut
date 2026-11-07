#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod demo;
mod dxf;
mod gcode;
mod geom;
mod import;
mod project;
mod svg;

use std::path::PathBuf;

pub struct Options {
    pub screenshot: Option<PathBuf>,
    pub open: Option<PathBuf>,
    pub tab: Option<String>,
    pub progress: Option<f64>,
}

fn parse_args() -> Options {
    let mut o = Options { screenshot: None, open: None, tab: None, progress: None };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--screenshot" => o.screenshot = args.next().map(PathBuf::from),
            "--tab" => o.tab = args.next(),
            "--export" => {
                args.next();
            }
            "--progress" => o.progress = args.next().and_then(|v| v.parse().ok()),
            _ if !a.starts_with("--") => o.open = Some(PathBuf::from(a)),
            _ => {}
        }
    }
    o
}

/// `labcut drawing.dxf --export out.gcode`: convert without opening the window.
fn export(input: &std::path::Path, out: &std::path::Path) -> Result<String, String> {
    let mut p = project::Project::empty();
    p.name = input.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    match import::load_file(input)? {
        import::Loaded::Drawing(d) => p.add_drawing(d, true),
        import::Loaded::GCode(_) => return Err("The input is already G-code.".into()),
    }
    let code = gcode::generate(&p);
    std::fs::write(out, &code).map_err(|e| e.to_string())?;
    let job = gcode::parse(&code, p.machine.max_s, p.machine.travel_speed);
    let b = p.placed_bbox();
    Ok(format!(
        "{} shapes, {} layers, {:.1} x {:.1} mm, {} lines, about {}",
        p.paths.len(),
        p.layers.len(),
        b.width(),
        b.height(),
        code.lines().count(),
        gcode::format_time(job.total_time)
    ))
}

/// The Windows release build has no console window; reuse the one it was started from, if any.
#[cfg(windows)]
fn attach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    unsafe {
        AttachConsole(u32::MAX);
    }
}
#[cfg(not(windows))]
fn attach_console() {}

const HELP: &str = "LabCut - laser G-code maker

Usage:
  labcut                          open the app with the sample project
  labcut FILE                     open a DXF, SVG or G-code file
  labcut FILE --export OUT.gcode  convert a drawing to G-code without opening the window
  labcut --help                   show this help

Other options:
  --tab design|sim|code           start on this tab
  --progress 0.5                  start the simulator part-way (0 to 1)
  --screenshot FILE.png           draw 120 frames, save a picture of the window and exit";

fn main() -> eframe::Result<()> {
    let opts = parse_args();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        attach_console();
        println!("{HELP}");
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--export") {
        attach_console();
        if opts.open.is_none() || args.get(i + 1).is_none() {
            eprintln!("Usage: labcut FILE --export OUT.gcode");
            std::process::exit(2);
        }
        if let (Some(input), Some(out)) = (opts.open.as_ref(), args.get(i + 1)) {
            match export(input, std::path::Path::new(out)) {
                Ok(msg) => println!("{msg}"),
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
            return Ok(());
        }
    }
    let native = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("LabCut — laser G-code maker")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("LabCut", native, Box::new(move |cc| Ok(Box::new(app::LabCut::new(cc, opts)))))
}
