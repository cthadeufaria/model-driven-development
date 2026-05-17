use anyhow::{Context, Result, bail};
use mdd_core::Project;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy)]
pub enum AgentKind {
    Cursor,
    Codex,
    Claude,
    Generic,
}

#[derive(Debug, Clone)]
pub struct AgentPreparationReport {
    pub instruction_file: String,
    pub hooks_run: Vec<String>,
}

impl AgentKind {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "cursor" => Ok(Self::Cursor),
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            "generic" => Ok(Self::Generic),
            other => bail!("unknown agent `{other}`; expected cursor, codex, claude, or generic"),
        }
    }

    fn file_stem(self) -> &'static str {
        match self {
            Self::Cursor => "cursor",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Generic => "generic",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Cursor => "Cursor",
            Self::Codex => "Codex",
            Self::Claude => "Claude",
            Self::Generic => "Generic agent",
        }
    }
}

pub fn prepare_agent(project: &Project, kind: AgentKind) -> Result<AgentPreparationReport> {
    let instruction_file = write_agent_instructions(project, kind)?;
    let hooks_run = run_pre_code_hooks(project)?;
    Ok(AgentPreparationReport {
        instruction_file,
        hooks_run,
    })
}

fn write_agent_instructions(project: &Project, kind: AgentKind) -> Result<String> {
    let path = project
        .root()
        .join(".mdd/docs")
        .join(format!("{}-mdd.md", kind.file_stem()));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let content = format!(
        "# Model-Driven Development Instructions for {}\n\n\
         You are working in a repository governed by `mdd`.\n\n\
         Required rules:\n\
         - Load `.mdd/docs/mdd-workflow.md` and `.mdd/docs/uml-and-ocl-guide.md` first.\n\
         - Use the project-local MDD skills installed under `.claude/skills` or `.codex/skills` when available.\n\
         - Treat `.mdd/models` and `.mdd/constraints` as the approved source of truth.\n\
         - Do not implement behavior that lacks a traceable model element.\n\
         - Keep source changes linked to `.mdd/trace.yml`.\n\
         - Preserve generated acceptance tests unless the approved model changes first.\n\
         - If implementation requires behavior not present in the model, stop and update the model before coding.\n\n\
         Start by reading:\n\
         - `.mdd/trace.yml`\n\
         - `.mdd/approvals.yml`\n\
         - `.mdd/tests/acceptance/`\n",
        kind.label()
    );
    fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(relative_to_project(project, path))
}

fn run_pre_code_hooks(project: &Project) -> Result<Vec<String>> {
    let hooks_dir = project.root().join(".mdd/hooks");
    if !hooks_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut hooks = fs::read_dir(&hooks_dir)
        .with_context(|| format!("failed to read {}", hooks_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("pre-code"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    hooks.sort();

    let mut hooks_run = Vec::new();
    for hook in hooks {
        let status = Command::new(&hook)
            .current_dir(project.root())
            .status()
            .with_context(|| format!("failed to run hook {}", hook.display()))?;
        if !status.success() {
            bail!("pre-code hook failed: {}", hook.display());
        }
        hooks_run.push(relative_to_project(project, hook));
    }

    Ok(hooks_run)
}

fn relative_to_project(project: &Project, path: PathBuf) -> String {
    path.strip_prefix(project.root())
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/")
}
