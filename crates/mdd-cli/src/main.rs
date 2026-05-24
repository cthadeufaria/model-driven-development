use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use mdd_core::{InitFileConflict, Project, RenderTree};
use mdd_render::RenderSelection;
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "mdd")]
#[command(about = "Bootstrap agent-first model-driven development workspaces")]
#[command(disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Initialize .mdd project structure and project-local agent skills.
    ///
    /// Re-running is safe. Regenerable files (docs, workflow skills,
    /// CLAUDE.md/AGENTS.md blocks, SessionStart hook) prompt per file on
    /// conflict, or with --force are overwritten from the current templates
    /// without prompting. The three accumulating state files (config.yml,
    /// trace.yml, approvals.yml) are never overwritten — they are created
    /// when missing and forward-migrated to the current schema version
    /// otherwise, preserving their content.
    Init {
        /// Overwrite all regenerable scaffolding from the current templates
        /// without the per-file prompt. Does not touch the config/trace/
        /// approvals state files (those are migrated, never overwritten).
        #[arg(long)]
        force: bool,
    },
    /// Remove .mdd project structure and generated project-local MDD skill files.
    #[command(alias = "deinit", alias = "uninit")]
    Clean {
        /// Remove generated MDD skill files even when they were modified after init.
        #[arg(long)]
        force: bool,
    },
    /// Open the interactive diagram viewer for the current project.
    View,
    /// Run the structural validation gate over the current and objective
    /// sides. Exits non-zero on a blocking structural error.
    ///
    /// Checks id presence/uniqueness, per-side @ref resolution, trace-link
    /// and source-link integrity, and the security-marker contract.
    /// Readiness gaps (approvals, acceptance coverage, rendered SVGs) are
    /// reported but never block. Independent of `mdd review`.
    Validate {
        /// Emit a slim {ok, errors, warnings} JSON object instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Run the cycle-closure review gate: ID parity, security parity, and
    /// traceability parity. Exits non-zero on a blocking mismatch.
    Review,
    /// Report whether the diagrams are current with the source (freshness).
    /// Exits non-zero when a tracked symbol has drifted since the
    /// whole-map's recorded source_revision.
    #[command(name = "map-status")]
    MapStatus,
    /// Print the session brief: a compact whole-map table of contents plus the
    /// freshness verdict. Wired by `mdd init` as the Claude Code SessionStart
    /// hook; always exits 0 (a briefing, not a gate).
    Context,
    /// Rasterize sources to SVG.
    ///
    /// With no arguments this renders the full tree set (models, cycle
    /// diffs, OCL, whole-map, deploy, review-diff) — the same set every
    /// caller sees, so the rendered tree never drifts from the model.
    Render {
        /// Restrict to specific trees (comma-separated or repeated):
        /// models, cycle-diffs, ocl, map, deploy, review.
        #[arg(long = "only", value_delimiter = ',')]
        only: Vec<String>,
        /// Explicit source files or directories to render instead of
        /// the full set (directories are walked for *.puml / *.ocl).
        paths: Vec<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init { force } => {
            let root = env::current_dir()?;
            let project = Project::at(root);
            // --force supplies an always-overwrite conflict handler for the
            // regenerable files; without it, init prompts per file. The state
            // files ignore the handler either way (they are migrated, never
            // overwritten), so --force can never reach config/trace/approvals.
            let report = if force {
                project.init_with_conflict_handler(|_| Ok(InitFileConflict::Overwrite))?
            } else {
                project.init_with_conflict_handler(prompt_init_conflict)?
            };
            println!("Initialized mdd project at {}", report.root.display());
            if report.created.is_empty()
                && report.overwritten.is_empty()
                && report.migrated.is_empty()
                && report.skipped.is_empty()
            {
                println!("No files changed; .mdd structure already exists.");
            } else {
                for created in report.created {
                    println!("created {created}");
                }
                for overwritten in report.overwritten {
                    println!("overwrote {overwritten}");
                }
                for migrated in report.migrated {
                    println!("migrated {migrated}");
                }
                for skipped in report.skipped {
                    println!("skipped {skipped}");
                }
            }
        }
        Commands::View => {
            let project = Project::discover(env::current_dir()?)?;
            mdd_viewer::run(project)?;
        }
        Commands::Validate { json } => {
            let project = Project::discover(env::current_dir()?)?;
            let report = project.validate()?;
            if json {
                // Slim machine contract for the /mdd-validate skill: the
                // verdict and messages only, never the full ModelRegistry.
                let view = serde_json::json!({
                    "ok": report.ok,
                    "errors": report.errors,
                    "warnings": report.warnings,
                });
                println!("{}", serde_json::to_string_pretty(&view)?);
            } else {
                for error in &report.errors {
                    println!("error:   {error}");
                }
                for warning in &report.warnings {
                    println!("warning: {warning}");
                }
                println!(
                    "\nVALIDATION: {}",
                    if report.ok { "PASSED" } else { "FAILED" }
                );
            }
            if !report.ok {
                std::process::exit(1);
            }
        }
        Commands::Review => {
            let project = Project::discover(env::current_dir()?)?;
            let report = project.review()?;
            println!("ID parity:       {}", pass_fail(report.ids_matched));
            for id in &report.missing_ids {
                println!("  missing in current: {id}");
            }
            println!(
                "security parity: {} ({:?})",
                pass_fail(report.security.matched),
                report.security.mode
            );
            for marker in &report.security.missing_markers {
                println!("  missing marker: {} on {}", marker.stereotype, marker.host);
            }

            let trace = &report.traceability;
            println!(
                "traceability:    {} ({:?}, base {})",
                pass_fail(trace.matched),
                trace.mode,
                trace.base
            );
            for err in &trace.forward_errors {
                println!(
                    "  forward (error): {} -> {} `{}` not found",
                    err.model_id, err.path, err.symbol
                );
            }
            if !trace.reverse_bucket_b.is_empty() {
                println!("  reverse bucket B (error) — edited behaviour with no diagram counterpart:");
                for v in &trace.reverse_bucket_b {
                    println!("    {} ({} {})", v.path, v.kind, v.symbol);
                }
            }
            if !trace.reverse_bucket_a.is_empty() {
                println!("  reverse bucket A (warn) — edited glue with no counterpart:");
                for a in &trace.reverse_bucket_a {
                    println!("    {a}");
                }
            }

            println!(
                "\nreview {}",
                if report.matched { "PASSED" } else { "FAILED" }
            );
            if !report.matched {
                std::process::exit(1);
            }
        }
        Commands::MapStatus => {
            let project = Project::discover(env::current_dir()?)?;
            let report = project.map_status()?;
            match &report.source_revision {
                None => println!("map-status: FRESH (no whole-map baseline yet)"),
                Some(rev) => {
                    if report.fresh {
                        println!("map-status: FRESH (no tracked symbol changed since {rev})");
                    } else {
                        println!("map-status: STALE since {rev} — {} drifted:", report.drift.len());
                        for d in &report.drift {
                            println!("  {} `{}` -> {}", d.path, d.symbol, d.model_id);
                        }
                    }
                }
            }
            if !report.fresh {
                std::process::exit(1);
            }
        }
        Commands::Context => {
            let project = Project::discover(env::current_dir()?)?;
            let ctx = project.session_context()?;
            print_session_context(&ctx);
        }
        Commands::Render { only, paths } => {
            let project = Project::discover(env::current_dir()?)?;
            let selection = if !paths.is_empty() {
                if !only.is_empty() {
                    bail!("pass either explicit paths or --only, not both");
                }
                RenderSelection::Paths(paths)
            } else if only.is_empty() {
                RenderSelection::All
            } else {
                let mut trees = Vec::new();
                for token in &only {
                    match RenderTree::parse(token) {
                        Some(tree) if !trees.contains(&tree) => trees.push(tree),
                        Some(_) => {}
                        None => bail!(
                            "unknown render tree `{token}` (expected: models, cycle-diffs, ocl, map, deploy, review)"
                        ),
                    }
                }
                RenderSelection::Trees(trees)
            };

            let report = mdd_render::render_selection(&project, &selection)?;
            for rendered in &report.rendered {
                println!("rendered {rendered}");
            }
            for diagnostic in &report.diagnostics {
                eprintln!("diagnostic {diagnostic}");
            }
            if report.rendered.is_empty() && report.diagnostics.is_empty() {
                println!("no render sources matched");
            } else {
                println!(
                    "{} rendered, {} diagnostic(s)",
                    report.rendered.len(),
                    report.diagnostics.len()
                );
            }
            if !report.diagnostics.is_empty() {
                std::process::exit(1);
            }
        }
        Commands::Clean { force } => {
            let root = env::current_dir()?;
            let project = Project::at(root);
            let report = project.clean(force)?;
            println!("Cleaned mdd artifacts from {}", report.root.display());
            if report.removed.is_empty() && report.skipped.is_empty() {
                println!("No mdd artifacts found.");
            } else {
                for removed in &report.removed {
                    println!("removed {removed}");
                }
                for skipped in &report.skipped {
                    println!("skipped {}: {}", skipped.path, skipped.reason);
                }
                if !report.skipped.is_empty() && !force {
                    println!("Rerun with --force to remove skipped generated MDD skill files.");
                }
            }
        }
    }

    Ok(())
}

