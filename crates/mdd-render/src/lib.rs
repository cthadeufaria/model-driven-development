use anyhow::{Context, Result, bail};
use mdd_core::Project;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct RenderReport {
    pub rendered: Vec<String>,
}

pub fn render_project(project: &Project) -> Result<RenderReport> {
    let model_files = project.model_files()?;
    if model_files.is_empty() {
        bail!("no PlantUML model files found under .mdd/models");
    }

    let plantuml = PlantUmlCommand::resolve()?;
    let mut rendered = Vec::new();
    for model_file in model_files {
        let source = fs::read_to_string(&model_file)
            .with_context(|| format!("failed to read {}", model_file.display()))?;
        let relative = model_file
            .strip_prefix(project.root())
            .with_context(|| format!("{} is outside project root", model_file.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        let output_path = project.rendered_svg_path(&relative);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let svg = render_plantuml_to_svg(&source, &plantuml)?;
        fs::write(&output_path, svg)
            .with_context(|| format!("failed to write {}", output_path.display()))?;
        rendered.push(
            output_path
                .strip_prefix(project.root())
                .unwrap_or(&output_path)
                .to_string_lossy()
                .replace('\\', "/"),
        );
    }

    Ok(RenderReport { rendered })
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
}
