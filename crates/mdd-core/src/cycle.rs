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

/// Deserialize a `u32` that historical manifests sometimes wrote zero-padded
/// (`number: 0020`) or quoted (`number: "0021"`) — both of which serde_yaml
/// surfaces as a *string*, not an integer. Accept an integer or any numeric
/// string so `CycleRegistry::scan` is robust to every manifest the skill has
/// written. `Project::review` now scans cycles (scoped parity), so a single
/// legacy manifest must not break the review gate or the viewer.
fn de_u32_flexible<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct U32Flexible;
    impl<'de> serde::de::Visitor<'de> for U32Flexible {
        type Value = u32;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a u32 or a numeric string (possibly zero-padded or quoted)")
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<u32, E> {
            u32::try_from(v).map_err(E::custom)
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<u32, E> {
            u32::try_from(v).map_err(E::custom)
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<u32, E> {
            v.trim().parse::<u32>().map_err(E::custom)
        }
    }
    deserializer.deserialize_any(U32Flexible)
}

/// On-disk `.mdd/cycles/<n>/manifest.yml`, authored by the skill.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct CycleManifest {
    #[serde(deserialize_with = "de_u32_flexible")]
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
    /// Optional parity scope: the objective `@id`s this cycle realizes. When
    /// non-empty the review gate narrows to just these ids (a realize-slice
    /// cycle: out-of-scope objective ids still absent from current are
    /// expected, not a mismatch). Empty — the default for ordinary cycles —
    /// is the whole-model gate, byte-identical to before scoped parity.
    /// Read by `Project::review`; written by the `/mdd-cycle` skill.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<String>,
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

    /// Project-relative path of the superposed `.diff.puml` for a model
    /// file in this cycle, e.g.
    /// `.mdd/models/current/domain/canvas-view.puml`
    ///   -> `.mdd/cycles/0002/domain/canvas-view.diff.puml`.
    ///
    /// Pure path transform (no filesystem access): strip the
    /// `.mdd/models/<side>/` prefix to the `<kind>/<name>` tail and place
    /// it under this cycle's directory with the `.diff.puml` suffix. The
    /// rendered SVG mirror is then `Project::rendered_svg_path` of this
    /// (OCL-DIFF-SVG-PATH-DERIVED). Returns `None` for non-model paths.
    pub fn diff_puml_rel(&self, model_rel: &str) -> Option<String> {
        let under_side = model_rel.strip_prefix(".mdd/models/")?;
        let (_side, kind_name) = under_side.split_once('/')?;
        let stem = kind_name.strip_suffix(".puml")?;
        Some(format!(
            "{}/{:04}/{stem}.diff.puml",
            CYCLES_DIR, self.manifest.number
        ))
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

/// PlantUML element keyword to use when re-injecting a removed element
/// as a red ghost. It must be valid for the diagram type, otherwise
/// PlantUML rejects the whole `.diff.puml`: `card` is fine in
/// use-case/class/component diagrams but illegal in a sequence diagram.
/// `None` means "do not inject removed shapes" (Salt mockups, whose
/// element identity is `@id`-only and which accept neither card nor
/// participant).
fn removed_ghost_keyword(after: &str) -> Option<&'static str> {
    let mut has_participant = false;
    let mut has_state_marker = false;
    for raw in after.lines() {
        let t = raw.trim();
        if t.starts_with("@startsalt") {
            return None;
        }
        if t.starts_with('\'') || t.starts_with("//") {
            continue;
        }
        if t.contains("[*]") {
            has_state_marker = true;
        }
        if t.starts_with("participant ")
            || t.starts_with("boundary ")
            || t.starts_with("control ")
            || t.starts_with("entity ")
            || t.starts_with("collections ")
            || t.starts_with("queue ")
        {
            has_participant = true;
        }
    }
    if has_state_marker {
        Some("state")
    } else if has_participant {
        Some("participant")
    } else {
        Some("card")
    }
}

/// Build a single superposed PlantUML document from a cycle's before
/// and after snapshots: shared elements rendered once, additions tagged
/// `<<added>>` (green), removals injected as `<<removed>>` (red) using a
/// diagram-type-appropriate ghost element (`removed_ghost_keyword`) so
/// the result is valid PlantUML for sequence/state diagrams too.
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
    let ghost = removed_ghost_keyword(after);
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
            if let Some(keyword) = ghost {
                for (idx, key) in removed.iter().enumerate() {
                    let label = key.trim_start_matches("id:").trim_start_matches("el:");
                    output.push_str(&format!(
                        "{keyword} \"{label}\" as RemovedByCycle{idx} <<removed>>\n"
                    ));
                }
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
        assert!(!diff.is_empty(), "a real add/remove is not is_empty()");
    }

    #[test]
    fn unchanged_only_diff_is_empty() {
        // A file present identically in before/ and after/ yields a
        // CycleDiff with only `unchanged` populated. is_empty() must be
        // true so Project::cycles_with_diff_for excludes that cycle from
        // the per-diagram selector (OCL-DIFF-CYCLE-SCOPED): no diff to
        // show, no rendered .diff.svg, so no button.
        let doc = "@startuml\n' @id(DOM-A)\nclass A\nclass B\n@enduml\n";
        let diff = diff_documents("domain/x.puml", doc, doc);
        assert!(diff.added.is_empty() && diff.removed.is_empty());
        assert!(!diff.unchanged.is_empty());
        assert!(diff.is_empty());
    }

    #[test]
    fn annotate_marks_added_and_removed() {
        let before = "@startuml\n' @id(USE-OLD)\nusecase \"Old\" as Old\n@enduml\n";
        let after = "@startuml\n' @id(USE-NEW)\nusecase \"New\" as New\n@enduml\n";
        let out = annotate_cycle_diff_puml(after, before);
        assert!(out.contains("<<added>>"));
        assert!(out.contains("RemovedByCycle0 <<removed>>"));
        // use-case diagram → card ghost is valid
        assert!(out.contains("card \"Old\" as RemovedByCycle0 <<removed>>"));
        assert!(out.contains("skinparam"));
    }

    #[test]
    fn annotate_removed_in_sequence_uses_participant_not_card() {
        // A removed participant must not be re-injected as `card`
        // (illegal in a sequence diagram → PlantUML syntax error).
        let before = "@startuml\nparticipant \"Old\" as Old\nparticipant \"Keep\" as Keep\n@enduml\n";
        let after = "@startuml\nparticipant \"Keep\" as Keep\n@enduml\n";
        let out = annotate_cycle_diff_puml(after, before);
        assert!(out.contains("participant \"Old\" as RemovedByCycle0 <<removed>>"));
        assert!(!out.contains("card \""));
    }

    #[test]
    fn annotate_removed_in_state_uses_state_keyword() {
        let before = "@startuml\n[*] --> A\nstate A\nstate Gone\n@enduml\n";
        let after = "@startuml\n[*] --> A\nstate A\n@enduml\n";
        let out = annotate_cycle_diff_puml(after, before);
        assert!(out.contains("state \"Gone\" as RemovedByCycle0 <<removed>>"));
        assert!(!out.contains("card \""));
    }

    #[test]
    fn annotate_salt_skips_removed_injection() {
        // Salt mockups accept neither card nor participant; element
        // identity is @id-only, so a removed @id must not inject a shape.
        let before = "@startsalt\n' @id(MCK-OLD)\n{\n  [Old]\n}\n@endsalt\n";
        let after = "@startsalt\n' @id(MCK-NEW)\n{\n  [New]\n}\n@endsalt\n";
        let out = annotate_cycle_diff_puml(after, before);
        assert!(!out.contains("RemovedByCycle"));
        assert!(!out.contains("card \""));
    }

    #[test]
    fn scan_missing_dir_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let reg = CycleRegistry::scan(tmp.path()).unwrap();
        assert!(reg.cycles.is_empty());
        assert!(reg.latest().is_none());
    }

    #[test]
    fn diff_puml_rel_mirrors_model_path_under_cycle() {
        let cycle = Cycle {
            manifest: CycleManifest {
                number: 2,
                slug: "x".into(),
                entry: EntryPoint::Generate,
                description: String::new(),
                status: CycleStatus::Closed,
                opened_at: String::new(),
                closed_at: None,
                touched_files: vec![],
                scope: vec![],
            },
            dir: PathBuf::from(".mdd/cycles/0002"),
            before_dir: PathBuf::from(".mdd/cycles/0002/before"),
            after_dir: Some(PathBuf::from(".mdd/cycles/0002/after")),
        };
        assert_eq!(
            cycle
                .diff_puml_rel(".mdd/models/current/domain/canvas-view.puml")
                .as_deref(),
            Some(".mdd/cycles/0002/domain/canvas-view.diff.puml")
        );
        assert_eq!(
            cycle
                .diff_puml_rel(".mdd/models/objective/use-cases/cycle-tracking.puml")
                .as_deref(),
            Some(".mdd/cycles/0002/use-cases/cycle-tracking.diff.puml")
        );
        assert_eq!(cycle.diff_puml_rel("crates/foo.rs"), None);
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

    #[test]
    fn manifest_number_accepts_padded_or_quoted() {
        // Historical manifests wrote `number` zero-padded or quoted; the reader
        // must accept both (serde_yaml surfaces them as strings) plus a plain
        // integer, so CycleRegistry::scan never breaks the review gate.
        let padded: CycleManifest =
            serde_yaml::from_str("number: 0020\nslug: x\nentry: generate\nstatus: closed\n")
                .unwrap();
        assert_eq!(padded.number, 20);
        let quoted: CycleManifest =
            serde_yaml::from_str("number: \"0021\"\nslug: x\nentry: generate\nstatus: closed\n")
                .unwrap();
        assert_eq!(quoted.number, 21);
        let plain: CycleManifest =
            serde_yaml::from_str("number: 26\nslug: x\nentry: generate\nstatus: open\n").unwrap();
        assert_eq!(plain.number, 26);
    }
}
