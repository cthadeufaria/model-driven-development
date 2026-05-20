use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

pub mod cycle;
mod templates;

pub use cycle::{Cycle, CycleDiff, CycleRegistry, CycleStatus, EntryPoint};

/// The complete, single enumeration of every renderable source tree.
/// The `mdd render` command, the `/mdd-cycle` close step, and the
/// `/mdd-render` thin-wrapper skill all consume exactly this set — there
/// is no second, hand-maintained list (OCL-RENDER-TREE-PARITY). Adding a
/// renderable tree means adding a variant here plus one match arm in
/// [`Project::render_sources`]; every caller then covers it for free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RenderTree {
    /// `.mdd/models/**/*.puml` (current + objective).
    Models,
    /// `.mdd/cycles/**/<rel>.diff.puml` superposed cycle diffs.
    CycleDiffs,
    /// `.mdd/constraints/*.ocl` (synthesized to a constraints diagram).
    OclConstraints,
    /// `.mdd/map/**/*.puml` accumulated whole-map baseline.
    WholeMap,
    /// `.mdd/deploy/**/*.puml` deployment diagrams.
    Deploy,
    /// `.mdd/rendered/review/*.diff.puml` review-mismatch diffs.
    ReviewDiff,
}

impl RenderTree {
    /// Every tree, in render order.
    pub const ALL: [RenderTree; 6] = [
        RenderTree::Models,
        RenderTree::CycleDiffs,
        RenderTree::OclConstraints,
        RenderTree::WholeMap,
        RenderTree::Deploy,
        RenderTree::ReviewDiff,
    ];

    /// Canonical `mdd render --only <key>` selector.
    pub fn key(self) -> &'static str {
        match self {
            RenderTree::Models => "models",
            RenderTree::CycleDiffs => "cycle-diffs",
            RenderTree::OclConstraints => "ocl",
            RenderTree::WholeMap => "map",
            RenderTree::Deploy => "deploy",
            RenderTree::ReviewDiff => "review",
        }
    }

    /// Parse a user-supplied tree selector, accepting a few aliases.
    pub fn parse(token: &str) -> Option<RenderTree> {
        match token.trim().to_ascii_lowercase().as_str() {
            "models" | "model" => Some(RenderTree::Models),
            "cycle-diffs" | "cycle-diff" | "cycles" | "cycle" | "diffs" => {
                Some(RenderTree::CycleDiffs)
            }
            "ocl" | "constraints" | "constraint" => Some(RenderTree::OclConstraints),
            "map" | "whole-map" | "wholemap" => Some(RenderTree::WholeMap),
            "deploy" | "deployment" => Some(RenderTree::Deploy),
            "review" | "review-diff" | "review-diffs" => Some(RenderTree::ReviewDiff),
            _ => None,
        }
    }
}

pub const MDD_DIR: &str = ".mdd";

const CONFIG_FILE: &str = ".mdd/config.yml";
const TRACE_FILE: &str = ".mdd/trace.yml";
const APPROVALS_FILE: &str = ".mdd/approvals.yml";

