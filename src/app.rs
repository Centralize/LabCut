//! The LabCut window.

use crate::gcode::{self, format_time, Job};
use crate::geom::*;
use crate::import::{self, Loaded};
use crate::project::*;
use crate::Options;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Design,
    Simulate,
    Code,
}

struct View {
    center: Pt,
    zoom: f32,
    needs_fit: bool,
}

struct Sim {
    t: f64,
    playing: bool,
    speed: f32,
    show_travel: bool,
}

struct Shot {
    path: PathBuf,
    frames: u32,
    requested: bool,
    result: Arc<Mutex<Option<Result<(), String>>>>,
}

pub struct LabCut {
    project: Project,
    /// Shapes on the bed (cached).
    placed: Vec<Path>,
    gcode: String,
    line_starts: Vec<usize>,
    job: Job,
    signature: String,
    rev: u64,
    /// The job came from an opened G-code file, not from the design.
    external: Option<String>,
    tab: Tab,
    view: View,
    selected: HashSet<usize>,
    sel_layer: usize,
    sim: Sim,
    status: String,
    status_warn: bool,
    shot: Option<Shot>,
}

const BG: Color32 = Color32::from_rgb(24, 27, 32);
const BED: Color32 = Color32::from_rgb(34, 38, 45);
const ACCENT: Color32 = Color32::from_rgb(255, 140, 60);

fn col(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}
fn col_a(c: [u8; 3], a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a)
}

impl LabCut {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> Self {
        let ctx = &cc.egui_ctx;
        ctx.set_theme(egui::Theme::Dark);
        ctx.style_mut_of(egui::Theme::Dark,|s| {
            s.spacing.item_spacing = egui::vec2(8.0, 6.0);
            s.spacing.button_padding = egui::vec2(8.0, 4.0);
            s.visuals.selection.bg_fill = Color32::from_rgb(200, 100, 40);
            s.visuals.hyperlink_color = ACCENT;
        });
        let mut app = LabCut {
            project: crate::demo::project(),
            placed: vec![],
            gcode: String::new(),
            line_starts: vec![],
            job: Job::empty(),
            signature: String::new(),
            rev: 0,
            external: None,
            tab: Tab::Design,
            view: View { center: Pt::new(0.0, 0.0), zoom: 2.0, needs_fit: true },
            selected: HashSet::new(),
            sel_layer: 0,
            sim: Sim { t: 0.0, playing: false, speed: 10.0, show_travel: true },
            status: "Sample project loaded. Open a DXF or SVG file to start your own.".into(),
            status_warn: false,
            shot: opts.screenshot.map(|path| Shot { path, frames: 0, requested: false, result: Arc::new(Mutex::new(None)) }),
        };
        if let Some(p) = opts.open {
            app.open_path(&p, true);
        }
        app.refresh();
        match opts.tab.as_deref() {
            Some("sim") | Some("simulate") => app.tab = Tab::Simulate,
            Some("code") | Some("gcode") => app.tab = Tab::Code,
            _ => {}
        }
        if let Some(f) = opts.progress {
            app.sim.t = app.job.total_time * f.clamp(0.0, 1.0);
        }
        app
    }

    fn set_status(&mut self, s: impl Into<String>, warn: bool) {
        self.status = s.into();
        self.status_warn = warn;
    }

    /// Re-make the G-code when anything in the design or settings changed.
    fn refresh(&mut self) {
        if self.external.is_some() {
            return;
        }
        let sig = format!("{:?}|{:?}|{:?}|{}", self.project.layers, self.project.place, self.project.machine, self.rev);
        if sig == self.signature {
            return;
        }
        self.signature = sig;
        self.placed = self.project.placed_paths();
        self.gcode = gcode::generate(&self.project);
        self.index_lines();
        let m = &self.project.machine;
        self.job = gcode::parse(&self.gcode, m.max_s, m.travel_speed);
        self.sim.t = self.sim.t.min(self.job.total_time);
    }

    fn index_lines(&mut self) {
        self.line_starts.clear();
        self.line_starts.push(0);
        for (i, b) in self.gcode.bytes().enumerate() {
            if b == b'\n' && i + 1 < self.gcode.len() {
                self.line_starts.push(i + 1);
            }
        }
    }

    fn open_path(&mut self, path: &std::path::Path, replace: bool) {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        match import::load_file(path) {
            Ok(Loaded::Drawing(d)) => {
                let notes = d.notes.join(" ");
                let n = d.paths.len();
                let layers = d.layers.len();
                if self.external.is_some() {
                    self.external = None;
                    self.project.paths.clear();
                    self.project.layers.clear();
                }
                self.project.add_drawing(d, replace || self.project.paths.is_empty());
                if replace {
                    self.project.name = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                    self.sel_layer = 0;
                }
                self.selected.clear();
                self.rev += 1;
                self.signature.clear();
                self.view.needs_fit = true;
                self.sim.t = 0.0;
                self.sim.playing = false;
                let b = self.project.placed_bbox();
                self.set_status(
                    format!("Opened {name}: {n} shape(s) on {layers} layer(s), {:.1} × {:.1} mm. {notes}", b.width(), b.height()),
                    false,
                );
            }
            Ok(Loaded::GCode(text)) => {
                let m = &self.project.machine;
                let job = gcode::parse(&text, m.max_s, m.travel_speed);
                if job.moves.is_empty() {
                    self.set_status(format!("{name} has no moves to show."), true);
                    return;
                }
                self.set_status(
                    format!("Opened G-code {name}: {} lines, about {} to run. Press play to simulate.", job.line_count, format_time(job.total_time)),
                    false,
                );
                self.gcode = text;
                self.index_lines();
                self.job = job;
                self.external = Some(name);
                self.selected.clear();
                self.view.needs_fit = true;
                self.sim.t = 0.0;
                self.sim.playing = false;
                self.tab = Tab::Simulate;
            }
            Err(e) => self.set_status(format!("{name}: {e}"), true),
        }
    }

