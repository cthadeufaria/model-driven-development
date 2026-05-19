use anyhow::{Context, Result, bail};
use mdd_core::{Project, RenderTree};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Default)]
pub struct RenderReport {
    /// Project-relative paths successfully written.
    pub rendered: Vec<String>,
    /// `path: message` for each source PlantUML failed to rasterize.
    /// Non-fatal: the engine renders the rest and reports these so the
    /// `/mdd-render` skill can triage and suggest fixes.
    pub diagnostics: Vec<String>,
}

/// What `mdd render` should rasterize.
#[derive(Debug, Clone)]
pub enum RenderSelection {
    /// Full tree parity — exactly `Project::all_render_sources`.
    All,
    /// Only the named trees.
    Trees(Vec<RenderTree>),
    /// Explicit files or directories (dirs are walked for `*.puml` /
    /// `*.ocl`). The fuzzy-subset path the `/mdd-render` skill resolves.
    Paths(Vec<PathBuf>),
}

/// The single deterministic render engine. It enumerates sources via
/// the one `mdd_core::Project` tree set (OCL-RENDER-TREE-PARITY),
/// rasterizes each to its deterministic mirror path
/// (OCL-RENDER-PATH-MIRROR), and reports per-file diagnostics instead
/// of aborting the whole run on one bad diagram.
pub fn render_selection(project: &Project, selection: &RenderSelection) -> Result<RenderReport> {
    let sources: Vec<(bool, PathBuf)> = match selection {
        RenderSelection::All => project
            .all_render_sources()?
            .into_iter()
            .map(|(tree, path)| (tree == RenderTree::OclConstraints, path))
            .collect(),
        RenderSelection::Trees(trees) => {
            let mut out = Vec::new();
            for &tree in trees {
                let is_ocl = tree == RenderTree::OclConstraints;
                for path in project.render_sources(tree)? {
                    out.push((is_ocl, path));
                }
            }
            out
        }
        RenderSelection::Paths(paths) => expand_paths(paths),
    };

    if sources.is_empty() {
        return Ok(RenderReport::default());
    }

    let plantuml = PlantUmlCommand::resolve()?;
    let mut report = RenderReport::default();
    for (is_ocl, source_file) in sources {
        let raw = fs::read_to_string(&source_file)
            .with_context(|| format!("failed to read {}", source_file.display()))?;
        let puml = if is_ocl {
            let stem = source_file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("constraints");
            synthesize_ocl_puml(&raw, stem)
        } else {
            raw
        };

        let output_path = project.rendered_mirror_path(&source_file);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let rel_out = output_path
            .strip_prefix(project.root())
            .unwrap_or(&output_path)
            .to_string_lossy()
            .replace('\\', "/");

        match render_plantuml_to_svg(&puml, &plantuml) {
            Ok(svg) => {
                fs::write(&output_path, svg)
                    .with_context(|| format!("failed to write {}", output_path.display()))?;
                report.rendered.push(rel_out);
            }
            Err(error) => {
                let src_rel = source_file
                    .strip_prefix(project.root())
                    .unwrap_or(&source_file)
                    .to_string_lossy()
                    .replace('\\', "/");
                report.diagnostics.push(format!("{src_rel}: {error}"));
            }
        }
    }
    Ok(report)
}

/// Expand explicit path arguments: a directory is walked for `*.puml`
/// and `*.ocl`; a file is taken as-is. OCL-ness is decided by extension.
fn expand_paths(paths: &[PathBuf]) -> Vec<(bool, PathBuf)> {
    fn is_ocl(p: &Path) -> bool {
        p.extension().and_then(|e| e.to_str()) == Some("ocl")
    }
    let mut out = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut walked: Vec<PathBuf> = walkdir::WalkDir::new(path)
                .into_iter()
                .filter_map(std::result::Result::ok)
                .filter(|e| e.file_type().is_file())
                .map(walkdir::DirEntry::into_path)
                .filter(|p| {
                    p.to_str().is_some_and(|s| s.ends_with(".puml")) || is_ocl(p)
                })
                .collect();
            walked.sort();
            for p in walked {
                out.push((is_ocl(&p), p));
            }
        } else if path.is_file() {
            out.push((is_ocl(path), path.clone()));
        }
    }
    out
}