#[derive(Debug, Clone)]
pub struct Project {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct InitReport {
    pub root: PathBuf,
    pub created: Vec<String>,
    pub overwritten: Vec<String>,
    pub skipped: Vec<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InitFileConflict {
    Skip,
    Overwrite,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanReport {
    pub root: PathBuf,
    pub removed: Vec<String>,
    pub skipped: Vec<CleanSkip>,
}

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct CleanSkip {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MddConfig {
    pub version: u32,
    pub model_source: String,
    pub constraint_source: String,
    pub rendered_dir: String,
    #[serde(default)]
    pub security: SecurityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq)]
pub struct SecurityConfig {
    #[serde(default)]
    pub parity_check: ParityMode,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ParityMode {
    Warn,
    #[default]
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub version: u32,
    #[serde(default)]
    pub links: Vec<TraceLink>,
    #[serde(default)]
    pub generated_tests: Vec<GeneratedTest>,
    #[serde(default)]
    pub generated_ui_tests: Vec<GeneratedUiTest>,
    #[serde(default)]
    pub source_links: Vec<SourceLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct TraceLink {
    pub from: String,
    pub to: String,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct GeneratedTest {
    pub id: String,
    pub path: String,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct GeneratedUiTest {
    pub id: String,
    pub path: String,
    pub model_id: String,
    #[serde(default = "default_ui_test_framework")]
    pub framework: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct SourceLink {
    pub model_id: String,
    pub path: String,
    #[serde(default)]
    pub symbol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approvals {
    pub version: u32,
    pub approved: bool,
    #[serde(default)]
    pub approved_at: Option<String>,
    #[serde(default)]
    pub files: Vec<ApprovedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ApprovedFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "kebab-case")]
pub enum ModelKind {
    UseCase,
    Sequence,
    Domain,
    Mockup,
    State,
    Other,
    Constraint,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "kebab-case")]
pub enum ModelSide {
    Current,
    Objective,
    Shared,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ModelFile {
    pub path: String,
    pub kind: ModelKind,
    pub side: ModelSide,
    pub ids: Vec<String>,
    pub refs: Vec<String>,
    #[serde(default)]
    pub rendered_pages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelRegistry {
    pub files: Vec<ModelFile>,
    pub ids: Vec<ModelElement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct ModelElement {
    pub id: String,
    pub file: String,
    pub kind: ModelKind,
    pub side: ModelSide,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationReport {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub registry: ModelRegistry,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalStatus {
    pub approved: bool,
    pub current: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalReport {
    pub approved_files: usize,
    pub approvals_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestGenerationReport {
    pub generated: Vec<String>,
    pub trace_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodeGateReport {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewReport {
    /// Combined gate: `ids_matched && (security.matched || security.mode == Warn)`.
    pub matched: bool,
    /// Pure ID-parity result (every objective `@id` present in current).
    pub ids_matched: bool,
    pub missing_ids: Vec<String>,
    pub extra_ids: Vec<String>,
    pub diff_puml_paths: Vec<String>,
    /// Security-marker parity pass, always run as part of `review()`.
    pub security: SecurityReviewReport,
}

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct MarkerSummary {
    pub host: String,
    pub stereotype: String,
    pub params: BTreeMap<String, String>,
    pub id: Option<String>,
    pub file: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SecurityReviewReport {
    pub matched: bool,
    pub mode: ParityMode,
    pub missing_markers: Vec<MarkerSummary>,
    pub extra_markers: Vec<MarkerSummary>,
    pub diff_puml_paths: Vec<String>,
}

impl Default for MddConfig {
    fn default() -> Self {
        Self {
            version: 1,
            model_source: "plantuml".to_string(),
            constraint_source: "ocl".to_string(),
            rendered_dir: ".mdd/rendered".to_string(),
            security: SecurityConfig::default(),
        }
    }
}

impl Default for Trace {
    fn default() -> Self {
        Self {
            version: 1,
            links: Vec::new(),
            generated_tests: Vec::new(),
            generated_ui_tests: Vec::new(),
            source_links: Vec::new(),
        }
    }
}

impl Default for Approvals {
    fn default() -> Self {
        Self {
            version: 1,
            approved: false,
            approved_at: None,
            files: Vec::new(),
        }
    }
}

impl Project {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn discover(start: impl AsRef<Path>) -> Result<Self> {
        let mut dir = start
            .as_ref()
            .canonicalize()
            .with_context(|| format!("failed to resolve {}", start.as_ref().display()))?;

        if dir.is_file() {
            dir.pop();
        }

        loop {
            if dir.join(MDD_DIR).is_dir() {
                return Ok(Self::at(dir));
            }

            if !dir.pop() {
                bail!("could not find a .mdd directory; run `mdd init` first");
            }
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn mdd_dir(&self) -> PathBuf {
        self.root.join(MDD_DIR)
    }

    pub fn init(&self) -> Result<InitReport> {
        self.init_with_conflict_handler(|_| Ok(InitFileConflict::Skip))
    }

    pub fn init_with_conflict_handler<F>(&self, mut on_conflict: F) -> Result<InitReport>
    where
        F: FnMut(&str) -> Result<InitFileConflict>,
    {
        let mut created = Vec::new();
        let mut overwritten = Vec::new();
        let mut skipped = Vec::new();
        let dirs = [
            ".mdd/models/current/use-cases",
            ".mdd/models/current/sequences",
            ".mdd/models/current/domain",
            ".mdd/models/current/components",
            ".mdd/models/current/mockups",
            ".mdd/models/current/states",
            ".mdd/models/objective/use-cases",
            ".mdd/models/objective/sequences",
            ".mdd/models/objective/domain",
            ".mdd/models/objective/components",
            ".mdd/models/objective/mockups",
            ".mdd/models/objective/states",
            ".mdd/constraints",
            ".mdd/rendered",
            ".mdd/tests/acceptance",
            ".mdd/tests/ui",
            ".mdd/docs",
            ".claude/skills",
            ".codex/skills",
        ];

        for dir in dirs {
            let path = self.root.join(dir);
            if !path.exists() {
                fs::create_dir_all(&path).with_context(|| format!("failed to create {dir}"))?;
                created.push(dir.to_string());
            }
        }

        self.write_yaml_with_conflict_handler(
            CONFIG_FILE,
            &MddConfig::default(),
            &mut on_conflict,
            &mut created,
            &mut overwritten,
            &mut skipped,
        )?;
        self.write_yaml_with_conflict_handler(
            TRACE_FILE,
            &Trace::default(),
            &mut on_conflict,
            &mut created,
            &mut overwritten,
            &mut skipped,
        )?;
        self.write_yaml_with_conflict_handler(
            APPROVALS_FILE,
            &Approvals::default(),
            &mut on_conflict,
            &mut created,
            &mut overwritten,
            &mut skipped,
        )?;
        self.write_text_if_missing(
            ".mdd/docs/mdd-workflow.md",
            templates::mdd_workflow_doc(),
            &mut created,
            &mut overwritten,
            &mut skipped,
            &mut on_conflict,
        )?;
        self.write_text_if_missing(
            ".mdd/docs/uml-and-ocl-guide.md",
            templates::uml_and_ocl_guide_doc(),
            &mut created,
            &mut overwritten,
            &mut skipped,
            &mut on_conflict,
        )?;
        self.write_text_if_missing(
            ".mdd/docs/security-profile.md",
            templates::security_profile_doc(),
            &mut created,
            &mut overwritten,
            &mut skipped,
            &mut on_conflict,
        )?;
        self.write_text_if_missing(
            ".mdd/docs/deploy-profile.md",
            templates::deploy_profile_doc(),
            &mut created,
            &mut overwritten,
            &mut skipped,
            &mut on_conflict,
        )?;
        self.write_managed_block(
            "CLAUDE.md",
            "claude-entrypoint",
            templates::claude_entrypoint(),
            &mut created,
            &mut overwritten,
        )?;
        self.write_managed_block(
            "AGENTS.md",
            "agents-entrypoint",
            templates::agents_entrypoint(),
            &mut created,
            &mut overwritten,
        )?;
        for skill in templates::WORKFLOW_SKILLS {
            let skill_content = templates::skill_markdown(skill);
            self.write_text_if_missing(
                &format!(".claude/skills/{}/SKILL.md", skill.name),
                &skill_content,
                &mut created,
                &mut overwritten,
                &mut skipped,
                &mut on_conflict,
            )?;
            self.write_text_if_missing(
                &format!(".codex/skills/{}/SKILL.md", skill.name),
                &skill_content,
                &mut created,
                &mut overwritten,
                &mut skipped,
                &mut on_conflict,
            )?;
        }

        Ok(InitReport {
            root: self.root.clone(),
            created,
            overwritten,
            skipped,
        })
    }

    pub fn clean(&self, force: bool) -> Result<CleanReport> {
        let mut removed = Vec::new();
        let mut skipped = Vec::new();

        self.remove_dir_all_if_exists(".mdd", &mut removed, &mut skipped)?;

        for agent_dir in [".claude", ".codex"] {
            for skill in templates::WORKFLOW_SKILLS {
                let skill_content = templates::skill_markdown(skill);
                let skill_file = format!("{agent_dir}/skills/{}/SKILL.md", skill.name);
                self.remove_generated_text_file(
                    &skill_file,
                    &skill_content,
                    force,
                    &mut removed,
                    &mut skipped,
                )?;

                let skill_dir = format!("{agent_dir}/skills/{}", skill.name);
                self.remove_empty_dir_if_exists(&skill_dir, &mut removed)?;
            }

            self.remove_empty_dir_if_exists(&format!("{agent_dir}/skills"), &mut removed)?;
            self.remove_empty_dir_if_exists(agent_dir, &mut removed)?;
        }

        self.remove_managed_block("CLAUDE.md", force, &mut removed, &mut skipped)?;
        self.remove_managed_block("AGENTS.md", force, &mut removed, &mut skipped)?;

        Ok(CleanReport {
            root: self.root.clone(),
            removed,
            skipped,
        })
    }

    pub fn read_config(&self) -> Result<MddConfig> {
        self.read_yaml(CONFIG_FILE)
    }

    pub fn read_trace(&self) -> Result<Trace> {
        self.read_yaml(TRACE_FILE)
    }

    pub fn write_trace(&self, trace: &Trace) -> Result<()> {
        self.write_yaml(TRACE_FILE, trace)
    }

    pub fn read_approvals(&self) -> Result<Approvals> {
        self.read_yaml(APPROVALS_FILE)
    }

    pub fn model_registry(&self) -> Result<ModelRegistry> {
        let mut registry = ModelRegistry::default();

        for path in self.model_and_constraint_files()? {
            let relative = self.relative_path(&path)?;
            let kind = kind_for_path(&relative);
            let side = side_for_path(&relative);
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let ids = extract_ids(&content)?;
            let refs = extract_refs(&content)?;

            for id in &ids {
                registry.ids.push(ModelElement {
                    id: id.clone(),
                    file: relative.clone(),
                    kind,
                    side,
                });
            }

            let rendered_pages = self.rendered_pages_for(&relative);

            registry.files.push(ModelFile {
                path: relative,
                kind,
                side,
                ids,
                refs,
                rendered_pages,
            });
        }

        registry.files.sort_by(|a, b| a.path.cmp(&b.path));
        registry.ids.sort_by(|a, b| a.id.cmp(&b.id));

        Ok(registry)
    }

    /// `/mdd-deploy` deployment diagrams (`.mdd/deploy/**/*.puml`) as
    /// [`ModelFile`] values, each paired with its rendered SVG mirror.
    ///
    /// This is the viewer's THIRD ingestion source, beside
    /// [`Project::model_registry`] and [`Project::cycle_registry`]. It
    /// reuses [`RenderTree::Deploy`] (the single source of truth for the
    /// deploy tree — `OCL-RENDER-TREE-PARITY`) and deliberately does NOT
    /// feed [`ModelRegistry`]: `/mdd-deploy` is a utility outside the
    /// current<->objective parity gate, so [`Project::review`] and
    /// validation — which read [`Project::model_registry`] — must keep
    /// ignoring `.mdd/deploy/`. An absent `.mdd/deploy/` yields an empty
    /// list (CMP-DEPLOY-VIEWER-SOURCE).
    pub fn deploy_files(&self) -> Result<Vec<ModelFile>> {
        let mut files = Vec::new();
        for path in self.render_sources(RenderTree::Deploy)? {
            let relative = self.relative_path(&path)?;
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            files.push(ModelFile {
                ids: extract_ids(&content)?,
                refs: extract_refs(&content)?,
                rendered_pages: self.rendered_pages_for(&relative),
                kind: ModelKind::Other,
                side: ModelSide::Shared,
                path: relative,
            });
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }

    /// Map of model `@id` -> its `@desc(...)` text across all model and
    /// constraint files. Used by the viewer's MODEL CONTEXT card.
    pub fn descriptions(&self) -> Result<BTreeMap<String, String>> {
        let mut out = BTreeMap::new();
        for path in self.model_and_constraint_files()? {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            out.extend(extract_descs(&content)?);
        }
        Ok(out)
    }

    /// All tracked MDD cycles under `.mdd/cycles/`, ordered by number.
    pub fn cycle_registry(&self) -> Result<cycle::CycleRegistry> {
        cycle::CycleRegistry::scan(&self.root)
    }

    /// Per-diagram before/after element diffs for one closed cycle.
    pub fn cycle_diffs(&self, number: u32) -> Result<Vec<cycle::CycleDiff>> {
        let registry = self.cycle_registry()?;
        match registry.cycle(number) {
            Some(cycle) => cycle::cycle_diffs(cycle),
            None => Ok(Vec::new()),
        }
    }

    /// Every superposed `<diagram>.diff.puml` under `.mdd/cycles/`, as
    /// absolute paths, sorted. The diff-render pass rasterizes each to its
    /// deterministic `rendered_svg_path` mirror under
    /// `.mdd/rendered/cycles/` (OCL-DIFF-SVG-PATH-DERIVED).
    pub fn cycle_diff_puml_files(&self) -> Result<Vec<PathBuf>> {
        let base = self.root.join(cycle::CYCLES_DIR);
        let mut files = Vec::new();
        if !base.is_dir() {
            return Ok(files);
        }
        for entry in WalkDir::new(&base)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            let path = entry.path();
            if entry.file_type().is_file()
                && path
                    .to_str()
                    .is_some_and(|p| p.ends_with(".diff.puml"))
            {
                files.push(path.to_path_buf());
            }
        }
        files.sort();
        Ok(files)
    }

    /// Cycle numbers (ascending) that actually **changed** `model_rel` —
    /// a `CycleDiff` for that file's `<kind>/<name>` key with a non-empty
    /// added or removed set (`!CycleDiff::is_empty()`), matching exactly
    /// the cycles that have a written `.diff.puml`/`.diff.svg`. A file
    /// merely present-unchanged in a cycle's before/after snapshot still
    /// yields an (unchanged-only) `CycleDiff`, but is deliberately
    /// excluded so the viewer's per-diagram cycle selector never offers
    /// a cycle that has no diff to show (OCL-DIFF-CYCLE-SCOPED).
    pub fn cycles_with_diff_for(&self, model_rel: &str) -> Result<Vec<u32>> {
        let Some(under) = model_rel.strip_prefix(".mdd/models/") else {
            return Ok(Vec::new());
        };
        let Some((_side, key)) = under.split_once('/') else {
            return Ok(Vec::new());
        };
        let registry = self.cycle_registry()?;
        let mut out = Vec::new();
        for cycle in &registry.cycles {
            let diffs = cycle::cycle_diffs(cycle).unwrap_or_default();
            if diffs
                .iter()
                .any(|d| d.diagram == key && !d.is_empty())
            {
                out.push(cycle.manifest.number);
            }
        }
        out.sort_unstable();
        Ok(out)
    }

    /// Every `.ocl` constraint file under `.mdd/constraints/`, absolute,
    /// sorted. The OCL-render pass synthesizes a PlantUML constraints
    /// diagram per file and rasterizes it to its `rendered_svg_path`
    /// mirror under `.mdd/rendered/constraints/`.
    pub fn constraint_files(&self) -> Result<Vec<PathBuf>> {
        let base = self.root.join(".mdd/constraints");
        let mut files = Vec::new();
        if !base.is_dir() {
            return Ok(files);
        }
        for entry in WalkDir::new(&base)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            let path = entry.path();
            if entry.file_type().is_file()
                && path.extension().and_then(|e| e.to_str()) == Some("ocl")
            {
                files.push(path.to_path_buf());
            }
        }
        files.sort();
        Ok(files)
    }

    /// Files under `rel` whose path ends with `suffix`, absolute and
    /// sorted. An absent directory yields an empty list (not an error):
    /// a greenfield repo has no `.mdd/map/`, `.mdd/deploy/`, or review
    /// diffs yet.
    fn walk_suffix(&self, rel: &str, suffix: &str) -> Vec<PathBuf> {
        let base = self.root.join(rel);
        let mut files = Vec::new();
        if !base.is_dir() {
            return files;
        }
        for entry in WalkDir::new(&base)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            let path = entry.path();
            if entry.file_type().is_file()
                && path.to_str().is_some_and(|p| p.ends_with(suffix))
            {
                files.push(path.to_path_buf());
            }
        }
        files.sort();
        files
    }

    /// Source files for one renderable tree, absolute and sorted. This
    /// is the single place the tree set is defined; `mdd render` and
    /// every other caller route through it (OCL-RENDER-TREE-PARITY).
    pub fn render_sources(&self, tree: RenderTree) -> Result<Vec<PathBuf>> {
        match tree {
            RenderTree::Models => self.model_files(),
            RenderTree::CycleDiffs => self.cycle_diff_puml_files(),
            RenderTree::OclConstraints => self.constraint_files(),
            RenderTree::WholeMap => Ok(self.walk_suffix(".mdd/map", ".puml")),
            RenderTree::Deploy => Ok(self.walk_suffix(".mdd/deploy", ".puml")),
            RenderTree::ReviewDiff => {
                Ok(self.walk_suffix(".mdd/rendered/review", ".diff.puml"))
            }
        }
    }

    /// Every renderable source across every tree, each paired with the
    /// tree it came from. The full `mdd render` set.
    pub fn all_render_sources(&self) -> Result<Vec<(RenderTree, PathBuf)>> {
        let mut out = Vec::new();
        for tree in RenderTree::ALL {
            for path in self.render_sources(tree)? {
                out.push((tree, path));
            }
        }
        Ok(out)
    }

    /// Deterministic rendered-output path for a render source: its
    /// project-relative path reparented under `.mdd/rendered/` with a
    /// `.svg` extension. Sources already under `.mdd/rendered/` (review
    /// diffs) keep their place — only the suffix changes
    /// (OCL-RENDER-PATH-MIRROR; the cycle case is
    /// OCL-DIFF-SVG-PATH-DERIVED).
    pub fn rendered_mirror_path(&self, source: &Path) -> PathBuf {
        let rel = self
            .relative_path(source)
            .unwrap_or_else(|_| source.to_string_lossy().into_owned());
        if rel.starts_with(".mdd/rendered/") {
            let mut already = PathBuf::from(&rel);
            already.set_extension("svg");
            return self.root.join(already);
        }
        self.rendered_svg_path(&rel)
    }

    pub fn validate(&self) -> Result<ValidationReport> {
        self.validate_inner(true)
    }

    pub fn review(&self) -> Result<ReviewReport> {
        let registry = self.model_registry()?;

        let current_ids: BTreeSet<String> = registry
            .ids
            .iter()
            .filter(|element| element.side == ModelSide::Current)
            .map(|element| element.id.clone())
            .collect();
        let objective_ids: BTreeSet<String> = registry
            .ids
            .iter()
            .filter(|element| element.side == ModelSide::Objective)
            .map(|element| element.id.clone())
            .collect();

        let missing: BTreeSet<String> =
            objective_ids.difference(&current_ids).cloned().collect();
        let extra: BTreeSet<String> =
            current_ids.difference(&objective_ids).cloned().collect();
        let ids_matched = missing.is_empty();

        let mut diff_puml_paths = Vec::new();
        if !ids_matched {
            let id_to_kind: BTreeMap<String, ModelKind> = registry
                .ids
                .iter()
                .map(|element| (element.id.clone(), element.kind))
                .collect();
            let extra_with_kind: Vec<(String, ModelKind)> = extra
                .iter()
                .map(|id| (id.clone(), id_to_kind.get(id).copied().unwrap_or(ModelKind::Other)))
                .collect();

            for file in registry.files.iter().filter(|f| f.side == ModelSide::Objective) {
                let file_missing: BTreeSet<String> = file
                    .ids
                    .iter()
                    .filter(|id| missing.contains(*id))
                    .cloned()
                    .collect();
                if file_missing.is_empty() && extra_with_kind.is_empty() {
                    continue;
                }

                let abs_path = self.root.join(&file.path);
                let content = fs::read_to_string(&abs_path)
                    .with_context(|| format!("failed to read {}", abs_path.display()))?;

                let rel_objective = file
                    .path
                    .strip_prefix(".mdd/models/objective/")
                    .unwrap_or(&file.path);
                let diff_rel = format!(
                    ".mdd/rendered/review/{}",
                    rel_objective.replacen(".puml", ".diff.puml", 1)
                );
                let diff_abs = self.root.join(&diff_rel);
                if let Some(parent) = diff_abs.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }

                let diff_content =
                    annotate_review_diff_puml(&content, &file_missing, &extra_with_kind)?;
                fs::write(&diff_abs, diff_content)
                    .with_context(|| format!("failed to write {}", diff_abs.display()))?;
                diff_puml_paths.push(diff_rel);
            }
        }

        let security = self.review_security()?;
        let security_gate_satisfied = security.matched || security.mode == ParityMode::Warn;
        let matched = ids_matched && security_gate_satisfied;

        Ok(ReviewReport {
            matched,
            ids_matched,
            missing_ids: missing.into_iter().collect(),
            extra_ids: extra.into_iter().collect(),
            diff_puml_paths,
            security,
        })
    }

    pub fn review_security(&self) -> Result<SecurityReviewReport> {
        let config = self.read_config()?;
        let mode = config.security.parity_check;
        let registry = self.model_registry()?;

        let mut current_keys: BTreeSet<String> = BTreeSet::new();
        let mut objective_keys: BTreeSet<String> = BTreeSet::new();
        let mut current_by_key: BTreeMap<String, MarkerSummary> = BTreeMap::new();
        let mut objective_by_key: BTreeMap<String, MarkerSummary> = BTreeMap::new();

        for file in &registry.files {
            if file.kind == ModelKind::Constraint {
                continue;
            }
            if file.side != ModelSide::Current && file.side != ModelSide::Objective {
                continue;
            }
            let content = fs::read_to_string(self.root.join(&file.path))
                .with_context(|| format!("failed to read {}", file.path))?;
            let markers = extract_sec_markers(&content)?;
            for marker in markers {
                if marker.stereotype.is_empty() || marker.host.is_empty() {
                    continue;
                }
                let summary = MarkerSummary {
                    host: marker.host.clone(),
                    stereotype: marker.stereotype.clone(),
                    params: marker.params.clone(),
                    id: marker.id.clone(),
                    file: file.path.clone(),
                };
                let key = parity_key(&summary);
                match file.side {
                    ModelSide::Current => {
                        current_keys.insert(key.clone());
                        current_by_key.entry(key).or_insert(summary);
                    }
                    ModelSide::Objective => {
                        objective_keys.insert(key.clone());
                        objective_by_key.entry(key).or_insert(summary);
                    }
                    ModelSide::Shared => {}
                }
            }
        }

        let missing: Vec<MarkerSummary> = objective_keys
            .difference(&current_keys)
            .filter_map(|key| objective_by_key.get(key).cloned())
            .collect();
        let extra: Vec<MarkerSummary> = current_keys
            .difference(&objective_keys)
            .filter_map(|key| current_by_key.get(key).cloned())
            .collect();
        let matched = missing.is_empty();

        let mut diff_puml_paths = Vec::new();
        if !matched {
            let missing_keys_by_file: BTreeMap<String, Vec<&MarkerSummary>> = {
                let mut map: BTreeMap<String, Vec<&MarkerSummary>> = BTreeMap::new();
                for marker in &missing {
                    map.entry(marker.file.clone()).or_default().push(marker);
                }
                map
            };

            for (file_path, file_missing) in missing_keys_by_file {
                let abs_path = self.root.join(&file_path);
                let content = fs::read_to_string(&abs_path)
                    .with_context(|| format!("failed to read {}", abs_path.display()))?;

                let rel_objective = Path::new(&file_path)
                    .strip_prefix(".mdd/models/objective/")
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| file_path.clone());
                let diff_rel = format!(
                    ".mdd/rendered/review/{}",
                    rel_objective.replacen(".puml", ".security.diff.puml", 1)
                );
                let diff_abs = self.root.join(&diff_rel);
                if let Some(parent) = diff_abs.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }

                let diff_content = annotate_security_diff_puml(&content, &file_missing);
                fs::write(&diff_abs, diff_content)
                    .with_context(|| format!("failed to write {}", diff_abs.display()))?;
                diff_puml_paths.push(diff_rel);
            }
        }

        Ok(SecurityReviewReport {
            matched,
            mode,
            missing_markers: missing,
            extra_markers: extra,
            diff_puml_paths,
        })
    }

    pub fn approve(&self) -> Result<ApprovalReport> {
        let report = self.validate_inner(false)?;
        if !report.ok {
            bail!(
                "cannot approve invalid models:\n{}",
                report.errors.join("\n")
            );
        }

        let mut files = Vec::new();
        for path in self.model_and_constraint_files()? {
            files.push(ApprovedFile {
                path: self.relative_path(&path)?,
                sha256: hash_file(&path)?,
            });
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));

        let approvals = Approvals {
            version: 1,
            approved: true,
            approved_at: Some(now_timestamp()),
            files,
        };
        self.write_yaml(APPROVALS_FILE, &approvals)?;

        Ok(ApprovalReport {
            approved_files: approvals.files.len(),
            approvals_path: APPROVALS_FILE.to_string(),
        })
    }

    pub fn approval_status(&self) -> Result<ApprovalStatus> {
        let approvals = self.read_approvals()?;
        if !approvals.approved {
            return Ok(ApprovalStatus {
                approved: false,
                current: false,
                errors: vec!["models have not been approved".to_string()],
            });
        }

        let approved_map: BTreeMap<_, _> = approvals
            .files
            .iter()
            .map(|file| (file.path.clone(), file.sha256.clone()))
            .collect();
        let mut current_map = BTreeMap::new();
        for path in self.model_and_constraint_files()? {
            current_map.insert(self.relative_path(&path)?, hash_file(&path)?);
        }

        let mut errors = Vec::new();
        for (path, expected_hash) in &approved_map {
            match current_map.get(path) {
                Some(actual_hash) if actual_hash == expected_hash => {}
                Some(_) => errors.push(format!("approved file changed: {path}")),
                None => errors.push(format!("approved file missing: {path}")),
            }
        }
        for path in current_map.keys() {
            if !approved_map.contains_key(path) {
                errors.push(format!("model file is not approved: {path}"));
            }
        }

        Ok(ApprovalStatus {
            approved: true,
            current: errors.is_empty(),
            errors,
        })
    }

    pub fn generate_acceptance_tests(&self) -> Result<TestGenerationReport> {
        let registry = self.model_registry()?;
        let use_case_ids = ids_by_kind(&registry, ModelKind::UseCase);
        if use_case_ids.is_empty() {
            bail!("no use case model IDs found");
        }

        let mut trace = self.read_trace()?;
        let mut generated = Vec::new();
        for model_id in use_case_ids {
            let slug = slugify(&model_id);
            let test_id = format!("AT-{model_id}");
            let rel_path = format!(".mdd/tests/acceptance/{slug}.feature");
            let path = self.root.join(&rel_path);
            let content = acceptance_test_scaffold(&test_id, &model_id);
            fs::write(&path, content)
                .with_context(|| format!("failed to write {}", path.display()))?;

            trace
                .generated_tests
                .retain(|existing| existing.id != test_id && existing.path != rel_path);
            trace.generated_tests.push(GeneratedTest {
                id: test_id,
                path: rel_path.clone(),
                model_id,
                category: None,
            });
            generated.push(rel_path);
        }

        trace
            .generated_tests
            .sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.path.cmp(&b.path)));
        self.write_trace(&trace)?;

        Ok(TestGenerationReport {
            generated,
            trace_path: TRACE_FILE.to_string(),
        })
    }

    pub fn generate_ui_tests(&self) -> Result<TestGenerationReport> {
        let contracts = self.mockup_contracts()?;
        if contracts.is_empty() {
            bail!("no mockup model IDs found");
        }

        let mut trace = self.read_trace()?;
        let mut generated = Vec::new();
        for contract in contracts {
            if !contract.is_implementation_ready() {
                continue;
            }

            let test_id = format!("UIT-{}", contract.model_id);
            let slug = slugify(&contract.model_id);
            let rel_path = format!(".mdd/tests/ui/{slug}.spec.ts");
            let path = self.root.join(&rel_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }

            let content = playwright_test_scaffold(&test_id, &contract);
            fs::write(&path, content)
                .with_context(|| format!("failed to write {}", path.display()))?;

            trace
                .generated_ui_tests
                .retain(|existing| existing.id != test_id && existing.path != rel_path);
            trace.generated_ui_tests.push(GeneratedUiTest {
                id: test_id,
                path: rel_path.clone(),
                model_id: contract.model_id,
                framework: default_ui_test_framework(),
            });
            generated.push(rel_path);
        }

        if generated.is_empty() {
            bail!("no implementation-ready mockup contracts found");
        }

        trace.generated_ui_tests.sort_by(|a, b| {
            a.id.cmp(&b.id)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.framework.cmp(&b.framework))
        });
        self.write_trace(&trace)?;

        Ok(TestGenerationReport {
            generated,
            trace_path: TRACE_FILE.to_string(),
        })
    }

    pub fn generate_security_tests(&self) -> Result<TestGenerationReport> {
        let registry = self.model_registry()?;
        let mut seen_ids: BTreeSet<String> = BTreeSet::new();
        let mut markers: Vec<(String, String, String, BTreeMap<String, String>)> = Vec::new();

        for file in &registry.files {
            if file.kind == ModelKind::Constraint {
                continue;
            }
            if file.side != ModelSide::Objective {
                continue;
            }
            let content = fs::read_to_string(self.root.join(&file.path))
                .with_context(|| format!("failed to read {}", file.path))?;
            for marker in extract_sec_markers(&content)? {
                if !SEC_ACTIVE_STEREOTYPES.contains(&marker.stereotype.as_str()) {
                    continue;
                }
                let Some(sec_id) = marker.id.clone() else {
                    continue;
                };
                if !marker_is_implementation_ready(&marker.stereotype, &marker.params) {
                    continue;
                }
                if !seen_ids.insert(sec_id.clone()) {
                    continue;
                }
                markers.push((
                    sec_id,
                    marker.stereotype.clone(),
                    marker.host.clone(),
                    marker.params.clone(),
                ));
            }
        }

        if markers.is_empty() {
            bail!(
                "no implementation-ready security markers found (need an active stereotype with id=SEC-... and the stereotype's primary tagged values — see .mdd/docs/security-profile.md)"
            );
        }

        let mut trace = self.read_trace()?;
        let mut generated = Vec::new();
        for (sec_id, stereotype, host, params) in markers {
            let slug = slugify(&sec_id);
            let test_id = format!("SECT-{sec_id}");
            let rel_path = format!(".mdd/tests/acceptance/{slug}.feature");
            let path = self.root.join(&rel_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            let content = security_test_scaffold(&stereotype, &test_id, &sec_id, &host, &params);
            fs::write(&path, content)
                .with_context(|| format!("failed to write {}", path.display()))?;

            trace
                .generated_tests
                .retain(|existing| existing.id != test_id && existing.path != rel_path);
            trace.generated_tests.push(GeneratedTest {
                id: test_id,
                path: rel_path.clone(),
                model_id: sec_id,
                category: Some("security".to_string()),
            });
            generated.push(rel_path);
        }

        trace
            .generated_tests
            .sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.path.cmp(&b.path)));
        self.write_trace(&trace)?;

        Ok(TestGenerationReport {
            generated,
            trace_path: TRACE_FILE.to_string(),
        })
    }

    pub fn code_gate(&self) -> Result<CodeGateReport> {
        let validation = self.validate_inner(false)?;
        let errors = validation.errors;
        let mut warnings = validation.warnings;

        let approval = self.approval_status()?;
        if !approval.approved || !approval.current {
            warnings.extend(approval.errors);
        }

        let registry = validation.registry;
        let use_case_ids = ids_by_kind(&registry, ModelKind::UseCase);
        let mockup_ids = ids_by_kind(&registry, ModelKind::Mockup);
        let trace = self.read_trace()?;
        let generated_by_model: BTreeSet<_> = trace
            .generated_tests
            .iter()
            .map(|test| test.model_id.as_str())
            .collect();
        let generated_ui_by_model: BTreeSet<_> = trace
            .generated_ui_tests
            .iter()
            .filter(|test| test.framework == "playwright")
            .map(|test| test.model_id.as_str())
            .collect();

        for use_case_id in &use_case_ids {
            if !generated_by_model.contains(use_case_id.as_str()) {
                warnings.push(format!(
                    "use case has no generated acceptance test: {use_case_id}"
                ));
            }
        }

        if trace.generated_tests.is_empty() {
            warnings.push("no generated acceptance tests found".to_string());
        }

        for mockup_id in &mockup_ids {
            if !generated_ui_by_model.contains(mockup_id.as_str()) {
                warnings.push(format!("mockup has no generated UI test: {mockup_id}"));
            }
        }

        if !mockup_ids.is_empty() && trace.generated_ui_tests.is_empty() {
            warnings.push("no generated UI tests found".to_string());
        }

        for file in registry
            .files
            .iter()
            .filter(|file| file.kind != ModelKind::Constraint)
        {
            let model_path = self.root.join(&file.path);
            let svg_path = self.rendered_svg_path(&file.path);
            if !svg_path.exists() {
                warnings.push(format!("rendered SVG is missing for {}", file.path));
                continue;
            }

            if is_stale(&model_path, &svg_path)? {
                warnings.push(format!("rendered SVG is stale for {}", file.path));
            }
        }

        if registry.files.is_empty() {
            warnings.push("no model files were discovered".to_string());
        }

        Ok(CodeGateReport {
            ok: errors.is_empty(),
            errors,
            warnings,
        })
    }

    pub fn rendered_svg_path(&self, model_relative_path: &str) -> PathBuf {
        let normalized = model_relative_path
            .strip_prefix(".mdd/")
            .unwrap_or(model_relative_path);
        let mut rel = PathBuf::from(normalized);
        rel.set_extension("svg");
        self.root.join(".mdd/rendered").join(rel)
    }

    pub fn diff_text(&self) -> Result<String> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .arg("diff")
            .arg("--")
            .arg(".mdd/models")
            .arg(".mdd/constraints")
            .output()
            .context("failed to run git diff")?;

        if !output.status.success() {
            bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
        }

        let diff = String::from_utf8(output.stdout).context("git diff output was not UTF-8")?;
        if diff.trim().is_empty() {
            Ok("No model or constraint diff found.".to_string())
        } else {
            Ok(diff)
        }
    }

    pub fn model_files(&self) -> Result<Vec<PathBuf>> {
        let files = self
            .model_and_constraint_files()?
            .into_iter()
            .filter(|path| {
                kind_for_path(&self.relative_path(path).unwrap_or_default())
                    != ModelKind::Constraint
            })
            .collect();
        Ok(files)
    }

    fn mockup_contracts(&self) -> Result<Vec<MockupContract>> {
        let mut contracts = Vec::new();
        for path in self.model_and_constraint_files()? {
            let relative = self.relative_path(&path)?;
            if kind_for_path(&relative) != ModelKind::Mockup {
                continue;
            }

            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let mockup_ids = extract_ids(&content)?
                .into_iter()
                .filter(|id| id.starts_with("MCK-"))
                .collect::<Vec<_>>();
            let route = extract_ui_route(&content);
            let viewports = extract_ui_viewports(&content)?;
            let elements = extract_ui_elements(&content)?;

            for model_id in mockup_ids {
                contracts.push(MockupContract {
                    model_id,
                    file: relative.clone(),
                    route: route.clone(),
                    viewports: viewports.clone(),
                    elements: elements.clone(),
                });
            }
        }

        contracts.sort_by(|a, b| {
            a.model_id
                .cmp(&b.model_id)
                .then_with(|| a.file.cmp(&b.file))
        });
        Ok(contracts)
    }

    fn validate_inner(&self, check_approval: bool) -> Result<ValidationReport> {
        if !self.mdd_dir().is_dir() {
            bail!("project is not initialized; run `mdd init` first");
        }

        let registry = self.model_registry()?;
        let trace = self.read_trace()?;
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        if registry.files.is_empty() {
            errors.push("no model or constraint files found".to_string());
        }

        let use_case_ids = ids_by_kind(&registry, ModelKind::UseCase);
        let sequence_ids = ids_by_kind(&registry, ModelKind::Sequence);
        let domain_ids = ids_by_kind(&registry, ModelKind::Domain);
        let mockup_ids = ids_by_kind(&registry, ModelKind::Mockup);
        let all_ids: BTreeSet<_> = registry
            .ids
            .iter()
            .map(|element| element.id.clone())
            .collect();
        let current_ids: BTreeSet<String> = registry
            .ids
            .iter()
            .filter(|element| element.side == ModelSide::Current)
            .map(|element| element.id.clone())
            .collect();
        let objective_ids: BTreeSet<String> = registry
            .ids
            .iter()
            .filter(|element| element.side == ModelSide::Objective)
            .map(|element| element.id.clone())
            .collect();

        if use_case_ids.is_empty() {
            errors.push(
                "no use case model IDs found under .mdd/models/(current|objective)/use-cases"
                    .to_string(),
            );
        }
        if sequence_ids.is_empty() {
            errors.push(
                "no sequence model IDs found under .mdd/models/(current|objective)/sequences"
                    .to_string(),
            );
        }
        if domain_ids.is_empty() {
            errors.push(
                "no domain model IDs found under .mdd/models/(current|objective)/domain"
                    .to_string(),
            );
        }

        let mut ui_contract_ids = BTreeMap::<String, String>::new();
        let mut implementation_ready_mockups = BTreeSet::<String>::new();
        let mut sec_id_registrations: Vec<(String, ModelSide, String)> = Vec::new();
        let mut sec_impl_ready: BTreeSet<String> = BTreeSet::new();

        for file in &registry.files {
            if file.ids.is_empty() && file.kind != ModelKind::Constraint {
                errors.push(format!("model file has no @id(...) marker: {}", file.path));
            }

            let content = if file.kind != ModelKind::Constraint {
                Some(
                    fs::read_to_string(self.root.join(&file.path))
                        .with_context(|| format!("failed to read {}", file.path))?,
                )
            } else {
                None
            };

            if file.kind == ModelKind::Mockup {
                let content = content
                    .as_deref()
                    .expect("non-constraint file should have content");

                let file_mockup_ids = file
                    .ids
                    .iter()
                    .filter(|id| id.starts_with("MCK-"))
                    .cloned()
                    .collect::<Vec<_>>();
                if file_mockup_ids.is_empty() {
                    errors.push(format!(
                        "mockup file has no MCK-... @id(...) marker: {}",
                        file.path
                    ));
                }

                let ui_elements = extract_ui_elements(content)?;
                for element in &ui_elements {
                    if !element.id.starts_with("UIC-") {
                        errors.push(format!(
                            "ui element contract has invalid ID {} in {}",
                            element.id, file.path
                        ));
                    }
                    if let Some(first_file) =
                        ui_contract_ids.insert(element.id.clone(), file.path.clone())
                    {
                        errors.push(format!(
                            "duplicate UI contract ID {} in {} and {}",
                            element.id, first_file, file.path
                        ));
                    }
                }

                if extract_ui_route(content).is_some() && !ui_elements.is_empty() {
                    implementation_ready_mockups.extend(file_mockup_ids);
                }
            }

            if let Some(content) = content.as_deref() {
                let sec_markers = extract_sec_markers(content)?;
                for marker in &sec_markers {
                    if marker.stereotype.is_empty() {
                        errors.push(format!(
                            "@sec(...) is missing required `stereotype=` key in {}",
                            file.path
                        ));
                        continue;
                    }
                    if !SEC_ACTIVE_STEREOTYPES.contains(&marker.stereotype.as_str()) {
                        errors.push(format!(
                            "@sec(stereotype={}, ...) uses unknown stereotype in {}; Phase 1 supports: {}",
                            marker.stereotype,
                            file.path,
                            SEC_ACTIVE_STEREOTYPES.join(", ")
                        ));
                        continue;
                    }
                    if marker.host.is_empty() {
                        errors.push(format!(
                            "@sec(stereotype={}, ...) is missing required `host=` key in {}",
                            marker.stereotype, file.path
                        ));
                        continue;
                    }
                    if !file.ids.contains(&marker.host) {
                        errors.push(format!(
                            "@sec(host={}, ...) host does not resolve to an @id(...) in the same file ({})",
                            marker.host, file.path
                        ));
                        continue;
                    }
                    let kind = detect_host_kind(content, &marker.host);
                    let has = |key: &str| {
                        marker
                            .params
                            .get(key)
                            .map(|value| !value.is_empty())
                            .unwrap_or(false)
                    };
                    let value_of = |key: &str| marker.params.get(key).map(String::as_str);
                    match marker.stereotype.as_str() {
                        "ByPassing" => match kind {
                            HostKind::Actor => {
                                if !has("role") {
                                    errors.push(format!(
                                        "@sec(stereotype=ByPassing, host={}, ...) on actor host requires `role=` in {}",
                                        marker.host, file.path
                                    ));
                                }
                            }
                            HostKind::UseCase => {
                                if !has("allowed") && !has("denied") {
                                    errors.push(format!(
                                        "@sec(stereotype=ByPassing, host={}, ...) on use-case host requires `allowed=` or `denied=` in {}",
                                        marker.host, file.path
                                    ));
                                }
                            }
                            _ => {
                                if !has("role") && !has("allowed") && !has("denied") {
                                    errors.push(format!(
                                        "@sec(stereotype=ByPassing, host={}, ...) requires `role=`, `allowed=`, or `denied=` in {}",
                                        marker.host, file.path
                                    ));
                                }
                            }
                        },
                        "Encrypt" => {
                            if !matches!(kind, HostKind::Class | HostKind::SequenceParticipant) {
                                errors.push(format!(
                                    "@sec(stereotype=Encrypt, host={}, ...) requires host to be a class or sequence participant in {}",
                                    marker.host, file.path
                                ));
                            }
                            if !has("algorithm") {
                                errors.push(format!(
                                    "@sec(stereotype=Encrypt, host={}, ...) is missing required `algorithm=` key in {}",
                                    marker.host, file.path
                                ));
                            }
                            match value_of("scope") {
                                None => errors.push(format!(
                                    "@sec(stereotype=Encrypt, host={}, ...) is missing required `scope=` key in {}",
                                    marker.host, file.path
                                )),
                                Some(scope) if !ENCRYPT_SCOPES.contains(&scope) => {
                                    errors.push(format!(
                                        "@sec(stereotype=Encrypt, host={}, ...) has invalid `scope={}` in {}; must be one of {}",
                                        marker.host, scope, file.path, ENCRYPT_SCOPES.join(", ")
                                    ));
                                }
                                _ => {}
                            }
                        }
                        "BufferOverflow" => {
                            if kind != HostKind::Class {
                                errors.push(format!(
                                    "@sec(stereotype=BufferOverflow, host={}, ...) requires host to be a class in {}",
                                    marker.host, file.path
                                ));
                            }
                            if !has("field") {
                                errors.push(format!(
                                    "@sec(stereotype=BufferOverflow, host={}, ...) is missing required `field=` key in {}",
                                    marker.host, file.path
                                ));
                            }
                            match value_of("max_length") {
                                None => errors.push(format!(
                                    "@sec(stereotype=BufferOverflow, host={}, ...) is missing required `max_length=` key in {}",
                                    marker.host, file.path
                                )),
                                Some(value) => match value.parse::<u32>() {
                                    Ok(n) if n > 0 => {}
                                    _ => errors.push(format!(
                                        "@sec(stereotype=BufferOverflow, host={}, ...) has invalid `max_length={}` in {}; must be a positive integer",
                                        marker.host, value, file.path
                                    )),
                                },
                            }
                        }
                        "SqlInjection" => {
                            if kind != HostKind::Class {
                                errors.push(format!(
                                    "@sec(stereotype=SqlInjection, host={}, ...) requires host to be a class in {}",
                                    marker.host, file.path
                                ));
                            }
                            if !has("field") {
                                errors.push(format!(
                                    "@sec(stereotype=SqlInjection, host={}, ...) is missing required `field=` key in {}",
                                    marker.host, file.path
                                ));
                            }
                            if !has("sink") {
                                errors.push(format!(
                                    "@sec(stereotype=SqlInjection, host={}, ...) is missing required `sink=` key in {}",
                                    marker.host, file.path
                                ));
                            }
                            match value_of("sanitizer") {
                                None => errors.push(format!(
                                    "@sec(stereotype=SqlInjection, host={}, ...) is missing required `sanitizer=` key in {}",
                                    marker.host, file.path
                                )),
                                Some(value) if !SQLI_SANITIZERS.contains(&value) => {
                                    errors.push(format!(
                                        "@sec(stereotype=SqlInjection, host={}, ...) has invalid `sanitizer={}` in {}; must be one of {}",
                                        marker.host, value, file.path, SQLI_SANITIZERS.join(", ")
                                    ));
                                }
                                _ => {}
                            }
                        }
                        "Flooding" => {
                            if !matches!(kind, HostKind::UseCase | HostKind::Component) {
                                errors.push(format!(
                                    "@sec(stereotype=Flooding, host={}, ...) requires host to be a use case or component in {}",
                                    marker.host, file.path
                                ));
                            }
                            if !has("max_rate") && !has("max_concurrent") {
                                errors.push(format!(
                                    "@sec(stereotype=Flooding, host={}, ...) requires at least one of `max_rate=` or `max_concurrent=` in {}",
                                    marker.host, file.path
                                ));
                            }
                            for key in ["max_rate", "max_concurrent"] {
                                if let Some(value) = value_of(key) {
                                    if value.parse::<u32>().map(|n| n == 0).unwrap_or(true) {
                                        errors.push(format!(
                                            "@sec(stereotype=Flooding, host={}, ...) has invalid `{}={}` in {}; must be a positive integer",
                                            marker.host, key, value, file.path
                                        ));
                                    }
                                }
                            }
                        }
                        "Expiration" => {
                            if kind != HostKind::Class {
                                errors.push(format!(
                                    "@sec(stereotype=Expiration, host={}, ...) requires host to be a class in {}",
                                    marker.host, file.path
                                ));
                            }
                            if !has("field") {
                                errors.push(format!(
                                    "@sec(stereotype=Expiration, host={}, ...) is missing required `field=` key in {}",
                                    marker.host, file.path
                                ));
                            }
                            if !has("ttl") {
                                errors.push(format!(
                                    "@sec(stereotype=Expiration, host={}, ...) is missing required `ttl=` key in {}",
                                    marker.host, file.path
                                ));
                            }
                        }
                        _ => {}
                    }
                    if let Some(sec_id) = &marker.id {
                        sec_id_registrations.push((sec_id.clone(), file.side, file.path.clone()));
                        if marker
                            .params
                            .get("link")
                            .map(|value| !value.is_empty())
                            .unwrap_or(false)
                        {
                            sec_impl_ready.insert(sec_id.clone());
                        }
                    }
                }
            }

            for reference in &file.refs {
                let valid = match (file.kind, file.side) {
                    (ModelKind::Constraint, _) => domain_ids.contains(reference),
                    (_, ModelSide::Current) => current_ids.contains(reference),
                    (_, ModelSide::Objective) => objective_ids.contains(reference),
                    (_, ModelSide::Shared) => all_ids.contains(reference),
                };
                if !valid {
                    errors.push(format!("unresolved @ref({reference}) in {}", file.path));
                }
            }
        }

        let mut seen_current = BTreeMap::<String, String>::new();
        let mut seen_objective = BTreeMap::<String, String>::new();
        let mut seen_shared = BTreeMap::<String, String>::new();
        for element in &registry.ids {
            let seen = match element.side {
                ModelSide::Current => &mut seen_current,
                ModelSide::Objective => &mut seen_objective,
                ModelSide::Shared => &mut seen_shared,
            };
            if let Some(first_file) = seen.insert(element.id.clone(), element.file.clone()) {
                errors.push(format!(
                    "duplicate model ID {} in {} and {} (same side)",
                    element.id, first_file, element.file
                ));
            }
        }

        for (sec_id, side, file_path) in &sec_id_registrations {
            let seen = match side {
                ModelSide::Current => &mut seen_current,
                ModelSide::Objective => &mut seen_objective,
                ModelSide::Shared => &mut seen_shared,
            };
            if let Some(first_file) = seen.insert(sec_id.clone(), file_path.clone()) {
                errors.push(format!(
                    "duplicate model ID {} in {} and {} (same side)",
                    sec_id, first_file, file_path
                ));
            }
        }

        for link in &trace.links {
            if !all_ids.contains(&link.from) {
                errors.push(format!(
                    "trace link references unknown source ID: {}",
                    link.from
                ));
            }
            if !all_ids.contains(&link.to) {
                errors.push(format!(
                    "trace link references unknown target ID: {}",
                    link.to
                ));
            }
        }

        for use_case_id in &use_case_ids {
            let has_sequence = trace
                .links
                .iter()
                .any(|link| link.from == *use_case_id && sequence_ids.contains(&link.to));
            if !has_sequence {
                errors.push(format!(
                    "use case {use_case_id} is not linked to a sequence diagram in .mdd/trace.yml"
                ));
            }
        }

        for test in &trace.generated_tests {
            if !all_ids.contains(&test.model_id) {
                errors.push(format!(
                    "generated test {} references unknown model ID {}",
                    test.id, test.model_id
                ));
            }
            if !self.root.join(&test.path).exists() {
                errors.push(format!("generated test file is missing: {}", test.path));
            }
        }

        for test in &trace.generated_ui_tests {
            if !all_ids.contains(&test.model_id) {
                errors.push(format!(
                    "generated UI test {} references unknown model ID {}",
                    test.id, test.model_id
                ));
            } else if !mockup_ids.contains(&test.model_id) {
                errors.push(format!(
                    "generated UI test {} references non-mockup model ID {}",
                    test.id, test.model_id
                ));
            }
            if !self.root.join(&test.path).exists() {
                errors.push(format!("generated UI test file is missing: {}", test.path));
            }
        }

        let generated_playwright_by_model: BTreeSet<_> = trace
            .generated_ui_tests
            .iter()
            .filter(|test| test.framework == "playwright")
            .map(|test| test.model_id.clone())
            .collect();
        for mockup_id in &implementation_ready_mockups {
            if !generated_playwright_by_model.contains(mockup_id) {
                errors.push(format!(
                    "implementation-ready mockup {mockup_id} has no generated Playwright UI test in .mdd/trace.yml"
                ));
            }
        }

        let security_test_ids: BTreeSet<&str> = trace
            .generated_tests
            .iter()
            .filter(|test| test.category.as_deref() == Some("security"))
            .map(|test| test.model_id.as_str())
            .collect();
        for sec_id in &sec_impl_ready {
            if !security_test_ids.contains(sec_id.as_str()) {
                warnings.push(format!(
                    "implementation-ready security marker {sec_id} has no generated security test in .mdd/tests/acceptance/ (run `/mdd-generate-security-tests` or supply the scaffold manually)"
                ));
            }
        }

        if check_approval {
            let approval = self.approval_status()?;
            if approval.approved && !approval.current {
                warnings.extend(approval.errors);
            }
        }

        if trace.links.is_empty() {
            warnings.push("trace.yml has no model-to-model links yet".to_string());
        }

        Ok(ValidationReport {
            ok: errors.is_empty(),
            errors,
            warnings,
            registry,
        })
    }

    fn rendered_pages_for(&self, relative_source: &str) -> Vec<String> {
        let Some(rest) = relative_source.strip_prefix(".mdd/") else {
            return Vec::new();
        };
        let rest_path = Path::new(rest);
        let Some(parent) = rest_path.parent() else {
            return Vec::new();
        };
        let Some(stem) = rest_path.file_stem().and_then(|s| s.to_str()) else {
            return Vec::new();
        };

        let rendered_dir = self.root.join(".mdd/rendered").join(parent);
        if !rendered_dir.exists() {
            return Vec::new();
        }

        let base_name = format!("{stem}.svg");
        let numbered_prefix = format!("{stem}_");
        let mut numbered: Vec<String> = Vec::new();
        let mut has_base = false;

        let Ok(entries) = fs::read_dir(&rendered_dir) else {
            return Vec::new();
        };
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if name == base_name {
                has_base = true;
            } else if name.starts_with(&numbered_prefix) && name.ends_with(".svg") {
                numbered.push(name);
            }
        }
        numbered.sort();

        let parent_str = parent.to_string_lossy().replace('\\', "/");
        let mut pages = Vec::new();
        if has_base {
            pages.push(format!("{parent_str}/{base_name}"));
        }
        for name in numbered {
            pages.push(format!("{parent_str}/{name}"));
        }
        pages
    }

    fn model_and_constraint_files(&self) -> Result<Vec<PathBuf>> {
        let roots = [
            self.root.join(".mdd/models"),
            self.root.join(".mdd/constraints"),
        ];
        let mut files = Vec::new();

        for root in roots {
            if !root.exists() {
                continue;
            }
            for entry in WalkDir::new(root)
                .into_iter()
                .filter_map(std::result::Result::ok)
            {
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.into_path();
                if is_model_or_constraint_file(&path) {
                    files.push(path);
                }
            }
        }

        files.sort();
        Ok(files)
    }

    fn write_yaml_with_conflict_handler<T: Serialize, F>(
        &self,
        relative: &str,
        value: &T,
        on_conflict: &mut F,
        created: &mut Vec<String>,
        overwritten: &mut Vec<String>,
        skipped: &mut Vec<String>,
    ) -> Result<()>
    where
        F: FnMut(&str) -> Result<InitFileConflict>,
    {
        let content = serde_yaml::to_string(value)
            .with_context(|| format!("failed to serialize {relative}"))?;
        self.write_text_with_conflict_handler(
            relative,
            &content,
            on_conflict,
            created,
            overwritten,
            skipped,
        )
    }

    fn write_text_if_missing<F>(
        &self,
        relative: &str,
        value: &str,
        created: &mut Vec<String>,
        overwritten: &mut Vec<String>,
        skipped: &mut Vec<String>,
        on_conflict: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&str) -> Result<InitFileConflict>,
    {
        self.write_text_with_conflict_handler(
            relative,
            value,
            on_conflict,
            created,
            overwritten,
            skipped,
        )
    }

    fn write_text_with_conflict_handler<F>(
        &self,
        relative: &str,
        value: &str,
        on_conflict: &mut F,
        created: &mut Vec<String>,
        overwritten: &mut Vec<String>,
        skipped: &mut Vec<String>,
    ) -> Result<()>
    where
        F: FnMut(&str) -> Result<InitFileConflict>,
    {
        let path = self.root.join(relative);
        if path.exists() {
            match on_conflict(relative)? {
                InitFileConflict::Skip => {
                    skipped.push(relative.to_string());
                    return Ok(());
                }
                InitFileConflict::Overwrite => {
                    if path.is_dir() {
                        skipped.push(relative.to_string());
                        return Ok(());
                    }
                    fs::write(&path, value)
                        .with_context(|| format!("failed to write {}", path.display()))?;
                    overwritten.push(relative.to_string());
                    return Ok(());
                }
            }
        } else {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(&path, value)
                .with_context(|| format!("failed to write {}", path.display()))?;
            created.push(relative.to_string());
        }
        Ok(())
    }

    /// Inject (or refresh in place) the deterministic mdd block in a file that
    /// the user may also own. Existing user content is always preserved; the
    /// init conflict handler is intentionally not consulted here.
    fn write_managed_block(
        &self,
        relative: &str,
        kind: &str,
        body: &str,
        created: &mut Vec<String>,
        overwritten: &mut Vec<String>,
    ) -> Result<()> {
        let path = self.root.join(relative);
        let block = render_managed_block(kind, body)?;

        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(&path, &block)
                .with_context(|| format!("failed to write {}", path.display()))?;
            created.push(relative.to_string());
            return Ok(());
        }

        let current = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;

        let updated = if let Some((start, end)) = find_managed_block(&current) {
            format!("{}{}{}", &current[..start], block, &current[end..])
        } else if current.is_empty() {
            block.clone()
        } else if current.ends_with("\n\n") {
            format!("{current}{block}")
        } else if current.ends_with('\n') {
            format!("{current}\n{block}")
        } else {
            format!("{current}\n\n{block}")
        };

        if updated == current {
            return Ok(());
        }

        fs::write(&path, &updated)
            .with_context(|| format!("failed to write {}", path.display()))?;
        overwritten.push(relative.to_string());
        Ok(())
    }

    fn remove_dir_all_if_exists(
        &self,
        relative: &str,
        removed: &mut Vec<String>,
        skipped: &mut Vec<CleanSkip>,
    ) -> Result<()> {
        let path = self.root.join(relative);
        if !path.exists() {
            return Ok(());
        }

        if !path.is_dir() {
            skipped.push(CleanSkip {
                path: relative.to_string(),
                reason: "expected a directory created by mdd init".to_string(),
            });
            return Ok(());
        }

        fs::remove_dir_all(&path)
            .with_context(|| format!("failed to remove {}", path.display()))?;
        removed.push(relative.to_string());
        Ok(())
    }

    fn remove_generated_text_file(
        &self,
        relative: &str,
        expected: &str,
        force: bool,
        removed: &mut Vec<String>,
        skipped: &mut Vec<CleanSkip>,
    ) -> Result<()> {
        let path = self.root.join(relative);
        if !path.exists() {
            return Ok(());
        }

        if path.is_dir() {
            skipped.push(CleanSkip {
                path: relative.to_string(),
                reason: "expected a file created by mdd init".to_string(),
            });
            return Ok(());
        }

        if !force {
            match fs::read_to_string(&path) {
                Ok(content) if content == expected => {}
                Ok(_) => {
                    skipped.push(CleanSkip {
                        path: relative.to_string(),
                        reason: "content differs from the generated mdd init template".to_string(),
                    });
                    return Ok(());
                }
                Err(error) => {
                    skipped.push(CleanSkip {
                        path: relative.to_string(),
                        reason: format!(
                            "could not verify generated content ({error}); use --force to remove"
                        ),
                    });
                    return Ok(());
                }
            }
        }

        fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
        removed.push(relative.to_string());
        Ok(())
    }

    /// Strip only the deterministic mdd block from a file, preserving any user
    /// content around it. The whole file is removed only when the block was its
    /// sole content. Files without the markers are left untouched.
    fn remove_managed_block(
        &self,
        relative: &str,
        force: bool,
        removed: &mut Vec<String>,
        skipped: &mut Vec<CleanSkip>,
    ) -> Result<()> {
        let path = self.root.join(relative);
        if !path.exists() {
            return Ok(());
        }

        if path.is_dir() {
            skipped.push(CleanSkip {
                path: relative.to_string(),
                reason: "expected a file created by mdd init".to_string(),
            });
            return Ok(());
        }

        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                skipped.push(CleanSkip {
                    path: relative.to_string(),
                    reason: format!(
                        "could not read file ({error}); use --force to remove the mdd block"
                    ),
                });
                return Ok(());
            }
        };

        let Some(span) = find_managed_block(&content) else {
            // No markers: not an mdd-managed file under the new format.
            return Ok(());
        };

        if !force {
            let verified = parse_block_meta(&content, span)
                .zip(extract_block_body(&content, span))
                .is_some_and(|(meta, body)| meta.content_sha256 == hash_str(body));
            if !verified {
                skipped.push(CleanSkip {
                    path: relative.to_string(),
                    reason: "managed mdd block was modified; rerun with --force to remove"
                        .to_string(),
                });
                return Ok(());
            }
        }

        let remainder = format!("{}{}", &content[..span.0], &content[span.1..]);
        if remainder.trim().is_empty() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
            removed.push(relative.to_string());
        } else {
            let remainder = format!("{}\n", remainder.trim_end());
            fs::write(&path, &remainder)
                .with_context(|| format!("failed to write {}", path.display()))?;
            removed.push(format!("{relative} (mdd block)"));
        }
        Ok(())
    }

    fn remove_empty_dir_if_exists(&self, relative: &str, removed: &mut Vec<String>) -> Result<()> {
        let path = self.root.join(relative);
        if !path.is_dir() {
            return Ok(());
        }

        let is_empty = fs::read_dir(&path)
            .with_context(|| format!("failed to read {}", path.display()))?
            .next()
            .is_none();
        if is_empty {
            fs::remove_dir(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
            removed.push(relative.to_string());
        }

        Ok(())
    }

    fn read_yaml<T: for<'de> Deserialize<'de>>(&self, relative: &str) -> Result<T> {
        let path = self.root.join(relative);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_yaml::from_str(&content).with_context(|| format!("failed to parse {relative}"))
    }

    fn write_yaml<T: Serialize>(&self, relative: &str, value: &T) -> Result<()> {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let content = serde_yaml::to_string(value)
            .with_context(|| format!("failed to serialize {relative}"))?;
        fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))
    }

    fn relative_path(&self, path: &Path) -> Result<String> {
        let relative = path
            .strip_prefix(&self.root)
            .with_context(|| format!("{} is outside {}", path.display(), self.root.display()))?;
        Ok(relative.to_string_lossy().replace('\\', "/"))
    }
}

#[derive(Debug, Clone)]
struct MockupContract {
    model_id: String,
    file: String,
    route: Option<String>,
    viewports: Vec<UiViewport>,
    elements: Vec<UiElementContract>,
}

impl MockupContract {
    fn is_implementation_ready(&self) -> bool {
        self.route.is_some() && !self.elements.is_empty()
    }
}

#[derive(Debug, Clone)]
struct UiViewport {
    name: String,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone)]
struct UiElementContract {
    id: String,
    role: Option<String>,
    name: Option<String>,
    required: bool,
}

#[derive(Debug, Clone)]
struct SecMarker {
    stereotype: String,
    host: String,
    id: Option<String>,
    params: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum HostKind {
    Actor,
    UseCase,
    Class,
    Component,
    SequenceParticipant,
    Unknown,
}

const SEC_ACTIVE_STEREOTYPES: &[&str] = &[
    "ByPassing",
    "Encrypt",
    "BufferOverflow",
    "SqlInjection",
    "Flooding",
    "Expiration",
];

const ENCRYPT_SCOPES: &[&str] = &["at_rest", "in_transit", "both"];
const SQLI_SANITIZERS: &[&str] =
    &["parameterized", "prepared-statement", "orm", "escape", "stored-procedure"];

fn default_ui_test_framework() -> String {
    "playwright".to_string()
}

fn ids_by_kind(registry: &ModelRegistry, kind: ModelKind) -> BTreeSet<String> {
    registry
        .ids
        .iter()
        .filter(|element| element.kind == kind)
        .map(|element| element.id.clone())
        .collect()
}

fn kind_for_path(relative: &str) -> ModelKind {
    let after_side = relative
        .strip_prefix(".mdd/models/current/")
        .or_else(|| relative.strip_prefix(".mdd/models/objective/"));
    if let Some(rest) = after_side {
        if rest.starts_with("use-cases/") {
            ModelKind::UseCase
        } else if rest.starts_with("sequences/") {
            ModelKind::Sequence
        } else if rest.starts_with("domain/") {
            ModelKind::Domain
        } else if rest.starts_with("components/") {
            ModelKind::Other
        } else if rest.starts_with("mockups/") {
            ModelKind::Mockup
        } else if rest.starts_with("states/") {
            ModelKind::State
        } else {
            ModelKind::Other
        }
    } else if relative.starts_with(".mdd/constraints/") {
        ModelKind::Constraint
    } else {
        ModelKind::Other
    }
}

fn side_for_path(relative: &str) -> ModelSide {
    if relative.starts_with(".mdd/models/current/") {
        ModelSide::Current
    } else if relative.starts_with(".mdd/models/objective/") {
        ModelSide::Objective
    } else {
        ModelSide::Shared
    }
}

fn is_model_or_constraint_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("puml" | "plantuml" | "uml" | "ocl")
    )
}

