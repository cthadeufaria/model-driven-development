//! Cycle tracking: read-side model for the per-cycle store under
//! `.mdd/cycles/`. The `/mdd-cycle` orchestration skill owns the write
//! side (opening cycles, snapshotting `before/` and `after/`, closing).
//! mdd-core only reads manifests and computes before/after diffs so the
//! viewer can render cycle grouping and the superposed diff view.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

pub const CYCLES_DIR: &str = ".mdd/cycles";

/// Where a cycle's run began. `Generate` iff a description was given,
/// `Map` otherwise (the no-description path behaves as `/mdd-map`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum EntryPoint {
    Generate,
    Map,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum CycleStatus {
    Open,
    Closed,
    Aborted,
}

/// On-disk `.mdd/cycles/<n>/manifest.yml`, authored by the skill.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct CycleManifest {
    pub number: u32,
    pub slug: String,
    pub entry: EntryPoint,
    #[serde(default)]
    pub description: String,
    pub status: CycleStatus,
    #[serde(default)]
    pub opened_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<String>,
    #[serde(default)]
    pub touched_files: Vec<String>,
}

/// A resolved cycle: its manifest plus the snapshot directories.
#[derive(Debug, Clone)]
pub struct Cycle {
    pub manifest: CycleManifest,
    pub dir: PathBuf,
    pub before_dir: PathBuf,
    pub after_dir: Option<PathBuf>,
}

impl Cycle {
    pub fn number(&self) -> u32 {
        self.manifest.number
    }

    pub fn is_closed(&self) -> bool {
        self.manifest.status == CycleStatus::Closed
    }

    /// Human label for the rail, e.g. `Cycle 0002 — tree-rail (closed)`.
    pub fn label(&self) -> String {
        let status = match self.manifest.status {
            CycleStatus::Open => "open",
            CycleStatus::Closed => "closed",
            CycleStatus::Aborted => "aborted",
        };
        format!(
            "Cycle {:04} — {} ({status})",
            self.manifest.number, self.manifest.slug
        )
    }
}

/// All cycles discovered under `.mdd/cycles/`, ordered by number.
#[derive(Debug, Clone, Default)]
pub struct CycleRegistry {
    pub cycles: Vec<Cycle>,
}

impl CycleRegistry {
    /// Scan `<root>/.mdd/cycles/*/manifest.yml`. A missing directory is
    /// not an error — it just yields an empty registry.
    pub fn scan(root: &Path) -> Result<Self> {
        let base = root.join(CYCLES_DIR);
        let mut cycles = Vec::new();
        if base.is_dir() {
            for entry in
                fs::read_dir(&base).with_context(|| format!("failed to read {}", base.display()))?
            {
                let entry = entry?;
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let manifest_path = path.join("manifest.yml");
                if !manifest_path.is_file() {
                    continue;
                }
                let content = fs::read_to_string(&manifest_path)
                    .with_context(|| format!("failed to read {}", manifest_path.display()))?;
                let manifest: CycleManifest = serde_yaml::from_str(&content)
                    .with_context(|| format!("failed to parse {}", manifest_path.display()))?;
                let before_dir = path.join("before");
                let after = path.join("after");
                let after_dir = after.is_dir().then_some(after);
                cycles.push(Cycle {
                    manifest,
                    dir: path,
                    before_dir,
                    after_dir,
                });
            }
        }
        cycles.sort_by_key(|cycle| cycle.manifest.number);
        Ok(Self { cycles })
    }

    pub fn cycle(&self, number: u32) -> Option<&Cycle> {
        self.cycles
            .iter()
            .find(|cycle| cycle.manifest.number == number)
    }

    pub fn latest(&self) -> Option<&Cycle> {
        self.cycles.last()
    }
}