    fn open_dialog(&mut self, replace: bool) {
        let dlg = rfd::FileDialog::new()
            .set_title(if replace { "Open a drawing or G-code file" } else { "Add a drawing to this project" })
            .add_filter("Drawings and G-code", &["dxf", "svg", "gcode", "nc", "gc", "ngc", "tap"])
            .add_filter("DXF drawing", &["dxf"])
            .add_filter("SVG drawing", &["svg"])
            .add_filter("G-code", &["gcode", "nc", "gc", "ngc", "tap"])
            .add_filter("All files", &["*"]);
        if let Some(p) = dlg.pick_file() {
            self.open_path(&p, replace);
        }
    }

    fn save_dialog(&mut self) {
        if self.job.moves.is_empty() {
            self.set_status("There's nothing to cut yet: every layer is off or empty.", true);
            return;
        }
        let name = format!("{}.gcode", if self.project.name.is_empty() { "job" } else { &self.project.name });
        let dlg = rfd::FileDialog::new()
            .set_title("Save G-code")
            .set_file_name(&name)
            .add_filter("G-code", &["gcode", "nc", "gc", "ngc"]);
        if let Some(p) = dlg.save_file() {
            match std::fs::write(&p, &self.gcode) {
                Ok(_) => self.set_status(format!("Saved {} ({} lines).", p.display(), self.line_starts.len()), false),
                Err(e) => self.set_status(format!("Couldn't save: {e}"), true),
            }
        }
    }

    fn load_sample(&mut self) {
        let machine = self.project.machine.clone();
        self.project = crate::demo::project();
        self.project.machine = machine;
        self.external = None;
        self.selected.clear();
        self.sel_layer = 0;
        self.rev += 1;
        self.signature.clear();
        self.view.needs_fit = true;
        self.sim.t = 0.0;
        self.set_status("Sample project loaded.", false);
    }