/// Rasterize every `.mdd/models/**/*.puml` (CMP-DIFF-RENDER). Thin
/// wrapper over [`render_selection`] so the tree set stays single-source.
pub fn render_project(project: &Project) -> Result<RenderReport> {
    let report = render_selection(project, &RenderSelection::Trees(vec![RenderTree::Models]))?;
    if report.rendered.is_empty() && report.diagnostics.is_empty() {
        bail!("no PlantUML model files found under .mdd/models");
    }
    Ok(report)
}

/// Rasterize every superposed `<diagram>.diff.puml` under `.mdd/cycles/`
/// to its deterministic `.mdd/rendered/cycles/<n>/<rel>.diff.svg` mirror
/// (CMP-DIFF-RENDER / SEQ-RENDER-DIFF-SVG). Invoked by the `/mdd-cycle`
/// close step and by `mdd render` / `/mdd-render`. An empty cycle store
/// is not an error — the viewer simply has no rendered diff to paint
/// until a cycle closes. Thin wrapper over [`render_selection`].
pub fn render_cycle_diffs(project: &Project) -> Result<RenderReport> {
    render_selection(project, &RenderSelection::Trees(vec![RenderTree::CycleDiffs]))
}

/// Synthesize a PlantUML "constraints" diagram from an OCL file: each
/// `context` is a node, its invariants become a note, and an arrow goes
/// to every `@ref(DOM-...)` it constrains. Pure string transform (no
/// PlantUML/IO) so it is unit-testable; the rasterization is done by
/// `render_ocl_diagrams` (CMP-OCL-RENDER / SEQ-VIEW-OCL).
pub fn synthesize_ocl_puml(ocl: &str, title: &str) -> String {
    fn marker<'a>(line: &'a str, tag: &str) -> Option<&'a str> {
        let start = line.find(tag)? + tag.len();
        let end = line[start..].find(')')? + start;
        Some(line[start..end].trim())
    }
    fn alias(prefix: &str, idx: usize) -> String {
        format!("{prefix}{idx}")
    }

    #[derive(Default)]
    struct Block {
        id: String,
        dom: Option<String>,
        ctx: Option<String>,
        body: Vec<String>,
    }
    let mut blocks: Vec<Block> = Vec::new();
    for raw in ocl.lines() {
        let line = raw.trim_end();
        let t = line.trim();
        if let Some(id) = marker(t, "@id(") {
            blocks.push(Block {
                id: id.to_string(),
                ..Block::default()
            });
            continue;
        }
        let Some(b) = blocks.last_mut() else {
            continue;
        };
        if let Some(r) = marker(t, "@ref(") {
            if r.starts_with("DOM-") {
                b.dom = Some(r.to_string());
            }
            continue;
        }
        if t.starts_with("--") {
            continue;
        }
        if let Some(rest) = t.strip_prefix("context ") {
            b.ctx = Some(rest.trim().to_string());
            continue;
        }
        if t.is_empty() {
            continue;
        }
        b.body.push(line.to_string());
    }

    let mut contexts: Vec<String> = Vec::new();
    let mut domains: Vec<String> = Vec::new();
    for b in &blocks {
        let c = b.ctx.clone().unwrap_or_else(|| "constraints".to_string());
        if !contexts.contains(&c) {
            contexts.push(c);
        }
        if let Some(d) = &b.dom
            && !domains.contains(d)
        {
            domains.push(d.clone());
        }
    }

    let mut out = String::new();
    out.push_str("@startuml\n");
    // The diagram mixes `class` (context) and `rectangle` (domain)
    // nodes; PlantUML requires this directive to allow that.
    out.push_str("allowmixing\n");
    out.push_str(&format!("title OCL constraints — {title}\n"));
    out.push_str("left to right direction\n");
    out.push_str("skinparam wrapWidth 480\n");
    out.push_str("skinparam classBackgroundColor #FFF8E1\n");
    out.push_str("skinparam rectangleBackgroundColor #E3F2FD\n");

    for (i, c) in contexts.iter().enumerate() {
        out.push_str(&format!(
            "class \"{}\" as {} <<context>>\n",
            c,
            alias("C", i)
        ));
    }
    for (i, d) in domains.iter().enumerate() {
        out.push_str(&format!(
            "rectangle \"{}\" as {} <<domain>>\n",
            d,
            alias("D", i)
        ));
    }

    for (i, c) in contexts.iter().enumerate() {
        out.push_str(&format!("note bottom of {}\n", alias("C", i)));
        for b in blocks
            .iter()
            .filter(|b| b.ctx.as_deref().unwrap_or("constraints") == c)
        {
            out.push_str(&format!("  {}\n", b.id));
            for l in &b.body {
                out.push_str(&format!("  {}\n", l.trim_end()));
            }
        }
        out.push_str("end note\n");
    }

    for b in &blocks {
        let c = b.ctx.clone().unwrap_or_else(|| "constraints".to_string());
        if let Some(d) = &b.dom {
            let ci = contexts.iter().position(|x| x == &c).unwrap_or(0);
            let di = domains.iter().position(|x| x == d).unwrap_or(0);
            out.push_str(&format!(
                "{} ..> {} : {}\n",
                alias("C", ci),
                alias("D", di),
                b.id
            ));
        }
    }
    out.push_str("@enduml\n");
    out
}