/// Per-diagram partition of element identities between a cycle's
/// `before/` and `after/` snapshots. The three buckets are pairwise
/// disjoint by construction (OCL-CYCLE-DIFF-PARTITION).
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CycleDiff {
    pub diagram: String,
    pub unchanged: Vec<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

impl CycleDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

/// Element identity for diffing: every `@id(...)`, plus every declared
/// PlantUML element keyed by its `as <alias>`, quoted label, or bare
/// token (objective note on SEQ-RENDER-CYCLE-DIFF).
fn element_keys(doc: &str) -> BTreeSet<String> {
    const ELEMENT_KEYWORDS: &[&str] = &[
        "usecase ",
        "actor ",
        "class ",
        "interface ",
        "enum ",
        "abstract ",
        "participant ",
        "boundary ",
        "control ",
        "entity ",
        "database ",
        "collections ",
        "queue ",
        "state ",
        "component ",
        "package ",
        "node ",
        "rectangle ",
        "circle ",
        "cloud ",
        "frame ",
        "folder ",
        "card ",
        "agent ",
    ];
    let id_pattern = Regex::new(r"@id\(([A-Za-z0-9_.:-]+)\)").expect("valid id regex");
    let alias_pattern = Regex::new(r#"\bas\s+("[^"]+"|[A-Za-z0-9_]+)"#).expect("valid alias regex");
    let quoted_pattern = Regex::new(r#""([^"]+)""#).expect("valid quoted regex");

    let mut keys = BTreeSet::new();
    for raw in doc.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let is_comment = line.starts_with('\'') || line.starts_with("//");
        if let Some(cap) = id_pattern.captures(line) {
            keys.insert(format!("id:{}", &cap[1]));
            continue;
        }
        if is_comment {
            continue;
        }
        if let Some(keyword) = ELEMENT_KEYWORDS.iter().find(|kw| line.starts_with(**kw)) {
            if let Some(cap) = alias_pattern.captures(line) {
                keys.insert(format!("el:{}", cap[1].trim_matches('"')));
            } else if let Some(cap) = quoted_pattern.captures(line) {
                keys.insert(format!("el:{}", &cap[1]));
            } else {
                let rest = line[keyword.len()..].trim();
                let token: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !token.is_empty() {
                    keys.insert(format!("el:{token}"));
                }
            }
        }
    }
    keys
}

/// Partition `before` vs `after` element identities into
/// unchanged / added / removed (sorted, disjoint).
pub fn diff_documents(diagram: &str, before: &str, after: &str) -> CycleDiff {
    let before_keys = element_keys(before);
    let after_keys = element_keys(after);
    CycleDiff {
        diagram: diagram.to_string(),
        unchanged: before_keys.intersection(&after_keys).cloned().collect(),
        added: after_keys.difference(&before_keys).cloned().collect(),
        removed: before_keys.difference(&after_keys).cloned().collect(),
    }
}

fn collect_puml(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return out;
    }
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|entry| entry.ok())
    {
        let path = entry.path();
        if path.is_file()
            && matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("puml" | "plantuml" | "uml" | "ocl")
            )
            && let Ok(relative) = path.strip_prefix(dir)
        {
            out.push((
                relative.to_string_lossy().replace('\\', "/"),
                path.to_path_buf(),
            ));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Diff every diagram between a cycle's `before/` and `after/`
/// snapshots. Files present on only one side are all-added or
/// all-removed. Returns an empty vec while the cycle is still open
/// (no `after/`).
pub fn cycle_diffs(cycle: &Cycle) -> Result<Vec<CycleDiff>> {
    let Some(after_dir) = &cycle.after_dir else {
        return Ok(Vec::new());
    };
    let before = collect_puml(&cycle.before_dir);
    let after = collect_puml(after_dir);
    let mut names: BTreeSet<String> = BTreeSet::new();
    names.extend(before.iter().map(|(name, _)| name.clone()));
    names.extend(after.iter().map(|(name, _)| name.clone()));

    let mut diffs = Vec::new();
    for name in names {
        let before_doc = before
            .iter()
            .find(|(n, _)| n == &name)
            .map(|(_, p)| fs::read_to_string(p))
            .transpose()?
            .unwrap_or_default();
        let after_doc = after
            .iter()
            .find(|(n, _)| n == &name)
            .map(|(_, p)| fs::read_to_string(p))
            .transpose()?
            .unwrap_or_default();
        let diff = diff_documents(&name, &before_doc, &after_doc);
        if !(diff.added.is_empty() && diff.removed.is_empty() && diff.unchanged.is_empty()) {
            diffs.push(diff);
        }
    }
    Ok(diffs)
}

/// Build a single superposed PlantUML document from a cycle's before
/// and after snapshots: shared elements rendered once, additions tagged
/// `<<added>>` (green), removals injected as `<<removed>>` (red).
/// Mirrors the review diff annotator's shape so `/mdd-render` can
/// rasterize it like any other diagram.
pub fn annotate_cycle_diff_puml(after: &str, before: &str) -> String {
    const ELEMENT_KEYWORDS: &[&str] = &[
        "usecase ",
        "actor ",
        "class ",
        "interface ",
        "enum ",
        "participant ",
        "state ",
        "component ",
        "rectangle ",
        "database ",
        "folder ",
        "node ",
        "card ",
        "agent ",
    ];
    let skinparam_block = "\
skinparam usecase {\n  BackgroundColor<<added>> #90EE90\n  BackgroundColor<<removed>> #FFB6C1\n}\n\
skinparam class {\n  BackgroundColor<<added>> #90EE90\n  BackgroundColor<<removed>> #FFB6C1\n}\n\
skinparam component {\n  BackgroundColor<<added>> #90EE90\n  BackgroundColor<<removed>> #FFB6C1\n}\n\
skinparam state {\n  BackgroundColor<<added>> #90EE90\n  BackgroundColor<<removed>> #FFB6C1\n}\n\
skinparam rectangle {\n  BackgroundColor<<added>> #90EE90\n  BackgroundColor<<removed>> #FFB6C1\n}\n\
skinparam actor {\n  BackgroundColor<<added>> #90EE90\n  BackgroundColor<<removed>> #FFB6C1\n}\n\
skinparam participant {\n  BackgroundColor<<added>> #90EE90\n  BackgroundColor<<removed>> #FFB6C1\n}\n";

    let before_keys = element_keys(before);
    let after_keys = element_keys(after);
    let removed: Vec<String> = before_keys.difference(&after_keys).cloned().collect();
    let id_pattern = Regex::new(r"@id\(([A-Za-z0-9_.:-]+)\)").expect("valid id regex");

    let mut output = String::new();
    let mut skinparams_inserted = false;
    let mut pending_added = false;

    for line in after.lines() {
        let trimmed = line.trim();

        if !skinparams_inserted
            && (trimmed.starts_with("@startuml") || trimmed.starts_with("@startsalt"))
        {
            output.push_str(line);
            output.push('\n');
            output.push_str(skinparam_block);
            skinparams_inserted = true;
            continue;
        }

        if trimmed.starts_with("@enduml") || trimmed.starts_with("@endsalt") {
            for (idx, key) in removed.iter().enumerate() {
                let label = key.trim_start_matches("id:").trim_start_matches("el:");
                output.push_str(&format!(
                    "card \"{label}\" as RemovedByCycle{idx} <<removed>>\n"
                ));
            }
            output.push_str(line);
            output.push('\n');
            continue;
        }

        if trimmed.starts_with('\'') || trimmed.starts_with("//") {
            if let Some(cap) = id_pattern.captures(trimmed)
                && !before_keys.contains(&format!("id:{}", &cap[1]))
            {
                pending_added = true;
            }
            output.push_str(line);
            output.push('\n');
            continue;
        }

        if pending_added
            && !trimmed.is_empty()
            && ELEMENT_KEYWORDS.iter().any(|kw| trimmed.starts_with(kw))
        {
            let injected = if let Some(brace_pos) = line.find('{') {
                format!(
                    "{} <<added>> {}",
                    line[..brace_pos].trim_end(),
                    &line[brace_pos..]
                )
            } else {
                format!("{} <<added>>", line.trim_end())
            };
            output.push_str(&injected);
            output.push('\n');
            pending_added = false;
            continue;
        }

        output.push_str(line);
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_partitions_into_disjoint_buckets() {
        let before = "@startuml\n' @id(DOM-A)\nclass A\nclass Gone\n@enduml\n";
        let after = "@startuml\n' @id(DOM-A)\nclass A\nclass New\n@enduml\n";
        let diff = diff_documents("domain/x.puml", before, after);
        assert!(diff.added.contains(&"el:New".to_string()));
        assert!(diff.removed.contains(&"el:Gone".to_string()));
        assert!(diff.unchanged.contains(&"id:DOM-A".to_string()));
        // disjoint
        for a in &diff.added {
            assert!(!diff.removed.contains(a) && !diff.unchanged.contains(a));
        }
    }

    #[test]
    fn annotate_marks_added_and_removed() {
        let before = "@startuml\n' @id(USE-OLD)\nusecase \"Old\" as Old\n@enduml\n";
        let after = "@startuml\n' @id(USE-NEW)\nusecase \"New\" as New\n@enduml\n";
        let out = annotate_cycle_diff_puml(after, before);
        assert!(out.contains("<<added>>"));
        assert!(out.contains("RemovedByCycle0 <<removed>>"));
        assert!(out.contains("skinparam"));
    }

    #[test]
    fn scan_missing_dir_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = CycleRegistry::scan(tmp.path()).unwrap();
        assert!(reg.cycles.is_empty());
        assert!(reg.latest().is_none());
    }

    #[test]
    fn scan_reads_manifest_and_resolves_snapshots() {
        let tmp = tempfile::tempdir().unwrap();
        let cdir = tmp.path().join(".mdd/cycles/0001");
        fs::create_dir_all(cdir.join("before")).unwrap();
        fs::write(
            cdir.join("manifest.yml"),
            "number: 1\nslug: tree-rail\nentry: generate\ndescription: add tree\nstatus: open\nopened_at: \"1\"\ntouched_files: []\n",
        )
        .unwrap();
        let reg = CycleRegistry::scan(tmp.path()).unwrap();
        assert_eq!(reg.cycles.len(), 1);
        let c = reg.cycle(1).unwrap();
        assert_eq!(c.manifest.entry, EntryPoint::Generate);
        assert!(!c.is_closed());
        assert!(c.after_dir.is_none());
        assert!(c.label().contains("Cycle 0001"));
    }
}
