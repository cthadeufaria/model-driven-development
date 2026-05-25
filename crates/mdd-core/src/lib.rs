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
pub mod traceability;

pub use cycle::{Cycle, CycleDiff, CycleRegistry, CycleStatus, EntryPoint};
pub use traceability::SymbolSpan;

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
    /// Accumulating state files (config/trace/approvals) upgraded in place
    /// from an older schema version, preserving their content. Distinct
    /// from `overwritten`: a migration round-trips the existing file rather
    /// than replacing it with a template (USE-INIT-MIGRATE).
    pub migrated: Vec<String>,
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
    #[serde(default)]
    pub traceability: TraceabilityConfig,
    #[serde(default)]
    pub test: TestConfig,
}

/// Config for diagram-driven tests (CMP-TEST-CONFIG). `gate` governs the
/// coverage-rule severity (and, in later cycles, the close-time green gate);
/// `layers` is the per-layer test profile (runner framework + command),
/// populated by the detect-then-confirm UX in a later cycle. Cycle A seeds an
/// empty `layers`, which keeps the coverage rule advisory (safe-by-default).
#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq)]
pub struct TestConfig {
    #[serde(default)]
    pub gate: ParityMode,
    #[serde(default)]
    pub layers: BTreeMap<String, TestLayerConfig>,
}

/// One configured test layer: the runner `framework` and its `command`.
/// `command` execution is deferred to a later cycle; Cycle A only validates
/// layer/framework membership against these keys.
#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq)]
pub struct TestLayerConfig {
    #[serde(default)]
    pub framework: String,
    #[serde(default)]
    pub command: String,
}

/// One resolved step of the deterministic test plan (DOM-TEST-PLAN-STEP).
/// Pure data — `Project::test_plan` builds these but never runs `command`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TestPlanStep {
    pub layer: String,
    pub id: String,
    pub model_id: String,
    pub command: String,
    pub cwd: Option<String>,
    pub expect: TestExpect,
    pub is_gap: bool,
}

/// One per-layer runner recommended by build-file detection (part of
/// DOM-TEST-DETECTION). Recommends only; never written to config un-confirmed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DetectedLayer {
    pub layer: String,
    pub framework: String,
    pub command: String,
    pub cwd: Option<String>,
}

/// The outcome of `Project::detect_test_profile` (DOM-TEST-DETECTION): the
/// per-layer recommendations plus every undecidable choice, surfaced as a
/// blocking question by the skill rather than guessed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct DetectedProfile {
    pub recommendations: Vec<DetectedLayer>,
    pub ambiguities: Vec<String>,
}

/// The exit status of one executed plan step, fed to the green gate by the
/// skill after it runs the plan via Bash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestRunResult {
    pub id: String,
    pub exit_code: i32,
}

/// The green-gate verdict (CMP-TEST-GREEN-GATE). `blocking` is true only when
/// a test is still red AND `test.gate = error`; under `warn` a still-red test
/// is reported but does not block (the opt-down).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GreenGateReport {
    pub all_green: bool,
    pub still_red: Vec<String>,
    pub blocking: bool,
}

/// The result of running a gap test in one phase (part of DOM-TEST-PHASE-RECORD).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum TestPhaseResult {
    Fail,
    Pass,
}

/// One observation of running a gap test (DOM-TEST-PHASE-RECORD): the runner's
/// own command, exit code, result, and a captured output excerpt — not a bare
/// boolean, so a recorded RED is hard to fabricate without actually running.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct PhaseRecord {
    pub command: String,
    pub exit_code: i32,
    pub result: TestPhaseResult,
    #[serde(default)]
    pub excerpt: String,
    #[serde(default)]
    pub at: String,
}

/// One gap test's red→green record (part of DOM-TEST-EVIDENCE). The red phase
/// writes `red`; the green step writes `green`.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct GapTest {
    pub id: String,
    pub model_id: String,
    pub layer: TestLayer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub red: Option<PhaseRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub green: Option<PhaseRecord>,
}

/// The per-cycle red→green evidence artifact (DOM-TEST-EVIDENCE),
/// `.mdd/cycles/<N>/test-evidence.yml`. Its shape is validated deterministically;
/// the fail/pass results are produced by agent-Bash running the tests.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct TestEvidence {
    pub version: u32,
    pub cycle: String,
    #[serde(default)]
    pub gap_tests: Vec<GapTest>,
}

/// The non-negotiable red→green verdict (CMP-TEST-RED-GATE). `satisfied` is
/// true only when every gap @id shows fail-then-pass; the three buckets name
/// exactly why it is not. No config disables this gate.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RedGreenReport {
    pub satisfied: bool,
    pub missing_evidence: Vec<String>,
    pub not_red_first: Vec<String>,
    pub still_red: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq)]
pub struct SecurityConfig {
    #[serde(default)]
    pub parity_check: ParityMode,
}

/// Config for the traceability parity pass (CMP-TRACE-GATE). `error`
/// (default) makes reverse bucket B and forward errors block cycle closure;
/// `warn` opts the whole pass down to advisory, like `security.parity_check`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq)]
pub struct TraceabilityConfig {
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
    /// Unified, kind-agnostic test links (CMP-TEST-TRACE-MODEL). Preferred
    /// home for new links across every layer; `#[serde(default)]` so existing
    /// trace.yml files that predate it deserialize unchanged. The legacy
    /// `generated_tests` / `generated_ui_tests` arrays are kept and projected
    /// into this same shape for validation (DOM-TEST-PROJECTION).
    #[serde(default)]
    pub tests: Vec<TestLink>,
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

/// One unified test link (DOM-TEST-LINK): the diagram element it verifies
/// (`model_id`), where the test lives (`path`), which layer it occupies, the
/// runner `framework` (resolved from the test profile; optional), and an
/// `expect` marker. Cycle A is pure structure — nothing here is executed.
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct TestLink {
    pub id: String,
    pub path: String,
    pub model_id: String,
    pub layer: TestLayer,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framework: Option<String>,
    #[serde(default)]
    pub expect: TestExpect,
}

/// The layer a test occupies (DOM-TEST-LAYER); see the §5.1 diagram-kind ->
/// layer taxonomy. Serialized kebab-case (`unit`, `e2e`, ...).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
#[serde(rename_all = "kebab-case")]
pub enum TestLayer {
    Unit,
    Integration,
    E2e,
    Acceptance,
    Ui,
    Security,
}

impl TestLayer {
    /// The profile key / config-layer name this variant matches.
    pub fn as_str(self) -> &'static str {
        match self {
            TestLayer::Unit => "unit",
            TestLayer::Integration => "integration",
            TestLayer::E2e => "e2e",
            TestLayer::Acceptance => "acceptance",
            TestLayer::Ui => "ui",
            TestLayer::Security => "security",
        }
    }
}

/// A test's expectation (DOM-TEST-EXPECT). `RedUntilImplemented` marks a gap
/// test authored before its code — a pure data marker in Cycle A; the
/// non-negotiable red->green evidence gate it seeds lands in Cycle C.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, PartialEq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TestExpect {
    #[default]
    Pass,
    RedUntilImplemented,
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

/// A YAML state file that `mdd init` accumulates rather than regenerates
/// (config, trace, approvals — the `AccumulatingState` of DOM-INIT-FILE-CLASS).
/// Each carries a `version` it can report and stamp, so init can
/// forward-migrate an older on-disk file in place rather than overwrite it
/// (DOM-STATE-MIGRATION). `schema_version` is read from the type's `Default`
/// — the single source of truth for the version the running tool writes.
trait StateFile: Serialize + serde::de::DeserializeOwned + Default {
    fn schema_version() -> u32;
    fn set_schema_version(&mut self, version: u32);
}

macro_rules! impl_state_file {
    ($t:ty) => {
        impl StateFile for $t {
            fn schema_version() -> u32 {
                Self::default().version
            }
            fn set_schema_version(&mut self, version: u32) {
                self.version = version;
            }
        }
    };
}

impl_state_file!(MddConfig);
impl_state_file!(Trace);
impl_state_file!(Approvals);

/// The `version:` field of a YAML state file, or 0 when absent or
/// unparseable — so a version-less file sorts older than any real schema
/// version and is offered to the (parse-guarded) migration path.
fn read_yaml_version(raw: &str) -> u32 {
    serde_yaml::from_str::<serde_yaml::Value>(raw)
        .ok()
        .and_then(|v| v.get("version").and_then(serde_yaml::Value::as_u64))
        .map(|n| n as u32)
        .unwrap_or(0)
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
    /// Parity scope in effect: the objective `@id`s the open cycle declared
    /// (its manifest `scope`). Empty = whole-model — `missing` was computed
    /// against the entire objective id set. When non-empty, `missing` and the
    /// security pass were narrowed to these ids and out-of-scope gaps were
    /// treated as expected. (greenfield-kickoff Cycle A)
    pub scope: Vec<String>,
    pub missing_ids: Vec<String>,
    pub extra_ids: Vec<String>,
    pub diff_puml_paths: Vec<String>,
    /// Security-marker parity pass, always run as part of `review()`.
    pub security: SecurityReviewReport,
    /// Traceability parity pass (pass 3), always run as part of `review()`.
    pub traceability: TraceabilityReport,
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

/// DOM-TRACE-REPORT: outcome of `Project::review_traceability()`.
/// `matched = forward_errors.is_empty() && reverse_bucket_b.is_empty()`
/// when `traceability.parity_check = error`; under `warn` everything is
/// advisory and `matched` is always true.
#[derive(Debug, Clone, Serialize, Default)]
pub struct TraceabilityReport {
    pub matched: bool,
    pub mode: ParityMode,
    /// The base revision the reverse changeset was computed against.
    pub base: String,
    /// Implementable `@id`s whose source_link points at code that does not
    /// exist (FORWARD; error).
    pub forward_errors: Vec<ForwardError>,
    /// Edited glue (imports/attrs/comments/consts/tests) with no diagram
    /// counterpart (REVERSE bucket A; warn + show).
    pub reverse_bucket_a: Vec<String>,
    /// Edited behaviour-bearing symbols with no diagram counterpart
    /// (REVERSE bucket B; error, blocks closure under `error` mode).
    pub reverse_bucket_b: Vec<ReverseViolation>,
}

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct ForwardError {
    pub model_id: String,
    pub path: String,
    pub symbol: String,
}

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct ReverseViolation {
    pub path: String,
    pub symbol: String,
    pub kind: String,
}

/// Result of `Project::map_status()` — the freshness check (USE-MAP-FRESHNESS).
#[derive(Debug, Clone, Serialize)]
pub struct MapStatusReport {
    /// True when no tracked symbol changed since the recorded `source_revision`.
    pub fresh: bool,
    /// The recorded baseline, or `None` when there is no whole-map yet.
    pub source_revision: Option<String>,
    /// Tracked symbols that drifted since the baseline.
    pub drift: Vec<MapDrift>,
}

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct MapDrift {
    pub path: String,
    pub symbol: String,
    pub model_id: String,
}

/// DOM-SESSION-CONTEXT: the session-start brief printed by `mdd context` and
/// injected by the SessionStart hook — a whole-map table of contents plus the
/// freshness verdict (mirrored from [`MapStatusReport`]). A brief, not a gate.
#[derive(Debug, Clone, Serialize)]
pub struct SessionContext {
    /// One entry per whole-map concept kind, in canonical reading order.
    pub toc: Vec<TocEntry>,
    /// True when no tracked symbol changed since the freshness baseline.
    pub fresh: bool,
    /// The resolved baseline (explicit or derived), or `None` when there is no
    /// whole-map yet.
    pub source_revision: Option<String>,
    /// Tracked symbols that drifted since the baseline.
    pub drift: Vec<MapDrift>,
}

/// One row of the whole-map table of contents: a concept kind and how many
/// concept files and `@id`s it contributes.
#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct TocEntry {
    /// Concept directory under `.mdd/map`: use-cases, sequences, domain,
    /// components, mockups, states.
    pub kind: String,
    pub concept_count: usize,
    pub id_count: usize,
}

