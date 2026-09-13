//! Command-line surface. Printing lives here; the commands return data.

use crate::commands::{apply, edit, hydrate, info, update, verify};
use crate::error::{Failures, Result};
use crate::manifest;
use crate::paths::Paths;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "skill-manager",
    about = "Installs, namespaces, and deploys AI agent skills from a declarative manifest",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Materialize the deployed tree from the manifest, lock, and store. Offline.
    Apply,

    /// Resolve and fetch remote sources, then rewrite the lock.
    Update {
        /// Update only this source ID.
        source_id: Option<String>,
        /// Rewrite the lock without deploying.
        #[arg(long)]
        no_apply: bool,
    },

    /// Fetch exactly what the lock pins into the store. Never deploys.
    Hydrate,

    /// Select skills under existing sources, then apply.
    Add {
        /// One or more `<source-id>-<skill>`.
        #[arg(required = true)]
        selections: Vec<String>,
    },

    /// Deselect skills, then apply.
    Remove {
        /// One or more `<source-id>-<skill>`.
        #[arg(required = true)]
        selections: Vec<String>,
    },

    /// Report where deployed skills came from.
    Info {
        /// A namespaced `<source-id>-<skill>`; omit to list everything.
        name: Option<String>,
        /// Also report locations and every provenance field.
        #[arg(long)]
        full: bool,
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
    },

    /// Check that store, generation, and targets agree. Read-only.
    Verify,
}

/// The exit code: 0 clean, 1 reported failures, 2 aborted.
pub const EXIT_OK: i32 = 0;
pub const EXIT_FAILURES: i32 = 1;
pub const EXIT_ABORTED: i32 = 2;

pub fn run(cli: Cli) -> Result<i32> {
    let paths = Paths::from_env()?;
    if manifest::scaffold(&paths.manifest)? {
        println!("created {}", paths.manifest.display());
        println!(
            "edit it to declare a source, or run `skill-manager add <source-id>-<skill>` to select a skill"
        );
        return Ok(EXIT_OK);
    }
    match cli.command {
        Command::Apply => {
            let report = apply::run(&paths)?;
            print_apply(&report);
            Ok(exit_code(&report.failures))
        }
        Command::Update {
            source_id,
            no_apply,
        } => {
            let report = update::run(&paths, source_id.as_deref(), no_apply)?;
            println!("updated {}", list(&report.updated));
            if let Some(applied) = &report.applied {
                print_apply(applied);
            }
            Ok(exit_code(&report.failures))
        }
        Command::Hydrate => {
            let report = hydrate::run(&paths)?;
            match report.added.len() {
                0 => println!("store already complete"),
                count => println!("hydrated {count} skill(s): {}", list(&report.added)),
            }
            print_failures("failed", &report.failures);
            Ok(exit_code(&report.failures))
        }
        Command::Add { selections } => {
            let report = edit::add(&paths, &selections)?;
            print_edit("added", &report);
            Ok(exit_code(&report.failures))
        }
        Command::Remove { selections } => {
            let report = edit::remove(&paths, &selections)?;
            print_edit("removed", &report);
            Ok(exit_code(&report.failures))
        }
        Command::Info { name, full, json } => {
            let report = info::report(&paths, name.as_deref())?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).expect("the report serializes")
                );
            } else if full {
                print!("{}", info::render_locations(&report.locations));
                println!();
                for record in &report.skills {
                    print!("{}", info::render_detail(record));
                }
            } else {
                for record in &report.skills {
                    println!("{}", info::render_line(record));
                }
            }
            Ok(EXIT_OK)
        }
        Command::Verify => {
            let report = verify::run(&paths)?;
            print_skipped(&report.skipped);
            if report.problems.is_empty() {
                println!("verified {} deployed skill(s)", report.checked);
            }
            print_failures("problem", &report.problems);
            Ok(exit_code(&report.problems))
        }
    }
}

fn print_apply(report: &apply::ApplyReport) {
    let state = if report.switched {
        "deployed"
    } else {
        "unchanged"
    };
    println!(
        "{state} {} skill(s) at generation {}",
        report.deployed.len(),
        report.generation
    );
    print_skipped(&report.skipped);
    for target in &report.targets {
        if !target.linked.is_empty() || !target.removed.is_empty() {
            println!(
                "  {}: +{} -{}",
                target.path.display(),
                target.linked.len(),
                target.removed.len()
            );
        }
    }
    print_failures("failed", &report.failures);
}

fn print_edit(verb: &str, report: &edit::EditReport) {
    if report.changed.is_empty() {
        println!("nothing {verb}");
    } else {
        println!("{verb} {}", list(&report.changed));
    }
    for note in &report.unchanged {
        println!("  {note}");
    }
    if let Some(applied) = &report.applied {
        print_apply(applied);
    }
}

/// A local source that is absent on this machine is reported, not failed.
fn print_skipped(skipped: &[String]) {
    for note in skipped {
        println!("{note}");
    }
}

fn print_failures(label: &str, failures: &Failures) {
    for failure in failures.iter() {
        eprintln!("{label}: {failure}");
    }
}

fn exit_code(failures: &Failures) -> i32 {
    if failures.is_empty() {
        EXIT_OK
    } else {
        EXIT_FAILURES
    }
}

fn list(items: &[String]) -> String {
    items.join(", ")
}