fn pass_fail(ok: bool) -> &'static str {
    if ok { "PASS" } else { "FAIL" }
}

/// Render the session brief (`mdd context`): the whole-map table of contents
/// followed by the freshness verdict. Always informational — never exits.
fn print_session_context(ctx: &mdd_core::SessionContext) {
    if ctx.toc.is_empty() {
        println!("map: (no whole-map yet — run a cycle to accumulate one)");
    } else {
        let total: usize = ctx.toc.iter().map(|entry| entry.id_count).sum();
        println!("map ({total} ids across {} kinds):", ctx.toc.len());
        for entry in &ctx.toc {
            println!(
                "  {:<11} {} concepts, {} ids",
                entry.kind, entry.concept_count, entry.id_count
            );
        }
    }
    match &ctx.source_revision {
        None => println!("freshness: FRESH (no baseline yet)"),
        Some(rev) => {
            let short = rev.get(..7).unwrap_or(rev.as_str());
            if ctx.fresh {
                println!("freshness: FRESH (no drift since {short})");
            } else {
                println!(
                    "freshness: STALE since {short} — {} symbol(s) drifted; /mdd-map the area first:",
                    ctx.drift.len()
                );
                for drift in &ctx.drift {
                    println!("  {} `{}` -> {}", drift.path, drift.symbol, drift.model_id);
                }
            }
        }
    }
}

fn prompt_init_conflict(path: &str) -> Result<InitFileConflict> {
    loop {
        print!("{path} already exists. Overwrite or skip? [o/S]: ");
        io::stdout().flush()?;

        let mut answer = String::new();
        let bytes = io::stdin().read_line(&mut answer)?;
        if bytes == 0 {
            return Ok(InitFileConflict::Skip);
        }

        match answer.trim().to_ascii_lowercase().as_str() {
            "" | "s" | "skip" | "n" | "no" => return Ok(InitFileConflict::Skip),
            "o" | "overwrite" | "y" | "yes" => return Ok(InitFileConflict::Overwrite),
            _ => {
                println!("Please enter overwrite or skip.");
            }
        }
    }
}
