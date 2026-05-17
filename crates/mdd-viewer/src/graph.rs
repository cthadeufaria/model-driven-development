use std::collections::{HashMap, HashSet, VecDeque};

use eframe::egui;
use mdd_core::{ModelKind, ModelRegistry, Trace};
use petgraph::stable_graph::{NodeIndex, StableUnGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};

#[derive(Debug, Clone)]
pub struct NodeAttr {
    pub id: String,
    pub kind: ModelKind,
    pub file: String,
}

#[derive(Debug, Clone)]
pub struct EdgeAttr {
    pub relation: String,
}

pub struct GraphData {
    pub graph: StableUnGraph<NodeAttr, EdgeAttr>,
    pub by_id: HashMap<String, NodeIndex>,
}

impl GraphData {
    pub fn build(registry: &ModelRegistry, trace: &Trace) -> Self {
        let mut graph = StableUnGraph::<NodeAttr, EdgeAttr>::default();
        let mut by_id: HashMap<String, NodeIndex> = HashMap::new();

        for element in &registry.ids {
            let idx = graph.add_node(NodeAttr {
                id: element.id.clone(),
                kind: element.kind,
                file: element.file.clone(),
            });
            by_id.insert(element.id.clone(), idx);
        }

        for link in &trace.links {
            for endpoint in [&link.from, &link.to] {
                if !by_id.contains_key(endpoint) {
                    let idx = graph.add_node(NodeAttr {
                        id: endpoint.clone(),
                        kind: ModelKind::Other,
                        file: String::new(),
                    });
                    by_id.insert(endpoint.clone(), idx);
                }
            }
            let a = by_id[&link.from];
            let b = by_id[&link.to];
            graph.add_edge(
                a,
                b,
                EdgeAttr {
                    relation: link.relation.clone(),
                },
            );
        }

        Self { graph, by_id }
    }

    pub fn neighbors_within(&self, start: NodeIndex, hops: usize) -> HashSet<NodeIndex> {
        let mut visited: HashSet<NodeIndex> = HashSet::new();
        visited.insert(start);
        let mut frontier: VecDeque<(NodeIndex, usize)> = VecDeque::new();
        frontier.push_back((start, 0));
        while let Some((node, depth)) = frontier.pop_front() {
            if depth >= hops {
                continue;
            }
            for neighbor in self.graph.neighbors(node) {
                if visited.insert(neighbor) {
                    frontier.push_back((neighbor, depth + 1));
                }
            }
        }
        visited
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ForceParams {
    pub repel: f32,
    pub link_distance: f32,
    pub link_strength: f32,
    pub center: f32,
    pub collide: f32,
    pub velocity_decay: f32,
    pub alpha_decay: f32,
}

impl Default for ForceParams {
    fn default() -> Self {
        Self {
            repel: 600.0,
            link_distance: 90.0,
            link_strength: 1.0,
            center: 0.02,
            collide: 14.0,
            velocity_decay: 0.6,
            alpha_decay: (0.001_f32).powf(1.0 / 300.0),
        }
    }
}

pub struct ForceSim {
    pub positions: HashMap<NodeIndex, egui::Vec2>,
    pub velocities: HashMap<NodeIndex, egui::Vec2>,
    pub alpha: f32,
}

impl ForceSim {
    pub fn new(graph: &StableUnGraph<NodeAttr, EdgeAttr>) -> Self {
        let nodes: Vec<NodeIndex> = graph.node_indices().collect();
        let n = nodes.len().max(1) as f32;
        let radius = 30.0 + 6.0 * n.sqrt();
        let mut positions = HashMap::with_capacity(nodes.len());
        let mut velocities = HashMap::with_capacity(nodes.len());
        for (i, idx) in nodes.iter().enumerate() {
            let theta = (i as f32 / n) * std::f32::consts::TAU;
            positions.insert(*idx, egui::vec2(theta.cos() * radius, theta.sin() * radius));
            velocities.insert(*idx, egui::Vec2::ZERO);
        }
        Self {
            positions,
            velocities,
            alpha: 1.0,
        }
    }

    pub fn reheat(&mut self) {
        self.alpha = 1.0;
    }

    pub fn step(
        &mut self,
        graph: &StableUnGraph<NodeAttr, EdgeAttr>,
        p: &ForceParams,
        active: Option<&HashSet<NodeIndex>>,
    ) {
        if self.alpha <= 1e-3 {
            return;
        }
        let nodes: Vec<NodeIndex> = graph
            .node_indices()
            .filter(|n| active.is_none_or(|a| a.contains(n)))
            .collect();
        if nodes.is_empty() {
            return;
        }
        let mut forces: HashMap<NodeIndex, egui::Vec2> =
            nodes.iter().map(|n| (*n, egui::Vec2::ZERO)).collect();

        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let a = nodes[i];
                let b = nodes[j];
                let pa = self.positions[&a];
                let pb = self.positions[&b];
                let mut delta = pb - pa;
                let mut dist2 = delta.length_sq();
                if dist2 < 1e-4 {
                    delta = egui::vec2((j as f32 * 0.31).sin(), (j as f32 * 0.17).cos());
                    dist2 = 1.0;
                }
                let dist = dist2.sqrt();
                let force_mag = p.repel / dist2;
                let dir = delta / dist;
                *forces.get_mut(&a).unwrap() -= dir * force_mag;
                *forces.get_mut(&b).unwrap() += dir * force_mag;
            }
        }

        for edge in graph.edge_references() {
            let a = edge.source();
            let b = edge.target();
            if !forces.contains_key(&a) || !forces.contains_key(&b) {
                continue;
            }
            let pa = self.positions[&a];
            let pb = self.positions[&b];
            let delta = pb - pa;
            let dist = delta.length().max(0.001);
            let displacement = dist - p.link_distance;
            let force = delta / dist * (displacement * p.link_strength);
            *forces.get_mut(&a).unwrap() += force * 0.5;
            *forces.get_mut(&b).unwrap() -= force * 0.5;
        }

        for n in &nodes {
            let pos = self.positions[n];
            *forces.get_mut(n).unwrap() -= pos * p.center;
        }

        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                let a = nodes[i];
                let b = nodes[j];
                let pa = self.positions[&a];
                let pb = self.positions[&b];
                let delta = pb - pa;
                let dist = delta.length().max(0.001);
                let min_dist = p.collide * 2.0;
                if dist < min_dist {
                    let overlap = (min_dist - dist) * 0.5;
                    let dir = delta / dist;
                    *forces.get_mut(&a).unwrap() -= dir * overlap;
                    *forces.get_mut(&b).unwrap() += dir * overlap;
                }
            }
        }

        for n in &nodes {
            let f = forces[n];
            let v = self.velocities.get_mut(n).unwrap();
            *v = (*v + f) * p.velocity_decay;
            let v_now = *v;
            let pos = self.positions.get_mut(n).unwrap();
            *pos += v_now * self.alpha;
        }

        self.alpha *= p.alpha_decay;
    }
}

