use assert_cmd::Command as AssertCommand;
use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

const WORKFLOW_SKILLS: &[&str] = &[
    "mdd-map",
    "mdd-generate",
    "mdd-validate",
    "mdd-implement",
    "mdd-review",
    "mdd-render",
    "mdd-cycle",
    "mdd-deploy",
];

#[test]
fn init_creates_agent_first_project_structure() {
    let dir = tempdir().unwrap();

    Command::cargo_bin("mdd")
        .unwrap()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized mdd project"));

    assert!(dir.path().join(".mdd/config.yml").is_file());
    assert!(dir.path().join(".mdd/trace.yml").is_file());
    assert!(dir.path().join(".mdd/approvals.yml").is_file());
    assert!(dir.path().join(".mdd/models/current/use-cases").is_dir());
    assert!(dir.path().join(".mdd/models/current/sequences").is_dir());
    assert!(dir.path().join(".mdd/models/current/domain").is_dir());
    assert!(dir.path().join(".mdd/models/current/components").is_dir());
    assert!(dir.path().join(".mdd/models/current/mockups").is_dir());
    assert!(dir.path().join(".mdd/models/objective/use-cases").is_dir());
    assert!(dir.path().join(".mdd/models/objective/states").is_dir());
    assert!(dir.path().join(".mdd/constraints").is_dir());
    assert!(dir.path().join(".mdd/rendered").is_dir());
    assert!(dir.path().join(".mdd/tests/acceptance").is_dir());
    assert!(dir.path().join(".mdd/tests/ui").is_dir());
    assert!(dir.path().join(".mdd/docs/mdd-workflow.md").is_file());
    assert!(dir.path().join(".mdd/docs/uml-and-ocl-guide.md").is_file());
    assert!(dir.path().join("CLAUDE.md").is_file());
    assert!(dir.path().join("AGENTS.md").is_file());
    assert!(!dir.path().join(".mdd/skills/map/SKILL.md").exists());

    for skill in WORKFLOW_SKILLS {
        assert!(
            dir.path()
                .join(format!(".claude/skills/{skill}/SKILL.md"))
                .is_file()
        );
        assert!(
            dir.path()
                .join(format!(".codex/skills/{skill}/SKILL.md"))
                .is_file()
        );
    }
}

#[test]
fn init_appends_block_preserving_existing_agents_file() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("AGENTS.md"), "custom instructions\n").unwrap();

    Command::cargo_bin("mdd")
        .unwrap()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("overwrote AGENTS.md"));

    let content = fs::read_to_string(dir.path().join("AGENTS.md")).unwrap();
    assert!(content.starts_with("custom instructions\n"));
    assert!(content.contains("<!-- mdd:begin -->"));
    assert!(content.contains("<!-- mdd:end -->"));
    assert!(content.contains("\"kind\":\"agents-entrypoint\""));
    assert!(content.contains("# Agent MDD Entry Point"));
}

#[test]
fn init_prompts_and_skips_existing_skill_file() {
    let dir = tempdir().unwrap();
    let skill = dir.path().join(".claude/skills/mdd-map/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, "custom skill\n").unwrap();

    AssertCommand::cargo_bin("mdd")
        .unwrap()
        .current_dir(dir.path())
        .arg("init")
        .write_stdin("s\n")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            ".claude/skills/mdd-map/SKILL.md already exists",
        ))
        .stdout(predicate::str::contains(
            "skipped .claude/skills/mdd-map/SKILL.md",
        ));

    assert_eq!(fs::read_to_string(&skill).unwrap(), "custom skill\n");
}

#[test]
fn clean_strips_block_but_keeps_user_content() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("CLAUDE.md"), "my own notes\n").unwrap();
    init(dir.path());

    Command::cargo_bin("mdd")
        .unwrap()
        .current_dir(dir.path())
        .arg("clean")
        .assert()
        .success()
        .stdout(predicate::str::contains("removed CLAUDE.md (mdd block)"));

    assert!(dir.path().join("CLAUDE.md").is_file());
    assert_eq!(
        fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        "my own notes\n"
    );
}

