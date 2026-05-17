mod graph;
mod tree;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result, anyhow};
use eframe::egui;
use mdd_core::cycle::{CycleDiff, CycleRegistry};
use mdd_core::{ModelFile, ModelKind, ModelRegistry, Project, Trace};

use crate::graph::{GraphAction, GraphData, GraphPanel};
use crate::tree::TreeNode;

pub fn run(project: Project) -> Result<()> {
    let title = format!("mdd · {}", project.root().display());
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title(&title),
        ..Default::default()
    };
    let app = MddViewer::new(project)?;
    eframe::run_native(
        "mdd-viewer",
        native_options,
        Box::new(move |_cc| Ok(Box::new(app))),
    )
    .map_err(|e| anyhow!("eframe error: {e}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Svg,
    Graph,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RailMode {
    Directory,
    ByCycle,
}

struct MddViewer {
    project: Project,
    registry: ModelRegistry,
    trace: Trace,
    selected_file: Option<String>,
    selected_id: Option<String>,
    pages: Vec<DiagramPage>,
    page_idx: usize,
    /// Top-left of the scene, in canvas-local coordinates (px).
    pan: egui::Vec2,
    zoom: f32,
    initial_pan: egui::Vec2,
    initial_zoom: f32,
    needs_fit: bool,
    load_error: Option<String>,
    view: View,
    graph_panel: GraphPanel,
    descriptions: BTreeMap<String, String>,
    cycles: CycleRegistry,
    rail_mode: RailMode,
    selected_cycle: Option<u32>,
    diff_cache: Option<(u32, Vec<CycleDiff>)>,
}

struct DiagramPage {
    rel_path: String,
    svg_bytes: Vec<u8>,
    intrinsic: egui::Vec2,
    texture: Option<egui::TextureHandle>,
    cached_zoom: f32,
}

impl MddViewer {
    fn new(project: Project) -> Result<Self> {
        let registry = project
            .model_registry()
            .context("failed to load model registry")?;
        let trace = project.read_trace().unwrap_or_else(|_| Trace {
            version: 1,
            links: Vec::new(),
            generated_tests: Vec::new(),
            generated_ui_tests: Vec::new(),
            source_links: Vec::new(),
        });
        let graph_panel = GraphPanel::new(GraphData::build(&registry, &trace));
        let descriptions = project.descriptions().unwrap_or_default();
        let cycles = project.cycle_registry().unwrap_or_default();
        let mut viewer = Self {
            project,
            registry,
            trace,
            selected_file: None,
            selected_id: None,
            pages: Vec::new(),
            page_idx: 0,
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            initial_pan: egui::Vec2::ZERO,
            initial_zoom: 1.0,
            needs_fit: false,
            load_error: None,
            view: View::Svg,
            graph_panel,
            descriptions,
            cycles,
            rail_mode: RailMode::Directory,
            selected_cycle: None,
            diff_cache: None,
        };
        if let Some(first) = viewer.registry.files.first().cloned() {
            viewer.load_file(&first);
        }
        Ok(viewer)
    }

    fn load_file(&mut self, file: &ModelFile) {
        self.selected_file = Some(file.path.clone());
        self.selected_id = None;
        self.page_idx = 0;
        self.pages.clear();
        self.load_error = None;

        if file.rendered_pages.is_empty() {
            return;
        }
        for rel in &file.rendered_pages {
            let abs: PathBuf = self.project.root().join(".mdd/rendered").join(rel);
            let bytes = match std::fs::read(&abs) {
                Ok(b) => b,
                Err(e) => {
                    self.load_error = Some(format!("read {}: {e}", abs.display()));
                    continue;
                }
            };
            let intrinsic = match parse_intrinsic_size(&bytes) {
                Ok(v) => v,
                Err(e) => {
                    self.load_error = Some(format!("parse {}: {e}", abs.display()));
                    continue;
                }
            };
            self.pages.push(DiagramPage {
                rel_path: rel.clone(),
                svg_bytes: bytes,
                intrinsic,
                texture: None,
                cached_zoom: 0.0,
            });
        }
        self.needs_fit = !self.pages.is_empty();
    }

    fn fit_to_rect(&mut self, rect: egui::Rect) {
        let Some(page) = self.pages.get(self.page_idx) else {
            return;
        };
        let margin = 0.95_f32;
        let zoom_x = rect.width() / page.intrinsic.x.max(1.0);
        let zoom_y = rect.height() / page.intrinsic.y.max(1.0);
        let zoom = (zoom_x.min(zoom_y) * margin).clamp(0.05, 20.0);
        let scaled = page.intrinsic * zoom;
        let pan = (rect.size() - scaled) * 0.5;
        self.zoom = zoom;
        self.pan = pan;
        self.initial_zoom = zoom;
        self.initial_pan = pan;
        self.needs_fit = false;
    }
}

impl eframe::App for MddViewer {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::TopBottomPanel::top("topbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("mdd");
                ui.separator();
                ui.monospace(self.project.root().display().to_string());
            });
        });

        let files = self.registry.files.clone();
        let rail_mode = self.rail_mode;
        let selected_file = self.selected_file.clone();

        let dir_tree = TreeNode::build(files.iter(), short_path, |f| kind_label(f.kind));

        let mut assigned: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut cycle_groups: Vec<(String, TreeNode)> = Vec::new();
        for cycle in &self.cycles.cycles {
            let mut leaves: Vec<ModelFile> = Vec::new();
            for touched in &cycle.manifest.touched_files {
                if let Some(file) = files.iter().find(|f| &f.path == touched) {
                    assigned.insert(file.path.clone());
                    leaves.push(file.clone());
                }
            }
            let tree =
                TreeNode::build(leaves.iter(), short_path, |f| kind_label(f.kind));
            cycle_groups.push((cycle.label(), tree));
        }
        let unassigned: Vec<ModelFile> = files
            .iter()
            .filter(|f| !assigned.contains(&f.path))
            .cloned()
            .collect();
        let unassigned_tree =
            TreeNode::build(unassigned.iter(), short_path, |f| kind_label(f.kind));

        let mut clicked: Option<String> = None;
        let mut new_mode = rail_mode;
        egui::SidePanel::left("rail")
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("MODELS").small().strong());
                ui.horizontal(|ui| {
                    if ui
                        .selectable_label(rail_mode == RailMode::Directory, "Directory")
                        .clicked()
                    {
                        new_mode = RailMode::Directory;
                    }
                    if ui
                        .selectable_label(rail_mode == RailMode::ByCycle, "By cycle")
                        .clicked()
                    {
                        new_mode = RailMode::ByCycle;
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| match rail_mode {
                    RailMode::Directory => {
                        if let Some(path) =
                            dir_tree.ui(ui, selected_file.as_deref(), "dir")
                        {
                            clicked = Some(path);
                        }
                    }
                    RailMode::ByCycle => {
                        if cycle_groups.is_empty() {
                            ui.weak("No cycles yet — run /mdd-cycle");
                        }
                        for (label, tree) in &cycle_groups {
                            let salt = format!("cyc/{label}");
                            let resp = egui::CollapsingHeader::new(
                                egui::RichText::new(label).strong(),
                            )
                            .id_salt(&salt)
                            .default_open(true)
                            .show(ui, |ui| {
                                tree.ui(ui, selected_file.as_deref(), &salt)
                            });
                            if let Some(Some(path)) = resp.body_returned {
                                clicked = Some(path);
                            }
                        }
                        if !unassigned.is_empty() {
                            let resp = egui::CollapsingHeader::new(
                                egui::RichText::new("Unassigned").weak(),
                            )
                            .id_salt("cyc/unassigned")
                            .default_open(false)
                            .show(ui, |ui| {
                                unassigned_tree.ui(
                                    ui,
                                    selected_file.as_deref(),
                                    "cyc/unassigned",
                                )
                            });
                            if let Some(Some(path)) = resp.body_returned {
                                clicked = Some(path);
                            }
                        }
                    }
                });
            });
        self.rail_mode = new_mode;
        if let Some(path) = clicked
            && let Some(file) = self.registry.files.iter().find(|f| f.path == path).cloned()
        {
            self.load_file(&file);
        }

        egui::SidePanel::right("context")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.label(egui::RichText::new("MODEL CONTEXT").small().strong());
                ui.separator();
                let active_file = self
                    .selected_file
                    .as_ref()
                    .and_then(|p| self.registry.files.iter().find(|f| &f.path == p))
                    .cloned();
                if let Some(file) = active_file {
                    ui.label(egui::RichText::new("file").small().weak());
                    ui.monospace(&file.path);
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("kind").small().weak());
                    ui.label(kind_label(file.kind));
                    ui.add_space(8.0);

                    ui.label(egui::RichText::new("IDS").small().strong());
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("ids")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for id in &file.ids {
                                let active = self.selected_id.as_deref() == Some(id.as_str());
                                if ui.selectable_label(active, id).clicked() {
                                    self.selected_id = Some(id.clone());
                                }
                            }
                        });

                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("DESCRIPTION").small().strong());
                    ui.separator();
                    if let Some(id) = self.selected_id.clone() {
                        if let Some(text) = self.descriptions.get(&id) {
                            egui::Frame::group(ui.style())
                                .fill(egui::Color32::from_rgb(32, 40, 54))
                                .stroke(egui::Stroke::new(
                                    1.0,
                                    egui::Color32::from_rgb(86, 156, 214),
                                ))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(&id).strong().color(
                                            egui::Color32::from_rgb(240, 230, 120),
                                        ),
                                    );
                                    ui.label(egui::RichText::new(text).italics());
                                });
                        } else {
                            ui.weak("No description for this element");
                        }
                    } else {
                        ui.weak("Select a model ID to see its description");
                    }

                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("TRACE").small().strong());
                    ui.separator();
                    if let Some(id) = self.selected_id.clone() {
                        let links: Vec<_> = self
                            .trace
                            .links
                            .iter()
                            .filter(|l| l.from == id || l.to == id)
                            .cloned()
                            .collect();
                        let tests: Vec<_> = self
                            .trace
                            .generated_tests
                            .iter()
                            .filter(|t| t.model_id == id)
                            .cloned()
                            .collect();
                        let ui_tests: Vec<_> = self
                            .trace
                            .generated_ui_tests
                            .iter()
                            .filter(|t| t.model_id == id)
                            .cloned()
                            .collect();
                        egui::ScrollArea::vertical().id_salt("trace").show(ui, |ui| {
                            if links.is_empty() && tests.is_empty() && ui_tests.is_empty() {
                                ui.weak("No trace links");
                            }
                            for link in &links {
                                ui.horizontal_wrapped(|ui| {
                                    ui.monospace(&link.from);
                                    ui.weak("→");
                                    ui.label(egui::RichText::new(&link.relation).strong());
                                    ui.weak("→");
                                    ui.monospace(&link.to);
                                });
                            }
                            for test in &tests {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(egui::RichText::new(&test.id).strong());
                                    ui.weak("acceptance");
                                    ui.monospace(&test.path);
                                });
                            }
                            for test in &ui_tests {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(egui::RichText::new(&test.id).strong());
                                    ui.weak(&test.framework);
                                    ui.monospace(&test.path);
                                });
                            }
                        });
                    } else {
                        ui.weak("Select a model ID");
                    }
                } else {
                    ui.weak("No diagram selected");
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(self.view == View::Svg, "Diagram")
                    .clicked()
                {
                    self.view = View::Svg;
                }
                if ui
                    .selectable_label(self.view == View::Graph, "Graph")
                    .clicked()
                {
                    self.view = View::Graph;
                }
                if ui
                    .selectable_label(self.view == View::Diff, "Diff")
                    .clicked()
                {
                    self.view = View::Diff;
                }
                ui.separator();
                match self.view {
                    View::Svg => self.svg_toolbar(ui),
                    View::Graph | View::Diff => {}
                }
            });

            if self.view == View::Diff {
                self.diff_ui(ui);
                return;
            }

            if self.view == View::Graph {
                let action = self.graph_panel.ui(ui, &mut self.selected_id);
                if let GraphAction::OpenFile(path) = action
                    && let Some(file) = self
                        .registry
                        .files
                        .iter()
                        .find(|f| f.path == path)
                        .cloned()
                {
                    let target_id = self.selected_id.clone();
                    self.load_file(&file);
                    self.selected_id = target_id;
                    self.view = View::Svg;
                }
                return;
            }

            let rect = ui.available_rect_before_wrap();
            let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());

            if self.needs_fit {
                self.fit_to_rect(rect);
            }

            if resp.dragged() {
                self.pan += resp.drag_delta();
            }

            if resp.hovered() {
                let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                if scroll.abs() > 0.01
                    && let Some(cursor) = resp.hover_pos()
                {
                    let factor = (scroll * 0.005).exp();
                    let cursor_in_canvas = cursor - rect.min;
                    let new_zoom = (self.zoom * factor).clamp(0.05, 20.0);
                    let scale = new_zoom / self.zoom;
                    self.pan = cursor_in_canvas - (cursor_in_canvas - self.pan) * scale;
                    self.zoom = new_zoom;
                }
            }

            let painter = ui.painter().with_clip_rect(rect);
            painter.rect_filled(rect, 0.0, egui::Color32::WHITE);

            let zoom = self.zoom;
            let pan = self.pan;
            let page_idx = self.page_idx;
            let mut raster_err: Option<String> = None;
            let mut painted_page = false;
            if let Some(page) = self.pages.get_mut(page_idx) {
                let needs_rebuild = page.texture.is_none()
                    || (page.cached_zoom / zoom).log2().abs() > 0.5;
                if needs_rebuild
                    && let Err(e) = page.rebuild(ctx, zoom)
                {
                    raster_err = Some(format!("raster: {e}"));
                }
                if let Some(tex) = page.texture.as_ref() {
                    let size = page.intrinsic * zoom;
                    let top_left = rect.min + pan;
                    let draw_rect = egui::Rect::from_min_size(top_left, size);
                    painter.image(
                        tex.id(),
                        draw_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    painted_page = true;
                }
            }
            if let Some(e) = raster_err {
                self.load_error = Some(e);
            }
            if !painted_page && self.selected_file.is_some() {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "No rendered output — run the render pipeline first",
                    egui::FontId::proportional(14.0),
                    egui::Color32::DARK_GRAY,
                );
            }
        });
    }
}