/// Rasterize a synthesized constraints diagram for every `.ocl` file to
/// its `.mdd/rendered/constraints/<name>.svg` mirror, so the viewer's
/// OCL Diagram sub-mode can paint it. Invoked by `/mdd-render` and the
/// cycle close. An empty constraint set is not an error.
pub fn render_ocl_diagrams(project: &Project) -> Result<RenderReport> {
    render_selection(
        project,
        &RenderSelection::Trees(vec![RenderTree::OclConstraints]),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlantUmlCommand {
    JavaJar { jar_path: PathBuf },
    PathExecutable,
}

impl PlantUmlCommand {
    pub fn resolve() -> Result<Self> {
        Self::resolve_from(
            env::var_os("MDD_PLANTUML_JAR"),
            bundled_jar_candidates(),
            command_exists_on_path("plantuml"),
        )
    }

    fn resolve_from(
        env_jar: Option<OsString>,
        bundled_candidates: Vec<PathBuf>,
        plantuml_on_path: bool,
    ) -> Result<Self> {
        if let Some(value) = env_jar {
            if value.is_empty() {
                bail!("MDD_PLANTUML_JAR is set but empty");
            }

            let jar_path = PathBuf::from(value);
            if jar_path.is_file() {
                return Ok(Self::JavaJar { jar_path });
            }

            bail!(
                "MDD_PLANTUML_JAR points to {}, but that file does not exist",
                jar_path.display()
            );
        }

        for jar_path in bundled_candidates {
            if jar_path.is_file() {
                return Ok(Self::JavaJar { jar_path });
            }
        }

        if plantuml_on_path {
            return Ok(Self::PathExecutable);
        }

        bail!(
            "PlantUML is not available. Install the bundled mdd package with share/mdd/plantuml.jar and Java/OpenJDK, set MDD_PLANTUML_JAR=/path/to/plantuml.jar, or install `plantuml` on PATH."
        );
    }

    fn command(&self) -> Command {
        match self {
            Self::JavaJar { jar_path } => {
                let mut command = Command::new("java");
                command.arg("-jar").arg(jar_path).arg("-tsvg").arg("-pipe");
                command
            }
            Self::PathExecutable => {
                let mut command = Command::new("plantuml");
                command.arg("-tsvg").arg("-pipe");
                command
            }
        }
    }

    fn spawn_error(&self) -> String {
        match self {
            Self::JavaJar { jar_path } => format!(
                "failed to start `java` for PlantUML jar {}; install Java/OpenJDK or add `java` to PATH",
                jar_path.display()
            ),
            Self::PathExecutable => {
                "failed to start `plantuml`; install PlantUML or add it to PATH".to_string()
            }
        }
    }

    fn failure_label(&self) -> String {
        match self {
            Self::JavaJar { jar_path } => format!("java -jar {}", jar_path.display()),
            Self::PathExecutable => "plantuml".to_string(),
        }
    }
}

fn render_plantuml_to_svg(source: &str, plantuml: &PlantUmlCommand) -> Result<Vec<u8>> {
    let mut child = plantuml
        .command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| plantuml.spawn_error())?;

    child
        .stdin
        .as_mut()
        .context("failed to open plantuml stdin")?
        .write_all(source.as_bytes())
        .context("failed to write PlantUML source to plantuml")?;

    let output = child
        .wait_with_output()
        .context("failed to wait for plantuml")?;
    if !output.status.success() {
        bail!(
            "{} failed:\n{}",
            plantuml.failure_label(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn bundled_jar_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(exe) = env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        candidates.push(exe_dir.join("../share/mdd/plantuml.jar"));
        candidates.push(exe_dir.join("plantuml.jar"));
        candidates.push(exe_dir.join("share/mdd/plantuml.jar"));
    }

    candidates
}

fn command_exists_on_path(command: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| {
            env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(command);
                is_executable_file(&candidate)
            })
        })
        .unwrap_or(false)
}