#[test]
fn clean_removes_mdd_artifacts_including_agent_scaffolding() {
    let dir = tempdir().unwrap();
    init(dir.path());

    Command::cargo_bin("mdd")
        .unwrap()
        .current_dir(dir.path())
        .arg("clean")
        .assert()
        .success()
        .stdout(predicate::str::contains("Cleaned mdd artifacts"))
        .stdout(predicate::str::contains("removed .mdd"));

    assert!(!dir.path().join(".mdd").exists());
    assert!(!dir.path().join(".claude").exists());
    assert!(!dir.path().join(".codex").exists());
    assert!(!dir.path().join("CLAUDE.md").exists());
    assert!(!dir.path().join("AGENTS.md").exists());
    for skill in WORKFLOW_SKILLS {
        assert!(
            !dir.path()
                .join(format!(".claude/skills/{skill}/SKILL.md"))
                .exists()
        );
        assert!(
            !dir.path()
                .join(format!(".codex/skills/{skill}/SKILL.md"))
                .exists()
        );
    }
}

#[test]
fn clean_preserves_modified_mdd_skill_files_without_force() {
    let dir = tempdir().unwrap();
    init(dir.path());
    fs::write(
        dir.path().join(".codex/skills/mdd-map/SKILL.md"),
        "custom skill\n",
    )
    .unwrap();

    Command::cargo_bin("mdd")
        .unwrap()
        .current_dir(dir.path())
        .arg("clean")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "skipped .codex/skills/mdd-map/SKILL.md",
        ));

    assert!(!dir.path().join(".mdd").exists());
    assert!(dir.path().join(".codex/skills/mdd-map/SKILL.md").is_file());
    assert!(!dir.path().join("AGENTS.md").exists());
    assert!(!dir.path().join("CLAUDE.md").exists());
}

#[test]
fn clean_force_removes_modified_mdd_skill_files() {
    let dir = tempdir().unwrap();
    init(dir.path());
    fs::write(
        dir.path().join(".codex/skills/mdd-map/SKILL.md"),
        "custom skill\n",
    )
    .unwrap();

    Command::cargo_bin("mdd")
        .unwrap()
        .current_dir(dir.path())
        .arg("clean")
        .arg("--force")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "removed .codex/skills/mdd-map/SKILL.md",
        ));

    assert!(!dir.path().join(".codex/skills/mdd-map/SKILL.md").exists());
    assert!(!dir.path().join(".claude").exists());
    assert!(!dir.path().join(".codex").exists());
    assert!(!dir.path().join("AGENTS.md").exists());
    assert!(!dir.path().join("CLAUDE.md").exists());
}

#[test]
fn help_exposes_init_clean_and_render_commands() {
    Command::cargo_bin("mdd")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("init"))
        .stdout(predicate::str::contains("clean"))
        // `mdd render` is now a real subcommand (the single deterministic
        // render engine); the public CLI no longer stops at init/clean.
        .stdout(predicate::str::contains("render"))
        // `mdd review` and `mdd map-status` are the deterministic gate +
        // freshness entry points added with the traceability engine.
        .stdout(predicate::str::contains("review"))
        .stdout(predicate::str::contains("map-status"))
        // `mdd context` is the session brief wired as the SessionStart hook.
        .stdout(predicate::str::contains("context"))
        .stdout(predicate::str::contains("describe").not())
        .stdout(predicate::str::contains("validate").not())
        .stdout(predicate::str::contains("diff").not())
        .stdout(predicate::str::contains("approve").not())
        .stdout(predicate::str::contains("test").not())
        .stdout(predicate::str::contains("code").not())
        .stdout(predicate::str::contains("app").not());
}

#[test]
fn removed_commands_fail_as_unknown() {
    for command in [
        "describe", "map", "validate", "diff", "approve", "test", "code", "app",
    ] {
        Command::cargo_bin("mdd")
            .unwrap()
            .arg(command)
            .assert()
            .failure()
            .stderr(predicate::str::contains("unrecognized subcommand"))
            .stderr(predicate::str::contains(command));
    }
}

#[test]
fn installed_skills_have_valid_frontmatter() {
    let dir = tempdir().unwrap();
    init(dir.path());

    for agent_dir in [".claude", ".codex"] {
        for skill in WORKFLOW_SKILLS {
            let path = dir
                .path()
                .join(format!("{agent_dir}/skills/{skill}/SKILL.md"));
            let content = fs::read_to_string(&path).unwrap();
            assert!(
                content.starts_with("---\n"),
                "{} missing frontmatter start",
                path.display()
            );
            assert!(
                content.contains(&format!("name: {skill}\n")),
                "{} missing skill name",
                path.display()
            );
            assert!(
                content.contains("description: "),
                "{} missing skill description",
                path.display()
            );
            assert!(
                content.contains("MDD, UML, PlantUML, and OCL specialist"),
                "{} missing specialist identity",
                path.display()
            );
        }
    }
}