fn extract_ids(content: &str) -> Result<Vec<String>> {
    let re = Regex::new(r"@id\(([A-Za-z0-9_.:-]+)\)")?;
    let mut ids = re
        .captures_iter(content)
        .map(|capture| capture[1].to_string())
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// Extract `@desc(<ID>, "text")` markers: a one-line human-readable
/// description of a model element, surfaced by the viewer in the
/// MODEL CONTEXT card. Later markers for the same ID win.
fn extract_descs(content: &str) -> Result<BTreeMap<String, String>> {
    let re = Regex::new(r#"@desc\(\s*([A-Za-z0-9_.:-]+)\s*,\s*"((?:[^"\\]|\\.)*)"\s*\)"#)?;
    let mut out = BTreeMap::new();
    for capture in re.captures_iter(content) {
        let id = capture[1].to_string();
        let text = capture[2].replace("\\\"", "\"").replace("\\\\", "\\");
        out.insert(id, text);
    }
    Ok(out)
}

fn extract_refs(content: &str) -> Result<Vec<String>> {
    let re = Regex::new(r"@ref\(([A-Za-z0-9_.:-]+)\)")?;
    let mut refs = re
        .captures_iter(content)
        .map(|capture| capture[1].to_string())
        .collect::<Vec<_>>();
    refs.sort();
    refs.dedup();
    Ok(refs)
}

fn extract_ui_route(content: &str) -> Option<String> {
    let re = Regex::new(r"@ui-route\(([^)]*)\)").ok()?;
    re.captures(content)
        .map(|capture| capture[1].trim().to_string())
        .filter(|route| !route.is_empty())
}

fn extract_ui_viewports(content: &str) -> Result<Vec<UiViewport>> {
    let re = Regex::new(r"@ui-viewport\(([^)]*)\)")?;
    let mut viewports = Vec::new();
    for capture in re.captures_iter(content) {
        let parts = split_contract_args(&capture[1]);
        if parts.len() != 3 {
            continue;
        }

        let width = match parts[1].trim().parse::<u32>() {
            Ok(width) => width,
            Err(_) => continue,
        };
        let height = match parts[2].trim().parse::<u32>() {
            Ok(height) => height,
            Err(_) => continue,
        };
        viewports.push(UiViewport {
            name: unquote(parts[0].trim()),
            width,
            height,
        });
    }

    Ok(viewports)
}

fn extract_ui_elements(content: &str) -> Result<Vec<UiElementContract>> {
    let re = Regex::new(r"@ui-element\(([^)]*)\)")?;
    let mut elements = Vec::new();
    for capture in re.captures_iter(content) {
        let parts = split_contract_args(&capture[1]);
        if parts.is_empty() {
            elements.push(UiElementContract {
                id: String::new(),
                role: None,
                name: None,
                required: true,
            });
            continue;
        }

        let mut role = None;
        let mut name = None;
        let mut required = true;
        for attr in parts.iter().skip(1) {
            let Some((key, value)) = attr.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = unquote(value.trim());
            match key {
                "role" => role = Some(value),
                "name" => name = Some(value),
                "required" => required = value.eq_ignore_ascii_case("true"),
                _ => {}
            }
        }

        elements.push(UiElementContract {
            id: parts[0].trim().to_string(),
            role,
            name,
            required,
        });
    }

    Ok(elements)
}

fn extract_sec_markers(content: &str) -> Result<Vec<SecMarker>> {
    let re = Regex::new(r"@sec\(([^)]*)\)")?;
    let mut markers = Vec::new();
    for capture in re.captures_iter(content) {
        let parts = split_contract_args(&capture[1]);
        let mut stereotype = String::new();
        let mut host = String::new();
        let mut id = None;
        let mut params = BTreeMap::new();
        for attr in &parts {
            let Some((key, value)) = attr.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = unquote(value.trim());
            match key {
                "stereotype" => stereotype = value,
                "host" => host = value,
                "id" => id = Some(value),
                _ => {
                    params.insert(key.to_string(), value);
                }
            }
        }
        markers.push(SecMarker {
            stereotype,
            host,
            id,
            params,
        });
    }
    Ok(markers)
}

fn detect_host_kind(content: &str, host_id: &str) -> HostKind {
    let needle = format!("@id({host_id})");
    let mut found_host = false;
    for line in content.lines() {
        if !found_host {
            if line.contains(&needle) {
                found_host = true;
            }
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('\'') || trimmed.starts_with('@') {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("actor") {
            return HostKind::Actor;
        }
        if lower.starts_with("usecase") || lower.starts_with("rectangle") || lower.starts_with('(')
        {
            return HostKind::UseCase;
        }
        if lower.starts_with("class ")
            || lower.starts_with("interface ")
            || lower.starts_with("enum ")
            || lower.starts_with("abstract ")
        {
            return HostKind::Class;
        }
        if lower.starts_with("component ")
            || lower.starts_with("package ")
            || lower.starts_with("node ")
            || lower.starts_with("folder ")
        {
            return HostKind::Component;
        }
        if lower.starts_with("participant ")
            || lower.starts_with("boundary ")
            || lower.starts_with("control ")
            || lower.starts_with("entity ")
            || lower.starts_with("database ")
            || lower.starts_with("collections ")
            || lower.starts_with("queue ")
        {
            return HostKind::SequenceParticipant;
        }
        return HostKind::Unknown;
    }
    HostKind::Unknown
}

fn split_contract_args(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;

    for ch in value.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_quotes => {
                current.push(ch);
                escaped = true;
            }
            '"' => {
                current.push(ch);
                in_quotes = !in_quotes;
            }
            ',' if !in_quotes => {
                let part = current.trim();
                if !part.is_empty() {
                    parts.push(part.to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    let part = current.trim();
    if !part.is_empty() {
        parts.push(part.to_string());
    }

    parts
}

fn unquote(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        trimmed[1..trimmed.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else {
        trimmed.to_string()
    }
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_str(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

const MANAGED_BLOCK_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManagedBlockMeta {
    tool: String,
    schema: u32,
    kind: String,
    content_sha256: String,
}

/// Render the deterministic, marker-delimited block that `mdd init` writes into
/// `CLAUDE.md` / `AGENTS.md`. The JSON metadata line lets `mdd clean` recognize
/// the block and detect hand-edits via `content_sha256`.
fn render_managed_block(kind: &str, body: &str) -> Result<String> {
    let meta = ManagedBlockMeta {
        tool: "mdd".to_string(),
        schema: MANAGED_BLOCK_SCHEMA,
        kind: kind.to_string(),
        content_sha256: hash_str(body),
    };
    let meta_json = serde_json::to_string(&meta)
        .with_context(|| format!("failed to serialize mdd block metadata for {kind}"))?;
    let body = if body.ends_with('\n') {
        body.to_string()
    } else {
        format!("{body}\n")
    };
    Ok(format!(
        "{begin}\n{prefix}{meta_json}{suffix}\n{body}{end}\n",
        begin = templates::MDD_BLOCK_BEGIN,
        prefix = templates::MDD_META_PREFIX,
        suffix = templates::MDD_META_SUFFIX,
        end = templates::MDD_BLOCK_END,
    ))
}

/// Locate the managed block inside `content`. Returns the byte range spanning
/// from the start of the begin-marker line through the newline that follows the
/// end-marker line (so removing the range leaves no dangling blank line).
fn find_managed_block(content: &str) -> Option<(usize, usize)> {
    let begin = content.find(templates::MDD_BLOCK_BEGIN)?;
    let end_marker = content[begin..].find(templates::MDD_BLOCK_END)? + begin;
    let mut end = end_marker + templates::MDD_BLOCK_END.len();
    if content[end..].starts_with('\n') {
        end += 1;
    }
    Some((begin, end))
}

/// Extract the entrypoint body from a located managed block: everything between
/// the metadata line and the closing sentinel. Used to re-verify the sha256.
fn extract_block_body(content: &str, span: (usize, usize)) -> Option<&str> {
    let block = &content[span.0..span.1];
    let after_meta = block
        .find(templates::MDD_META_PREFIX)
        .and_then(|meta_start| block[meta_start..].find('\n').map(|nl| meta_start + nl + 1))?;
    let end_marker = block.find(templates::MDD_BLOCK_END)?;
    if after_meta > end_marker {
        return None;
    }
    Some(&block[after_meta..end_marker])
}

fn parse_block_meta(content: &str, span: (usize, usize)) -> Option<ManagedBlockMeta> {
    let block = &content[span.0..span.1];
    let meta_start = block.find(templates::MDD_META_PREFIX)? + templates::MDD_META_PREFIX.len();
    let rest = &block[meta_start..];
    let meta_end = rest.find(templates::MDD_META_SUFFIX)?;
    serde_json::from_str(rest[..meta_end].trim()).ok()
}

fn now_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    seconds.to_string()
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

fn acceptance_test_scaffold(test_id: &str, model_id: &str) -> String {
    format!(
        "@model:{model_id}\n@test:{test_id}\nFeature: {model_id}\n\n  Scenario: Modeled behavior is implemented\n    Given the model element \"{model_id}\"\n    When the feature is exercised through its public interface\n    Then the observable behavior matches the model\n"
    )
}

fn marker_is_implementation_ready(stereotype: &str, params: &BTreeMap<String, String>) -> bool {
    let has = |key: &str| params.get(key).map(|v| !v.is_empty()).unwrap_or(false);
    match stereotype {
        "ByPassing" => has("link"),
        "Encrypt" => has("algorithm") && has("scope"),
        "BufferOverflow" => has("max_length") && has("field"),
        "SqlInjection" => has("sink") && has("sanitizer") && has("field"),
        "Flooding" => has("max_rate") || has("max_concurrent"),
        "Expiration" => has("ttl") && has("field"),
        _ => false,
    }
}

fn security_test_scaffold(
    stereotype: &str,
    test_id: &str,
    sec_id: &str,
    host: &str,
    params: &BTreeMap<String, String>,
) -> String {
    match stereotype {
        "ByPassing" => bypass_test_scaffold(test_id, sec_id, host, params),
        "Encrypt" => encrypt_test_scaffold(test_id, sec_id, host, params),
        "BufferOverflow" => overflow_test_scaffold(test_id, sec_id, host, params),
        "SqlInjection" => sqli_test_scaffold(test_id, sec_id, host, params),
        "Flooding" => flooding_test_scaffold(test_id, sec_id, host, params),
        "Expiration" => expiration_test_scaffold(test_id, sec_id, host, params),
        _ => format!(
            "@model:{sec_id}\n@test:{test_id}\n@security:{stereotype}\n@host:{host}\nFeature: {stereotype} on {host}\n\n  Scenario: TODO security scenario\n    Given the security marker for {sec_id}\n    When the protected behavior is exercised\n    Then the {stereotype} property holds per the security profile\n"
        ),
    }
}

fn sec_test_header(stereotype: &str, test_id: &str, sec_id: &str, host: &str, summary: &str) -> String {
    format!(
        "@model:{sec_id}\n@test:{test_id}\n@security:{stereotype}\n@host:{host}\nFeature: {summary}\n\n"
    )
}

fn encrypt_test_scaffold(
    test_id: &str,
    sec_id: &str,
    host: &str,
    params: &BTreeMap<String, String>,
) -> String {
    let algorithm = params
        .get("algorithm")
        .map(String::as_str)
        .unwrap_or("(unspecified-algorithm)");
    let scope = params
        .get("scope")
        .map(String::as_str)
        .unwrap_or("(unspecified-scope)");
    let field = params.get("field").map(String::as_str).unwrap_or(host);
    let mut output = sec_test_header(
        "Encrypt",
        test_id,
        sec_id,
        host,
        &format!("Encryption of {field} ({scope}) under {algorithm}"),
    );
    if scope == "at_rest" || scope == "both" {
        output.push_str(&format!(
            "  Scenario: {field} is ciphertext at rest\n    Given a stored record containing {field}\n    When the underlying storage is read directly\n    Then the bytes are encrypted under \"{algorithm}\"\n    And the plaintext value is not present\n\n"
        ));
    }
    if scope == "in_transit" || scope == "both" {
        output.push_str(&format!(
            "  Scenario: {field} is ciphertext in transit\n    Given a request carrying {field}\n    When the payload is observed on the wire\n    Then the channel is protected by \"{algorithm}\"\n    And the plaintext value is not visible\n\n"
        ));
    }
    output
}

fn overflow_test_scaffold(
    test_id: &str,
    sec_id: &str,
    host: &str,
    params: &BTreeMap<String, String>,
) -> String {
    let field = params.get("field").map(String::as_str).unwrap_or(host);
    let max_length = params
        .get("max_length")
        .map(String::as_str)
        .unwrap_or("(unspecified)");
    let mut output = sec_test_header(
        "BufferOverflow",
        test_id,
        sec_id,
        host,
        &format!("Length bound on {host}.{field} ({max_length} chars)"),
    );
    output.push_str(&format!(
        "  Scenario: input within bounds is accepted\n    Given a value for {field} of length up to {max_length}\n    When the value is submitted\n    Then the request is accepted\n\n"
    ));
    output.push_str(&format!(
        "  Scenario: input over the bound is rejected\n    Given a value for {field} of length greater than {max_length}\n    When the value is submitted\n    Then the request is rejected with a length-bound error\n    And no truncated value is persisted\n"
    ));
    output
}

fn sqli_test_scaffold(
    test_id: &str,
    sec_id: &str,
    host: &str,
    params: &BTreeMap<String, String>,
) -> String {
    let field = params.get("field").map(String::as_str).unwrap_or(host);
    let sink = params
        .get("sink")
        .map(String::as_str)
        .unwrap_or("(unspecified-sink)");
    let sanitizer = params
        .get("sanitizer")
        .map(String::as_str)
        .unwrap_or("(unspecified-sanitizer)");
    let mut output = sec_test_header(
        "SqlInjection",
        test_id,
        sec_id,
        host,
        &format!("SQL-injection guard on {host}.{field} -> {sink}"),
    );
    output.push_str(&format!(
        "  Scenario: SQL-meta payload is treated as data\n    Given a value for {field} containing a SQL-injection payload (e.g. \"' OR 1=1 --\")\n    When the request reaches {sink}\n    Then the {sanitizer} boundary binds the value as a parameter\n    And the payload is not executed as SQL\n\n"
    ));
    output.push_str(&format!(
        "  Scenario: benign value still works\n    Given a benign value for {field}\n    When the request reaches {sink}\n    Then the operation succeeds without escaping the {sanitizer} boundary\n"
    ));
    output
}

fn flooding_test_scaffold(
    test_id: &str,
    sec_id: &str,
    host: &str,
    params: &BTreeMap<String, String>,
) -> String {
    let link = params
        .get("link")
        .map(String::as_str)
        .unwrap_or("(unspecified-endpoint)");
    let max_rate = params.get("max_rate").map(String::as_str);
    let max_concurrent = params.get("max_concurrent").map(String::as_str);
    let window = params.get("window").map(String::as_str).unwrap_or("1s");
    let action = params
        .get("action")
        .map(String::as_str)
        .unwrap_or("throttle");
    let summary = match (max_rate, max_concurrent) {
        (Some(r), _) => format!("Rate limit {r} req/{window} on {link}"),
        (_, Some(c)) => format!("Concurrency limit {c} on {link}"),
        _ => format!("Flood limit on {link}"),
    };
    let mut output = sec_test_header("Flooding", test_id, sec_id, host, &summary);
    if let Some(rate) = max_rate {
        output.push_str(&format!(
            "  Scenario: traffic over the rate limit is {action}d\n    Given an aggressive client targeting {link}\n    When more than {rate} requests are sent within {window}\n    Then excess requests are {action}d\n    And legitimate clients keep service within the limit\n\n"
        ));
    }
    if let Some(concurrent) = max_concurrent {
        output.push_str(&format!(
            "  Scenario: concurrency over the bound is {action}d\n    Given {concurrent} concurrent in-flight requests against {link}\n    When an additional request arrives\n    Then the additional request is {action}d until a slot frees\n"
        ));
    }
    output
}

fn expiration_test_scaffold(
    test_id: &str,
    sec_id: &str,
    host: &str,
    params: &BTreeMap<String, String>,
) -> String {
    let field = params.get("field").map(String::as_str).unwrap_or(host);
    let ttl = params
        .get("ttl")
        .map(String::as_str)
        .unwrap_or("(unspecified-ttl)");
    let mut output = sec_test_header(
        "Expiration",
        test_id,
        sec_id,
        host,
        &format!("TTL on {host}.{field} ({ttl})"),
    );
    output.push_str(&format!(
        "  Scenario: value is accepted before expiry\n    Given a {field} issued at time T\n    When the value is presented before T + {ttl}\n    Then the value is accepted\n\n"
    ));
    output.push_str(&format!(
        "  Scenario: value is rejected after expiry\n    Given a {field} issued at time T\n    When the value is presented after T + {ttl}\n    Then the value is rejected as expired\n    And no renewal is granted without re-authentication\n"
    ));
    output
}

fn bypass_test_scaffold(
    test_id: &str,
    sec_id: &str,
    host: &str,
    params: &BTreeMap<String, String>,
) -> String {
    let link = params
        .get("link")
        .map(String::as_str)
        .unwrap_or("(unspecified-link)");
    let allowed: Vec<&str> = params
        .get("allowed")
        .map(|value| value.split('|').filter(|role| !role.is_empty()).collect())
        .unwrap_or_default();
    let denied: Vec<&str> = params
        .get("denied")
        .map(|value| value.split('|').filter(|role| !role.is_empty()).collect())
        .unwrap_or_default();

    let mut output = String::new();
    output.push_str(&format!("@model:{sec_id}\n"));
    output.push_str(&format!("@test:{test_id}\n"));
    output.push_str("@security:ByPassing\n");
    output.push_str(&format!("@host:{host}\n"));
    output.push_str(&format!(
        "Feature: Access control on {host} via {link}\n\n"
    ));

    if allowed.is_empty() && denied.is_empty() {
        output.push_str(
            "  Scenario: Access control is enforced\n    Given an authenticated user\n    When the user requests the protected resource\n    Then access is decided per the security profile\n",
        );
        return output;
    }

    for role in &allowed {
        output.push_str(&format!("  Scenario: {role} can reach {link}\n"));
        output.push_str(&format!("    Given a user with role \"{role}\"\n"));
        output.push_str(&format!("    When the user requests \"{link}\"\n"));
        output.push_str("    Then access is granted\n\n");
    }
    for role in &denied {
        output.push_str(&format!(
            "  Scenario: {role} is denied access to {link}\n"
        ));
        output.push_str(&format!("    Given a user with role \"{role}\"\n"));
        output.push_str(&format!("    When the user requests \"{link}\"\n"));
        output.push_str("    Then access is rejected\n\n");
    }

    output
}

fn playwright_test_scaffold(test_id: &str, contract: &MockupContract) -> String {
    let route = contract.route.as_deref().unwrap_or("/");
    let viewports = if contract.viewports.is_empty() {
        vec![UiViewport {
            name: "desktop".to_string(),
            width: 1280,
            height: 720,
        }]
    } else {
        contract.viewports.clone()
    };

    let mut output = String::new();
    output.push_str("import { test, expect } from '@playwright/test';\n\n");
    output.push_str(&format!("// @model:{}\n", contract.model_id));
    output.push_str(&format!("// @test:{test_id}\n"));
    output.push_str(&format!(
        "test.describe({}, () => {{\n",
        ts_string(&contract.model_id)
    ));

    for viewport in viewports {
        output.push_str(&format!(
            "  test({}, async ({{ page }}) => {{\n",
            ts_string(&format!("{test_id} {}", viewport.name))
        ));
        output.push_str(&format!(
            "    await page.setViewportSize({{ width: {}, height: {} }});\n",
            viewport.width, viewport.height
        ));
        output.push_str(&format!("    await page.goto({});\n", ts_string(route)));

        for element in &contract.elements {
            let locator = playwright_locator(element);
            output.push_str(&format!("    // {}\n", element.id));
            if element.required {
                output.push_str(&format!("    await expect({locator}).toBeVisible();\n"));
            } else {
                output.push_str(&format!("    await {locator}.count();\n"));
            }
        }

        output.push_str("  });\n");
    }

    output.push_str("});\n");
    output
}

fn playwright_locator(element: &UiElementContract) -> String {
    match (&element.role, &element.name) {
        (Some(role), Some(name)) => format!(
            "page.getByRole({}, {{ name: {} }})",
            ts_string(role),
            ts_string(name)
        ),
        (Some(role), None) => format!("page.getByRole({})", ts_string(role)),
        (None, Some(name)) => format!("page.getByText({})", ts_string(name)),
        (None, None) => "page.locator('body')".to_string(),
    }
}

fn ts_string(value: &str) -> String {
    format!(
        "'{}'",
        value
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
    )
}

fn annotate_review_diff_puml(
    objective_content: &str,
    missing_ids: &BTreeSet<String>,
    extras: &[(String, ModelKind)],
) -> Result<String> {
    const ELEMENT_KEYWORDS: &[&str] = &[
        "usecase ", "actor ", "class ", "interface ", "enum ", "abstract ",
        "participant ", "boundary ", "control ", "entity ", "database ", "collections ",
        "queue ", "state ", "component ", "package ", "node ", "rectangle ",
        "circle ", "cloud ", "frame ", "folder ", "card ", "agent ",
    ];
    let skinparam_block = "\
skinparam usecase {\n  BackgroundColor<<missing>> #90EE90\n  BackgroundColor<<extra>> #FFB6C1\n}\n\
skinparam class {\n  BackgroundColor<<missing>> #90EE90\n  BackgroundColor<<extra>> #FFB6C1\n}\n\
skinparam component {\n  BackgroundColor<<missing>> #90EE90\n  BackgroundColor<<extra>> #FFB6C1\n}\n\
skinparam state {\n  BackgroundColor<<missing>> #90EE90\n  BackgroundColor<<extra>> #FFB6C1\n}\n\
skinparam rectangle {\n  BackgroundColor<<missing>> #90EE90\n  BackgroundColor<<extra>> #FFB6C1\n}\n\
skinparam actor {\n  BackgroundColor<<missing>> #90EE90\n  BackgroundColor<<extra>> #FFB6C1\n}\n\
skinparam participant {\n  BackgroundColor<<missing>> #90EE90\n  BackgroundColor<<extra>> #FFB6C1\n}\n\
skinparam node {\n  BackgroundColor<<missing>> #90EE90\n  BackgroundColor<<extra>> #FFB6C1\n}\n";

    let id_pattern = Regex::new(r"@id\(([A-Za-z0-9_.:-]+)\)")?;
    let mut output = String::new();
    let mut pending_missing = false;
    let mut skinparams_inserted = false;
    let mut enduml_emitted = false;

    let lines: Vec<&str> = objective_content.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        if !skinparams_inserted && (trimmed.starts_with("@startuml") || trimmed.starts_with("@startsalt")) {
            output.push_str(line);
            output.push('\n');
            output.push_str(skinparam_block);
            skinparams_inserted = true;
            continue;
        }

        if trimmed.starts_with("@enduml") || trimmed.starts_with("@endsalt") {
            for (id, kind) in extras {
                output.push_str(&extra_pseudo_element(id, *kind));
                output.push('\n');
            }
            output.push_str(line);
            output.push('\n');
            enduml_emitted = true;
            for remaining in &lines[idx + 1..] {
                output.push_str(remaining);
                output.push('\n');
            }
            break;
        }

        if trimmed.starts_with('\'') || trimmed.starts_with("//") {
            if let Some(cap) = id_pattern.captures(trimmed) {
                if missing_ids.contains(&cap[1]) {
                    pending_missing = true;
                }
            }
            output.push_str(line);
            output.push('\n');
            continue;
        }

        if pending_missing && !trimmed.is_empty() {
            if ELEMENT_KEYWORDS.iter().any(|kw| trimmed.starts_with(kw)) {
                let injected = if let Some(brace_pos) = line.find('{') {
                    let head = line[..brace_pos].trim_end();
                    format!("{head} <<missing>> {}", &line[brace_pos..])
                } else {
                    format!("{} <<missing>>", line.trim_end())
                };
                output.push_str(&injected);
                output.push('\n');
                pending_missing = false;
                continue;
            }
        }

        output.push_str(line);
        output.push('\n');
    }

    if !enduml_emitted {
        for (id, kind) in extras {
            output.push_str(&extra_pseudo_element(id, *kind));
            output.push('\n');
        }
    }

    Ok(output)
}

fn parity_key(marker: &MarkerSummary) -> String {
    let params_pairs: Vec<String> = marker
        .params
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    format!(
        "{}|{}|{}",
        marker.host,
        marker.stereotype,
        params_pairs.join(",")
    )
}

fn annotate_security_diff_puml(objective_content: &str, missing: &[&MarkerSummary]) -> String {
    let note_alias = "MissingSecMarkers";
    let mut note_body = String::from("**MISSING SECURITY MARKERS**\\n");
    for marker in missing {
        let params_str = if marker.params.is_empty() {
            String::new()
        } else {
            let pairs: Vec<String> = marker
                .params
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect();
            format!(" ({})", pairs.join(", "))
        };
        let id_suffix = marker
            .id
            .as_ref()
            .map(|id| format!(" [id={id}]"))
            .unwrap_or_default();
        note_body.push_str(&format!(
            "• {host}: <<{stereotype}>>{params}{id}\\n",
            host = marker.host,
            stereotype = marker.stereotype,
            params = params_str,
            id = id_suffix,
        ));
    }

    let note_block = format!("note as {note_alias} #FFB6C1\n{note_body}\nend note\n");

    let mut output = String::new();
    let mut inserted = false;
    for line in objective_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("@enduml") || trimmed.starts_with("@endsalt") {
            if !inserted {
                output.push_str(&note_block);
                inserted = true;
            }
        }
        output.push_str(line);
        output.push('\n');
    }
    if !inserted {
        output.push_str(&note_block);
    }
    output
}

fn extra_pseudo_element(id: &str, kind: ModelKind) -> String {
    match kind {
        ModelKind::UseCase => format!("usecase \"{id}\" as {id}_extra <<extra>>"),
        ModelKind::Domain => format!("class \"{id}\" as {id}_extra <<extra>>"),
        ModelKind::Sequence => format!("participant \"{id}\" as {id}_extra <<extra>>"),
        ModelKind::State => format!("state \"{id}\" as {id}_extra <<extra>>"),
        ModelKind::Mockup => format!("note \"extra (mockup): {id}\" as {id}_extra #FFB6C1"),
        ModelKind::Other => format!("rectangle \"{id}\" as {id}_extra <<extra>>"),
        ModelKind::Constraint => format!("note \"extra constraint: {id}\" as {id}_extra #FFB6C1"),
    }
}

fn is_stale(source: &Path, generated: &Path) -> Result<bool> {
    let source_modified = fs::metadata(source)
        .with_context(|| format!("failed to stat {}", source.display()))?
        .modified()
        .with_context(|| format!("failed to read mtime for {}", source.display()))?;
    let generated_modified = fs::metadata(generated)
        .with_context(|| format!("failed to stat {}", generated.display()))?
        .modified()
        .with_context(|| format!("failed to read mtime for {}", generated.display()))?;
    Ok(generated_modified < source_modified)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn init_creates_expected_structure() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());

        let report = project.init().unwrap();

        assert!(report.created.contains(&".mdd/config.yml".to_string()));
        assert!(report.overwritten.is_empty());
        assert!(report.skipped.is_empty());
        assert!(dir.path().join(".mdd/models/current/use-cases").is_dir());
        assert!(dir.path().join(".mdd/models/current/mockups").is_dir());
        assert!(dir.path().join(".mdd/models/current/states").is_dir());
        assert!(dir.path().join(".mdd/models/objective/use-cases").is_dir());
        assert!(dir.path().join(".mdd/models/objective/states").is_dir());
        assert!(dir.path().join(".mdd/trace.yml").is_file());
        assert!(dir.path().join(".mdd/approvals.yml").is_file());
        assert!(dir.path().join(".mdd/tests/ui").is_dir());
    }

    #[test]
    fn init_appends_block_to_existing_files() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        fs::write(dir.path().join("AGENTS.md"), "custom instructions\n").unwrap();

        let report = project.init().unwrap();

        assert!(report.overwritten.contains(&"AGENTS.md".to_string()));
        assert!(!report.skipped.contains(&"AGENTS.md".to_string()));
        let content = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
        assert!(content.starts_with("custom instructions\n"));
        assert!(content.contains(templates::MDD_BLOCK_BEGIN));
        assert!(content.contains(templates::MDD_BLOCK_END));
        assert!(content.contains("# Agent MDD Entry Point"));
        assert!(content.contains("\"kind\":\"agents-entrypoint\""));
    }

    #[test]
    fn init_reinit_replaces_block_in_place() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        fs::write(dir.path().join("CLAUDE.md"), "user header\n").unwrap();

        project.init().unwrap();
        let after_first = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();

        let report = project.init().unwrap();
        let after_second = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();

        assert_eq!(after_first, after_second);
        assert!(!report.overwritten.contains(&"CLAUDE.md".to_string()));
        assert_eq!(after_second.matches(templates::MDD_BLOCK_BEGIN).count(), 1);
        assert!(after_second.starts_with("user header\n"));
    }

    #[test]
    fn clean_removes_init_artifacts() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();

        let report = project.clean(false).unwrap();

        assert!(report.removed.contains(&".mdd".to_string()));
        assert!(report.skipped.is_empty());
        assert!(!dir.path().join(".mdd").exists());
        assert!(!dir.path().join(".claude").exists());
        assert!(!dir.path().join(".codex").exists());
        assert!(!dir.path().join("CLAUDE.md").exists());
        assert!(!dir.path().join("AGENTS.md").exists());
        for skill in templates::WORKFLOW_SKILLS {
            assert!(
                !dir.path()
                    .join(format!(".claude/skills/{}/SKILL.md", skill.name))
                    .exists()
            );
            assert!(
                !dir.path()
                    .join(format!(".codex/skills/{}/SKILL.md", skill.name))
                    .exists()
            );
        }
    }

    #[test]
    fn clean_skips_modified_generated_files_without_force() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();
        fs::write(
            dir.path().join(".claude/skills/mdd-map/SKILL.md"),
            "custom skill\n",
        )
        .unwrap();

        let report = project.clean(false).unwrap();

        assert!(dir.path().join(".claude/skills/mdd-map/SKILL.md").is_file());
        assert!(
            report
                .skipped
                .iter()
                .any(|skip| skip.path == ".claude/skills/mdd-map/SKILL.md")
        );
        assert!(!dir.path().join("CLAUDE.md").exists());
        assert!(!dir.path().join("AGENTS.md").exists());
    }

    #[test]
    fn clean_force_removes_modified_generated_files() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();
        fs::write(
            dir.path().join(".claude/skills/mdd-map/SKILL.md"),
            "custom skill\n",
        )
        .unwrap();

        let report = project.clean(true).unwrap();

        assert!(report.skipped.is_empty());
        assert!(!dir.path().join(".claude/skills/mdd-map/SKILL.md").exists());
        assert!(!dir.path().join(".claude").exists());
        assert!(!dir.path().join(".codex").exists());
        assert!(!dir.path().join("CLAUDE.md").exists());
        assert!(!dir.path().join("AGENTS.md").exists());
    }

    #[test]
    fn clean_preserves_user_content_around_block() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        fs::write(dir.path().join("CLAUDE.md"), "keep me\n").unwrap();
        project.init().unwrap();

        let report = project.clean(false).unwrap();

        assert!(dir.path().join("CLAUDE.md").is_file());
        let content = fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap();
        assert_eq!(content, "keep me\n");
        assert!(!content.contains(templates::MDD_BLOCK_BEGIN));
        assert!(
            report
                .removed
                .contains(&"CLAUDE.md (mdd block)".to_string())
        );
        assert!(report.skipped.iter().all(|s| s.path != "CLAUDE.md"));
    }

    #[test]
    fn clean_skips_hand_edited_block_without_force() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();
        let path = dir.path().join("CLAUDE.md");
        let tampered =
            fs::read_to_string(&path)
                .unwrap()
                .replace("# Claude Code MDD Entry Point", "# Hacked Heading");
        fs::write(&path, &tampered).unwrap();

        let report = project.clean(false).unwrap();

        assert!(path.is_file());
        assert!(
            report
                .skipped
                .iter()
                .any(|s| s.path == "CLAUDE.md"
                    && s.reason.contains("rerun with --force"))
        );

        let forced = project.clean(true).unwrap();
        assert!(!path.exists());
        assert!(forced.removed.contains(&"CLAUDE.md".to_string()));
    }

    #[test]
    fn clean_leaves_files_without_markers_untouched() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        let original = "my own agents file\n";
        fs::write(dir.path().join("AGENTS.md"), original).unwrap();

        let report = project.clean(false).unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join("AGENTS.md")).unwrap(),
            original
        );
        assert!(report.removed.iter().all(|r| !r.starts_with("AGENTS.md")));
        assert!(report.skipped.iter().all(|s| s.path != "AGENTS.md"));
    }

    #[test]
    fn validate_fails_without_models() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("no model or constraint files"))
        );
    }

    #[test]
    fn approval_becomes_stale_after_model_change() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/current/sequences/login.puml"),
            "@startuml\n' @id(SEQ-LOGIN)\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/current/domain/user.puml"),
            "@startuml\n' @id(DOM-USER)\n@enduml\n",
        )
        .unwrap();

        let mut trace = project.read_trace().unwrap();
        trace.links.push(TraceLink {
            from: "USE-LOGIN".to_string(),
            to: "SEQ-LOGIN".to_string(),
            relation: "realizes".to_string(),
        });
        project.write_trace(&trace).unwrap();

        project.approve().unwrap();
        fs::write(
            dir.path().join(".mdd/models/current/domain/user.puml"),
            "@startuml\n' @id(DOM-USER)\nclass User\n@enduml\n",
        )
        .unwrap();

        let status = project.approval_status().unwrap();
        assert!(status.approved);
        assert!(!status.current);
    }

    #[test]
    fn generate_acceptance_tests_without_approval() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);

        let report = project.generate_acceptance_tests().unwrap();

        assert_eq!(report.generated.len(), 1);
        assert!(
            dir.path()
                .join(".mdd/tests/acceptance/use-login.feature")
                .is_file()
        );

        let trace = project.read_trace().unwrap();
        assert!(trace.generated_tests.iter().any(|test| {
            test.id == "AT-USE-LOGIN"
                && test.path == ".mdd/tests/acceptance/use-login.feature"
                && test.model_id == "USE-LOGIN"
        }));
    }

    #[test]
    fn generated_test_deserializes_legacy_yaml_without_category() {
        let trace: Trace = serde_yaml::from_str(
            "version: 1\nlinks: []\ngenerated_tests:\n  - id: AT-USE-LOGIN\n    path: .mdd/tests/acceptance/use-login.feature\n    model_id: USE-LOGIN\ngenerated_ui_tests: []\nsource_links: []\n",
        )
        .unwrap();

        assert_eq!(trace.generated_tests.len(), 1);
        assert!(trace.generated_tests[0].category.is_none());

        let yaml = serde_yaml::to_string(&trace).unwrap();
        assert!(!yaml.contains("category:"));
    }

    #[test]
    fn generated_test_round_trips_security_category() {
        let trace: Trace = serde_yaml::from_str(
            "version: 1\nlinks: []\ngenerated_tests:\n  - id: SECT-SEC-LOGIN-GUARD\n    path: .mdd/tests/acceptance/sec-login-guard.feature\n    model_id: SEC-LOGIN-GUARD\n    category: security\ngenerated_ui_tests: []\nsource_links: []\n",
        )
        .unwrap();

        assert_eq!(
            trace.generated_tests[0].category.as_deref(),
            Some("security")
        );

        let yaml = serde_yaml::to_string(&trace).unwrap();
        assert!(yaml.contains("category: security"));
    }

    #[test]
    fn generate_security_tests_writes_gherkin_and_trace_entry() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, link=/login, allowed=Admin|Editor, denied=Anonymous, id=SEC-LOGIN-GUARD)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.generate_security_tests().unwrap();

        assert_eq!(
            report.generated,
            vec![".mdd/tests/acceptance/sec-login-guard.feature"]
        );
        let feature_path = dir
            .path()
            .join(".mdd/tests/acceptance/sec-login-guard.feature");
        assert!(feature_path.is_file());
        let content = fs::read_to_string(&feature_path).unwrap();
        assert!(content.contains("@security:ByPassing"));
        assert!(content.contains("@host:USE-LOGIN"));
        assert!(content.contains("Scenario: Admin can reach /login"));
        assert!(content.contains("Scenario: Editor can reach /login"));
        assert!(content.contains("Scenario: Anonymous is denied access to /login"));

        let trace = project.read_trace().unwrap();
        let entry = trace
            .generated_tests
            .iter()
            .find(|t| t.id == "SECT-SEC-LOGIN-GUARD")
            .expect("trace entry exists");
        assert_eq!(entry.model_id, "SEC-LOGIN-GUARD");
        assert_eq!(entry.path, ".mdd/tests/acceptance/sec-login-guard.feature");
        assert_eq!(entry.category.as_deref(), Some("security"));
    }

    #[test]
    fn generate_security_tests_skips_markers_without_id_or_link() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        // marker without id=
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, link=/login, allowed=Admin)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
        // marker without link=
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/admin.puml"),
            "@startuml\n' @id(USE-ADMIN)\n' @sec(stereotype=ByPassing, host=USE-ADMIN, allowed=Admin, id=SEC-ADMIN)\nusecase \"Admin\" as Admin\n@enduml\n",
        )
        .unwrap();
        let mut trace = project.read_trace().unwrap();
        trace.links.push(TraceLink {
            from: "USE-ADMIN".to_string(),
            to: "SEQ-LOGIN".to_string(),
            relation: "realizes".to_string(),
        });
        project.write_trace(&trace).unwrap();

        let err = project.generate_security_tests().unwrap_err();
        assert!(
            err.to_string().contains("no implementation-ready security markers"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn validate_warns_when_impl_ready_security_marker_lacks_scaffold() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, link=/login, allowed=Admin, id=SEC-LOGIN-GUARD)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(report.ok, "errors: {:?}", report.errors);
        assert!(
            report.warnings.iter().any(|warning| {
                warning.contains("implementation-ready security marker SEC-LOGIN-GUARD")
            }),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn validate_no_security_warning_when_scaffold_present() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, link=/login, allowed=Admin, id=SEC-LOGIN-GUARD)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
        project.generate_security_tests().unwrap();

        let report = project.validate().unwrap();

        assert!(
            report
                .warnings
                .iter()
                .all(|warning| !warning.contains("SEC-LOGIN-GUARD")),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn validate_no_security_warning_for_descriptive_marker_without_link() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, allowed=Admin, id=SEC-DESCRIPTIVE)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(
            report
                .warnings
                .iter()
                .all(|warning| !warning.contains("SEC-DESCRIPTIVE")),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn trace_serializes_generated_ui_tests_with_default_framework() {
        let trace: Trace = serde_yaml::from_str(
            "version: 1\nlinks: []\ngenerated_tests: []\ngenerated_ui_tests:\n  - id: UIT-MCK-LOGIN-FORM\n    path: .mdd/tests/ui/mck-login-form.spec.ts\n    model_id: MCK-LOGIN-FORM\nsource_links: []\n",
        )
        .unwrap();

        assert_eq!(trace.generated_ui_tests[0].framework, "playwright");

        let yaml = serde_yaml::to_string(&trace).unwrap();
        assert!(yaml.contains("generated_ui_tests:"));
        assert!(yaml.contains("framework: playwright"));
    }

    #[test]
    fn validate_accepts_mockup_contract_with_generated_ui_test() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_login_mockup(&project, "MCK-LOGIN-FORM", "UIC-LOGIN-SUBMIT", "/login");
        write_ui_test_trace_link(&project, "MCK-LOGIN-FORM");

        let report = project.validate().unwrap();

        assert!(report.ok, "errors: {:?}", report.errors);
        assert!(
            report
                .registry
                .ids
                .iter()
                .any(|element| element.id == "MCK-LOGIN-FORM" && element.kind == ModelKind::Mockup)
        );
    }

    #[test]
    fn validate_reports_unresolved_mockup_refs() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/mockups/login.puml"),
            "@startsalt\n' @id(MCK-LOGIN-FORM)\n' @ref(USE-MISSING)\n{\n  [Log in]\n}\n@endsalt\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("unresolved @ref(USE-MISSING)"))
        );
    }

    #[test]
    fn validate_reports_duplicate_ui_contract_ids() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_login_mockup(&project, "MCK-LOGIN-FORM", "UIC-DUPLICATE", "/login");
        fs::write(
            dir.path().join(".mdd/models/current/mockups/signup.puml"),
            "@startsalt\n' @id(MCK-SIGNUP-FORM)\n' @ref(USE-LOGIN)\n' @ui-route(/signup)\n' @ui-element(UIC-DUPLICATE, role=button, name=\"Sign up\", required=true)\n{\n  [Sign up]\n}\n@endsalt\n",
        )
        .unwrap();
        write_ui_test_trace_link(&project, "MCK-LOGIN-FORM");
        write_ui_test_trace_link(&project, "MCK-SIGNUP-FORM");

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("duplicate UI contract ID UIC-DUPLICATE"))
        );
    }

    #[test]
    fn validate_reports_missing_ui_tests_for_implementation_ready_mockups() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_login_mockup(&project, "MCK-LOGIN-FORM", "UIC-LOGIN-SUBMIT", "/login");

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(report.errors.iter().any(|error| {
            error.contains(
                "implementation-ready mockup MCK-LOGIN-FORM has no generated Playwright UI test",
            )
        }));
    }

    #[test]
    fn generate_ui_tests_from_salt_mockup_contract() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_login_mockup(&project, "MCK-LOGIN-FORM", "UIC-LOGIN-SUBMIT", "/login");

        let report = project.generate_ui_tests().unwrap();

        assert_eq!(
            report.generated,
            vec![".mdd/tests/ui/mck-login-form.spec.ts"]
        );
        let test_path = dir.path().join(".mdd/tests/ui/mck-login-form.spec.ts");
        let content = fs::read_to_string(test_path).unwrap();
        assert!(content.contains("page.goto('/login')"));
        assert!(content.contains("page.getByRole('button', { name: 'Log in' })"));

        let trace = project.read_trace().unwrap();
        assert!(trace.generated_ui_tests.iter().any(|test| {
            test.id == "UIT-MCK-LOGIN-FORM"
                && test.path == ".mdd/tests/ui/mck-login-form.spec.ts"
                && test.model_id == "MCK-LOGIN-FORM"
                && test.framework == "playwright"
        }));
    }

    #[test]
    fn code_gate_reports_readiness_gaps_as_warnings() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);

        let report = project.code_gate().unwrap();

        assert!(report.ok);
        assert!(report.errors.is_empty());
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("models have not been approved"))
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("use case has no generated acceptance test"))
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains("rendered SVG is missing"))
        );
    }

    #[test]
    fn validate_allows_same_id_in_current_and_objective() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(
            report
                .errors
                .iter()
                .all(|error| !error.contains("duplicate model ID USE-LOGIN")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_reports_duplicate_within_same_side() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login-copy.puml"),
            "@startuml\n' @id(USE-LOGIN)\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("duplicate model ID USE-LOGIN")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn review_reports_match_when_objective_covered_by_current() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.review().unwrap();

        assert!(report.matched, "missing: {:?}", report.missing_ids);
        assert!(report.missing_ids.is_empty());
        assert!(report.diff_puml_paths.is_empty());
    }

    #[test]
    fn review_reports_mismatch_when_objective_has_id_missing_in_current() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/checkout.puml"),
            "@startuml\n' @id(USE-CHECKOUT)\nusecase \"Check out\" as Checkout\n@enduml\n",
        )
        .unwrap();

        let report = project.review().unwrap();

        assert!(!report.matched);
        assert_eq!(report.missing_ids, vec!["USE-CHECKOUT".to_string()]);
        assert_eq!(report.diff_puml_paths.len(), 1);
        let diff_path = dir.path().join(&report.diff_puml_paths[0]);
        assert!(diff_path.is_file());
        let content = fs::read_to_string(&diff_path).unwrap();
        assert!(content.contains("<<missing>>"), "diff content: {content}");
        assert!(
            content.contains("skinparam usecase"),
            "missing skinparam block"
        );
    }

    #[test]
    fn review_reports_extra_when_current_has_id_missing_in_objective() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        // Objective mirrors current's minimal set, plus extra current-only ID.
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/sequences/login.puml"),
            "@startuml\n' @id(SEQ-LOGIN)\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/domain/user.puml"),
            "@startuml\n' @id(DOM-USER)\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/extra.puml"),
            "@startuml\n' @id(USE-EXTRA)\nusecase \"Extra\" as Extra\n@enduml\n",
        )
        .unwrap();

        let report = project.review().unwrap();

        assert!(report.matched, "missing: {:?}", report.missing_ids);
        assert!(
            report.extra_ids.contains(&"USE-EXTRA".to_string()),
            "extras: {:?}",
            report.extra_ids
        );
    }

    /// Author the same use case on both sides, but only the objective side
    /// carries a `<<ByPassing>>` security marker. IDs match; security does not.
    fn write_security_parity_mismatch_fixture(project: &Project) {
        let root = project.root();
        write_minimal_valid_models(project);
        fs::write(
            root.join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, link=/login, allowed=Admin, denied=Anonymous, id=SEC-LOGIN-GUARD)\nusecase \"Log in\" as Login <<ByPassing>>\n@enduml\n",
        )
        .unwrap();
        fs::write(
            root.join(".mdd/models/objective/sequences/login.puml"),
            "@startuml\n' @id(SEQ-LOGIN)\n@enduml\n",
        )
        .unwrap();
        fs::write(
            root.join(".mdd/models/objective/domain/user.puml"),
            "@startuml\n' @id(DOM-USER)\n@enduml\n",
        )
        .unwrap();
        // Current side has the same USE-LOGIN id but no @sec marker (the code
        // does not enforce the guard yet).
        fs::write(
            root.join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
    }

    #[test]
    fn review_blocks_on_security_mismatch_in_error_mode() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_security_parity_mismatch_fixture(&project);

        let report = project.review().unwrap();

        assert!(report.ids_matched, "ids missing: {:?}", report.missing_ids);
        assert!(!report.security.matched);
        assert_eq!(report.security.mode, ParityMode::Error);
        assert!(
            !report.matched,
            "error-mode security mismatch must block cycle closure"
        );
        assert!(
            !report.security.diff_puml_paths.is_empty(),
            "expected a .security.diff.puml to be written"
        );
    }

    #[test]
    fn review_warns_but_passes_on_security_mismatch_in_warn_mode() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_security_parity_mismatch_fixture(&project);
        let mut config = project.read_config().unwrap();
        config.security.parity_check = ParityMode::Warn;
        project.write_yaml(".mdd/config.yml", &config).unwrap();

        let report = project.review().unwrap();

        assert!(report.ids_matched);
        assert!(!report.security.matched);
        assert_eq!(report.security.mode, ParityMode::Warn);
        assert!(
            report.matched,
            "warn-mode security mismatch must not block cycle closure"
        );
    }

    #[test]
    fn review_passes_when_ids_and_security_both_match() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        let guarded = "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, link=/login, allowed=Admin, denied=Anonymous, id=SEC-LOGIN-GUARD)\nusecase \"Log in\" as Login <<ByPassing>>\n@enduml\n";
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            guarded,
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            guarded,
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/sequences/login.puml"),
            "@startuml\n' @id(SEQ-LOGIN)\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/domain/user.puml"),
            "@startuml\n' @id(DOM-USER)\n@enduml\n",
        )
        .unwrap();

        let report = project.review().unwrap();

        assert!(report.ids_matched);
        assert!(report.security.matched, "missing: {:?}", report.security.missing_markers);
        assert!(report.matched);
    }

    #[test]
    fn validate_accepts_well_formed_sec_bypass_on_use_case() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, link=/login, allowed=Admin, denied=Anonymous|Customer, id=SEC-LOGIN-GUARD)\nusecase \"Log in\" as Login <<ByPassing>>\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(report.ok, "errors: {:?}", report.errors);
        assert!(
            report
                .errors
                .iter()
                .all(|error| !error.contains("@sec")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_rejects_unknown_sec_stereotype() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=Bypassing, host=USE-LOGIN, allowed=Admin)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("uses unknown stereotype")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_rejects_sec_missing_host() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, allowed=Admin)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("is missing required `host=`")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_rejects_sec_unresolved_host() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-MISSING, allowed=Admin)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("host does not resolve to an @id")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_rejects_bypassing_on_actor_without_role() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\nusecase \"Log in\" as Login\n' @id(ACTOR-ADMIN)\n' @sec(stereotype=ByPassing, host=ACTOR-ADMIN, link=/admin)\nactor Admin <<ByPassing>>\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("on actor host requires `role=`")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_rejects_bypassing_on_use_case_without_allowed_or_denied() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, link=/login)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("on use-case host requires `allowed=` or `denied=`")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_accepts_bypassing_with_pipe_separated_denied_list() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, denied=Anonymous|Customer|Vendor)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(report.ok, "errors: {:?}", report.errors);
    }

    #[test]
    fn validate_reports_duplicate_sec_ids_within_same_side() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, allowed=Admin, id=SEC-GUARD)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/admin.puml"),
            "@startuml\n' @id(USE-ADMIN)\n' @sec(stereotype=ByPassing, host=USE-ADMIN, allowed=Admin, id=SEC-GUARD)\nusecase \"Admin\" as Admin\n@enduml\n",
        )
        .unwrap();
        let mut trace = project.read_trace().unwrap();
        trace.links.push(TraceLink {
            from: "USE-ADMIN".to_string(),
            to: "SEQ-LOGIN".to_string(),
            relation: "realizes".to_string(),
        });
        project.write_trace(&trace).unwrap();

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("duplicate model ID SEC-GUARD")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_allows_same_sec_id_across_current_and_objective() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, allowed=Admin, id=SEC-LOGIN-GUARD)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, allowed=Admin, id=SEC-LOGIN-GUARD)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(
            report
                .errors
                .iter()
                .all(|error| !error.contains("duplicate model ID SEC-LOGIN-GUARD")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn review_security_matched_when_current_mirrors_objective() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        let body = "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, allowed=Admin)\nusecase \"Log in\" as Login\n@enduml\n";
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            body,
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            body,
        )
        .unwrap();

        let report = project.review_security().unwrap();

        assert!(report.matched, "missing: {:?}", report.missing_markers);
        assert!(report.missing_markers.is_empty());
        assert!(report.diff_puml_paths.is_empty());
        assert_eq!(report.mode, ParityMode::Error);
    }

    #[test]
    fn review_security_reports_missing_marker_and_emits_diff() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, allowed=Admin)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.review_security().unwrap();

        assert!(!report.matched);
        assert_eq!(report.missing_markers.len(), 1);
        assert_eq!(report.missing_markers[0].host, "USE-LOGIN");
        assert_eq!(report.missing_markers[0].stereotype, "ByPassing");
        assert_eq!(report.diff_puml_paths.len(), 1);
        let diff_path = dir.path().join(&report.diff_puml_paths[0]);
        assert!(diff_path.is_file());
        let diff_content = fs::read_to_string(&diff_path).unwrap();
        assert!(diff_content.contains("MISSING SECURITY MARKERS"));
        assert!(diff_content.contains("USE-LOGIN"));
        assert!(diff_content.contains("ByPassing"));
    }

    #[test]
    fn review_security_treats_param_differences_as_mismatch() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, allowed=Customer)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, allowed=Admin)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.review_security().unwrap();

        assert!(!report.matched);
        assert!(
            report
                .missing_markers
                .iter()
                .any(|m| m.params.get("allowed").map(String::as_str) == Some("Admin"))
        );
        assert!(
            report
                .extra_markers
                .iter()
                .any(|m| m.params.get("allowed").map(String::as_str) == Some("Customer"))
        );
    }

    #[test]
    fn review_security_treats_different_sec_ids_as_match_when_body_matches() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, allowed=Admin, id=SEC-CURRENT-LOGIN)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, allowed=Admin, id=SEC-OBJECTIVE-LOGIN)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.review_security().unwrap();

        assert!(report.matched, "missing: {:?}", report.missing_markers);
        assert!(report.missing_markers.is_empty());
        assert!(report.extra_markers.is_empty());
    }

    #[test]
    fn review_security_reports_extras_when_current_has_marker_absent_from_objective() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=ByPassing, host=USE-LOGIN, allowed=Admin)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.review_security().unwrap();

        assert!(report.matched, "missing: {:?}", report.missing_markers);
        assert_eq!(report.extra_markers.len(), 1);
        assert_eq!(report.extra_markers[0].host, "USE-LOGIN");
        assert!(report.diff_puml_paths.is_empty());
    }

    #[test]
    fn review_security_returns_error_mode_when_config_sets_error() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        let mut config = project.read_config().unwrap();
        config.security.parity_check = ParityMode::Error;
        project.write_yaml(".mdd/config.yml", &config).unwrap();

        let report = project.review_security().unwrap();

        assert_eq!(report.mode, ParityMode::Error);
    }

    fn write_domain_class(project: &Project, file: &str, id: &str, sec_line: &str) {
        let path = project.root().join(format!(".mdd/models/current/domain/{file}"));
        let body = format!(
            "@startuml\n' @id({id})\n{sec_line}\nclass {id} {{\n  + value : String\n}}\n@enduml\n"
        );
        fs::write(path, body).unwrap();
    }

    fn sec_errors(report: &ValidationReport, needle: &str) -> Vec<String> {
        report
            .errors
            .iter()
            .filter(|error| error.contains(needle))
            .cloned()
            .collect()
    }

    #[test]
    fn validate_accepts_encrypt_on_class_with_algorithm_and_scope() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_domain_class(
            &project,
            "secret.puml",
            "DOM-SECRET",
            "' @sec(stereotype=Encrypt, host=DOM-SECRET, algorithm=AES-256-GCM, scope=at_rest, field=value, id=SEC-SECRET-ENCRYPT)",
        );

        let report = project.validate().unwrap();

        assert!(
            sec_errors(&report, "Encrypt").is_empty(),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_rejects_encrypt_missing_algorithm() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_domain_class(
            &project,
            "secret.puml",
            "DOM-SECRET",
            "' @sec(stereotype=Encrypt, host=DOM-SECRET, scope=at_rest, field=value, id=SEC-SECRET-ENCRYPT)",
        );

        let report = project.validate().unwrap();
        let errors = sec_errors(&report, "Encrypt");
        assert!(!report.ok);
        assert!(
            errors.iter().any(|e| e.contains("algorithm")),
            "errors: {:?}",
            errors
        );
    }

    #[test]
    fn validate_rejects_encrypt_with_invalid_scope() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_domain_class(
            &project,
            "secret.puml",
            "DOM-SECRET",
            "' @sec(stereotype=Encrypt, host=DOM-SECRET, algorithm=AES-256-GCM, scope=somewhere, field=value, id=SEC-SECRET-ENCRYPT)",
        );

        let report = project.validate().unwrap();
        let errors = sec_errors(&report, "Encrypt");
        assert!(!report.ok);
        assert!(
            errors.iter().any(|e| e.contains("scope=somewhere")),
            "errors: {:?}",
            errors
        );
    }

    #[test]
    fn validate_accepts_buffer_overflow_with_max_length() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_domain_class(
            &project,
            "input.puml",
            "DOM-USER-INPUT",
            "' @sec(stereotype=BufferOverflow, host=DOM-USER-INPUT, field=value, max_length=64, id=SEC-INPUT-LEN)",
        );

        let report = project.validate().unwrap();

        assert!(
            sec_errors(&report, "BufferOverflow").is_empty(),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_rejects_buffer_overflow_missing_max_length() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_domain_class(
            &project,
            "input.puml",
            "DOM-USER-INPUT",
            "' @sec(stereotype=BufferOverflow, host=DOM-USER-INPUT, field=value, id=SEC-INPUT-LEN)",
        );

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            sec_errors(&report, "BufferOverflow")
                .iter()
                .any(|e| e.contains("max_length")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_accepts_sql_injection_with_sink_and_sanitizer() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_domain_class(
            &project,
            "search.puml",
            "DOM-SEARCH-QUERY",
            "' @sec(stereotype=SqlInjection, host=DOM-SEARCH-QUERY, field=query, sink=BookRepository::find_by_title, sanitizer=parameterized, id=SEC-SEARCH-SQLI)",
        );

        let report = project.validate().unwrap();

        assert!(
            sec_errors(&report, "SqlInjection").is_empty(),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_rejects_sql_injection_missing_sanitizer() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_domain_class(
            &project,
            "search.puml",
            "DOM-SEARCH-QUERY",
            "' @sec(stereotype=SqlInjection, host=DOM-SEARCH-QUERY, field=query, sink=BookRepository::find_by_title, id=SEC-SEARCH-SQLI)",
        );

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            sec_errors(&report, "SqlInjection")
                .iter()
                .any(|e| e.contains("sanitizer")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_accepts_flooding_on_use_case_with_max_rate() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=Flooding, host=USE-LOGIN, link=/login, max_rate=100, window=1s, action=throttle, id=SEC-LOGIN-FLOOD)\nusecase \"Log in\" as Login <<Flooding>>\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(
            sec_errors(&report, "Flooding").is_empty(),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_rejects_flooding_without_rate_or_concurrent() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=Flooding, host=USE-LOGIN, link=/login, window=1s, id=SEC-LOGIN-FLOOD)\nusecase \"Log in\" as Login <<Flooding>>\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            sec_errors(&report, "Flooding")
                .iter()
                .any(|e| e.contains("max_rate") && e.contains("max_concurrent")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_accepts_expiration_with_ttl() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_domain_class(
            &project,
            "session.puml",
            "DOM-SESSION",
            "' @sec(stereotype=Expiration, host=DOM-SESSION, field=token, ttl=15m, renewal=false, id=SEC-SESSION-TTL)",
        );

        let report = project.validate().unwrap();

        assert!(
            sec_errors(&report, "Expiration").is_empty(),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_rejects_expiration_missing_ttl() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_domain_class(
            &project,
            "session.puml",
            "DOM-SESSION",
            "' @sec(stereotype=Expiration, host=DOM-SESSION, field=token, id=SEC-SESSION-TTL)",
        );

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            sec_errors(&report, "Expiration")
                .iter()
                .any(|e| e.contains("ttl")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn validate_rejects_encrypt_on_use_case_host() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n' @sec(stereotype=Encrypt, host=USE-LOGIN, algorithm=AES-256-GCM, scope=in_transit, id=SEC-LOGIN-ENCRYPT)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(
            sec_errors(&report, "Encrypt")
                .iter()
                .any(|e| e.contains("requires host to be a class")),
            "errors: {:?}",
            report.errors
        );
    }

    #[test]
    fn generate_security_tests_emits_per_stereotype_scaffold() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        // Author one implementation-ready marker of each non-ByPassing stereotype on the objective side.
        fs::write(
            dir.path().join(".mdd/models/objective/domain/secret.puml"),
            "@startuml\n' @id(DOM-SECRET)\n' @sec(stereotype=Encrypt, host=DOM-SECRET, algorithm=AES-256-GCM, scope=at_rest, field=value, id=SEC-SECRET-ENCRYPT)\nclass DOM-SECRET {\n  + value : String\n}\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/domain/input.puml"),
            "@startuml\n' @id(DOM-USER-INPUT)\n' @sec(stereotype=BufferOverflow, host=DOM-USER-INPUT, field=value, max_length=64, id=SEC-INPUT-LEN)\nclass DOM-USER-INPUT {\n  + value : String\n}\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/domain/search.puml"),
            "@startuml\n' @id(DOM-SEARCH-QUERY)\n' @sec(stereotype=SqlInjection, host=DOM-SEARCH-QUERY, field=query, sink=BookRepository::find, sanitizer=parameterized, id=SEC-SEARCH-SQLI)\nclass DOM-SEARCH-QUERY {\n  + query : String\n}\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN-OBJ)\n' @sec(stereotype=Flooding, host=USE-LOGIN-OBJ, link=/login, max_rate=100, window=1s, action=throttle, id=SEC-LOGIN-FLOOD)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/domain/session.puml"),
            "@startuml\n' @id(DOM-SESSION)\n' @sec(stereotype=Expiration, host=DOM-SESSION, field=token, ttl=15m, id=SEC-SESSION-TTL)\nclass DOM-SESSION {\n  + token : String\n}\n@enduml\n",
        )
        .unwrap();

        let report = project.generate_security_tests().unwrap();

        assert_eq!(report.generated.len(), 5);
        let read = |slug: &str| {
            fs::read_to_string(dir.path().join(format!(".mdd/tests/acceptance/{slug}.feature")))
                .unwrap()
        };
        assert!(read("sec-secret-encrypt").contains("@security:Encrypt"));
        assert!(read("sec-secret-encrypt").contains("AES-256-GCM"));
        assert!(read("sec-input-len").contains("@security:BufferOverflow"));
        assert!(read("sec-input-len").contains("length greater than 64"));
        assert!(read("sec-search-sqli").contains("@security:SqlInjection"));
        assert!(read("sec-search-sqli").contains("SQL-injection payload"));
        assert!(read("sec-login-flood").contains("@security:Flooding"));
        assert!(read("sec-login-flood").contains("throttle"));
        assert!(read("sec-session-ttl").contains("@security:Expiration"));
        assert!(read("sec-session-ttl").contains("15m"));
    }

    #[test]
    fn deploy_files_surfaces_deploy_puml_without_touching_the_parity_gate() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);

        // Greenfield (no .mdd/deploy/) -> empty, not an error.
        assert!(project.deploy_files().unwrap().is_empty());

        let deploy_dir = dir.path().join(".mdd/deploy/azure-container-apps");
        fs::create_dir_all(&deploy_dir).unwrap();
        fs::write(
            deploy_dir.join("diagram.puml"),
            "@startuml\n' @id(DEPLOY-ACA-X)\nnode App\n@enduml\n",
        )
        .unwrap();
        let rendered_dir = dir
            .path()
            .join(".mdd/rendered/deploy/azure-container-apps");
        fs::create_dir_all(&rendered_dir).unwrap();
        fs::write(rendered_dir.join("diagram.svg"), "<svg/>").unwrap();

        let deploy = project.deploy_files().unwrap();
        assert_eq!(deploy.len(), 1);
        let f = &deploy[0];
        assert_eq!(f.path, ".mdd/deploy/azure-container-apps/diagram.puml");
        assert_eq!(f.ids, vec!["DEPLOY-ACA-X".to_string()]);
        assert_eq!(
            f.rendered_pages,
            vec!["deploy/azure-container-apps/diagram.svg".to_string()]
        );
        assert_eq!(f.side, ModelSide::Shared);

        // The parity gate must NOT see deploy: not in the model registry,
        // so review()/validate() keep ignoring .mdd/deploy/.
        let registry = project.model_registry().unwrap();
        assert!(
            registry.files.iter().all(|m| !m.path.starts_with(".mdd/deploy/")),
            "deploy file leaked into ModelRegistry"
        );
        assert!(
            registry.ids.iter().all(|e| e.id != "DEPLOY-ACA-X"),
            "deploy id leaked into the parity-gated id set"
        );
        let review = project.review().unwrap();
        assert!(
            !review.missing_ids.iter().any(|i| i == "DEPLOY-ACA-X")
                && !review.extra_ids.iter().any(|i| i == "DEPLOY-ACA-X"),
            "deploy id reached ID parity"
        );
    }

    fn write_minimal_valid_models(project: &Project) {
        let root = project.root();
        project.init().unwrap();
        fs::write(
            root.join(".mdd/models/current/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\n@enduml\n",
        )
        .unwrap();
        fs::write(
            root.join(".mdd/models/current/sequences/login.puml"),
            "@startuml\n' @id(SEQ-LOGIN)\n' @ref(USE-LOGIN)\n@enduml\n",
        )
        .unwrap();
        fs::write(
            root.join(".mdd/models/current/domain/user.puml"),
            "@startuml\n' @id(DOM-USER)\n@enduml\n",
        )
        .unwrap();

        let mut trace = project.read_trace().unwrap();
        trace.links.push(TraceLink {
            from: "USE-LOGIN".to_string(),
            to: "SEQ-LOGIN".to_string(),
            relation: "realizes".to_string(),
        });
        project.write_trace(&trace).unwrap();
    }

    fn write_login_mockup(project: &Project, model_id: &str, ui_id: &str, route: &str) {
        fs::write(
            project.root().join(".mdd/models/current/mockups/login.puml"),
            format!(
                "@startsalt\n' @id({model_id})\n' @ref(USE-LOGIN)\n' @ui-route({route})\n' @ui-viewport(desktop,1280,720)\n' @ui-element({ui_id}, role=button, name=\"Log in\", required=true)\n{{\n  [Log in]\n}}\n@endsalt\n"
            ),
        )
        .unwrap();
    }

    fn write_ui_test_trace_link(project: &Project, model_id: &str) {
        let slug = slugify(model_id);
        let path = format!(".mdd/tests/ui/{slug}.spec.ts");
        fs::write(
            project.root().join(&path),
            format!(
                "import {{ test }} from '@playwright/test';\n\ntest('{}', async () => {{}});\n",
                model_id
            ),
        )
        .unwrap();

        let mut trace = project.read_trace().unwrap();
        trace.generated_ui_tests.push(GeneratedUiTest {
            id: format!("UIT-{model_id}"),
            path,
            model_id: model_id.to_string(),
            framework: "playwright".to_string(),
        });
        project.write_trace(&trace).unwrap();
    }
}