#[derive(Debug, Clone)]
pub enum GraphAction {
    None,
    OpenFile(String),
}

pub struct GraphPanel {
    pub data: GraphData,
    pub sim: ForceSim,
    pub params: ForceParams,
    pub hop_depth: usize,
    pub local_mode: bool,
    pub show_forces: bool,
    pub pan: egui::Vec2,
    pub zoom: f32,
    pub hover_id: Option<String>,
}

impl GraphPanel {
    pub fn new(data: GraphData) -> Self {
        let sim = ForceSim::new(&data.graph);
        Self {
            data,
            sim,
            params: ForceParams::default(),
            hop_depth: 1,
            local_mode: false,
            show_forces: false,
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            hover_id: None,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, selected_id: &mut Option<String>) -> GraphAction {
        let mut action = GraphAction::None;

        ui.horizontal(|ui| {
            if ui.checkbox(&mut self.local_mode, "Local graph").changed() {
                self.sim.reheat();
            }
            ui.separator();
            ui.label("Hop depth");
            if ui
                .add(egui::Slider::new(&mut self.hop_depth, 1..=4).integer())
                .changed()
            {
                self.sim.reheat();
            }
            ui.separator();
            if ui.button("Reset view").clicked() {
                self.pan = egui::Vec2::ZERO;
                self.zoom = 1.0;
                self.sim.reheat();
            }
            ui.toggle_value(&mut self.show_forces, "Forces");
            ui.separator();
            ui.weak(format!(
                "{} nodes  ·  {} edges",
                self.data.graph.node_count(),
                self.data.graph.edge_count()
            ));
        });

        if self.show_forces {
            ui.horizontal_wrapped(|ui| {
                let p = &mut self.params;
                let mut changed = false;
                changed |= ui
                    .add(egui::Slider::new(&mut p.repel, 50.0..=2000.0).text("repel"))
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut p.link_distance, 20.0..=300.0).text("link dist"),
                    )
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut p.link_strength, 0.05..=3.0).text("link str"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut p.center, 0.0..=0.2).text("center"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut p.collide, 0.0..=40.0).text("collide"))
                    .changed();
                if changed {
                    self.sim.reheat();
                }
            });
        }

        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());

        let active_set: Option<HashSet<NodeIndex>> = if self.local_mode {
            selected_id
                .as_ref()
                .and_then(|id| self.data.by_id.get(id))
                .map(|&idx| self.data.neighbors_within(idx, self.hop_depth))
        } else {
            None
        };

        for _ in 0..2 {
            self.sim
                .step(&self.data.graph, &self.params, active_set.as_ref());
        }
        if self.sim.alpha > 1e-3 {
            ui.ctx().request_repaint();
        }

        if resp.dragged() {
            self.pan += resp.drag_delta();
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.01
                && let Some(cursor) = resp.hover_pos()
            {
                let factor = (scroll * 0.005).exp();
                let cursor_in_canvas = cursor - rect.center();
                let new_zoom = (self.zoom * factor).clamp(0.1, 6.0);
                let scale = new_zoom / self.zoom;
                self.pan = cursor_in_canvas - (cursor_in_canvas - self.pan) * scale;
                self.zoom = new_zoom;
            }
        }

        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(20, 22, 28));

        let center = rect.center();
        let pan = self.pan;
        let zoom = self.zoom;
        let to_screen = |p: egui::Vec2| -> egui::Pos2 { center + (p * zoom) + pan };

        let cursor = resp.hover_pos();
        let mut hovered_node: Option<NodeIndex> = None;
        let mut hovered_dist_sq = f32::MAX;
        for idx in self.data.graph.node_indices() {
            if let Some(set) = &active_set
                && !set.contains(&idx)
            {
                continue;
            }
            let pos_world = self.sim.positions[&idx];
            let screen = to_screen(pos_world);
            if let Some(c) = cursor {
                let d = (screen - c).length_sq();
                let r = self.node_radius(idx);
                if d < r * r && d < hovered_dist_sq {
                    hovered_dist_sq = d;
                    hovered_node = Some(idx);
                }
            }
        }

        let highlight: Option<HashSet<NodeIndex>> =
            hovered_node.map(|n| self.data.neighbors_within(n, self.hop_depth));

        for edge in self.data.graph.edge_references() {
            let a = edge.source();
            let b = edge.target();
            if let Some(set) = &active_set
                && (!set.contains(&a) || !set.contains(&b))
            {
                continue;
            }
            let pa = to_screen(self.sim.positions[&a]);
            let pb = to_screen(self.sim.positions[&b]);
            let highlighted = highlight
                .as_ref()
                .is_some_and(|h| h.contains(&a) && h.contains(&b));
            let alpha = if highlight.is_some() && !highlighted {
                30
            } else {
                110
            };
            let width = if highlighted { 1.8 } else { 1.0 };
            painter.line_segment(
                [pa, pb],
                egui::Stroke::new(
                    width,
                    egui::Color32::from_rgba_unmultiplied(180, 180, 200, alpha),
                ),
            );
            if highlighted && hovered_node.is_some() {
                let mid = pa + (pb - pa) * 0.5;
                painter.text(
                    mid,
                    egui::Align2::CENTER_CENTER,
                    &edge.weight().relation,
                    egui::FontId::proportional(9.0),
                    egui::Color32::from_rgba_unmultiplied(200, 200, 210, 180),
                );
            }
        }

        for idx in self.data.graph.node_indices() {
            if let Some(set) = &active_set
                && !set.contains(&idx)
            {
                continue;
            }
            let attr = &self.data.graph[idx];
            let pos = to_screen(self.sim.positions[&idx]);
            let radius = self.node_radius(idx);
            let in_highlight = highlight.as_ref().is_none_or(|h| h.contains(&idx));
            let base = kind_color(attr.kind);
            let fill = if in_highlight {
                base
            } else {
                egui::Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 64)
            };
            let stroke = if selected_id.as_deref() == Some(attr.id.as_str()) {
                egui::Stroke::new(2.5, egui::Color32::from_rgb(240, 230, 120))
            } else {
                egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, 180))
            };
            painter.circle(pos, radius, fill, stroke);

            let show_label =
                in_highlight && (zoom > 0.85 || hovered_node == Some(idx) || highlight.is_some());
            if show_label {
                painter.text(
                    pos + egui::vec2(radius + 4.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    &attr.id,
                    egui::FontId::proportional(11.0),
                    egui::Color32::from_rgb(220, 220, 225),
                );
            }
        }

        if resp.clicked()
            && let Some(n) = hovered_node
        {
            *selected_id = Some(self.data.graph[n].id.clone());
        }
        if resp.double_clicked()
            && let Some(n) = hovered_node
        {
            let file = self.data.graph[n].file.clone();
            if !file.is_empty() {
                action = GraphAction::OpenFile(file);
            }
        }
        self.hover_id = hovered_node.map(|n| self.data.graph[n].id.clone());

        action
    }

    fn node_radius(&self, idx: NodeIndex) -> f32 {
        let degree = self.data.graph.neighbors(idx).count();
        (4.0 + 1.4 * (degree as f32).sqrt()).min(22.0)
    }
}