impl MddViewer {
    fn apply_zoom_at_canvas_center(&mut self, factor: f32, canvas_size: egui::Vec2) {
        let center = canvas_size * 0.5;
        let new_zoom = (self.zoom * factor).clamp(0.05, 20.0);
        let scale = new_zoom / self.zoom;
        self.pan = center - (center - self.pan) * scale;
        self.zoom = new_zoom;
    }

    fn svg_toolbar(&mut self, ui: &mut egui::Ui) {
        if self.pages.len() > 1 {
            ui.label(egui::RichText::new("PAGE").small().weak());
            for i in 0..self.pages.len() {
                let active = i == self.page_idx;
                if ui.selectable_label(active, format!("{}", i + 1)).clicked() {
                    self.page_idx = i;
                    self.needs_fit = true;
                }
            }
            ui.separator();
        }
        if ui.button("Reset").clicked() {
            self.pan = self.initial_pan;
            self.zoom = self.initial_zoom;
        }
        if ui.button("Zoom +").clicked() {
            self.apply_zoom_at_canvas_center(1.25, ui.available_rect_before_wrap().size());
        }
        if ui.button("Zoom −").clicked() {
            self.apply_zoom_at_canvas_center(0.8, ui.available_rect_before_wrap().size());
        }
        if let Some(err) = &self.load_error {
            ui.colored_label(egui::Color32::from_rgb(143, 44, 34), err);
        }
    }

