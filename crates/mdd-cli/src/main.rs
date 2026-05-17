use anyhow::Result;
use clap::{Parser, Subcommand};
use mdd_core::{InitFileConflict, Project};
use std::env;
use std::io::{self, Write};

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
    Init,
    /// Remove .mdd project structure and generated project-local MDD skill files.
    #[command(alias = "deinit", alias = "uninit")]
    Clean {
        /// Remove generated MDD skill files even when they were modified after init.
        #[arg(long)]
        force: bool,
    },
    /// Open the interactive diagram viewer for the current project.
    View,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            let root = env::current_dir()?;
            let project = Project::at(root);
            let report = project.init_with_conflict_handler(prompt_init_conflict)?;
            println!("Initialized mdd project at {}", report.root.display());
            if report.created.is_empty()
                && report.overwritten.is_empty()
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
                for skipped in report.skipped {
                    println!("skipped {skipped}");
                }
            }
        }
        Commands::View => {
            let project = Project::discover(env::current_dir()?)?;
            mdd_viewer::run(project)?;
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