    // ---------------------------------------------------------------- panels

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("LabCut").strong().size(18.0).color(ACCENT));
            ui.separator();
            if ui.button("📂 Open…").on_hover_text("Open a DXF, SVG or G-code file (Ctrl+O)").clicked() {
                self.open_dialog(true);
            }
            if ui.button("➕ Add file…").on_hover_text("Add another drawing to this project").clicked() {
                self.open_dialog(false);
            }
            if ui.button("★ Sample").on_hover_text("Load the sample project").clicked() {
                self.load_sample();
            }
            let save = ui.add(egui::Button::new(egui::RichText::new("💾 Save G-code…").strong()).fill(Color32::from_rgb(150, 70, 25)));
            if save.on_hover_text("Write the G-code file for your laser (Ctrl+S)").clicked() {
                self.save_dialog();
            }
            ui.separator();
            for (t, label) in [(Tab::Design, "✏ Design"), (Tab::Simulate, "▶ Simulate"), (Tab::Code, "📄 G-code")] {
                if ui.selectable_label(self.tab == t, label).clicked() {
                    self.tab = t;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let j = &self.job;
                ui.label(
                    egui::RichText::new(format!(
                        "⏱ {}   ✂ {:.0} mm cut   ↔ {:.0} mm travel",
                        format_time(j.total_time),
                        j.cut_length,
                        j.travel_length
                    ))
                    .color(Color32::from_gray(190)),
                );
            });
        });
    }

    fn layers_panel(&mut self, ui: &mut egui::Ui) {
        if let Some(name) = &self.external {
            ui.heading("G-code file");
            ui.label(format!("Showing {name}."));
            ui.label("This file was made elsewhere, so it can be previewed and simulated but not edited.");
            ui.add_space(8.0);
            if ui.button("Back to my design").clicked() {
                self.external = None;
                self.signature.clear();
                self.tab = Tab::Design;
                self.view.needs_fit = true;
            }
            return;
        }
        ui.heading("Layers");
        ui.label(egui::RichText::new("Cut in order from top to bottom. Put outer cuts last.").small().color(Color32::from_gray(150)));
        ui.add_space(4.0);
        let counts: Vec<usize> = (0..self.project.layers.len()).map(|i| self.project.layer_shape_count(i)).collect();
        if self.sel_layer >= self.project.layers.len() {
            self.sel_layer = 0;
        }
        for (i, count) in counts.iter().enumerate() {
            let selected = self.sel_layer == i;
            let frame = egui::Frame::new()
                .fill(if selected { Color32::from_rgb(52, 46, 42) } else { Color32::from_rgb(36, 39, 45) })
                .corner_radius(5.0)
                .inner_margin(egui::Margin::symmetric(6, 4))
                .stroke(if selected { Stroke::new(1.0, ACCENT) } else { Stroke::NONE });
            let resp = frame
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let l = &mut self.project.layers[i];
                        let eye = if l.visible { "👁" } else { "–" };
                        if ui.small_button(eye).on_hover_text("Show or hide on the canvas").clicked() {
                            l.visible = !l.visible;
                        }
                        ui.color_edit_button_srgb(&mut l.color);
                        let name = egui::RichText::new(&l.name).strong().color(if l.mode == Mode::Off { Color32::GRAY } else { Color32::WHITE });
                        if ui.add(egui::Label::new(name).sense(Sense::click())).clicked() {
                            self.sel_layer = i;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let l = &self.project.layers[i];
                            let info = match l.mode {
                                Mode::Off => "off".to_string(),
                                _ => format!("{} {:.0}% {:.0} ×{}", l.mode.short(), l.power, l.speed, l.passes),
                            };
                            ui.label(egui::RichText::new(info).small().color(Color32::from_gray(170)));
                            ui.label(egui::RichText::new(format!("{count}")).small().color(Color32::from_gray(120)))
                                .on_hover_text("Shapes on this layer");
                        });
                    });
                })
                .response;
            if resp.interact(Sense::click()).clicked() {
                self.sel_layer = i;
            }
        }
        ui.horizontal(|ui| {
            if ui.button("➕ New layer").clicked() {
                self.sel_layer = self.project.add_layer();
            }
            let i = self.sel_layer;
            if ui.add_enabled(i > 0, egui::Button::new("⬆")).on_hover_text("Cut this layer earlier").clicked() {
                self.project.swap_layers(i, i - 1);
                self.sel_layer -= 1;
                self.rev += 1;
            }
            if ui.add_enabled(i + 1 < self.project.layers.len(), egui::Button::new("⬇")).on_hover_text("Cut this layer later").clicked() {
                self.project.swap_layers(i, i + 1);
                self.sel_layer += 1;
                self.rev += 1;
            }
            if ui
                .add_enabled(self.project.layers.len() > 1, egui::Button::new("🗑"))
                .on_hover_text("Remove this layer (its shapes move to another layer)")
                .clicked()
            {
                self.project.remove_layer(i);
                self.sel_layer = self.sel_layer.min(self.project.layers.len() - 1);
                self.selected.clear();
                self.rev += 1;
            }
        });

        ui.add_space(6.0);
        if let Some(l) = self.project.layers.get_mut(self.sel_layer) {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Grid::new("layer_settings").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut l.name).desired_width(150.0));
                    ui.end_row();
                    ui.label("Mode");
                    egui::ComboBox::from_id_salt("mode").selected_text(l.mode.label()).show_ui(ui, |ui| {
                        for m in [Mode::Line, Mode::Fill, Mode::Off] {
                            ui.selectable_value(&mut l.mode, m, m.label());
                        }
                    });
                    ui.end_row();
                    ui.label("Power");
                    ui.add(egui::Slider::new(&mut l.power, 0.0..=100.0).suffix(" %").fixed_decimals(0));
                    ui.end_row();
                    ui.label("Speed");
                    ui.add(egui::DragValue::new(&mut l.speed).speed(10.0).range(1.0..=30000.0).suffix(" mm/min").fixed_decimals(0));
                    ui.end_row();
                    ui.label("Passes");
                    ui.add(egui::DragValue::new(&mut l.passes).speed(0.1).range(1..=50));
                    ui.end_row();
                    if l.mode == Mode::Fill {
                        ui.label("Line gap");
                        ui.add(egui::DragValue::new(&mut l.fill_spacing).speed(0.01).range(0.02..=10.0).suffix(" mm").fixed_decimals(2));
                        ui.end_row();
                        ui.label("Angle");
                        ui.add(egui::Slider::new(&mut l.fill_angle, 0.0..=180.0).suffix("°").fixed_decimals(0));
                        ui.end_row();
                        ui.label("");
                        ui.checkbox(&mut l.crosshatch, "Cross-hatch");
                        ui.end_row();
                    }
                });
                if l.mode == Mode::Fill {
                    ui.label(egui::RichText::new("Fill only works on closed shapes.").small().color(Color32::from_gray(150)));
                }
            });
        }
        let sl = self.sel_layer;
        if ui.small_button("Select all shapes on this layer").clicked() {
            self.selected = self.project.paths.iter().enumerate().filter(|(_, p)| p.layer == sl).map(|(i, _)| i).collect();
        }

        ui.add_space(8.0);
        ui.separator();
        ui.heading("Selection");
        if self.selected.is_empty() {
            ui.label(egui::RichText::new("Click shapes on the canvas to select them (Shift adds more).").small().color(Color32::from_gray(150)));
        } else {
            ui.label(format!("{} shape(s) selected", self.selected.len()));
            ui.horizontal(|ui| {
                ui.label("Move to");
                let mut target: Option<usize> = None;
                egui::ComboBox::from_id_salt("move_to").selected_text("choose layer…").show_ui(ui, |ui| {
                    for (i, l) in self.project.layers.iter().enumerate() {
                        if ui.selectable_label(false, &l.name).clicked() {
                            target = Some(i);
                        }
                    }
                    if ui.selectable_label(false, "➕ a new layer").clicked() {
                        target = Some(usize::MAX);
                    }
                });
                if let Some(mut t) = target {
                    if t == usize::MAX {
                        t = self.project.add_layer();
                    }
                    for &i in &self.selected {
                        self.project.paths[i].layer = t;
                    }
                    self.sel_layer = t;
                    self.rev += 1;
                }
            });
            ui.horizontal(|ui| {
                if ui.button("🗑 Delete").clicked() {
                    self.delete_selected();
                }
                if ui.button("Clear").clicked() {
                    self.selected.clear();
                }
            });
        }

        ui.add_space(8.0);
        ui.separator();
        ui.heading("Position & size");
        let b = self.project.placed_bbox();
        let raw = self.project.raw_bbox();
        let pl = &mut self.project.place;
        egui::Grid::new("place").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Left (X)");
            ui.add(egui::DragValue::new(&mut pl.x).speed(0.5).suffix(" mm").fixed_decimals(1));
            ui.end_row();
            ui.label("Bottom (Y)");
            ui.add(egui::DragValue::new(&mut pl.y).speed(0.5).suffix(" mm").fixed_decimals(1));
            ui.end_row();
            ui.label("Scale");
            ui.add(egui::DragValue::new(&mut pl.scale).speed(0.5).range(1.0..=2000.0).suffix(" %").fixed_decimals(1));
            ui.end_row();
            ui.label("Width");
            let mut w = raw.width() * pl.scale / 100.0;
            if pl.rotate % 2 == 1 {
                w = raw.height() * pl.scale / 100.0;
            }
            let base_w = if pl.rotate % 2 == 1 { raw.height() } else { raw.width() };
            if ui.add(egui::DragValue::new(&mut w).speed(0.5).range(0.1..=5000.0).suffix(" mm").fixed_decimals(1)).changed() && base_w > 0.0 {
                pl.scale = w / base_w * 100.0;
            }
            ui.end_row();
            ui.label("Size");
            ui.label(format!("{:.1} × {:.1} mm", b.width(), b.height()));
            ui.end_row();
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut pl.mirror_x, "Mirror ↔");
            ui.checkbox(&mut pl.mirror_y, "Mirror ↕");
            if ui.button("⟲ 90°").clicked() {
                pl.rotate = (pl.rotate + 1) % 4;
            }
        });
        let bed = (self.project.machine.bed_w, self.project.machine.bed_h);
        ui.horizontal(|ui| {
            if ui.button("Centre on bed").clicked() {
                let pl = &mut self.project.place;
                pl.x = (bed.0 - b.width()) / 2.0;
                pl.y = (bed.1 - b.height()) / 2.0;
            }
            if ui.button("To corner").clicked() {
                let pl = &mut self.project.place;
                pl.x = 0.0;
                pl.y = 0.0;
            }
        });
    }

    fn machine_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Machine");
        let m = &mut self.project.machine;
        egui::Grid::new("machine").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("Bed width");
            ui.add(egui::DragValue::new(&mut m.bed_w).speed(1.0).range(10.0..=3000.0).suffix(" mm").fixed_decimals(0));
            ui.end_row();
            ui.label("Bed depth");
            ui.add(egui::DragValue::new(&mut m.bed_h).speed(1.0).range(10.0..=3000.0).suffix(" mm").fixed_decimals(0));
            ui.end_row();
            ui.label("Full power S");
            ui.add(egui::DragValue::new(&mut m.max_s).speed(1.0).range(1.0..=100000.0).fixed_decimals(0))
                .on_hover_text("The S value your laser treats as 100 %. GRBL is usually 1000, Marlin 255.");
            ui.end_row();
            ui.label("Laser control");
            egui::ComboBox::from_id_salt("lasercmd")
                .selected_text(match m.laser_cmd {
                    LaserCmd::M4Dynamic => "M4 dynamic",
                    LaserCmd::M3Constant => "M3 constant",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut m.laser_cmd, LaserCmd::M4Dynamic, "M4 dynamic (GRBL laser mode)");
                    ui.selectable_value(&mut m.laser_cmd, LaserCmd::M3Constant, "M3 constant (M3 on / M5 off)");
                });
            ui.end_row();
            ui.label("Travel speed");
            ui.add(egui::DragValue::new(&mut m.travel_speed).speed(10.0).range(10.0..=50000.0).suffix(" mm/min").fixed_decimals(0));
            ui.end_row();
            ui.label("Decimals");
            ui.add(egui::DragValue::new(&mut m.decimals).range(1..=5));
            ui.end_row();
        });
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("GRBL 0-1000").clicked() {
                m.max_s = 1000.0;
                m.laser_cmd = LaserCmd::M4Dynamic;
            }
            if ui.small_button("GRBL 0-255").clicked() {
                m.max_s = 255.0;
                m.laser_cmd = LaserCmd::M4Dynamic;
            }
            if ui.small_button("Marlin").clicked() {
                m.max_s = 255.0;
                m.laser_cmd = LaserCmd::M3Constant;
            }
        });
        ui.add_space(4.0);
        ui.checkbox(&mut m.air_assist, "Air assist (M8 / M9)");
        ui.checkbox(&mut m.return_home, "Return to 0,0 at the end");
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Cutting order").strong());
        ui.checkbox(&mut m.optimize, "Shortest travel between shapes");
        ui.add_enabled(m.optimize, egui::Checkbox::new(&mut m.inside_first, "Cut inner shapes first"))
            .on_hover_text("Holes are cut before the outline around them, so parts don't drop out early.");
        ui.add_space(4.0);
        egui::CollapsingHeader::new("Start & end G-code").default_open(false).show(ui, |ui| {
            ui.label(egui::RichText::new("Added at the start:").small());
            ui.add(egui::TextEdit::multiline(&mut m.header).desired_rows(2).code_editor().desired_width(f32::INFINITY));
            ui.label(egui::RichText::new("Added at the end:").small());
            ui.add(egui::TextEdit::multiline(&mut m.footer).desired_rows(2).code_editor().desired_width(f32::INFINITY));
        });

        ui.add_space(8.0);
        ui.separator();
        ui.heading("Job");
        let j = &self.job;
        egui::Grid::new("jobinfo").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            ui.label("Run time");
            ui.label(format!("≈ {}", format_time(j.total_time)));
            ui.end_row();
            ui.label("Cutting");
            ui.label(format!("{:.0} mm", j.cut_length));
            ui.end_row();
            ui.label("Travel");
            ui.label(format!("{:.0} mm", j.travel_length));
            ui.end_row();
            ui.label("G-code lines");
            ui.label(format!("{}", self.line_starts.len()));
            ui.end_row();
            if !j.bbox.is_empty() {
                ui.label("Burn area");
                ui.label(format!("{:.1} × {:.1} mm", j.bbox.width(), j.bbox.height()));
                ui.end_row();
            }
        });
        let mb = &self.project.machine;
        if !j.bbox.is_empty() && (j.bbox.min.x < -0.001 || j.bbox.min.y < -0.001 || j.bbox.max.x > mb.bed_w + 0.001 || j.bbox.max.y > mb.bed_h + 0.001) {
            ui.colored_label(Color32::from_rgb(255, 110, 90), "⚠ Part of the job is outside the bed.");
        }
        ui.label(egui::RichText::new("Run time is an estimate without acceleration.").small().color(Color32::from_gray(140)));
    }

    fn delete_selected(&mut self) {
        if self.selected.is_empty() {
            return;
        }
        let mut idx: Vec<usize> = self.selected.drain().collect();
        idx.sort_unstable_by(|a, b| b.cmp(a));
        for i in idx {
            if i < self.project.paths.len() {
                self.project.paths.remove(i);
            }
        }
        self.rev += 1;
        self.set_status("Deleted the selected shapes.", false);
    }

    fn sim_bar(&mut self, ui: &mut egui::Ui) {
        let total = self.job.total_time;
        ui.horizontal(|ui| {
            if ui.button("⏮").on_hover_text("Back to the start").clicked() {
                self.sim.t = 0.0;
            }
            let play_label = if self.sim.playing { "⏸ Pause" } else { "▶ Play" };
            if ui.add(egui::Button::new(egui::RichText::new(play_label).strong()).min_size(egui::vec2(80.0, 0.0))).clicked() {
                if !self.sim.playing && self.sim.t >= total {
                    self.sim.t = 0.0;
                }
                self.sim.playing = !self.sim.playing;
            }
            if ui.button("⏭").on_hover_text("Jump to the end").clicked() {
                self.sim.t = total;
                self.sim.playing = false;
            }
            ui.label("Speed");
            egui::ComboBox::from_id_salt("simspeed").selected_text(format!("{}×", self.sim.speed)).width(60.0).show_ui(ui, |ui| {
                for s in [1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 1000.0] {
                    ui.selectable_value(&mut self.sim.speed, s, format!("{s}×"));
                }
            });
            ui.checkbox(&mut self.sim.show_travel, "Show travel");
            ui.label(format!("{} / {}", format_time(self.sim.t), format_time(total)));
            ui.spacing_mut().slider_width = (ui.available_width() - 20.0).max(100.0);
            let mut t = self.sim.t;
            if ui.add(egui::Slider::new(&mut t, 0.0..=total.max(0.001)).show_value(false)).changed() {
                self.sim.t = t;
            }
        });
        // details about the moment shown
        let (pos, firing, power, layer) = self.head_state();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("X {:.2}  Y {:.2}", pos.x, pos.y)).monospace());
            if firing {
                ui.colored_label(Color32::from_rgb(255, 90, 60), format!("● laser on {:.0}%", power * 100.0));
            } else {
                ui.colored_label(Color32::from_gray(150), "○ laser off");
            }
            if let Some(l) = layer.and_then(|l| self.project.layers.get(l)).filter(|_| self.external.is_none()) {
                ui.label(format!("Layer: {}", l.name));
            }
            let pct = if total > 0.0 { self.sim.t / total * 100.0 } else { 0.0 };
            ui.label(format!("{pct:.0}% done"));
        });
    }

    fn head_state(&self) -> (Pt, bool, f32, Option<usize>) {
        let moves = &self.job.moves;
        if moves.is_empty() {
            return (Pt::new(0.0, 0.0), false, 0.0, None);
        }
        let i = self.job.index_at(self.sim.t);
        let m = &moves[i];
        if self.sim.t <= 0.0 {
            return (m.from, false, 0.0, m.layer);
        }
        let f = ((self.sim.t - m.t0) / m.dur.max(1e-9)).clamp(0.0, 1.0);
        let done = self.sim.t >= self.job.total_time;
        (m.from.lerp(m.to, f), m.laser && !done, m.power, m.layer)
    }

    fn code_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(format!("{} lines", self.line_starts.len()));
            if ui.button("📋 Copy all").clicked() {
                ui.ctx().copy_text(self.gcode.clone());
                self.set_status("G-code copied to the clipboard.", false);
            }
            if ui.button("💾 Save G-code…").clicked() {
                self.save_dialog();
            }
        });
        ui.separator();
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
        let n = self.line_starts.len();
        let gcode = &self.gcode;
        let starts = &self.line_starts;
        egui::ScrollArea::both().auto_shrink([false, false]).show_rows(ui, row_h, n, |ui, range| {
            for i in range {
                let s = starts[i];
                let e = starts.get(i + 1).copied().unwrap_or(gcode.len());
                let line = gcode[s..e].trim_end();
                let color = if line.starts_with(';') {
                    Color32::from_rgb(120, 170, 120)
                } else if line.starts_with("G0") {
                    Color32::from_rgb(130, 160, 220)
                } else if line.starts_with('M') {
                    Color32::from_rgb(230, 170, 90)
                } else {
                    Color32::from_gray(215)
                };
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{:>6}", i + 1)).monospace().color(Color32::from_gray(90)));
                    ui.label(egui::RichText::new(line).monospace().color(color));
                });
            }
        });
    }

    // ---------------------------------------------------------------- canvas

    fn fit_view(&mut self, rect: Rect) {
        let mut b = if self.external.is_some() { self.job.bbox } else { bbox_of(&self.placed) };
        if b.is_empty() {
            b = BBox { min: Pt::new(0.0, 0.0), max: Pt::new(self.project.machine.bed_w, self.project.machine.bed_h) };
        }
        let w = b.width().max(10.0) * 1.25;
        let h = b.height().max(10.0) * 1.25;
        self.view.zoom = ((rect.width() as f64 / w).min(rect.height() as f64 / h)) as f32;
        self.view.center = Pt::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0);
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        if self.view.needs_fit && rect.width() > 50.0 {
            self.fit_view(rect);
            self.view.needs_fit = false;
        }
        // pan & zoom
        if resp.dragged() {
            let d = resp.drag_delta();
            self.view.center.x -= (d.x / self.view.zoom) as f64;
            self.view.center.y += (d.y / self.view.zoom) as f64;
        }
        if let Some(hover) = resp.hover_pos() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = zoom * (scroll / 300.0).exp();
            if (factor - 1.0).abs() > 1e-4 {
                let before = self.to_mm(rect, hover);
                self.view.zoom = (self.view.zoom * factor).clamp(0.05, 400.0);
                let after = self.to_mm(rect, hover);
                self.view.center = self.view.center.add(before.sub(after));
            }
        }
        if resp.double_clicked() {
            self.view.needs_fit = true;
        }

        painter.rect_filled(rect, 0.0, BG);
        let painter = painter.with_clip_rect(rect);
        self.draw_bed(&painter, rect);

        match self.tab {
            Tab::Simulate => self.draw_sim(&painter, rect),
            _ => self.draw_design(&painter, rect),
        }

        // click to select (design only)
        if self.tab == Tab::Design && self.external.is_none() && resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let p = self.to_mm(rect, pos);
                let tol = 6.0 / self.view.zoom as f64;
                let mut best: Option<(usize, f64)> = None;
                for (i, path) in self.placed.iter().enumerate() {
                    let l = &self.project.layers[path.layer.min(self.project.layers.len() - 1)];
                    if !l.visible {
                        continue;
                    }
                    let b = path.bbox();
                    if p.x < b.min.x - tol || p.x > b.max.x + tol || p.y < b.min.y - tol || p.y > b.max.y + tol {
                        continue;
                    }
                    let d = path.distance_to(p);
                    if d < tol && best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((i, d));
                    }
                }
                let shift = ui.input(|i| i.modifiers.shift || i.modifiers.command);
                match best {
                    Some((i, _)) => {
                        if shift {
                            if !self.selected.remove(&i) {
                                self.selected.insert(i);
                            }
                        } else {
                            self.selected.clear();
                            self.selected.insert(i);
                        }
                        self.sel_layer = self.project.paths[i].layer;
                    }
                    None => {
                        if !shift {
                            self.selected.clear();
                        }
                    }
                }
            }
        }

        // overlay: fit button and hints
        let hint = match self.tab {
            Tab::Design if self.external.is_none() => "Drag to pan · scroll to zoom · double-click to fit · click a shape to select",
            _ => "Drag to pan · scroll to zoom · double-click to fit",
        };
        painter.text(rect.left_bottom() + egui::vec2(10.0, -10.0), Align2::LEFT_BOTTOM, hint, FontId::proportional(12.0), Color32::from_gray(120));
        if let Some(hover) = resp.hover_pos() {
            let p = self.to_mm(rect, hover);
            painter.text(
                rect.right_bottom() + egui::vec2(-10.0, -10.0),
                Align2::RIGHT_BOTTOM,
                format!("X {:.1}  Y {:.1} mm", p.x, p.y),
                FontId::monospace(12.0),
                Color32::from_gray(150),
            );
        }
        let fit_rect = Rect::from_min_size(rect.right_top() + egui::vec2(-58.0, 10.0), egui::vec2(48.0, 24.0));
        if ui.put(fit_rect, egui::Button::new("Fit")).clicked() {
            self.view.needs_fit = true;
        }
    }

    fn to_screen(&self, rect: Rect, p: Pt) -> Pos2 {
        let c = rect.center();
        Pos2::new(
            c.x + ((p.x - self.view.center.x) as f32) * self.view.zoom,
            c.y - ((p.y - self.view.center.y) as f32) * self.view.zoom,
        )
    }

    fn to_mm(&self, rect: Rect, s: Pos2) -> Pt {
        let c = rect.center();
        Pt::new(
            self.view.center.x + ((s.x - c.x) / self.view.zoom) as f64,
            self.view.center.y - ((s.y - c.y) / self.view.zoom) as f64,
        )
    }

    fn draw_bed(&self, painter: &egui::Painter, rect: Rect) {
        let m = &self.project.machine;
        let a = self.to_screen(rect, Pt::new(0.0, 0.0));
        let b = self.to_screen(rect, Pt::new(m.bed_w, m.bed_h));
        let bed = Rect::from_two_pos(a, b);
        painter.rect_filled(bed, 2.0, BED);
        // grid
        let z = self.view.zoom;
        let step = if z * 10.0 >= 8.0 { 10.0 } else if z * 50.0 >= 8.0 { 50.0 } else { 100.0 };
        let mut x = 0.0;
        while x <= m.bed_w + 1e-6 {
            let major = (x / 50.0).fract().abs() < 1e-6;
            let c = if major { Color32::from_gray(58) } else { Color32::from_gray(44) };
            let p0 = self.to_screen(rect, Pt::new(x, 0.0));
            let p1 = self.to_screen(rect, Pt::new(x, m.bed_h));
            painter.line_segment([p0, p1], Stroke::new(1.0, c));
            if major && x > 0.0 {
                painter.text(p0 + egui::vec2(0.0, 4.0), Align2::CENTER_TOP, format!("{x:.0}"), FontId::proportional(10.0), Color32::from_gray(110));
            }
            x += step;
        }
        let mut y = 0.0;
        while y <= m.bed_h + 1e-6 {
            let major = (y / 50.0).fract().abs() < 1e-6;
            let c = if major { Color32::from_gray(58) } else { Color32::from_gray(44) };
            let p0 = self.to_screen(rect, Pt::new(0.0, y));
            let p1 = self.to_screen(rect, Pt::new(m.bed_w, y));
            painter.line_segment([p0, p1], Stroke::new(1.0, c));
            if major && y > 0.0 {
                painter.text(p0 + egui::vec2(-4.0, 0.0), Align2::RIGHT_CENTER, format!("{y:.0}"), FontId::proportional(10.0), Color32::from_gray(110));
            }
            y += step;
        }
        painter.rect_stroke(bed, 2.0, Stroke::new(1.0, Color32::from_gray(80)), StrokeKind::Outside);
        // origin
        painter.line_segment([a, a + egui::vec2(28.0, 0.0)], Stroke::new(2.0, Color32::from_rgb(220, 80, 80)));
        painter.line_segment([a, a + egui::vec2(0.0, -28.0)], Stroke::new(2.0, Color32::from_rgb(80, 200, 100)));
        painter.text(a + egui::vec2(-4.0, 4.0), Align2::RIGHT_TOP, "0,0", FontId::proportional(10.0), Color32::from_gray(140));
    }

    fn screen_pts(&self, rect: Rect, pts: &[Pt]) -> Vec<Pos2> {
        pts.iter().map(|p| self.to_screen(rect, *p)).collect()
    }

    fn draw_design(&self, painter: &egui::Painter, rect: Rect) {
        if self.external.is_some() {
            // just the toolpath of the opened file
            for m in &self.job.moves {
                if m.laser {
                    painter.line_segment([self.to_screen(rect, m.from), self.to_screen(rect, m.to)], Stroke::new(1.2, Color32::from_rgb(240, 120, 70)));
                }
            }
            return;
        }
        let layers = &self.project.layers;
        // fill previews (from the generated toolpath)
        for m in &self.job.moves {
            if !m.laser {
                continue;
            }
            let Some(l) = m.layer.and_then(|l| layers.get(l)) else { continue };
            if l.mode != Mode::Fill || !l.visible {
                continue;
            }
            let w = (l.fill_spacing * self.view.zoom * 0.6).clamp(0.6, 6.0);
            painter.line_segment([self.to_screen(rect, m.from), self.to_screen(rect, m.to)], Stroke::new(w, col_a(l.color, 70)));
        }
        for (i, p) in self.placed.iter().enumerate() {
            let Some(l) = layers.get(p.layer) else { continue };
            if !l.visible {
                continue;
            }
            let sel = self.selected.contains(&i);
            let color = if l.mode == Mode::Off { col_a(l.color, 70) } else { col(l.color) };
            let pts = self.screen_pts(rect, &p.pts);
            if sel {
                let glow = Stroke::new(5.0, Color32::from_rgba_unmultiplied(255, 255, 255, 60));
                painter.add(if p.closed { Shape::closed_line(pts.clone(), glow) } else { Shape::line(pts.clone(), glow) });
            }
            let stroke = Stroke::new(if sel { 2.2 } else { 1.4 }, if sel { Color32::WHITE } else { color });
            painter.add(if p.closed { Shape::closed_line(pts, stroke) } else { Shape::line(pts, stroke) });
        }
        // outline of the job
        let b = bbox_of(&self.placed);
        if !b.is_empty() {
            let r = Rect::from_two_pos(self.to_screen(rect, b.min), self.to_screen(rect, b.max)).expand(6.0);
            let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.extend(Shape::dashed_line(&pts, Stroke::new(1.0, Color32::from_gray(110)), 5.0, 4.0));
            painter.text(
                r.left_top() + egui::vec2(0.0, -4.0),
                Align2::LEFT_BOTTOM,
                format!("{:.1} × {:.1} mm", b.width(), b.height()),
                FontId::proportional(11.0),
                Color32::from_gray(150),
            );
        }
        if self.placed.is_empty() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Open a DXF or SVG file to begin", FontId::proportional(18.0), Color32::from_gray(140));
        }
    }

    fn draw_sim(&self, painter: &egui::Painter, rect: Rect) {
        let moves = &self.job.moves;
        let bb = if self.external.is_some() { self.job.bbox } else { bbox_of(&self.placed) };
        if !bb.is_empty() {
            // the sheet of material
            let r = Rect::from_two_pos(
                self.to_screen(rect, Pt::new(bb.min.x - 6.0, bb.min.y - 6.0)),
                self.to_screen(rect, Pt::new(bb.max.x + 6.0, bb.max.y + 6.0)),
            );
            painter.rect_filled(r, 2.0, Color32::from_rgb(206, 176, 130));
            let mut gy = r.top() + 7.0;
            let mut k = 0;
            while gy < r.bottom() {
                let c = if k % 3 == 0 { Color32::from_rgba_unmultiplied(150, 115, 70, 40) } else { Color32::from_rgba_unmultiplied(235, 210, 170, 40) };
                painter.line_segment([Pos2::new(r.left(), gy), Pos2::new(r.right(), gy)], Stroke::new(1.0, c));
                gy += 5.0 + (k % 4) as f32 * 2.5;
                k += 1;
            }
        }
        if moves.is_empty() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "Nothing to simulate: every layer is off or empty", FontId::proportional(16.0), Color32::from_gray(160));
            return;
        }
        let t = self.sim.t;
        let cur = self.job.index_at(t);
        let started = t > 0.0;
        let layers = &self.project.layers;
        let line_w = |m: &gcode::Move, done: bool| -> f32 {
            let base = match m.layer.and_then(|l| layers.get(l)).filter(|_| self.external.is_none()) {
                Some(l) if l.mode == Mode::Fill => (l.fill_spacing * self.view.zoom * 1.05).clamp(0.8, 30.0),
                _ => (0.18 * self.view.zoom).clamp(1.2, 4.0),
            };
            if done { base } else { base.min(1.2) }
        };
        // planned (not yet cut) moves, faint
        for (i, m) in moves.iter().enumerate() {
            if started && i < cur {
                continue;
            }
            if m.laser {
                let c = match m.layer.and_then(|l| layers.get(l)).filter(|_| self.external.is_none()) {
                    Some(l) => col_a(l.color, 150),
                    None => Color32::from_rgba_unmultiplied(80, 60, 40, 120),
                };
                painter.line_segment([self.to_screen(rect, m.from), self.to_screen(rect, m.to)], Stroke::new(line_w(m, false), c));
            }
        }
        if started {
            for (i, m) in moves.iter().enumerate().take(cur + 1) {
                let to = if i == cur { m.from.lerp(m.to, ((t - m.t0) / m.dur.max(1e-9)).clamp(0.0, 1.0)) } else { m.to };
                let a = self.to_screen(rect, m.from);
                let b = self.to_screen(rect, to);
                if m.laser {
                    let dark = (40.0 + 40.0 * (1.0 - m.power)) as u8;
                    let alpha = (110.0 + 145.0 * m.power) as u8;
                    painter.line_segment([a, b], Stroke::new(line_w(m, true), Color32::from_rgba_unmultiplied(dark, (dark as f32 * 0.6) as u8, 15, alpha)));
                } else if self.sim.show_travel {
                    painter.line_segment([a, b], Stroke::new(0.8, Color32::from_rgba_unmultiplied(50, 110, 230, 110)));
                }
            }
        }
        // laser head
        let (pos, firing, _, _) = self.head_state();
        let p = self.to_screen(rect, pos);
        if firing {
            for (r, a) in [(16.0, 30), (10.0, 60), (6.0, 120)] {
                painter.circle_filled(p, r, Color32::from_rgba_unmultiplied(255, 80, 30, a));
            }
            painter.circle_filled(p, 3.0, Color32::from_rgb(255, 240, 200));
        } else {
            painter.circle_stroke(p, 7.0, Stroke::new(1.5, Color32::from_rgb(80, 160, 255)));
            painter.line_segment([p - egui::vec2(11.0, 0.0), p + egui::vec2(11.0, 0.0)], Stroke::new(1.0, Color32::from_rgb(80, 160, 255)));
            painter.line_segment([p - egui::vec2(0.0, 11.0), p + egui::vec2(0.0, 11.0)], Stroke::new(1.0, Color32::from_rgb(80, 160, 255)));
        }
    }

    fn handle_input(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| !p.as_os_str().is_empty()).collect());
        for (k, p) in dropped.iter().enumerate() {
            self.open_path(p, k == 0);
        }
        let typing = ctx.egui_wants_keyboard_input();
        let (open, save, del, space, esc) = ctx.input(|i| {
            (
                i.modifiers.command && i.key_pressed(egui::Key::O),
                i.modifiers.command && i.key_pressed(egui::Key::S),
                i.key_pressed(egui::Key::Delete),
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if open {
            self.open_dialog(true);
        }
        if save {
            self.save_dialog();
        }
        if !typing {
            if del && self.tab == Tab::Design {
                self.delete_selected();
            }
            if esc {
                self.selected.clear();
            }
            if space && self.tab == Tab::Simulate {
                if !self.sim.playing && self.sim.t >= self.job.total_time {
                    self.sim.t = 0.0;
                }
                self.sim.playing = !self.sim.playing;
            }
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.shot.as_mut() else { return };
        ctx.request_repaint();
        shot.frames += 1;
        if let Some(res) = shot.result.lock().unwrap().take() {
            if let Err(e) = res {
                eprintln!("screenshot failed: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if shot.frames >= 120 && !shot.requested {
            shot.requested = true;
            let path = shot.path.clone();
            let result = shot.result.clone();
            ctx.request_screenshot(move |img: Arc<egui::ColorImage>| {
                let [w, h] = img.size;
                let mut bytes = Vec::with_capacity(w * h * 4);
                for p in &img.pixels {
                    let [r, g, b, _] = p.to_array();
                    bytes.extend_from_slice(&[r, g, b, 255]);
                }
                let r = (|| -> Result<(), String> {
                    if let Some(dir) = path.parent() {
                        if !dir.as_os_str().is_empty() {
                            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                        }
                    }
                    let im = image::RgbaImage::from_raw(w as u32, h as u32, bytes).ok_or("bad image size")?;
                    im.save(&path).map_err(|e| e.to_string())
                })();
                *result.lock().unwrap() = Some(r);
            });
        }
        if shot.frames > 600 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for LabCut {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_input(&ctx);

        if self.sim.playing {
            let dt = ctx.input(|i| i.stable_dt).min(0.1) as f64;
            self.sim.t += dt * self.sim.speed as f64;
            if self.sim.t >= self.job.total_time {
                self.sim.t = self.job.total_time;
                self.sim.playing = false;
            }
            ctx.request_repaint();
        }

        egui::Panel::top("top").frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(10, 8))).show(ui, |ui| {
            self.top_bar(ui);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let c = if self.status_warn { Color32::from_rgb(255, 120, 90) } else { Color32::from_gray(170) };
                ui.colored_label(c, &self.status);
            });
        });
        if self.tab == Tab::Simulate {
            egui::Panel::bottom("simbar").frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(10, 8))).show(ui, |ui| {
                self.sim_bar(ui);
            });
        }
        egui::Panel::left("layers").default_size(330.0).min_size(280.0).resizable(true).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(6.0);
                self.layers_panel(ui);
                ui.add_space(10.0);
            });
        });
        egui::Panel::right("machine").default_size(260.0).min_size(220.0).resizable(true).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.add_space(6.0);
                self.machine_panel(ui);
                ui.add_space(10.0);
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG)).show(ui, |ui| {
            if self.tab == Tab::Code {
                egui::Frame::new().inner_margin(10).show(ui, |ui| self.code_view(ui));
            } else {
                self.canvas(ui);
            }
        });

        self.refresh();
        self.handle_screenshot(&ctx);
    }
}

#[allow(dead_code)]
fn _unused(_: Vec2) {}
