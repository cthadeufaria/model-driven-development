mod theme;
mod tree;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use anyhow::{Context, Result, anyhow};
use eframe::egui;
use mdd_core::cycle::{CycleDiff, CycleRegistry};
use mdd_core::{ModelFile, ModelKind, ModelRegistry, Project, Trace};

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
        Box::new(move |cc| {
            // Tier-1 theming — register bundled fonts once at startup
            // (SEQ-APPLY-THEME construction half). Per-frame theme
            // application happens in MddViewer::update.
            theme::register_fonts(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow!("eframe error: {e}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Svg,
    Diff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RailMode {
    Directory,
    ByCycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffSubMode {
    Diagram,
    List,
}

/// DOM-DIFF-VIEW: viewer Diff-mode state. Diff mode is bound to BOTH the
/// cycle selected in the CYCLE row and the model file selected in the
/// left rail; it renders exactly that one file's change for that cycle
/// (OCL-DIFF-VIEW-BINDING). `sub_mode` toggles the pre-rendered
/// superposed diagram against the per-file element buckets. The Diagram
/// sub-mode paints only when the mirrored `.diff.svg` exists
/// (OCL-DIFF-DIAGRAM-NEEDS-RENDER); otherwise a placeholder is shown.
struct DiffView {
    sub_mode: DiffSubMode,
    /// The (cycle, file) pair currently rasterized into `page`, so the
    /// diff canvas reloads only when the bound selection changes.
    loaded_key: Option<(u32, String)>,
    page: Option<DiagramPage>,
}

impl DiffView {
    /// `<kind>/<name>.puml` key (the `CycleDiff::diagram` form) for a
    /// model file path, or `None` for non-model paths.
    fn diagram_key(model_rel: &str) -> Option<String> {
        let under_side = model_rel.strip_prefix(".mdd/models/")?;
        let (_side, kind_name) = under_side.split_once('/')?;
        Some(kind_name.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OclSubMode {
    Source,
    Diagram,
}

/// DOM-OCL-VIEW: viewer state for a selected `.ocl` constraint file.
/// `Source` paints the raw OCL text (read-only); `Diagram` paints the
/// synthesized constraints SVG on the shared canvas. Diagram paints only
/// when that SVG exists, else a placeholder
/// (OCL-OCL-VIEW-SUBMODE, OCL-OCL-DIAGRAM-NEEDS-RENDER).
struct OclView {
    sub_mode: OclSubMode,
    /// The `.ocl` path currently loaded into `source`/`page`, so we read
    /// and rasterize only when the selection changes.
    loaded: Option<String>,
    source: String,
    page: Option<DiagramPage>,
}

/// DOM-CANVAS-VIEW: base zoom/pan placement of the diagram plus the
/// center-fixed fisheye (focus+context) parameters. The fisheye is a
/// display-time warp layered on top of the base zoom; its focal point
/// is always the canvas-rect centre (OCL-FISHEYE-FOCAL-CENTER).
struct CanvasView {
    /// Base scale, driven only by scroll-at-cursor.
    base_zoom: f32,
    /// Top-left of the scene, in canvas-local coordinates (px). Drag-only.
    pan: egui::Vec2,
    fisheye_enabled: bool,
    /// 0.0 = plain base placement (no warp); clamped to [0, 1].
    fisheye_strength: f32,
    /// Radius of the warp region in px; recomputed per frame from the
    /// canvas rect so the corners stay fixed. Always > 0.
    fisheye_radius: f32,
}

impl CanvasView {
    /// The fisheye focal point: always the canvas-rect centre.
    fn focal(rect: egui::Rect) -> egui::Pos2 {
        rect.center()
    }

    /// Remap a base-placed screen point through the center-fixed fisheye.
    /// Near the focal point the radius is expanded (magnify/bulge); toward
    /// the warp radius it is the identity, so the canvas corners do not
    /// move and content compresses smoothly in between.
    fn warp(&self, p: egui::Pos2, focal: egui::Pos2) -> egui::Pos2 {
        if !self.fisheye_enabled || self.fisheye_strength <= 0.0 {
            return p;
        }
        let v = p - focal;
        let d = v.length();
        if d < 1.0e-3 {
            return p;
        }
        let r = self.fisheye_radius.max(1.0);
        let nd = (d / r).min(1.0);
        let s = self.fisheye_strength.clamp(0.0, 1.0);
        // f(0)=0, f(1)=1, f'(0) > 1 (centre magnifies): a smooth bulge.
        let f = (1.0 - s) * nd + s * (nd * std::f32::consts::FRAC_PI_2).sin();
        focal + v * (r * f / d)
    }

    /// Build a textured triangle mesh of the diagram over `draw_rect`,
    /// warped by the center-fixed fisheye. Texture stays rasterized at
    /// base zoom; only vertex positions are remapped, so text is crisp.
    fn fisheye_mesh(
        &self,
        tex: egui::TextureId,
        draw_rect: egui::Rect,
        focal: egui::Pos2,
    ) -> egui::Mesh {
        const GRID: usize = 56;
        let mut mesh = egui::Mesh::with_texture(tex);
        for j in 0..=GRID {
            for i in 0..=GRID {
                let u = i as f32 / GRID as f32;
                let v = j as f32 / GRID as f32;
                let base = egui::pos2(
                    draw_rect.min.x + u * draw_rect.width(),
                    draw_rect.min.y + v * draw_rect.height(),
                );
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: self.warp(base, focal),
                    uv: egui::pos2(u, v),
                    color: egui::Color32::WHITE,
                });
            }
        }
        let row = (GRID + 1) as u32;
        for j in 0..GRID as u32 {
            for i in 0..GRID as u32 {
                let a = j * row + i;
                let b = a + 1;
                let c = a + row;
                let d = c + 1;
                mesh.indices.extend_from_slice(&[a, b, c, b, d, c]);
            }
        }
        mesh
    }
}

/// DOM-PANEL-LAYOUT: collapse state of the two side panels. A collapsed
/// panel still renders a positive-width sliver so it can always be
/// re-expanded (OCL-PANEL-SLIVER-MIN).
struct PanelLayout {
    left_collapsed: bool,
    right_collapsed: bool,
    sliver_width: f32,
}

impl PanelLayout {
    fn toggle_left(&mut self) {
        self.left_collapsed = !self.left_collapsed;
    }

    fn toggle_right(&mut self) {
        self.right_collapsed = !self.right_collapsed;
    }
}

struct MddViewer {
    project: Project,
    registry: ModelRegistry,
    trace: Trace,
    selected_file: Option<String>,
    selected_id: Option<String>,
    pages: Vec<DiagramPage>,
    page_idx: usize,
    canvas: CanvasView,
    panels: PanelLayout,
    needs_fit: bool,
    load_error: Option<String>,
    view: View,
    descriptions: BTreeMap<String, String>,
    cycles: CycleRegistry,
    rail_mode: RailMode,
    selected_cycle: Option<u32>,
    diff_cache: Option<(u32, Vec<CycleDiff>)>,
    diff_view: DiffView,
    ocl_view: OclView,
    /// CMP-DEPLOY-VIEWER-SOURCE: the third rail source. `/mdd-deploy`
    /// diagrams (`.mdd/deploy/**/*.puml`), shown in a dedicated DEPLOY
    /// section that is OUTSIDE the parity gate. Never part of
    /// `registry`, so `/mdd-validate` and `/mdd-review` keep ignoring
    /// `.mdd/deploy/`.
    deploy_files: Vec<ModelFile>,
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
        let descriptions = project.descriptions().unwrap_or_default();
        let cycles = project.cycle_registry().unwrap_or_default();
        let deploy_files = project.deploy_files().unwrap_or_default();
        let mut viewer = Self {
            project,
            registry,
            trace,
            selected_file: None,
            selected_id: None,
            pages: Vec::new(),
            page_idx: 0,
            canvas: CanvasView {
                base_zoom: 1.0,
                pan: egui::Vec2::ZERO,
                fisheye_enabled: true,
                fisheye_strength: 0.55,
                fisheye_radius: 1.0,
            },
            panels: PanelLayout {
                left_collapsed: false,
                right_collapsed: false,
                sliver_width: 18.0,
            },
            needs_fit: false,
            load_error: None,
            view: View::Svg,
            descriptions,
            cycles,
            rail_mode: RailMode::Directory,
            selected_cycle: None,
            diff_cache: None,
            diff_view: DiffView {
                sub_mode: DiffSubMode::Diagram,
                loaded_key: None,
                page: None,
            },
            ocl_view: OclView {
                sub_mode: OclSubMode::Source,
                loaded: None,
                source: String::new(),
                page: None,
            },
            deploy_files,
        };
        // Open the first model file; if there are none (e.g. a deploy-only
        // project like ../atlas-ate-server), fall back to the first deploy
        // diagram so `mdd view` is not blank.
        let first = viewer
            .registry
            .files
            .first()
            .or_else(|| viewer.deploy_files.first())
            .cloned();
        if let Some(first) = first {
            viewer.load_file(&first);
        }
        Ok(viewer)
    }

    /// Find a rail file by path across BOTH the parity-gated model
    /// registry and the non-gated DEPLOY source.
    fn lookup_file(&self, path: &str) -> Option<ModelFile> {
        self.registry
            .files
            .iter()
            .chain(self.deploy_files.iter())
            .find(|f| f.path == path)
            .cloned()
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

    /// Paint one optional `DiagramPage` on the canvas with scroll-zoom,
    /// drag-pan and the center-fixed fisheye. Shared by the Diagram view
    /// and the Diff Diagram sub-mode so both reuse the exact same
    /// pan/zoom/fisheye behavior. Returns a raster error string, if any.
    /// When there is no page, paints `placeholder` centered (skipped if
    /// `placeholder` is empty).
    fn paint_canvas(
        canvas: &mut CanvasView,
        page: Option<&mut DiagramPage>,
        needs_fit: &mut bool,
        placeholder: &str,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> Option<String> {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());

        if *needs_fit
            && let Some(p) = page.as_deref()
        {
            let margin = 0.95_f32;
            let zoom_x = rect.width() / p.intrinsic.x.max(1.0);
            let zoom_y = rect.height() / p.intrinsic.y.max(1.0);
            let zoom = (zoom_x.min(zoom_y) * margin).clamp(0.05, 20.0);
            canvas.base_zoom = zoom;
            canvas.pan = (rect.size() - p.intrinsic * zoom) * 0.5;
            *needs_fit = false;
        }

        if resp.dragged() {
            canvas.pan += resp.drag_delta();
        }
        if resp.hovered() {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.01
                && let Some(cursor) = resp.hover_pos()
            {
                let factor = (scroll * 0.005).exp();
                let cursor_in_canvas = cursor - rect.min;
                let base = canvas.base_zoom;
                let new_zoom = (base * factor).clamp(0.05, 20.0);
                let scale = new_zoom / base;
                canvas.pan = cursor_in_canvas - (cursor_in_canvas - canvas.pan) * scale;
                canvas.base_zoom = new_zoom;
            }
        }

        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::WHITE);

        let zoom = canvas.base_zoom;
        let pan = canvas.pan;
        // Center-fixed fisheye: focal = canvas centre, warp radius spans
        // to the farthest corner so the corners stay put while the centre
        // bulges. (OCL-FISHEYE-FOCAL-CENTER)
        let focal = CanvasView::focal(rect);
        canvas.fisheye_radius = (rect.max - focal)
            .length()
            .max((focal - rect.min).length())
            .max(1.0);

        let mut raster_err: Option<String> = None;
        let mut painted: Option<(egui::TextureId, egui::Rect)> = None;
        if let Some(p) = page {
            let needs_rebuild =
                p.texture.is_none() || (p.cached_zoom / zoom).log2().abs() > 0.5;
            if needs_rebuild
                && let Err(e) = p.rebuild(ctx, zoom)
            {
                raster_err = Some(format!("raster: {e}"));
            }
            if let Some(tex) = p.texture.as_ref() {
                let size = p.intrinsic * zoom;
                let draw_rect = egui::Rect::from_min_size(rect.min + pan, size);
                painted = Some((tex.id(), draw_rect));
            }
        }
        if let Some((tex_id, draw_rect)) = painted {
            let mesh = canvas.fisheye_mesh(tex_id, draw_rect, focal);
            painter.add(egui::Shape::mesh(mesh));
        } else if !placeholder.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                placeholder,
                egui::FontId::proportional(14.0),
                egui::Color32::DARK_GRAY,
            );
        }
        raster_err
    }
}

impl eframe::App for MddViewer {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        // Tier-1 theming (SEQ-APPLY-THEME, per-frame half): reapply
        // catppuccin + spacing/rounding each frame so flipping OS
        // appearance live (System Settings → Appearance) flips the
        // viewer chrome on the next paint.
        theme::apply_theme(ctx);

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

        let deploy_files = self.deploy_files.clone();
        let deploy_tree =
            TreeNode::build(deploy_files.iter(), short_path, |_| "deployment");

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
        let left_collapsed = self.panels.left_collapsed;
        let left_sliver = self.panels.sliver_width;
        let mut toggle_left = false;
        let left_panel = egui::SidePanel::left("rail");
        let left_panel = if left_collapsed {
            left_panel.resizable(false).exact_width(left_sliver)
        } else {
            left_panel.resizable(true).default_width(280.0)
        };
        left_panel.show(ctx, |ui| {
                if left_collapsed {
                    let sz = ui.available_size();
                    if ui
                        .add_sized(sz, egui::Button::new(">"))
                        .on_hover_text("Expand MODELS panel")
                        .clicked()
                    {
                        toggle_left = true;
                    }
                    return;
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("MODELS").small().strong());
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui
                                .button("<")
                                .on_hover_text("Collapse MODELS panel")
                                .clicked()
                            {
                                toggle_left = true;
                            }
                        },
                    );
                });
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
                egui::ScrollArea::vertical().show(ui, |ui| {
                    match rail_mode {
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
                    }

                    // CMP-DEPLOY-VIEWER-SOURCE: the third source. Shown in
                    // BOTH rail modes (it is not cycle-scoped) and visibly
                    // labelled as a utility outside the parity gate.
                    if !deploy_files.is_empty() {
                        ui.add_space(8.0);
                        let resp = egui::CollapsingHeader::new(
                            egui::RichText::new("DEPLOY").strong(),
                        )
                        .id_salt("deploy")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(
                                    "/mdd-deploy · utility, not parity-gated",
                                )
                                .small()
                                .weak(),
                            );
                            deploy_tree.ui(ui, selected_file.as_deref(), "deploy")
                        });
                        if let Some(Some(path)) = resp.body_returned {
                            clicked = Some(path);
                        }
                    }
                });
            });
        if toggle_left {
            self.panels.toggle_left();
        }
        self.rail_mode = new_mode;
        if let Some(path) = clicked
            && let Some(file) = self.lookup_file(&path)
        {
            self.load_file(&file);
        }

        let right_collapsed = self.panels.right_collapsed;
        let right_sliver = self.panels.sliver_width;
        let mut toggle_right = false;
        let right_panel = egui::SidePanel::right("context");
        let right_panel = if right_collapsed {
            right_panel.resizable(false).exact_width(right_sliver)
        } else {
            right_panel.resizable(true).default_width(320.0)
        };
        right_panel.show(ctx, |ui| {
                if right_collapsed {
                    let sz = ui.available_size();
                    if ui
                        .add_sized(sz, egui::Button::new("<"))
                        .on_hover_text("Expand MODEL CONTEXT panel")
                        .clicked()
                    {
                        toggle_right = true;
                    }
                    return;
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui
                        .button(">")
                        .on_hover_text("Collapse MODEL CONTEXT panel")
                        .clicked()
                    {
                        toggle_right = true;
                    }
                    ui.label(egui::RichText::new("MODEL CONTEXT").small().strong());
                });
                ui.separator();
                let active_file = self
                    .selected_file
                    .as_ref()
                    .and_then(|p| self.lookup_file(p));
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
        if toggle_right {
            self.panels.toggle_right();
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(self.view == View::Svg, "Diagram")
                    .clicked()
                {
                    self.view = View::Svg;
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
                    View::Diff => {}
                }
            });

            if self.view == View::Diff {
                self.diff_ui(ui, ctx);
                return;
            }

            // An .ocl constraint file is not a PlantUML diagram: show it
            // with a Source | Diagram toggle instead of the empty canvas.
            let is_constraint = self
                .selected_file
                .as_ref()
                .and_then(|p| self.registry.files.iter().find(|f| &f.path == p))
                .is_some_and(|f| f.kind == ModelKind::Constraint);
            if is_constraint {
                self.ocl_ui(ui, ctx);
                return;
            }

            let placeholder = if self.selected_file.is_some() {
                "No rendered output — run the render pipeline first"
            } else {
                ""
            };
            let page = self.pages.get_mut(self.page_idx);
            if let Some(err) = Self::paint_canvas(
                &mut self.canvas,
                page,
                &mut self.needs_fit,
                placeholder,
                ui,
                ctx,
            ) {
                self.load_error = Some(err);
            }
        });
    }
}