#[test]
fn shared_docs_cover_required_workflow_terms() {
    let dir = tempdir().unwrap();
    init(dir.path());

    let docs = [
        ".mdd/docs/mdd-workflow.md",
        ".mdd/docs/uml-and-ocl-guide.md",
    ]
    .into_iter()
    .map(|path| fs::read_to_string(dir.path().join(path)).unwrap())
    .collect::<Vec<_>>()
    .join("\n");

    for term in [
        "UML",
        "PlantUML",
        "OCL",
        "@id",
        "@ref",
        "trace links",
        "validation",
        "rendering",
        "readiness warnings",
    ] {
        assert!(docs.contains(term), "docs missing {term}");
    }
}

#[test]
fn init_writes_session_start_hook() {
    let dir = tempdir().unwrap();
    init(dir.path());

    let settings = dir.path().join(".claude/settings.json");
    assert!(settings.is_file(), "init should write .claude/settings.json");
    let content = fs::read_to_string(&settings).unwrap();
    assert!(content.contains("SessionStart"));
    assert!(content.contains("mdd context"));
}

#[test]
fn init_session_hook_is_idempotent() {
    let dir = tempdir().unwrap();
    init(dir.path());
    init(dir.path()); // re-init

    let content = fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
    assert_eq!(
        content.matches("mdd context").count(),
        1,
        "re-init must not duplicate the managed hook"
    );
}

#[test]
fn init_session_hook_preserves_existing_settings() {
    let dir = tempdir().unwrap();
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    fs::write(
        dir.path().join(".claude/settings.json"),
        "{\n  \"permissions\": { \"allow\": [\"Bash(ls)\"] }\n}\n",
    )
    .unwrap();

    init(dir.path());

    let content = fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
    assert!(content.contains("permissions"));
    assert!(content.contains("Bash(ls)"));
    assert!(content.contains("mdd context"));
}

#[test]
fn clean_removes_session_hook_and_reclaims_claude_dir() {
    let dir = tempdir().unwrap();
    init(dir.path());
    assert!(dir.path().join(".claude/settings.json").is_file());

    Command::cargo_bin("mdd")
        .unwrap()
        .current_dir(dir.path())
        .arg("clean")
        .assert()
        .success();

    assert!(!dir.path().join(".claude/settings.json").exists());
    assert!(!dir.path().join(".claude").exists());
}

#[test]
fn context_reports_empty_map_and_no_baseline_on_fresh_init() {
    let dir = tempdir().unwrap();
    init(dir.path());

    Command::cargo_bin("mdd")
        .unwrap()
        .current_dir(dir.path())
        .arg("context")
        .assert()
        .success()
        .stdout(predicate::str::contains("no whole-map yet"))
        .stdout(predicate::str::contains("FRESH"));
}

#[test]
fn context_lists_whole_map_table_of_contents() {
    let dir = tempdir().unwrap();
    init(dir.path());

    let map = dir.path().join(".mdd/map");
    fs::create_dir_all(map.join("use-cases")).unwrap();
    fs::create_dir_all(map.join("domain")).unwrap();
    fs::write(
        map.join("use-cases/sample.puml"),
        "@startuml\n' @id(USE-SAMPLE)\n@enduml\n",
    )
    .unwrap();
    fs::write(
        map.join("domain/sample.puml"),
        "@startuml\n' @id(DOM-SAMPLE)\n' @id(DOM-OTHER)\n@enduml\n",
    )
    .unwrap();
    fs::write(
        map.join("manifest.yml"),
        "version: 1\nlast_cycle: 1\nfiles:\n  - use-cases/sample.puml\n  - domain/sample.puml\n",
    )
    .unwrap();

    Command::cargo_bin("mdd")
        .unwrap()
        .current_dir(dir.path())
        .arg("context")
        .assert()
        .success()
        .stdout(predicate::str::contains("use-cases"))
        .stdout(predicate::str::contains("domain"))
        .stdout(predicate::str::contains("2 ids"));
}

fn init(dir: &Path) {
    Command::cargo_bin("mdd")
        .unwrap()
        .current_dir(dir)
        .arg("init")
        .assert()
        .success();
}