impl Default for MddConfig {
    fn default() -> Self {
        Self {
            // v2 adds the `test:` block (CMP-TEST-CONFIG); bumping the schema
            // version forward-migrates existing v1 config.yml files in place so
            // the block materializes without overwriting any operator content.
            version: 2,
            model_source: "plantuml".to_string(),
            constraint_source: "ocl".to_string(),
            rendered_dir: ".mdd/rendered".to_string(),
            security: SecurityConfig::default(),
            traceability: TraceabilityConfig::default(),
            test: TestConfig::default(),
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
            tests: Vec::new(),
            source_links: Vec::new(),
        }
    }
}

impl Trace {
    /// Project every test link — native `tests` plus the legacy
    /// `generated_tests` / `generated_ui_tests` arrays — into one unified
    /// `TestLink` view (DOM-TEST-PROJECTION). Legacy arrays are never mutated;
    /// this gives `validate` a single shape to reason over while existing
    /// trace.yml files keep working untouched.
    pub fn unified_tests(&self) -> Vec<TestLink> {
        let mut out = self.tests.clone();
        for t in &self.generated_tests {
            let layer = match t.category.as_deref() {
                Some("security") => TestLayer::Security,
                Some("unit") => TestLayer::Unit,
                Some("integration") => TestLayer::Integration,
                Some("e2e") => TestLayer::E2e,
                Some("ui") => TestLayer::Ui,
                // Acceptance is the historical meaning of generated_tests
                // (use-case `.feature` files), so an absent/other category
                // projects to acceptance.
                _ => TestLayer::Acceptance,
            };
            out.push(TestLink {
                id: t.id.clone(),
                path: t.path.clone(),
                model_id: t.model_id.clone(),
                layer,
                framework: None,
                expect: TestExpect::Pass,
            });
        }
        for t in &self.generated_ui_tests {
            out.push(TestLink {
                id: t.id.clone(),
                path: t.path.clone(),
                model_id: t.model_id.clone(),
                layer: TestLayer::Ui,
                framework: Some(t.framework.clone()),
                expect: TestExpect::Pass,
            });
        }
        out
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
        let mut migrated = Vec::new();
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
            ".mdd/ralph",
            ".mdd/architecture",
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

        // Authoritative, accumulating state (trace links, parity config,
        // approval hashes) is never overwritten with empty defaults. It is
        // created when missing, else forward-migrated to the current schema
        // version in place — preserving its content (USE-INIT-MIGRATE). The
        // conflict handler is deliberately not consulted here, so `--force`
        // cannot reach these three files. (Regenerable docs and skills below
        // keep the prompt-on-conflict path so upgrades can refresh them.)
        self.write_yaml_create_or_migrate::<MddConfig>(
            CONFIG_FILE,
            &mut created,
            &mut migrated,
            &mut skipped,
        )?;
        self.write_yaml_create_or_migrate::<Trace>(
            TRACE_FILE,
            &mut created,
            &mut migrated,
            &mut skipped,
        )?;
        self.write_yaml_create_or_migrate::<Approvals>(
            APPROVALS_FILE,
            &mut created,
            &mut migrated,
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
        // Test-profile doc (USE-INIT-TEST-PROFILE) — Regenerable, like the
        // other docs. The machine-readable `test:` block rides config.yml's
        // create-or-forward-migrate path above (AccumulatingState).
        self.write_text_if_missing(
            ".mdd/docs/test-profile.md",
            templates::test_profile_doc(),
            &mut created,
            &mut overwritten,
            &mut skipped,
            &mut on_conflict,
        )?;
        // Project brief (USE-KICKOFF) — SeededOnce like PLAN.md: `mdd init`
        // writes the template once, then /mdd-kickoff (or the developer) owns
        // it, so --force never clobbers a filled-in brief.
        self.write_text_create_if_missing(
            ".mdd/docs/brief.md",
            templates::brief(),
            &mut created,
            &mut skipped,
        )?;
        // Architecture-tracking how-to (USE-TRACK-ARCH) — Regenerable, like the
        // other docs: it puts the architecture-SoT workflow into any agent's
        // context.
        self.write_text_if_missing(
            ".mdd/docs/architecture-tracking.md",
            templates::architecture_tracking_doc(),
            &mut created,
            &mut overwritten,
            &mut skipped,
            &mut on_conflict,
        )?;
        // Architecture source of truth (DOM-ARCH-SPEC / CMP-ARCH-TRACKING) —
        // SeededOnce like brief.md / PLAN.md: mdd init writes the
        // documented-but-empty templates once, then /mdd-kickoff and agents own
        // the content, so --force never clobbers a real architecture spec.
        self.write_text_create_if_missing(
            ".mdd/architecture/components.yml",
            templates::arch_components_template(),
            &mut created,
            &mut skipped,
        )?;
        self.write_text_create_if_missing(
            ".mdd/architecture/decisions.yml",
            templates::arch_decisions_template(),
            &mut created,
            &mut skipped,
        )?;
        self.write_text_create_if_missing(
            ".mdd/architecture/constraints.yml",
            templates::arch_constraints_template(),
            &mut created,
            &mut skipped,
        )?;
        // Ralph workspace (USE-INIT-RALPH). PROMPT.md is Regenerable — it goes
        // through the conflict handler like every other template. PLAN.md is
        // SeededOnce (DOM-INIT-SEED-ONCE): created from the starter template
        // only when missing, then never touched — the conflict handler is
        // deliberately not consulted, so --force can never overwrite a plan
        // that the model gap, a backlog, or another agent now owns.
        self.write_text_if_missing(
            ".mdd/ralph/PROMPT.md",
            templates::ralph_prompt(),
            &mut created,
            &mut overwritten,
            &mut skipped,
            &mut on_conflict,
        )?;
        self.write_text_create_if_missing(
            ".mdd/ralph/PLAN.md",
            templates::ralph_plan(),
            &mut created,
            &mut skipped,
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

        self.write_session_hook(&mut created, &mut overwritten)?;

        Ok(InitReport {
            root: self.root.clone(),
            created,
            overwritten,
            migrated,
            skipped,
        })
    }

    pub fn clean(&self, force: bool) -> Result<CleanReport> {
        let mut removed = Vec::new();
        let mut skipped = Vec::new();

        self.remove_dir_all_if_exists(".mdd", &mut removed, &mut skipped)?;
        // Strip the SessionStart hook before pruning .claude, so an
        // mdd-only settings.json is removed and the dir can be reclaimed.
        self.remove_session_hook(&mut removed, &mut skipped)?;

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

    /// Build the deterministic, ordered test plan (CMP-TEST-PLAN): resolve
    /// `config.test.layers` x the unified trace tests into one step per test
    /// whose layer is configured, attaching the layer's command and flagging
    /// the gap subset (`expect = red-until-implemented`). Ordered by layer
    /// (taxonomy order) then test id. No execution — pure data.
    pub fn test_plan(&self) -> Result<Vec<TestPlanStep>> {
        let cfg = self.read_config()?.test;
        let trace = self.read_trace()?;
        let mut steps: Vec<TestPlanStep> = trace
            .unified_tests()
            .into_iter()
            .filter_map(|t| {
                let layer = t.layer.as_str();
                cfg.layers.get(layer).map(|layer_cfg| TestPlanStep {
                    layer: layer.to_string(),
                    id: t.id,
                    model_id: t.model_id,
                    command: layer_cfg.command.clone(),
                    cwd: None,
                    expect: t.expect,
                    is_gap: t.expect == TestExpect::RedUntilImplemented,
                })
            })
            .collect();
        // Stable order: taxonomy layer order, then id.
        let layer_rank = |l: &str| match l {
            "unit" => 0,
            "integration" => 1,
            "e2e" => 2,
            "acceptance" => 3,
            "ui" => 4,
            "security" => 5,
            _ => 6,
        };
        steps.sort_by(|a, b| {
            layer_rank(&a.layer)
                .cmp(&layer_rank(&b.layer))
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(steps)
    }

    /// Recommend a per-layer test runner by inspecting the repo's build files
    /// (CMP-TEST-DETECT). Read-only — recommends, never writes config. Genuine
    /// ambiguity (e.g. two unit frameworks) is recorded in `ambiguities` for
    /// the skill to surface as a blocking question, never auto-picked.
    pub fn detect_test_profile(&self) -> Result<DetectedProfile> {
        let exists = |rel: &str| self.root.join(rel).exists();
        let mut recs: Vec<DetectedLayer> = Vec::new();
        let mut ambiguities: Vec<String> = Vec::new();
        let mut unit_frameworks: Vec<&str> = Vec::new();

        if exists("Cargo.toml") {
            recs.push(DetectedLayer {
                layer: "unit".to_string(),
                framework: "cargo-test".to_string(),
                command: "cargo test --workspace --lib".to_string(),
                cwd: None,
            });
            recs.push(DetectedLayer {
                layer: "integration".to_string(),
                framework: "cargo-test".to_string(),
                command: "cargo test --workspace --test '*'".to_string(),
                cwd: None,
            });
            unit_frameworks.push("cargo-test");
        }
        if exists("package.json") {
            let pkg = fs::read_to_string(self.root.join("package.json")).unwrap_or_default();
            if pkg.contains("\"vitest\"") {
                recs.push(DetectedLayer {
                    layer: "unit".to_string(),
                    framework: "vitest".to_string(),
                    command: "npm test".to_string(),
                    cwd: None,
                });
                unit_frameworks.push("vitest");
            } else if pkg.contains("\"jest\"") {
                recs.push(DetectedLayer {
                    layer: "unit".to_string(),
                    framework: "jest".to_string(),
                    command: "npm test".to_string(),
                    cwd: None,
                });
                unit_frameworks.push("jest");
            }
            if pkg.contains("@playwright/test") || pkg.contains("\"playwright\"") {
                recs.push(DetectedLayer {
                    layer: "ui".to_string(),
                    framework: "playwright".to_string(),
                    command: "npx playwright test".to_string(),
                    cwd: None,
                });
            } else if pkg.contains("\"cypress\"") {
                recs.push(DetectedLayer {
                    layer: "ui".to_string(),
                    framework: "cypress".to_string(),
                    command: "npx cypress run".to_string(),
                    cwd: None,
                });
            }
        }
        if exists("pyproject.toml") || exists("setup.py") {
            recs.push(DetectedLayer {
                layer: "unit".to_string(),
                framework: "pytest".to_string(),
                command: "pytest".to_string(),
                cwd: None,
            });
            unit_frameworks.push("pytest");
        }
        if exists("go.mod") {
            recs.push(DetectedLayer {
                layer: "unit".to_string(),
                framework: "go-test".to_string(),
                command: "go test ./...".to_string(),
                cwd: None,
            });
            unit_frameworks.push("go-test");
        }

        if unit_frameworks.len() > 1 {
            ambiguities.push(format!(
                "multiple unit-test frameworks detected ({}); operator must confirm which to use",
                unit_frameworks.join(", ")
            ));
        }
        if recs.is_empty() {
            ambiguities.push(
                "no known build file found; the test profile must be configured manually"
                    .to_string(),
            );
        }
        Ok(DetectedProfile {
            recommendations: recs,
            ambiguities,
        })
    }

    /// The deterministic green-gate verdict (CMP-TEST-GREEN-GATE). A test is
    /// "still red" when its result is missing or its exit code is non-zero.
    /// `blocking` is true only when something is still red AND
    /// `config.test.gate = error`; under `warn` it is advisory (the opt-down).
    pub fn evaluate_green_gate(&self, results: &[TestRunResult]) -> Result<GreenGateReport> {
        let gate = self.read_config()?.test.gate;
        let still_red: Vec<String> = results
            .iter()
            .filter(|r| r.exit_code != 0)
            .map(|r| r.id.clone())
            .collect();
        let all_green = still_red.is_empty();
        Ok(GreenGateReport {
            all_green,
            blocking: !all_green && gate == ParityMode::Error,
            still_red,
        })
    }

    /// Read `.mdd/cycles/<cycle>/test-evidence.yml` if present (CMP-TEST-EVIDENCE).
    /// Returns `None` when the file is absent (a no-gap or pre-Cycle-C cycle).
    pub fn read_test_evidence(&self, cycle: &str) -> Result<Option<TestEvidence>> {
        let path = self
            .mdd_dir()
            .join("cycles")
            .join(cycle)
            .join("test-evidence.yml");
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let evidence: TestEvidence = serde_yaml::from_str(&raw)
            .with_context(|| format!("malformed test-evidence.yml at {}", path.display()))?;
        Ok(Some(evidence))
    }

    /// The non-negotiable red→green verdict (CMP-TEST-RED-GATE). Given the
    /// evidence and the cycle's gap @id set, `satisfied` is true only when every
    /// gap id has an entry whose `red` failed (exit != 0) and `green` passed
    /// (exit == 0). No config switch — this is distinct from `test.gate` (which
    /// governs only the green side). An empty `gap_ids` is vacuously satisfied
    /// (a pure refactor closes on the green gate alone).
    pub fn evaluate_red_green_gate(
        &self,
        evidence: Option<&TestEvidence>,
        gap_ids: &[String],
    ) -> RedGreenReport {
        let mut missing_evidence = Vec::new();
        let mut not_red_first = Vec::new();
        let mut still_red = Vec::new();

        for id in gap_ids {
            let entry = evidence.and_then(|e| e.gap_tests.iter().find(|g| &g.id == id));
            match entry {
                None => missing_evidence.push(id.clone()),
                Some(g) => {
                    let red_failed = matches!(
                        &g.red,
                        Some(r) if r.result == TestPhaseResult::Fail && r.exit_code != 0
                    );
                    let green_passed = matches!(
                        &g.green,
                        Some(gr) if gr.result == TestPhaseResult::Pass && gr.exit_code == 0
                    );
                    if !red_failed {
                        not_red_first.push(id.clone());
                    }
                    if !green_passed {
                        still_red.push(id.clone());
                    }
                }
            }
        }
        RedGreenReport {
            satisfied: missing_evidence.is_empty()
                && not_red_first.is_empty()
                && still_red.is_empty(),
            missing_evidence,
            not_red_first,
            still_red,
        }
    }

    /// Resolve the framework for an authored UI test from the test profile
    /// (CMP-TEST-CONFIG): the `ui` layer's configured framework when present,
    /// else the `playwright` fallback. Replaces the previous hardcoded default
    /// at the authoring site with profile resolution.
    pub fn resolve_ui_framework(&self) -> Result<String> {
        let framework = self
            .read_config()?
            .test
            .layers
            .get("ui")
            .map(|layer| layer.framework.clone())
            .filter(|fw| !fw.is_empty())
            .unwrap_or_else(default_ui_test_framework);
        Ok(framework)
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

    /// The parity scope for the currently open cycle, or empty when none.
    /// Reads the highest-numbered `Open` cycle's manifest `scope`; an empty
    /// result means the whole-model gate (the default for ordinary cycles,
    /// and whenever no cycle is open). (greenfield-kickoff Cycle A)
    fn open_cycle_scope(&self) -> Result<BTreeSet<String>> {
        let registry = self.cycle_registry()?;
        Ok(registry
            .cycles
            .iter()
            .filter(|c| c.manifest.status == CycleStatus::Open)
            .max_by_key(|c| c.manifest.number)
            .map(|c| c.manifest.scope.iter().cloned().collect())
            .unwrap_or_default())
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

        // Scoped parity (greenfield-kickoff Cycle A): when the open cycle
        // declares a manifest `scope`, narrow the ID gate to those objective
        // ids — objective ids outside the scope that are still absent from
        // current are expected, not a mismatch. An empty scope (ordinary
        // cycles, or no open cycle) is the whole-model gate, byte-identical
        // to before scoped parity.
        let scope = self.open_cycle_scope()?;
        let missing: BTreeSet<String> = objective_ids
            .difference(&current_ids)
            .filter(|id| scope.is_empty() || scope.contains(*id))
            .cloned()
            .collect();
        // Scope `extra` the same way so a realize-slice review annotates only
        // the slice: the rest of the (current) system is not flagged "extra"
        // relative to one slice's objective. Empty scope keeps every extra
        // (whole-model, unchanged).
        let extra: BTreeSet<String> = current_ids
            .difference(&objective_ids)
            .filter(|id| scope.is_empty() || scope.contains(*id))
            .cloned()
            .collect();
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

        let security = self.review_security_scoped(&scope)?;
        let security_gate_satisfied = security.matched || security.mode == ParityMode::Warn;

        let traceability = self.review_traceability(&registry)?;
        let traceability_gate_satisfied =
            traceability.matched || traceability.mode == ParityMode::Warn;

        let matched = ids_matched && security_gate_satisfied && traceability_gate_satisfied;

        Ok(ReviewReport {
            matched,
            ids_matched,
            scope: scope.into_iter().collect(),
            missing_ids: missing.into_iter().collect(),
            extra_ids: extra.into_iter().collect(),
            diff_puml_paths,
            security,
            traceability,
        })
    }

    pub fn review_security(&self) -> Result<SecurityReviewReport> {
        self.review_security_scoped(&BTreeSet::new())
    }

    /// Security-marker parity, optionally narrowed to a scope of objective
    /// `@id`s (greenfield-kickoff Cycle A). When `scope` is non-empty, only
    /// markers whose `host=` id is in scope are compared, so a realize-slice
    /// cycle is not blocked by guards the rest of the objective requires. An
    /// empty `scope` is the whole-model pass — the public `review_security`.
    fn review_security_scoped(&self, scope: &BTreeSet<String>) -> Result<SecurityReviewReport> {
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
                // Scoped parity: skip markers hosted on out-of-scope ids so a
                // realize-slice cycle only enforces its own slice's guards.
                if !scope.is_empty() && !scope.contains(&marker.host) {
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

    /// Implementable `@id` prefixes — the kinds whose source_links the
    /// forward traceability pass resolves (CMP-TRACE-GATE). Use cases
    /// (`USE-`), constraints (`OCL-`), and security IDs are excluded; they
    /// reach code through an implementable element, not directly.
    const IMPLEMENTABLE_PREFIXES: [&'static str; 4] = ["CMP-", "SEQ-", "DOM-", "STM-"];

    /// Resolve a source_link against the working tree. Returns an error
    /// string when the link is broken: the file is missing, or (for a `.rs`
    /// file) the named symbol is not found by the syn index. A file-only
    /// link, or a symbol on a non-Rust file, passes once the file exists.
    /// Shared by `validate` (USE-VERIFY-SOURCE-LINK) and the forward pass.
    fn resolve_source_link(&self, link: &SourceLink) -> Option<String> {
        let abs = self.root.join(&link.path);
        if !abs.exists() {
            return Some(format!(
                "source_link for {} points at missing file {}",
                link.model_id, link.path
            ));
        }
        if let Some(symbol) = &link.symbol {
            if link.path.ends_with(".rs") {
                let Ok(src) = fs::read_to_string(&abs) else {
                    return Some(format!(
                        "source_link for {} could not read {}",
                        link.model_id, link.path
                    ));
                };
                let symbols = traceability::extract_symbols(&link.path, &src);
                let found = symbols
                    .iter()
                    .any(|s| traceability::symbol_matches(&s.symbol, symbol));
                if !found {
                    return Some(format!(
                        "source_link for {} points at symbol `{}` not found in {}",
                        link.model_id, symbol, link.path
                    ));
                }
            }
        }
        None
    }

    /// Pass 3 of `review()` (SEQ-TRACE-REVIEW). FORWARD: every source_link on
    /// an implementable current-side `@id` must resolve to real code (broken
    /// link = error, a diagram must not lie). REVERSE: every code symbol
    /// changed since the cycle base must be covered by a source_link —
    /// behaviour-bearing symbols block (bucket B), glue only warns (bucket A).
    pub fn review_traceability(&self, registry: &ModelRegistry) -> Result<TraceabilityReport> {
        let mode = self.read_config()?.traceability.parity_check;
        let trace = self.read_trace()?;
        let base = self.traceability_base()?;

        // FORWARD
        let mut forward_errors = Vec::new();
        for element in &registry.ids {
            if element.side != ModelSide::Current
                || !Self::IMPLEMENTABLE_PREFIXES
                    .iter()
                    .any(|p| element.id.starts_with(p))
            {
                continue;
            }
            for link in trace.source_links.iter().filter(|l| l.model_id == element.id) {
                if self.resolve_source_link(link).is_some() {
                    forward_errors.push(ForwardError {
                        model_id: element.id.clone(),
                        path: link.path.clone(),
                        symbol: link.symbol.clone().unwrap_or_default(),
                    });
                }
            }
        }

        // REVERSE (changeset since base)
        let changed = traceability::changed_code(&self.root, &base)?;
        let mut reverse_bucket_a: BTreeSet<String> = BTreeSet::new();
        let mut reverse_bucket_b = Vec::new();
        for sym in &changed.symbols {
            let covered = trace.source_links.iter().any(|l| {
                l.path == sym.path
                    && l.symbol
                        .as_deref()
                        .map(|s| traceability::symbol_matches(&sym.symbol, s))
                        .unwrap_or(true)
            });
            if covered {
                continue;
            }
            if traceability::is_behaviour_kind(&sym.kind) {
                reverse_bucket_b.push(ReverseViolation {
                    path: sym.path.clone(),
                    symbol: sym.symbol.clone(),
                    kind: sym.kind.clone(),
                });
            } else {
                reverse_bucket_a.insert(format!("{} ({} {})", sym.path, sym.kind, sym.symbol));
            }
        }
        for glue in &changed.glue {
            reverse_bucket_a.insert(format!("{} [{}]", glue.path, glue.label));
        }

        let matched = forward_errors.is_empty() && reverse_bucket_b.is_empty();
        Ok(TraceabilityReport {
            matched,
            mode,
            base,
            forward_errors,
            reverse_bucket_a: reverse_bucket_a.into_iter().collect(),
            reverse_bucket_b,
        })
    }

    /// Freshness check (SEQ-MAP-STATUS): are the diagrams still current with
    /// the code? Diffs the source tree from the whole-map's recorded
    /// `source_revision` to the working tree and reports any tracked symbol
    /// (one a source_link points at) that drifted. No whole-map yet -> fresh.
    pub fn map_status(&self) -> Result<MapStatusReport> {
        let Some(source_revision) = self.read_map_source_revision()? else {
            return Ok(MapStatusReport {
                fresh: true,
                source_revision: None,
                drift: Vec::new(),
            });
        };
        let trace = self.read_trace()?;
        let changed = traceability::changed_code(&self.root, &source_revision)?;
        let mut drift = Vec::new();
        for sym in &changed.symbols {
            for link in &trace.source_links {
                let symbol_match = link
                    .symbol
                    .as_deref()
                    .map(|s| traceability::symbol_matches(&sym.symbol, s))
                    .unwrap_or(true);
                if link.path == sym.path && symbol_match {
                    drift.push(MapDrift {
                        path: sym.path.clone(),
                        symbol: sym.symbol.clone(),
                        model_id: link.model_id.clone(),
                    });
                }
            }
        }
        drift.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then_with(|| a.symbol.cmp(&b.symbol))
                .then_with(|| a.model_id.cmp(&b.model_id))
        });
        drift.dedup();
        Ok(MapStatusReport {
            fresh: drift.is_empty(),
            source_revision: Some(source_revision),
            drift,
        })
    }

    /// SEQ-SESSION-BRIEF / CMP-SESSION-CLI: the session-start brief. Builds the
    /// whole-map table of contents (concepts grouped by kind under `.mdd/map`)
    /// and folds in the [`Project::map_status`] freshness verdict. Reuses the
    /// existing freshness engine rather than re-deriving anything; it is a
    /// briefing, never a gate (the CLI always exits 0).
    pub fn session_context(&self) -> Result<SessionContext> {
        let toc = self.map_toc()?;
        let status = self.map_status()?;
        Ok(SessionContext {
            toc,
            fresh: status.fresh,
            source_revision: status.source_revision,
            drift: status.drift,
        })
    }

    /// The whole-map table of contents: one [`TocEntry`] per concept directory
    /// present under `.mdd/map`, counting concept files and `@id`s. Empty when
    /// there is no whole-map yet. Canonical kinds lead, in reading order; any
    /// unexpected directory follows alphabetically.
    fn map_toc(&self) -> Result<Vec<TocEntry>> {
        const KIND_ORDER: &[&str] = &[
            "use-cases",
            "sequences",
            "domain",
            "components",
            "mockups",
            "states",
        ];
        let map_dir = self.mdd_dir().join("map");
        let mut files: BTreeMap<String, usize> = BTreeMap::new();
        let mut ids: BTreeMap<String, usize> = BTreeMap::new();
        if map_dir.is_dir() {
            for entry in WalkDir::new(&map_dir)
                .into_iter()
                .filter_map(std::result::Result::ok)
            {
                let path = entry.path();
                if !entry.file_type().is_file()
                    || path.extension().and_then(|e| e.to_str()) != Some("puml")
                {
                    continue;
                }
                let Ok(rel) = path.strip_prefix(&map_dir) else {
                    continue;
                };
                let Some(kind) = rel.components().next().and_then(|c| c.as_os_str().to_str())
                else {
                    continue;
                };
                let content = fs::read_to_string(path)
                    .with_context(|| format!("failed to read {}", path.display()))?;
                *files.entry(kind.to_string()).or_default() += 1;
                *ids.entry(kind.to_string()).or_default() += extract_ids(&content)?.len();
            }
        }
        let mut toc = Vec::new();
        let mut seen = BTreeSet::new();
        for kind in KIND_ORDER {
            if let Some(&concept_count) = files.get(*kind) {
                toc.push(TocEntry {
                    kind: (*kind).to_string(),
                    concept_count,
                    id_count: ids.get(*kind).copied().unwrap_or(0),
                });
                seen.insert((*kind).to_string());
            }
        }
        for (kind, &concept_count) in &files {
            if !seen.contains(kind) {
                toc.push(TocEntry {
                    kind: kind.clone(),
                    concept_count,
                    id_count: ids.get(kind).copied().unwrap_or(0),
                });
            }
        }
        Ok(toc)
    }

    /// The base revision for the reverse changeset: the `base_revision` of
    /// the highest-numbered open cycle (so review inside `/mdd-cycle` scopes
    /// to that cycle), or `HEAD` when no cycle is open (standalone review).
    fn traceability_base(&self) -> Result<String> {
        let cycles_dir = self.mdd_dir().join("cycles");
        let mut best: Option<(String, String)> = None; // (cycle dir name, base_revision)
        if let Ok(entries) = fs::read_dir(&cycles_dir) {
            for entry in entries.flatten() {
                let manifest = entry.path().join("manifest.yml");
                let Ok(text) = fs::read_to_string(&manifest) else {
                    continue;
                };
                let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&text) else {
                    continue;
                };
                let open = value.get("status").and_then(|s| s.as_str()) == Some("open");
                let base = value
                    .get("base_revision")
                    .and_then(|s| s.as_str())
                    .map(str::to_string);
                if let (true, Some(base)) = (open, base) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if best.as_ref().map(|(n, _)| name > *n).unwrap_or(true) {
                        best = Some((name, base));
                    }
                }
            }
        }
        Ok(best.map(|(_, base)| base).unwrap_or_else(|| "HEAD".to_string()))
    }

    /// Resolve the freshness baseline (DOM-MAP-BASELINE). An explicit
    /// `source_revision` in the whole-map manifest wins; when the manifest
    /// exists but carries none, derive it from git — the last commit that
    /// touched the current-side diagrams. No whole-map manifest means no
    /// baseline (greenfield: `map_status` reports FRESH, no baseline). Nothing
    /// is ever written: the baseline self-maintains off commit history.
    fn read_map_source_revision(&self) -> Result<Option<String>> {
        let manifest = self.mdd_dir().join("map").join("manifest.yml");
        let Ok(text) = fs::read_to_string(&manifest) else {
            return Ok(None);
        };
        let value: serde_yaml::Value = serde_yaml::from_str(&text)
            .with_context(|| "failed to parse .mdd/map/manifest.yml")?;
        if let Some(rev) = value
            .get("source_revision")
            .and_then(|s| s.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Ok(Some(rev.to_string()));
        }
        Ok(self.derive_map_baseline())
    }

    /// The last commit that touched the current-side diagrams
    /// (`.mdd/models/current`), or `None` when git is unavailable or no such
    /// commit exists yet. Reuses git the same way the traceability engine does;
    /// it writes nothing.
    fn derive_map_baseline(&self) -> Option<String> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["log", "-1", "--format=%H", "--", ".mdd/models/current"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let rev = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if rev.is_empty() { None } else { Some(rev) }
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
                framework: self.resolve_ui_framework()?,
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

        // Per-side uniqueness for UIC-... IDs, matching the rule for
        // USE-/SEQ-/DOM-/CMP-/STM-/SEC-: the same UIC may appear in
        // current/ and objective/ mockups when both sides describe the
        // same UI element in two states. A UIC- collision is only an
        // error when it happens twice on the same side.
        let mut ui_contract_ids = BTreeMap::<(ModelSide, String), String>::new();
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
                    if let Some(first_file) = ui_contract_ids
                        .insert((file.side, element.id.clone()), file.path.clone())
                    {
                        errors.push(format!(
                            "duplicate UI contract ID {} in {} and {} (same side)",
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

        // Diagram-driven tests — structural rules (CMP-TEST-VALIDATE).
        // Existence/linkage/membership/coverage only; nothing is executed in
        // this cycle. Safe-by-default: the coverage and membership rules stay
        // inert until the test profile actually configures layers, so a repo
        // that has not yet adopted diagram-driven tests is unaffected.
        let test_cfg = &self.read_config()?.test;
        let unified_tests = trace.unified_tests();

        // Every native `tests` link references a known model ID and an
        // existing file (mirrors the legacy generated_* path checks).
        for test in &trace.tests {
            if !all_ids.contains(&test.model_id) {
                errors.push(format!(
                    "test {} references unknown model ID {}",
                    test.id, test.model_id
                ));
            }
            if !self.root.join(&test.path).exists() {
                errors.push(format!("test file is missing: {}", test.path));
            }
        }

        if !test_cfg.layers.is_empty() {
            // Layer/framework membership (USE-TEST-LAYER-MEMBERSHIP): replaces
            // the historical silent drop of non-playwright frameworks. Only
            // meaningful once a profile defines the layer set.
            for test in &unified_tests {
                match test_cfg.layers.get(test.layer.as_str()) {
                    None => errors.push(format!(
                        "test {} declares layer `{}`, which is not in the configured test profile (config.test.layers)",
                        test.id,
                        test.layer.as_str()
                    )),
                    Some(layer_cfg) => {
                        if let Some(fw) = &test.framework
                            && !layer_cfg.framework.is_empty()
                            && &layer_cfg.framework != fw
                        {
                            errors.push(format!(
                                "test {} declares framework `{}` for layer `{}`, but the profile configures `{}`",
                                test.id,
                                fw,
                                test.layer.as_str(),
                                layer_cfg.framework
                            ));
                        }
                    }
                }
            }

            // Coverage (USE-TEST-COVERAGE): every implementation-bearing @id
            // has >=1 linked test of the layer its kind expects (§5.1). A gap
            // blocks when test.gate=error, else warns. Runs only with a
            // configured profile (above check), so this repo stays quiet until
            // it opts in.
            // Components have no dedicated ModelKind (they classify as Other),
            // so collect them by their `CMP-` id prefix.
            let component_ids: BTreeSet<String> = all_ids
                .iter()
                .filter(|id| id.starts_with("CMP-"))
                .cloned()
                .collect();
            let coverage_blocks = test_cfg.gate == ParityMode::Error;
            let expected: [(&BTreeSet<String>, &[TestLayer], &str); 5] = [
                (&domain_ids, &[TestLayer::Unit], "unit"),
                (&sequence_ids, &[TestLayer::Integration], "integration"),
                (&component_ids, &[TestLayer::Integration], "integration"),
                (
                    &use_case_ids,
                    &[TestLayer::E2e, TestLayer::Acceptance],
                    "e2e/acceptance",
                ),
                (&mockup_ids, &[TestLayer::Ui], "ui"),
            ];
            for (ids, layers, label) in expected {
                for id in ids {
                    let covered = unified_tests
                        .iter()
                        .any(|t| &t.model_id == id && layers.contains(&t.layer));
                    if !covered {
                        let msg = format!(
                            "implementation-bearing {id} has no linked {label} test in .mdd/trace.yml"
                        );
                        if coverage_blocks {
                            errors.push(msg);
                        } else {
                            warnings.push(msg);
                        }
                    }
                }
            }
        }

        // test-evidence.yml shape check (CMP-TEST-EVIDENCE): when a cycle dir
        // carries one, it must parse and every gap entry must be well-formed —
        // a present red failed (exit != 0), a present green passed (exit == 0),
        // and the model_id is known. The fail/pass values themselves are
        // produced by agent-Bash; this gates only the SHAPE.
        let cycles_dir = self.mdd_dir().join("cycles");
        if cycles_dir.is_dir() {
            let mut cycle_dirs: Vec<_> = fs::read_dir(&cycles_dir)
                .with_context(|| format!("failed to read {}", cycles_dir.display()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .collect();
            cycle_dirs.sort();
            for dir in cycle_dirs {
                let ev_path = dir.join("test-evidence.yml");
                if !ev_path.exists() {
                    continue;
                }
                let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                let raw = fs::read_to_string(&ev_path).unwrap_or_default();
                match serde_yaml::from_str::<TestEvidence>(&raw) {
                    Err(e) => errors.push(format!(
                        "test-evidence.yml in cycle {name} is malformed: {e}"
                    )),
                    Ok(evidence) => {
                        for g in &evidence.gap_tests {
                            if !all_ids.contains(&g.model_id) {
                                errors.push(format!(
                                    "test-evidence.yml (cycle {name}) gap test {} references unknown model ID {}",
                                    g.id, g.model_id
                                ));
                            }
                            if let Some(r) = &g.red
                                && (r.result != TestPhaseResult::Fail || r.exit_code == 0)
                            {
                                errors.push(format!(
                                    "test-evidence.yml (cycle {name}) gap test {}: red phase must be a failure (result=fail, exit_code != 0)",
                                    g.id
                                ));
                            }
                            if let Some(gr) = &g.green
                                && (gr.result != TestPhaseResult::Pass || gr.exit_code != 0)
                            {
                                errors.push(format!(
                                    "test-evidence.yml (cycle {name}) gap test {}: green phase must be a pass (result=pass, exit_code == 0)",
                                    g.id
                                ));
                            }
                        }
                    }
                }
            }
        }

        // Source-link existence (USE-VERIFY-SOURCE-LINK): honor checklist
        // item 5 — every source_link must point at a file that exists and,
        // when a symbol is given on a .rs file, a symbol syn can resolve.
        for link in &trace.source_links {
            if !all_ids.contains(&link.model_id) {
                errors.push(format!(
                    "source_link references unknown model ID {}",
                    link.model_id
                ));
            }
            if let Some(error) = self.resolve_source_link(link) {
                errors.push(error);
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

        // Greenfield-kickoff PLAN `@scope` rules (Cycle B). Inert unless the
        // Ralph plan carries `@scope` markers (i.e. a kickoff-decomposed plan),
        // so a repo that does not drive Ralph from a kickoff PLAN gets no new
        // warnings. Both rules are WARNING, never blocking.
        let plan_scopes = self.plan_scope_ids()?;
        if !plan_scopes.is_empty() {
            let objective_ids: BTreeSet<&str> = registry
                .ids
                .iter()
                .filter(|element| element.side == ModelSide::Objective)
                .map(|element| element.id.as_str())
                .collect();
            // Rule 1 (OCL-KICKOFF-SCOPE-IDS-EXIST): every PLAN `@scope` id
            // resolves to an objective `@id`.
            for id in &plan_scopes {
                if !objective_ids.contains(id.as_str()) {
                    warnings.push(format!(
                        "PLAN.md @scope references unknown objective @id {id}"
                    ));
                }
            }
            // Rule 2 (OCL-KICKOFF-PLAN-COVERS-OBJECTIVE): the `@scope` union
            // covers every implementation-bearing objective `@id`, so finishing
            // the PLAN coincides with whole-model parity.
            const IMPL_PREFIXES: [&str; 6] = ["USE-", "SEQ-", "DOM-", "CMP-", "STM-", "MCK-"];
            let scope_set: BTreeSet<&str> = plan_scopes.iter().map(|s| s.as_str()).collect();
            for element in registry.ids.iter().filter(|e| e.side == ModelSide::Objective) {
                let impl_bearing = IMPL_PREFIXES.iter().any(|p| element.id.starts_with(p));
                if impl_bearing && !scope_set.contains(element.id.as_str()) {
                    warnings.push(format!(
                        "PLAN.md @scope union does not cover objective @id {} (no PLAN item realizes it)",
                        element.id
                    ));
                }
            }
        }

        Ok(ValidationReport {
            ok: errors.is_empty(),
            errors,
            warnings,
            registry,
        })
    }

    /// Objective `@id`s named by `@scope(...)` markers in the Ralph plan
    /// (`.mdd/ralph/PLAN.md`). Empty when the plan is missing or carries no
    /// `@scope` — `/mdd-kickoff` is the source that writes them, so this is the
    /// PLAN-consumption contract for the greenfield handoff (Cycle B).
    fn plan_scope_ids(&self) -> Result<Vec<String>> {
        let path = self.root.join(".mdd/ralph/PLAN.md");
        if !path.is_file() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        extract_scope_ids(&content)
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

    /// Write a YAML state file only if it does not already exist. An existing
    /// file is left untouched and recorded as skipped — never overwritten.
    /// Used for authoritative, accumulating state (`trace.yml`, `config.yml`,
    /// `approvals.yml`) so a re-`init` can never destroy it.
    /// Write an accumulating state file (config/trace/approvals): create it
    /// from `T::default()` when missing, else forward-migrate it in place to
    /// the current schema version. Migration fires only when the on-disk
    /// `version` is strictly older than `T::schema_version()` — the file is
    /// parsed through the current struct (so fields added since it was
    /// written fill from their defaults), the version is bumped, and it is
    /// re-serialized, preserving all prior content. An equal-or-newer version
    /// is left byte-for-byte untouched (idempotent; never downgrades; keeps
    /// curated comments). A file that fails to parse is left untouched rather
    /// than clobbered. This path never consults the init conflict handler, so
    /// `--force` can never overwrite these three files (OCL-INIT-FORCE-SCOPE,
    /// OCL-INIT-STATE-FORWARD-ONLY).
    fn write_yaml_create_or_migrate<T: StateFile>(
        &self,
        relative: &str,
        created: &mut Vec<String>,
        migrated: &mut Vec<String>,
        skipped: &mut Vec<String>,
    ) -> Result<()> {
        let path = self.root.join(relative);
        if !path.exists() {
            let content = serde_yaml::to_string(&T::default())
                .with_context(|| format!("failed to serialize {relative}"))?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(&path, content)
                .with_context(|| format!("failed to write {}", path.display()))?;
            created.push(relative.to_string());
            return Ok(());
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let current = T::schema_version();
        // Equal or newer on disk -> never rewrite (idempotent; no downgrade).
        if read_yaml_version(&raw) >= current {
            skipped.push(relative.to_string());
            return Ok(());
        }
        let Ok(mut value) = serde_yaml::from_str::<T>(&raw) else {
            // Malformed older file: preserve it untouched rather than lose state.
            skipped.push(relative.to_string());
            return Ok(());
        };
        value.set_schema_version(current);
        let content = serde_yaml::to_string(&value)
            .with_context(|| format!("failed to serialize {relative}"))?;
        fs::write(&path, content)
            .with_context(|| format!("failed to write {}", path.display()))?;
        migrated.push(relative.to_string());
        Ok(())
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

    /// Write a SeededOnce file (DOM-INIT-SEED-ONCE): create it from the given
    /// template only when it is missing, and otherwise leave it untouched. The
    /// init conflict handler is intentionally NOT consulted, so `--force` can
    /// never overwrite it — unlike a Regenerable file written through
    /// [`Project::write_text_if_missing`]. There is no migration path either:
    /// the file is seeded once and thereafter owned by whoever edits it
    /// (e.g. `.mdd/ralph/PLAN.md`, consumed and rewritten by the Ralph loop).
    fn write_text_create_if_missing(
        &self,
        relative: &str,
        value: &str,
        created: &mut Vec<String>,
        skipped: &mut Vec<String>,
    ) -> Result<()> {
        let path = self.root.join(relative);
        if path.exists() {
            skipped.push(relative.to_string());
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&path, value)
            .with_context(|| format!("failed to write {}", path.display()))?;
        created.push(relative.to_string());
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

    /// CMP-INIT-HOOK: merge the mdd-managed SessionStart hook (DOM-SESSION-HOOK)
    /// into `.claude/settings.json`. Idempotent and keyed by command — re-init
    /// never duplicates it, and any user-authored hooks/permissions are
    /// preserved. The JSON analogue of [`Project::write_managed_block`]; the
    /// init conflict handler is intentionally not consulted.
    fn write_session_hook(
        &self,
        created: &mut Vec<String>,
        overwritten: &mut Vec<String>,
    ) -> Result<()> {
        let relative = ".claude/settings.json";
        let path = self.root.join(relative);
        let existed = path.exists();
        let mut settings = if existed {
            let text = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            if text.trim().is_empty() {
                serde_json::Value::Object(Default::default())
            } else {
                serde_json::from_str(&text)
                    .with_context(|| format!("failed to parse {} as JSON", path.display()))?
            }
        } else {
            serde_json::Value::Object(Default::default())
        };

        let changed = upsert_session_hook(&mut settings, templates::SESSION_HOOK_COMMAND);
        if existed && !changed {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut json = serde_json::to_string_pretty(&settings)
            .with_context(|| "failed to serialize .claude/settings.json")?;
        json.push('\n');
        fs::write(&path, json)
            .with_context(|| format!("failed to write {}", path.display()))?;
        if existed {
            overwritten.push(relative.to_string());
        } else {
            created.push(relative.to_string());
        }
        Ok(())
    }

    /// Strip the mdd-managed SessionStart hook from `.claude/settings.json`,
    /// preserving any user-authored settings. The file is deleted only when the
    /// hook was its sole content. A settings file that cannot be parsed is
    /// skipped (never clobbered).
    fn remove_session_hook(
        &self,
        removed: &mut Vec<String>,
        skipped: &mut Vec<CleanSkip>,
    ) -> Result<()> {
        let relative = ".claude/settings.json";
        let path = self.root.join(relative);
        if !path.exists() {
            return Ok(());
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let mut settings: serde_json::Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(_) => {
                skipped.push(CleanSkip {
                    path: relative.to_string(),
                    reason: "could not parse settings.json as JSON; left untouched".to_string(),
                });
                return Ok(());
            }
        };
        if !strip_session_hook(&mut settings, templates::SESSION_HOOK_COMMAND) {
            return Ok(());
        }
        let now_empty = settings.as_object().is_some_and(serde_json::Map::is_empty);
        if now_empty {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        } else {
            let mut json = serde_json::to_string_pretty(&settings)
                .with_context(|| "failed to serialize .claude/settings.json")?;
            json.push('\n');
            fs::write(&path, json)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
        removed.push(format!("{relative} (mdd hook)"));
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

/// Serde default for the legacy `GeneratedUiTest.framework` field — kept so
/// pre-existing trace.yml files (which omit it) still deserialize. New
/// authoring resolves the framework from the test profile instead
/// (`Project::resolve_ui_framework`), not this hardcoded constant.
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

/// Extract objective `@id`s from `@scope(<id>, <id>, …)` markers on Ralph PLAN
/// items (greenfield-kickoff Cycle B). The body is a comma-separated id list,
/// reusing the `@id`/`@ref` marker idiom; whitespace is trimmed and empties
/// dropped. Returns the sorted, de-duplicated union across all markers.
fn extract_scope_ids(content: &str) -> Result<Vec<String>> {
    let re = Regex::new(r"@scope\(([^)]*)\)")?;
    let mut ids = Vec::new();
    for capture in re.captures_iter(content) {
        for raw in capture[1].split(',') {
            let id = raw.trim();
            if !id.is_empty() {
                ids.push(id.to_string());
            }
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
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

/// Ensure `settings` carries the mdd-managed SessionStart command hook exactly
/// once. Returns true when `settings` was modified. Idempotent: a second call
/// is a no-op (OCL-HOOK-IDEMPOTENT). User-authored hooks are left in place.
fn upsert_session_hook(settings: &mut serde_json::Value, command: &str) -> bool {
    use serde_json::{Map, Value};
    if !settings.is_object() {
        *settings = Value::Object(Map::new());
    }
    let root = settings.as_object_mut().expect("settings is an object");
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    if !hooks.is_object() {
        *hooks = Value::Object(Map::new());
    }
    let hooks = hooks.as_object_mut().expect("hooks is an object");
    let session = hooks
        .entry("SessionStart")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !session.is_array() {
        *session = Value::Array(Vec::new());
    }
    let groups = session.as_array_mut().expect("SessionStart is an array");
    if groups.iter().any(|group| group_has_command(group, command)) {
        return false;
    }
    groups.push(serde_json::json!({
        "hooks": [ { "type": "command", "command": command } ]
    }));
    true
}

/// Remove every mdd-managed SessionStart command hook from `settings`, pruning
/// emptied groups and keys. Returns true when anything was removed. Surgical:
/// only the matching command hook is touched.
fn strip_session_hook(settings: &mut serde_json::Value, command: &str) -> bool {
    use serde_json::Value;
    let Some(root) = settings.as_object_mut() else {
        return false;
    };
    let Some(hooks) = root.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    let session_empty;
    {
        let Some(session) = hooks.get_mut("SessionStart").and_then(Value::as_array_mut) else {
            return false;
        };
        let before = session.len();
        for group in session.iter_mut() {
            if let Some(inner) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let n = inner.len();
                inner.retain(|hook| !command_hook_matches(hook, command));
                changed |= inner.len() != n;
            }
        }
        session.retain(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .map(|inner| !inner.is_empty())
                .unwrap_or(true)
        });
        changed |= session.len() != before;
        session_empty = session.is_empty();
    }
    if session_empty {
        hooks.remove("SessionStart");
    }
    if hooks.is_empty() {
        root.remove("hooks");
    }
    changed
}

/// True when `group` (a SessionStart matcher group) contains a command hook
/// running `command`.
fn group_has_command(group: &serde_json::Value, command: &str) -> bool {
    group
        .get("hooks")
        .and_then(serde_json::Value::as_array)
        .map(|inner| inner.iter().any(|hook| command_hook_matches(hook, command)))
        .unwrap_or(false)
}

/// True when `hook` is `{ "type": "command", "command": <command> }`.
fn command_hook_matches(hook: &serde_json::Value, command: &str) -> bool {
    hook.get("type").and_then(serde_json::Value::as_str) == Some("command")
        && hook.get("command").and_then(serde_json::Value::as_str) == Some(command)
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
    fn session_hook_upsert_is_idempotent() {
        let mut settings = serde_json::json!({});
        assert!(upsert_session_hook(&mut settings, "mdd context"));
        assert!(!upsert_session_hook(&mut settings, "mdd context"));
        let groups = settings["hooks"]["SessionStart"].as_array().unwrap();
        let count = groups
            .iter()
            .filter(|group| group_has_command(group, "mdd context"))
            .count();
        assert_eq!(count, 1, "re-init must not duplicate the managed hook");
    }

    #[test]
    fn session_hook_preserves_user_settings() {
        let mut settings = serde_json::json!({
            "permissions": { "allow": ["Bash(ls)"] },
            "hooks": {
                "SessionStart": [
                    { "hooks": [ { "type": "command", "command": "echo hi" } ] }
                ]
            }
        });
        assert!(upsert_session_hook(&mut settings, "mdd context"));
        let groups = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(groups.len(), 2, "our hook is added alongside the user's");

        // strip removes only ours, keeping the user's hook and permissions
        assert!(strip_session_hook(&mut settings, "mdd context"));
        assert!(settings.get("permissions").is_some());
        let groups = settings["hooks"]["SessionStart"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert!(group_has_command(&groups[0], "echo hi"));
        assert!(!group_has_command(&groups[0], "mdd context"));
    }

    #[test]
    fn session_hook_strip_to_empty_clears_object() {
        let mut settings = serde_json::json!({});
        upsert_session_hook(&mut settings, "mdd context");
        assert!(strip_session_hook(&mut settings, "mdd context"));
        assert_eq!(
            settings,
            serde_json::json!({}),
            "an mdd-only settings object empties out so the file can be removed"
        );
        // stripping again is a no-op
        assert!(!strip_session_hook(&mut settings, "mdd context"));
    }

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
        // USE-INIT-RALPH: the Ralph workspace is scaffolded by init.
        assert!(dir.path().join(".claude/skills/mdd-ralph/SKILL.md").is_file());
        assert!(dir.path().join(".codex/skills/mdd-ralph/SKILL.md").is_file());
        assert!(dir.path().join(".mdd/ralph").is_dir());
        assert!(dir.path().join(".mdd/ralph/PROMPT.md").is_file());
        assert!(dir.path().join(".mdd/ralph/PLAN.md").is_file());
        assert!(report.created.contains(&".mdd/ralph/PLAN.md".to_string()));
    }

    #[test]
    fn init_seeds_ralph_plan_once_force_never_overwrites() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();

        // A populated plan stands in for one anything else has written.
        let plan = dir.path().join(".mdd/ralph/PLAN.md");
        let populated = "# Ralph plan\n\n## Items\n- [ ] ship the thing\n";
        fs::write(&plan, populated).unwrap();

        // SeededOnce: even --force (always-Overwrite handler) leaves it intact,
        // reported skipped, never overwritten — unlike a Regenerable file.
        let report = project
            .init_with_conflict_handler(|_| Ok(InitFileConflict::Overwrite))
            .unwrap();
        assert!(report.skipped.contains(&".mdd/ralph/PLAN.md".to_string()));
        assert!(!report.overwritten.contains(&".mdd/ralph/PLAN.md".to_string()));
        assert_eq!(fs::read_to_string(&plan).unwrap(), populated);

        // PROMPT.md is Regenerable, so --force does overwrite it.
        assert!(report
            .overwritten
            .contains(&".mdd/ralph/PROMPT.md".to_string()));
    }

    #[test]
    fn reinit_never_overwrites_populated_trace() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();

        // Simulate accumulated state from prior cycles.
        let populated = "version: 1\nlinks:\n  - from: USE-LOGIN\n    to: SEQ-LOGIN\n    relation: realizes\ngenerated_tests: []\ngenerated_ui_tests: []\nsource_links: []\n";
        let trace = dir.path().join(".mdd/trace.yml");
        fs::write(&trace, populated).unwrap();

        // A re-init must leave the populated state untouched and report it
        // as skipped — never overwritten with the empty default.
        let report = project.init().unwrap();
        assert!(report.skipped.contains(&".mdd/trace.yml".to_string()));
        assert!(!report.overwritten.contains(&".mdd/trace.yml".to_string()));
        assert_eq!(fs::read_to_string(&trace).unwrap(), populated);
    }

    #[test]
    fn init_migrates_older_state_file_forward() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".mdd")).unwrap();
        // An older config: version 0, with no security/traceability blocks.
        let old = "version: 0\nmodel_source: plantuml\nconstraint_source: ocl\nrendered_dir: .mdd/rendered\n";
        let cfg_path = dir.path().join(".mdd/config.yml");
        fs::write(&cfg_path, old).unwrap();

        let project = Project::at(dir.path());
        let report = project.init().unwrap();

        assert!(report.migrated.contains(&".mdd/config.yml".to_string()));
        assert!(!report.overwritten.contains(&".mdd/config.yml".to_string()));
        let cfg = project.read_config().unwrap();
        assert_eq!(
            cfg.version,
            MddConfig::default().version,
            "version bumped to current schema"
        );
        let raw = fs::read_to_string(&cfg_path).unwrap();
        assert!(raw.contains("model_source: plantuml"), "prior content preserved");
        assert!(
            raw.contains("parity_check"),
            "fields added since the file was written are materialized from defaults"
        );
        assert!(
            raw.contains("test:"),
            "the v2 test: block is materialized by forward migration"
        );
    }

    #[test]
    fn init_migration_preserves_accumulated_trace_content() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".mdd")).unwrap();
        let old = "version: 0\nlinks:\n  - from: USE-X\n    to: SEQ-X\n    relation: realizes\nsource_links:\n  - model_id: DOM-X\n    path: src/x.rs\n";
        fs::write(dir.path().join(".mdd/trace.yml"), old).unwrap();

        let project = Project::at(dir.path());
        let report = project.init().unwrap();

        assert!(report.migrated.contains(&".mdd/trace.yml".to_string()));
        let trace = project.read_trace().unwrap();
        assert_eq!(trace.version, 1);
        assert_eq!(trace.links.len(), 1);
        assert_eq!(trace.links[0].from, "USE-X");
        assert_eq!(trace.source_links.len(), 1);
        assert_eq!(trace.source_links[0].model_id, "DOM-X");
    }

    #[test]
    fn init_leaves_current_version_state_untouched_and_idempotent() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap(); // creates state files at the current version
        let cfg_path = dir.path().join(".mdd/config.yml");
        let before = fs::read_to_string(&cfg_path).unwrap();

        let report = project.init().unwrap(); // second run
        let after = fs::read_to_string(&cfg_path).unwrap();

        assert_eq!(before, after, "current-version file is not rewritten");
        assert!(report.migrated.is_empty());
        assert!(report.skipped.contains(&".mdd/config.yml".to_string()));
    }

    #[test]
    fn init_never_downgrades_newer_state_file() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".mdd")).unwrap();
        // A file written by a tool newer than us (version far ahead).
        let newer = "version: 999\nmodel_source: plantuml\nconstraint_source: ocl\nrendered_dir: .mdd/rendered\n";
        let cfg_path = dir.path().join(".mdd/config.yml");
        fs::write(&cfg_path, newer).unwrap();

        let project = Project::at(dir.path());
        let report = project.init().unwrap();

        assert_eq!(fs::read_to_string(&cfg_path).unwrap(), newer, "left untouched");
        assert!(!report.migrated.contains(&".mdd/config.yml".to_string()));
        assert!(report.skipped.contains(&".mdd/config.yml".to_string()));
    }

    #[test]
    fn force_overwrite_handler_never_reaches_state_files() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();
        // Distinctive accumulated content at the current version.
        let trace_path = dir.path().join(".mdd/trace.yml");
        let kept = "version: 1\nlinks:\n  - from: USE-KEEP\n    to: SEQ-KEEP\n    relation: realizes\ngenerated_tests: []\ngenerated_ui_tests: []\nsource_links: []\n";
        fs::write(&trace_path, kept).unwrap();

        // The --force path supplies an always-Overwrite handler.
        let report = project
            .init_with_conflict_handler(|_| Ok(InitFileConflict::Overwrite))
            .unwrap();

        let trace = project.read_trace().unwrap();
        assert_eq!(trace.links.len(), 1, "state file not clobbered to default");
        assert_eq!(trace.links[0].from, "USE-KEEP");
        assert!(!report.overwritten.contains(&".mdd/trace.yml".to_string()));
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
    fn validate_allows_same_ui_contract_id_across_current_and_objective() {
        // Cycle 0011: UIC- uniqueness is per-side, matching USE-/DOM-/CMP-/...
        // The same logical UI element naturally appears on both sides
        // when current/ mirrors an implemented objective/ contract.
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_login_mockup(&project, "MCK-LOGIN-FORM", "UIC-LOGIN-SUBMIT", "/login");
        fs::write(
            dir.path()
                .join(".mdd/models/objective/mockups/login.puml"),
            "@startsalt\n' @id(MCK-LOGIN-FORM)\n' @ref(USE-LOGIN)\n' @ui-route(/login)\n' @ui-viewport(desktop,1280,720)\n' @ui-element(UIC-LOGIN-SUBMIT, role=button, name=\"Log in\", required=true)\n{\n  [Log in]\n}\n@endsalt\n",
        )
        .unwrap();
        write_ui_test_trace_link(&project, "MCK-LOGIN-FORM");

        let report = project.validate().unwrap();

        assert!(
            !report
                .errors
                .iter()
                .any(|error| error.contains("duplicate UI contract ID UIC-LOGIN-SUBMIT")),
            "expected no duplicate-UIC error across sides, got: {:?}",
            report.errors,
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

    /// Write an `Open` cycle manifest carrying a parity `scope`, so
    /// `review()` narrows to that slice (greenfield-kickoff Cycle A).
    fn write_open_cycle_with_scope(project: &Project, number: u32, scope: &[&str]) {
        let dir = project.root().join(format!(".mdd/cycles/{number:04}"));
        fs::create_dir_all(dir.join("before")).unwrap();
        let scope_yaml: String = scope.iter().map(|s| format!("  - {s}\n")).collect();
        fs::write(
            dir.join("manifest.yml"),
            format!(
                "number: {number}\nslug: test-slice\nentry: generate\nstatus: open\nscope:\n{scope_yaml}"
            ),
        )
        .unwrap();
    }

    #[test]
    fn review_scope_excludes_out_of_scope_gaps() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/checkout.puml"),
            "@startuml\n' @id(USE-CHECKOUT)\nusecase \"Check out\" as Checkout\n@enduml\n",
        )
        .unwrap();

        // Whole-model (no open cycle): USE-CHECKOUT is a gap -> mismatch.
        let whole = project.review().unwrap();
        assert!(!whole.matched);
        assert_eq!(whole.missing_ids, vec!["USE-CHECKOUT".to_string()]);
        assert!(whole.scope.is_empty());

        // Open a realize-slice cycle scoped to USE-LOGIN (already covered).
        // The out-of-scope USE-CHECKOUT gap is now expected -> parity matches.
        write_open_cycle_with_scope(&project, 1, &["USE-LOGIN"]);
        let scoped = project.review().unwrap();
        assert!(scoped.matched, "missing: {:?}", scoped.missing_ids);
        assert!(scoped.missing_ids.is_empty());
        assert_eq!(scoped.scope, vec!["USE-LOGIN".to_string()]);
        assert!(scoped.diff_puml_paths.is_empty());
    }

    #[test]
    fn review_scope_still_blocks_in_scope_gap() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\nusecase \"Log in\" as Login\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/checkout.puml"),
            "@startuml\n' @id(USE-CHECKOUT)\nusecase \"Check out\" as Checkout\n@enduml\n",
        )
        .unwrap();

        // The scope names the gap itself, so it still blocks closure.
        write_open_cycle_with_scope(&project, 1, &["USE-CHECKOUT"]);
        let report = project.review().unwrap();
        assert!(!report.matched);
        assert_eq!(report.missing_ids, vec!["USE-CHECKOUT".to_string()]);
        assert_eq!(report.scope, vec!["USE-CHECKOUT".to_string()]);
        assert_eq!(report.diff_puml_paths.len(), 1);
    }

    #[test]
    fn review_security_scopes_to_open_cycle_scope() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_security_parity_mismatch_fixture(&project);

        // Whole-model: the objective requires a guard on USE-LOGIN the current
        // side does not enforce -> security mismatch blocks (error mode).
        let whole = project.review().unwrap();
        assert!(whole.ids_matched);
        assert!(!whole.security.matched);
        assert!(!whole.matched);

        // A realize-slice cycle scoped to DOM-USER does not touch the
        // USE-LOGIN guard, so the out-of-scope marker is not enforced here.
        write_open_cycle_with_scope(&project, 1, &["DOM-USER"]);
        let scoped = project.review().unwrap();
        assert!(scoped.ids_matched, "missing: {:?}", scoped.missing_ids);
        assert!(
            scoped.security.matched,
            "out-of-scope guard must not block a scoped cycle"
        );
        assert!(scoped.matched);
    }

    #[test]
    fn extract_scope_ids_parses_comma_list() {
        let plan = "# Ralph plan\n- [ ] foo @scope(USE-A, SEQ-B)\n- [ ] bar\n- [x] baz @scope(DOM-C)\n";
        let ids = extract_scope_ids(plan).unwrap();
        assert_eq!(
            ids,
            vec!["DOM-C".to_string(), "SEQ-B".to_string(), "USE-A".to_string()]
        );
    }

    #[test]
    fn validate_warns_on_plan_scope_gaps_when_scope_present() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        // Objective gains USE-LOGIN (implementation-bearing); the PLAN names an
        // unknown scope id and does not cover USE-LOGIN.
        fs::write(
            dir.path().join(".mdd/models/objective/use-cases/login.puml"),
            "@startuml\n' @id(USE-LOGIN)\nusecase \"Log in\" as L\n@enduml\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(".mdd/ralph/PLAN.md"),
            "# Ralph plan\n- [ ] do a thing @scope(USE-NOPE)\n",
        )
        .unwrap();

        let report = project.validate().unwrap();

        assert!(report.ok, "scope rules are WARNING, never blocking");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("unknown objective @id USE-NOPE")),
            "warnings: {:?}",
            report.warnings
        );
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("does not cover objective @id USE-LOGIN")),
            "warnings: {:?}",
            report.warnings
        );
    }