#[cfg(unix)]
fn is_executable_file(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.is_file()
        && path
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &std::path::Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolver_prefers_env_jar() {
        let dir = tempdir().unwrap();
        let env_jar = dir.path().join("env.jar");
        let bundled_jar = dir.path().join("bundled.jar");
        fs::write(&env_jar, "fake jar").unwrap();
        fs::write(&bundled_jar, "fake jar").unwrap();

        let command = PlantUmlCommand::resolve_from(
            Some(env_jar.clone().into_os_string()),
            vec![bundled_jar],
            true,
        )
        .unwrap();

        assert_eq!(command, PlantUmlCommand::JavaJar { jar_path: env_jar });
    }

    #[test]
    fn resolver_finds_bundled_jar() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing.jar");
        let bundled_jar = dir.path().join("plantuml.jar");
        fs::write(&bundled_jar, "fake jar").unwrap();

        let command =
            PlantUmlCommand::resolve_from(None, vec![missing, bundled_jar.clone()], true).unwrap();

        assert_eq!(
            command,
            PlantUmlCommand::JavaJar {
                jar_path: bundled_jar
            }
        );
    }

    #[test]
    fn resolver_falls_back_to_plantuml_on_path() {
        let dir = tempdir().unwrap();
        let command =
            PlantUmlCommand::resolve_from(None, vec![dir.path().join("missing.jar")], true)
                .unwrap();

        assert_eq!(command, PlantUmlCommand::PathExecutable);
    }

    #[test]
    fn resolver_errors_clearly_when_nothing_is_available() {
        let err = PlantUmlCommand::resolve_from(
            None,
            vec![PathBuf::from("/missing/plantuml.jar")],
            false,
        )
        .unwrap_err();
        let message = err.to_string();

        assert!(message.contains("PlantUML is not available"));
        assert!(message.contains("MDD_PLANTUML_JAR"));
        assert!(message.contains("plantuml"));
    }

    #[test]
    fn synthesize_ocl_puml_emits_context_domain_and_notes() {
        let ocl = "-- @id(OCL-A)\n-- @ref(DOM-X)\ncontext Foo\ninv One: self.a > 0\n\n\
                   -- @id(OCL-B)\n-- @ref(DOM-X)\ncontext Foo\ninv Two: self.b <> ''\n";
        let out = synthesize_ocl_puml(ocl, "sample");
        assert!(out.starts_with("@startuml\n"));
        // class + rectangle mix requires the allowmixing directive
        assert!(out.contains("\nallowmixing\n"));
        assert!(out.contains("title OCL constraints — sample"));
        assert!(out.contains("class \"Foo\" as C0 <<context>>"));
        assert!(out.contains("rectangle \"DOM-X\" as D0 <<domain>>"));
        // both invariants aggregate under the single Foo context note
        assert!(out.contains("OCL-A"));
        assert!(out.contains("inv One: self.a > 0"));
        assert!(out.contains("OCL-B"));
        assert!(out.contains("C0 ..> D0 : OCL-A"));
        assert!(out.trim_end().ends_with("@enduml"));
        // one context node only (deduped)
        assert_eq!(out.matches("<<context>>").count(), 1);
    }
}