impl MddViewer {
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
        ui.label(
            egui::RichText::new("scroll = zoom · drag = pan · centre = fisheye focus")
                .small()
                .weak(),
        );
        if let Some(err) = &self.load_error {
            ui.colored_label(egui::Color32::from_rgb(143, 44, 34), err);
        }
    }

    fn diff_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.cycles.cycles.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.weak("No cycles tracked yet — run /mdd-cycle to record one.");
            });
            return;
        }

        // Diff mode is keyed on the rail-selected file.
        let Some(file_rel) = self.selected_file.clone() else {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.weak("Select a model file in the rail to see its diff.");
            });
            return;
        };

        // Offer ONLY the cycles that actually changed this file
        // (OCL-DIFF-CYCLE-SCOPED), newest first — never a global picker.
        let mut cycle_nums = self
            .project
            .cycles_with_diff_for(&file_rel)
            .unwrap_or_default();
        cycle_nums.sort_unstable();
        cycle_nums.reverse();
        if cycle_nums.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.weak("This diagram didn't change in any cycle — nothing to diff.");
            });
            return;
        }
        // Constrain the active cycle to this file's set; default latest.
        if self
            .selected_cycle
            .map(|n| !cycle_nums.contains(&n))
            .unwrap_or(true)
        {
            self.selected_cycle = cycle_nums.first().copied();
        }
        let cycle_labels: Vec<(u32, String)> = cycle_nums
            .iter()
            .map(|n| {
                (
                    *n,
                    self.cycles
                        .cycle(*n)
                        .map(|c| c.label())
                        .unwrap_or_else(|| format!("Cycle {n:04}")),
                )
            })
            .collect();

        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("CYCLE").small().strong());
            if let [(_, only)] = cycle_labels.as_slice() {
                ui.label(egui::RichText::new(only).small().strong());
            } else {
                for (n, label) in &cycle_labels {
                    let active = self.selected_cycle == Some(*n);
                    if ui.selectable_label(active, label).clicked() {
                        self.selected_cycle = Some(*n);
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("VIEW").small().strong());
            if ui
                .selectable_label(
                    self.diff_view.sub_mode == DiffSubMode::Diagram,
                    "Diagram",
                )
                .clicked()
            {
                self.diff_view.sub_mode = DiffSubMode::Diagram;
            }
            if ui
                .selectable_label(self.diff_view.sub_mode == DiffSubMode::List, "List")
                .clicked()
            {
                self.diff_view.sub_mode = DiffSubMode::List;
            }
            ui.separator();
            ui.label(
                egui::RichText::new("only cycles that changed this diagram")
                    .small()
                    .weak(),
            );
        });
        ui.separator();

        let Some(number) = self.selected_cycle else {
            return;
        };

        if self.diff_cache.as_ref().map(|(n, _)| *n) != Some(number) {
            let diffs = self.project.cycle_diffs(number).unwrap_or_default();
            self.diff_cache = Some((number, diffs));
        }
        let Some(key) = DiffView::diagram_key(&file_rel) else {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.weak("This file is not a model diagram.");
            });
            return;
        };
        let diff = self
            .diff_cache
            .as_ref()
            .and_then(|(_, d)| d.iter().find(|d| d.diagram == key))
            .cloned();
        let Some(diff) = diff else {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.weak(format!(
                    "“{key}” did not change in cycle {number:04} — nothing to diff."
                ));
            });
            return;
        };

        match self.diff_view.sub_mode {
            DiffSubMode::List => {
                let green = egui::Color32::from_rgb(80, 200, 120);
                let red = egui::Color32::from_rgb(224, 108, 117);
                let neutral = egui::Color32::from_rgb(170, 175, 185);
                let strip = |k: &str| {
                    k.trim_start_matches("id:")
                        .trim_start_matches("el:")
                        .to_string()
                };
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&diff.diagram).strong().size(15.0));
                ui.weak(format!(
                    "{} shared · {} added · {} removed (superposed)",
                    diff.unchanged.len(),
                    diff.added.len(),
                    diff.removed.len()
                ));
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for k in &diff.unchanged {
                        ui.label(
                            egui::RichText::new(format!("  {}", strip(k))).color(neutral),
                        );
                    }
                    for k in &diff.added {
                        ui.label(
                            egui::RichText::new(format!("+ {}", strip(k)))
                                .color(green)
                                .strong(),
                        );
                    }
                    for k in &diff.removed {
                        ui.label(
                            egui::RichText::new(format!("− {}", strip(k)))
                                .color(red)
                                .strong()
                                .strikethrough(),
                        );
                    }
                });
            }
            DiffSubMode::Diagram => {
                let puml_rel = self
                    .cycles
                    .cycle(number)
                    .and_then(|c| c.diff_puml_rel(&file_rel));
                let svg_abs = puml_rel
                    .as_ref()
                    .map(|r| self.project.rendered_svg_path(r));

                let bind_key = (number, file_rel.clone());
                if self.diff_view.loaded_key.as_ref() != Some(&bind_key) {
                    self.diff_view.page = None;
                    if let Some(abs) = svg_abs.as_ref().filter(|p| p.is_file()) {
                        let rel = puml_rel.clone().unwrap_or_default();
                        match DiagramPage::load(abs, rel) {
                            Ok(p) => {
                                self.diff_view.page = Some(p);
                                self.needs_fit = true;
                            }
                            Err(e) => self.load_error = Some(e.to_string()),
                        }
                    }
                    self.diff_view.loaded_key = Some(bind_key);
                }

                let placeholder = if self.diff_view.page.is_none() {
                    "No rendered diff for this file — run the render pipeline (/mdd-render)"
                } else {
                    ""
                };
                if let Some(err) = Self::paint_canvas(
                    &mut self.canvas,
                    self.diff_view.page.as_mut(),
                    &mut self.needs_fit,
                    placeholder,
                    ui,
                    ctx,
                ) {
                    self.load_error = Some(err);
                }
            }
        }
    }

    /// SEQ-VIEW-OCL: render the selected `.ocl` file with a
    /// Source | Diagram toggle. Source paints the raw OCL text;
    /// Diagram paints the synthesized constraints SVG on the shared
    /// canvas (placeholder when it has not been rendered).
    fn ocl_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(file_rel) = self.selected_file.clone() else {
            return;
        };

        if self.ocl_view.loaded.as_deref() != Some(file_rel.as_str()) {
            let abs = self.project.root().join(&file_rel);
            self.ocl_view.source = std::fs::read_to_string(&abs)
                .unwrap_or_else(|e| format!("(could not read {file_rel}: {e})"));
            self.ocl_view.page = None;
            self.ocl_view.loaded = Some(file_rel.clone());
            self.needs_fit = true;
        }

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("VIEW").small().strong());
            if ui
                .selectable_label(
                    self.ocl_view.sub_mode == OclSubMode::Source,
                    "Source",
                )
                .clicked()
            {
                self.ocl_view.sub_mode = OclSubMode::Source;
            }
            if ui
                .selectable_label(
                    self.ocl_view.sub_mode == OclSubMode::Diagram,
                    "Diagram",
                )
                .clicked()
            {
                self.ocl_view.sub_mode = OclSubMode::Diagram;
                self.needs_fit = true;
            }
            ui.separator();
            ui.monospace(&file_rel);
        });
        ui.separator();

        match self.ocl_view.sub_mode {
            OclSubMode::Source => {
                let id_c = egui::Color32::from_rgb(240, 230, 120);
                let ctx_c = egui::Color32::from_rgb(86, 156, 214);
                let inv_c = egui::Color32::from_rgb(80, 200, 120);
                egui::ScrollArea::both().show(ui, |ui| {
                    for raw in self.ocl_view.source.lines() {
                        let t = raw.trim_start();
                        let color = if t.starts_with("-- @id(")
                            || t.starts_with("-- @ref(")
                        {
                            Some(id_c)
                        } else if t.starts_with("context ") {
                            Some(ctx_c)
                        } else if t.starts_with("inv ") {
                            Some(inv_c)
                        } else {
                            None
                        };
                        let text = if raw.is_empty() { " " } else { raw };
                        let mut rt = egui::RichText::new(text).monospace();
                        if let Some(c) = color {
                            rt = rt.color(c).strong();
                        }
                        ui.label(rt);
                    }
                });
            }
            OclSubMode::Diagram => {
                let svg_abs = self.project.rendered_svg_path(&file_rel);
                if self.ocl_view.page.is_none()
                    && svg_abs.is_file()
                {
                    match DiagramPage::load(
                        &svg_abs,
                        file_rel.replace(".mdd/", ""),
                    ) {
                        Ok(p) => {
                            self.ocl_view.page = Some(p);
                            self.needs_fit = true;
                        }
                        Err(e) => self.load_error = Some(e.to_string()),
                    }
                }
                let placeholder = if self.ocl_view.page.is_none() {
                    "No rendered constraints diagram — run the render pipeline (/mdd-render)"
                } else {
                    ""
                };
                if let Some(err) = Self::paint_canvas(
                    &mut self.canvas,
                    self.ocl_view.page.as_mut(),
                    &mut self.needs_fit,
                    placeholder,
                    ui,
                    ctx,
                ) {
                    self.load_error = Some(err);
                }
            }
        }
    }
}

impl DiagramPage {
    /// Load a single rendered SVG into an unrasterized page. Used for the
    /// Diff Diagram sub-mode, which paints one cycle diff SVG rather than
    /// a model file's page set.
    fn load(abs: &std::path::Path, rel: String) -> Result<Self> {
        let bytes = std::fs::read(abs)
            .with_context(|| format!("read {}", abs.display()))?;
        let intrinsic = parse_intrinsic_size(&bytes)
            .with_context(|| format!("parse {}", abs.display()))?;
        Ok(Self {
            rel_path: rel,
            svg_bytes: bytes,
            intrinsic,
            texture: None,
            cached_zoom: 0.0,
        })
    }

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
        .or_else(|| {
            path.strip_prefix(".mdd/deploy/")
                .map(|s| format!("deploy/{s}"))
        })
        .unwrap_or_else(|| path.to_string())
}