    #[test]
    fn validate_silent_on_plan_without_scope() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        // Seed PLAN with no @scope (the default): both kickoff rules are inert.
        let report = project.validate().unwrap();
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.contains("@scope")),
            "no @scope in PLAN must produce no scope warnings: {:?}",
            report.warnings
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

    /// Overwrite config.yml with a `test:` block (gate + the named layers,
    /// each given a same-named framework) for the coverage/membership tests.
    fn write_test_config(project: &Project, gate: &str, layers: &[&str]) {
        let mut yaml = format!(
            "version: 2\nmodel_source: plantuml\nconstraint_source: ocl\nrendered_dir: .mdd/rendered\ntest:\n  gate: {gate}\n"
        );
        if layers.is_empty() {
            yaml.push_str("  layers: {}\n");
        } else {
            yaml.push_str("  layers:\n");
            for layer in layers {
                yaml.push_str(&format!("    {layer}:\n      framework: {layer}-runner\n"));
            }
        }
        fs::write(project.root().join(".mdd/config.yml"), yaml).unwrap();
    }

    #[test]
    fn unified_tests_projects_legacy_arrays() {
        let mut trace = Trace::default();
        trace.tests.push(TestLink {
            id: "UT-DOM-USER".to_string(),
            path: "src/user.rs".to_string(),
            model_id: "DOM-USER".to_string(),
            layer: TestLayer::Unit,
            framework: Some("cargo-test".to_string()),
            expect: TestExpect::RedUntilImplemented,
        });
        trace.generated_tests.push(GeneratedTest {
            id: "AT-USE-LOGIN".to_string(),
            path: ".mdd/tests/acceptance/use-login.feature".to_string(),
            model_id: "USE-LOGIN".to_string(),
            category: None,
        });
        trace.generated_tests.push(GeneratedTest {
            id: "SECT-SEC-X".to_string(),
            path: ".mdd/tests/acceptance/sec-x.feature".to_string(),
            model_id: "SEC-X".to_string(),
            category: Some("security".to_string()),
        });
        trace.generated_ui_tests.push(GeneratedUiTest {
            id: "UIT-MCK-LOGIN".to_string(),
            path: ".mdd/tests/ui/login.spec.ts".to_string(),
            model_id: "MCK-LOGIN".to_string(),
            framework: "playwright".to_string(),
        });

        let unified = trace.unified_tests();
        assert_eq!(unified.len(), 4);
        // Native link preserved verbatim.
        assert!(unified.iter().any(|t| t.id == "UT-DOM-USER"
            && t.layer == TestLayer::Unit
            && t.expect == TestExpect::RedUntilImplemented));
        // Legacy acceptance (no category) -> acceptance layer.
        assert!(unified
            .iter()
            .any(|t| t.id == "AT-USE-LOGIN" && t.layer == TestLayer::Acceptance));
        // category=security -> security layer.
        assert!(unified
            .iter()
            .any(|t| t.id == "SECT-SEC-X" && t.layer == TestLayer::Security));
        // UI test -> ui layer with its framework.
        assert!(unified.iter().any(|t| t.id == "UIT-MCK-LOGIN"
            && t.layer == TestLayer::Ui
            && t.framework.as_deref() == Some("playwright")));
    }

    #[test]
    fn validate_coverage_inert_without_configured_layers() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);

        let report = project.validate().unwrap();

        assert!(report.ok, "errors: {:?}", report.errors);
        assert!(
            !report.warnings.iter().any(|w| w.contains("no linked")),
            "coverage must be inert with no configured layers: {:?}",
            report.warnings
        );
    }

    #[test]
    fn validate_coverage_warns_when_layers_configured_and_gate_warn() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_test_config(&project, "warn", &["unit", "integration", "e2e"]);

        let report = project.validate().unwrap();

        assert!(report.ok, "warn gate never blocks: {:?}", report.errors);
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("DOM-USER") && w.contains("unit")));
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("SEQ-LOGIN") && w.contains("integration")));
    }

    #[test]
    fn validate_coverage_blocks_when_layers_configured_and_gate_error() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        write_test_config(&project, "error", &["unit", "integration", "e2e"]);

        let report = project.validate().unwrap();

        assert!(!report.ok, "uncovered ids must block under gate=error");
        assert!(report
            .errors
            .iter()
            .any(|e| e.contains("DOM-USER") && e.contains("no linked unit test")));
    }

    #[test]
    fn validate_rejects_test_link_with_unconfigured_layer() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        // A real file the link can point at.
        let test_path = ".mdd/tests/acceptance/user.feature";
        fs::write(dir.path().join(test_path), "Feature: x\n").unwrap();
        let mut trace = project.read_trace().unwrap();
        trace.tests.push(TestLink {
            id: "SECT-DOM-USER".to_string(),
            path: test_path.to_string(),
            model_id: "DOM-USER".to_string(),
            layer: TestLayer::Security,
            framework: None,
            expect: TestExpect::Pass,
        });
        project.write_trace(&trace).unwrap();
        // Profile configures only `unit` — `security` is not a member.
        write_test_config(&project, "warn", &["unit"]);

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|e| e.contains("SECT-DOM-USER") && e.contains("not in the configured test profile")));
    }

    #[test]
    fn validate_rejects_test_link_with_missing_file() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        let mut trace = project.read_trace().unwrap();
        trace.tests.push(TestLink {
            id: "UT-DOM-USER".to_string(),
            path: "crates/does/not/exist.rs".to_string(),
            model_id: "DOM-USER".to_string(),
            layer: TestLayer::Unit,
            framework: None,
            expect: TestExpect::Pass,
        });
        project.write_trace(&trace).unwrap();

        let report = project.validate().unwrap();

        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|e| e.contains("test file is missing") && e.contains("exist.rs")));
    }

    #[test]
    fn resolve_ui_framework_prefers_profile_over_playwright_default() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();

        // No `ui` layer configured -> playwright fallback.
        assert_eq!(project.resolve_ui_framework().unwrap(), "playwright");

        // A configured `ui` layer framework wins.
        let yaml = "version: 2\nmodel_source: plantuml\nconstraint_source: ocl\nrendered_dir: .mdd/rendered\ntest:\n  gate: error\n  layers:\n    ui:\n      framework: cypress\n";
        fs::write(dir.path().join(".mdd/config.yml"), yaml).unwrap();
        assert_eq!(project.resolve_ui_framework().unwrap(), "cypress");
    }

    #[test]
    fn test_plan_resolves_configured_layers_and_flags_gaps() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        // Configure unit + integration with commands.
        let yaml = "version: 2\nmodel_source: plantuml\nconstraint_source: ocl\nrendered_dir: .mdd/rendered\ntest:\n  gate: error\n  layers:\n    unit:\n      framework: cargo-test\n      command: cargo test --lib\n    integration:\n      framework: cargo-test\n      command: cargo test --test '*'\n";
        fs::write(dir.path().join(".mdd/config.yml"), yaml).unwrap();
        // A unit test (gap) and an integration test (not gap); plus an unconfigured ui test.
        fs::write(dir.path().join("u.rs"), "// t\n").unwrap();
        fs::write(dir.path().join("i.rs"), "// t\n").unwrap();
        fs::write(dir.path().join("x.spec.ts"), "// t\n").unwrap();
        let mut trace = project.read_trace().unwrap();
        trace.tests.push(TestLink {
            id: "IT-SEQ-LOGIN".to_string(),
            path: "i.rs".to_string(),
            model_id: "SEQ-LOGIN".to_string(),
            layer: TestLayer::Integration,
            framework: Some("cargo-test".to_string()),
            expect: TestExpect::Pass,
        });
        trace.tests.push(TestLink {
            id: "UT-DOM-USER".to_string(),
            path: "u.rs".to_string(),
            model_id: "DOM-USER".to_string(),
            layer: TestLayer::Unit,
            framework: Some("cargo-test".to_string()),
            expect: TestExpect::RedUntilImplemented,
        });
        trace.tests.push(TestLink {
            id: "UIT-MCK-X".to_string(),
            path: "x.spec.ts".to_string(),
            model_id: "DOM-USER".to_string(),
            layer: TestLayer::Ui,
            framework: Some("playwright".to_string()),
            expect: TestExpect::Pass,
        });
        project.write_trace(&trace).unwrap();

        let plan = project.test_plan().unwrap();
        // ui layer is not configured -> excluded; ordered unit before integration.
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].id, "UT-DOM-USER");
        assert_eq!(plan[0].layer, "unit");
        assert_eq!(plan[0].command, "cargo test --lib");
        assert!(plan[0].is_gap, "red-until-implemented step is a gap");
        assert_eq!(plan[1].id, "IT-SEQ-LOGIN");
        assert!(!plan[1].is_gap);
    }

    #[test]
    fn detect_test_profile_recommends_from_build_files_and_flags_ambiguity() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname='x'\n").unwrap();
        fs::write(
            dir.path().join("package.json"),
            "{\"devDependencies\":{\"vitest\":\"1\",\"@playwright/test\":\"1\"}}",
        )
        .unwrap();

        let profile = project.detect_test_profile().unwrap();
        assert!(profile
            .recommendations
            .iter()
            .any(|r| r.layer == "unit" && r.framework == "cargo-test"));
        assert!(profile
            .recommendations
            .iter()
            .any(|r| r.layer == "ui" && r.framework == "playwright"));
        // Two unit frameworks (cargo-test + vitest) => a blocking ambiguity.
        assert!(profile
            .ambiguities
            .iter()
            .any(|a| a.contains("multiple unit-test frameworks")));
    }

    #[test]
    fn evaluate_green_gate_blocks_under_error_warns_under_warn() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();
        let results = vec![
            TestRunResult { id: "UT-A".to_string(), exit_code: 0 },
            TestRunResult { id: "UT-B".to_string(), exit_code: 1 },
        ];

        // Default gate is error -> a red test blocks.
        let rep = project.evaluate_green_gate(&results).unwrap();
        assert!(!rep.all_green);
        assert_eq!(rep.still_red, vec!["UT-B".to_string()]);
        assert!(rep.blocking);

        // gate=warn -> still red, but not blocking.
        let yaml = "version: 2\nmodel_source: plantuml\nconstraint_source: ocl\nrendered_dir: .mdd/rendered\ntest:\n  gate: warn\n  layers: {}\n";
        fs::write(dir.path().join(".mdd/config.yml"), yaml).unwrap();
        let rep = project.evaluate_green_gate(&results).unwrap();
        assert!(!rep.all_green);
        assert!(!rep.blocking, "warn gate never blocks");

        // all green -> not blocking regardless of gate.
        let green = vec![TestRunResult { id: "UT-A".to_string(), exit_code: 0 }];
        assert!(project.evaluate_green_gate(&green).unwrap().all_green);
    }

    fn phase(result: TestPhaseResult, exit: i32) -> PhaseRecord {
        PhaseRecord {
            command: "cargo test x".to_string(),
            exit_code: exit,
            result,
            excerpt: "…".to_string(),
            at: "2026-05-25T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn red_green_gate_satisfied_only_on_fail_then_pass() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        let evidence = TestEvidence {
            version: 1,
            cycle: "0025".to_string(),
            gap_tests: vec![GapTest {
                id: "UT-DOM-USER".to_string(),
                model_id: "DOM-USER".to_string(),
                layer: TestLayer::Unit,
                red: Some(phase(TestPhaseResult::Fail, 101)),
                green: Some(phase(TestPhaseResult::Pass, 0)),
            }],
        };
        let gap = vec!["UT-DOM-USER".to_string()];
        let rep = project.evaluate_red_green_gate(Some(&evidence), &gap);
        assert!(rep.satisfied, "fail-then-pass satisfies the gate");

        // Empty gap set is vacuously satisfied (pure refactor).
        assert!(project.evaluate_red_green_gate(None, &[]).satisfied);

        // Missing evidence for a gap blocks.
        let rep = project.evaluate_red_green_gate(None, &gap);
        assert!(!rep.satisfied);
        assert_eq!(rep.missing_evidence, gap);
    }

    #[test]
    fn red_green_gate_flags_vacuous_red_and_still_red() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        // red recorded as pass (vacuous) + green missing.
        let evidence = TestEvidence {
            version: 1,
            cycle: "0025".to_string(),
            gap_tests: vec![GapTest {
                id: "UT-X".to_string(),
                model_id: "DOM-USER".to_string(),
                layer: TestLayer::Unit,
                red: Some(phase(TestPhaseResult::Pass, 0)),
                green: None,
            }],
        };
        let rep = project.evaluate_red_green_gate(Some(&evidence), &["UT-X".to_string()]);
        assert!(!rep.satisfied);
        assert_eq!(rep.not_red_first, vec!["UT-X".to_string()]);
        assert_eq!(rep.still_red, vec!["UT-X".to_string()]);
    }

    #[test]
    fn validate_rejects_malformed_red_phase_in_evidence() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        write_minimal_valid_models(&project);
        let cdir = dir.path().join(".mdd/cycles/0025");
        fs::create_dir_all(&cdir).unwrap();
        // red recorded as a PASS — illegal shape (a red must fail).
        let yaml = "version: 1\ncycle: '0025'\ngap_tests:\n  - id: UT-DOM-USER\n    model_id: DOM-USER\n    layer: unit\n    red:\n      command: cargo test\n      exit_code: 0\n      result: pass\n";
        fs::write(cdir.join("test-evidence.yml"), yaml).unwrap();

        let report = project.validate().unwrap();
        assert!(!report.ok);
        assert!(report
            .errors
            .iter()
            .any(|e| e.contains("red phase must be a failure") && e.contains("0025")));
    }

    #[test]
    fn read_test_evidence_absent_is_none() {
        let dir = tempdir().unwrap();
        let project = Project::at(dir.path());
        project.init().unwrap();
        assert!(project.read_test_evidence("0099").unwrap().is_none());
    }
}