    fn diff_ui(&mut self, ui: &mut egui::Ui) {
        if self.cycles.cycles.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.weak("No cycles tracked yet — run /mdd-cycle to record one.");
            });
            return;
        }

        let cycle_meta: Vec<(u32, String, bool)> = self
            .cycles
            .cycles
            .iter()
            .map(|c| (c.number(), c.label(), c.after_dir.is_some()))
            .collect();
        if self.selected_cycle.is_none() {
            self.selected_cycle = cycle_meta
                .iter()
                .rev()
                .find(|(_, _, has_after)| *has_after)
                .or_else(|| cycle_meta.last())
                .map(|(n, _, _)| *n);
        }

        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("CYCLE").small().strong());
            for (number, label, _) in &cycle_meta {
                let active = self.selected_cycle == Some(*number);
                if ui.selectable_label(active, label).clicked() {
                    self.selected_cycle = Some(*number);
                }
            }
        });
        ui.separator();

        let Some(number) = self.selected_cycle else {
            return;
        };
        let has_after = cycle_meta
            .iter()
            .find(|(n, _, _)| *n == number)
            .map(|(_, _, a)| *a)
            .unwrap_or(false);
        if !has_after {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.weak("This cycle is still open — the diff is available after it closes.");
            });
            return;
        }

        if self.diff_cache.as_ref().map(|(n, _)| *n) != Some(number) {
            let diffs = self.project.cycle_diffs(number).unwrap_or_default();
            self.diff_cache = Some((number, diffs));
        }
        let diffs = self
            .diff_cache
            .as_ref()
            .map(|(_, d)| d.clone())
            .unwrap_or_default();

        if diffs.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.weak("No element changes between before/ and after/ for this cycle.");
            });
            return;
        }

        let green = egui::Color32::from_rgb(80, 200, 120);
        let red = egui::Color32::from_rgb(224, 108, 117);
        let neutral = egui::Color32::from_rgb(170, 175, 185);
        let strip = |k: &str| k.trim_start_matches("id:").trim_start_matches("el:").to_string();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for diff in &diffs {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&diff.diagram).strong().size(15.0));
                ui.weak(format!(
                    "{} shared · {} added · {} removed (superposed)",
                    diff.unchanged.len(),
                    diff.added.len(),
                    diff.removed.len()
                ));
                ui.separator();
                for key in &diff.unchanged {
                    ui.label(egui::RichText::new(format!("  {}", strip(key))).color(neutral));
                }
                for key in &diff.added {
                    ui.label(
                        egui::RichText::new(format!("+ {}", strip(key)))
                            .color(green)
                            .strong(),
                    );
                }
                for key in &diff.removed {
                    ui.label(
                        egui::RichText::new(format!("− {}", strip(key)))
                            .color(red)
                            .strong()
                            .strikethrough(),
                    );
                }
                ui.add_space(10.0);
            }
        });
    }
}

