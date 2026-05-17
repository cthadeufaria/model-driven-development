//! VSCode-style collapsible directory tree for the left rail.

use std::collections::BTreeMap;

use eframe::egui;
use mdd_core::ModelFile;

pub struct FileLeaf {
    pub label: String,
    pub path: String,
    pub kind: String,
}

#[derive(Default)]
pub struct TreeNode {
    pub dirs: BTreeMap<String, TreeNode>,
    pub files: Vec<FileLeaf>,
}

impl TreeNode {
    /// Build a nested tree from model files, splitting `strip(path)` on `/`.
    pub fn build<'a>(
        files: impl Iterator<Item = &'a ModelFile>,
        strip: impl Fn(&str) -> String,
        kind_label: impl Fn(&ModelFile) -> &'static str,
    ) -> Self {
        let mut root = TreeNode::default();
        for file in files {
            let rel = strip(&file.path);
            let mut comps: Vec<String> = rel.split('/').map(str::to_string).collect();
            let name = comps.pop().unwrap_or_else(|| rel.clone());
            let mut node = &mut root;
            for comp in comps {
                node = node.dirs.entry(comp).or_default();
            }
            node.files.push(FileLeaf {
                label: name,
                path: file.path.clone(),
                kind: kind_label(file).to_string(),
            });
        }
        root
    }

    /// Render recursively. Returns the path of a clicked leaf, if any.
    pub fn ui(&self, ui: &mut egui::Ui, selected: Option<&str>, salt: &str) -> Option<String> {
        let mut clicked = None;
        for (name, child) in &self.dirs {
            let child_salt = format!("{salt}/{name}");
            let resp = egui::CollapsingHeader::new(egui::RichText::new(name).strong())
                .id_salt(&child_salt)
                .default_open(true)
                .show(ui, |ui| child.ui(ui, selected, &child_salt));
            if let Some(Some(path)) = resp.body_returned {
                clicked = Some(path);
            }
        }
        for leaf in &self.files {
            let active = selected == Some(leaf.path.as_str());
            let resp = ui.add(egui::SelectableLabel::new(
                active,
                egui::WidgetText::from(egui::RichText::new(&leaf.label).strong()),
            ));
            ui.add_space(-4.0);
            ui.label(egui::RichText::new(&leaf.kind).small().weak());
            ui.add_space(2.0);
            if resp.clicked() {
                clicked = Some(leaf.path.clone());
            }
        }
        clicked
    }
}