fn kind_color(kind: ModelKind) -> egui::Color32 {
    match kind {
        ModelKind::UseCase => egui::Color32::from_rgb(86, 156, 214),
        ModelKind::Sequence => egui::Color32::from_rgb(78, 201, 176),
        ModelKind::Domain => egui::Color32::from_rgb(197, 134, 192),
        ModelKind::Mockup => egui::Color32::from_rgb(220, 120, 178),
        ModelKind::State => egui::Color32::from_rgb(148, 200, 102),
        ModelKind::Constraint => egui::Color32::from_rgb(190, 145, 100),
        ModelKind::Other => egui::Color32::from_rgb(150, 150, 150),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdd_core::{ModelElement, ModelRegistry, ModelSide, Trace, TraceLink};

    fn registry(elements: &[(&str, ModelKind, &str)]) -> ModelRegistry {
        ModelRegistry {
            files: Vec::new(),
            ids: elements
                .iter()
                .map(|(id, kind, file)| ModelElement {
                    id: (*id).to_string(),
                    file: (*file).to_string(),
                    kind: *kind,
                    side: ModelSide::Current,
                })
                .collect(),
        }
    }

    fn trace(links: &[(&str, &str, &str)]) -> Trace {
        Trace {
            version: 1,
            links: links
                .iter()
                .map(|(from, to, rel)| TraceLink {
                    from: (*from).to_string(),
                    to: (*to).to_string(),
                    relation: (*rel).to_string(),
                })
                .collect(),
            generated_tests: Vec::new(),
            generated_ui_tests: Vec::new(),
            source_links: Vec::new(),
        }
    }

    #[test]
    fn build_counts_nodes_from_registry_and_synthetic_endpoints() {
        let reg = registry(&[
            ("A", ModelKind::UseCase, "a.puml"),
            ("B", ModelKind::Sequence, "b.puml"),
        ]);
        let tr = trace(&[("A", "B", "realizes"), ("A", "C", "implemented_by")]);
        let data = GraphData::build(&reg, &tr);
        assert_eq!(data.graph.node_count(), 3, "A, B, and synthetic C");
        assert_eq!(data.graph.edge_count(), 2);
        assert!(data.by_id.contains_key("A"));
        assert!(data.by_id.contains_key("C"));
    }

    #[test]
    fn build_dedups_endpoints_across_links() {
        let reg = registry(&[("A", ModelKind::UseCase, "a.puml")]);
        let tr = trace(&[("A", "B", "r"), ("A", "B", "r"), ("B", "C", "r")]);
        let data = GraphData::build(&reg, &tr);
        assert_eq!(data.graph.node_count(), 3);
        assert_eq!(data.graph.edge_count(), 3);
    }

    #[test]
    fn neighbors_within_depth_one_returns_immediate_neighbors() {
        let reg = registry(&[
            ("A", ModelKind::UseCase, "a.puml"),
            ("B", ModelKind::Sequence, "b.puml"),
            ("C", ModelKind::Sequence, "c.puml"),
            ("D", ModelKind::Domain, "d.puml"),
        ]);
        let tr = trace(&[("A", "B", "r"), ("B", "C", "r"), ("C", "D", "r")]);
        let data = GraphData::build(&reg, &tr);
        let a = data.by_id["A"];
        let one_hop = data.neighbors_within(a, 1);
        assert_eq!(one_hop.len(), 2, "self + B");
        assert!(one_hop.contains(&data.by_id["B"]));
        assert!(!one_hop.contains(&data.by_id["C"]));
    }

    #[test]
    fn neighbors_within_depth_two_walks_further() {
        let reg = registry(&[
            ("A", ModelKind::UseCase, "a.puml"),
            ("B", ModelKind::Sequence, "b.puml"),
            ("C", ModelKind::Sequence, "c.puml"),
            ("D", ModelKind::Domain, "d.puml"),
        ]);
        let tr = trace(&[("A", "B", "r"), ("B", "C", "r"), ("C", "D", "r")]);
        let data = GraphData::build(&reg, &tr);
        let a = data.by_id["A"];
        let two_hop = data.neighbors_within(a, 2);
        assert_eq!(two_hop.len(), 3, "A, B, C");
        assert!(!two_hop.contains(&data.by_id["D"]));
    }

    #[test]
    fn neighbors_within_handles_isolated_node() {
        let reg = registry(&[("A", ModelKind::UseCase, "a.puml")]);
        let tr = trace(&[]);
        let data = GraphData::build(&reg, &tr);
        let a = data.by_id["A"];
        let set = data.neighbors_within(a, 3);
        assert_eq!(set.len(), 1);
        assert!(set.contains(&a));
    }
}