impl DiagramPage {
    fn rebuild(&mut self, ctx: &egui::Context, zoom: f32) -> Result<()> {
        let opt = usvg::Options {
            fontdb: shared_fontdb(),
            ..usvg::Options::default()
        };
        let tree = usvg::Tree::from_data(&self.svg_bytes, &opt)
            .with_context(|| format!("invalid SVG: {}", self.rel_path))?;
        let scale = zoom.clamp(0.5, 4.0);
        let w = ((self.intrinsic.x * scale).ceil() as u32).max(1);
        let h = ((self.intrinsic.y * scale).ceil() as u32).max(1);
        let mut pix = tiny_skia::Pixmap::new(w, h)
            .ok_or_else(|| anyhow!("pixmap alloc {}x{}", w, h))?;
        resvg::render(
            &tree,
            tiny_skia::Transform::from_scale(scale, scale),
            &mut pix.as_mut(),
        );
        let rgba = demultiply_rgba(pix.data());
        let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
        self.texture = Some(ctx.load_texture("page", img, egui::TextureOptions::LINEAR));
        self.cached_zoom = zoom;
        Ok(())
    }
}

fn demultiply_rgba(premul: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(premul.len());
    for chunk in premul.chunks_exact(4) {
        let r = chunk[0];
        let g = chunk[1];
        let b = chunk[2];
        let a = chunk[3];
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            let inv = 255.0 / a as f32;
            out.push((r as f32 * inv).min(255.0) as u8);
            out.push((g as f32 * inv).min(255.0) as u8);
            out.push((b as f32 * inv).min(255.0) as u8);
            out.push(a);
        }
    }
    out
}

/// System-font database for the SVG rasterizer, loaded once and shared.
///
/// `usvg::Options::default()` ships an empty font database; PlantUML emits
/// labels as `<text>` nodes, so without loaded fonts resvg silently drops
/// every glyph (shapes render, text does not). Loading system fonts is
/// expensive, so it happens once and the `Arc` is cheaply cloned per parse.
fn shared_fontdb() -> Arc<usvg::fontdb::Database> {
    static FONTDB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTDB
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

fn parse_intrinsic_size(svg_bytes: &[u8]) -> Result<egui::Vec2> {
    let opt = usvg::Options {
        fontdb: shared_fontdb(),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(svg_bytes, &opt).context("invalid SVG")?;
    let size = tree.size();
    Ok(egui::vec2(size.width(), size.height()))
}

fn kind_label(kind: ModelKind) -> &'static str {
    match kind {
        ModelKind::UseCase => "use case",
        ModelKind::Sequence => "sequence",
        ModelKind::Domain => "domain",
        ModelKind::Mockup => "mockup",
        ModelKind::State => "state machine",
        ModelKind::Other => "other",
        ModelKind::Constraint => "constraint",
    }
}

fn short_path(path: &str) -> String {
    path.strip_prefix(".mdd/models/")
        .map(|s| s.to_string())
        .or_else(|| {
            path.strip_prefix(".mdd/constraints/")
                .map(|s| format!("constraints/{s}"))
        })
        .unwrap_or_else(|| path.to_string())
}
