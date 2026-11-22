//! Command-line surface. Printing lives here; the commands return data.

use crate::change::Change;
use crate::commands::{apply, edit, hydrate, info, update, verify};
use crate::error::{Failures, Result};
use crate::generate::namespaced;
use crate::lock::{LockedSkill, SourceDiff};
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
    /// Materialize the deployed tree from the manifest, lock, and store,
    /// fetching locked content missing from the store.
    Apply {
        /// Never fetch; fail if a locked store entry is missing.
        #[arg(long)]
        offline: bool,
    },

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
        Command::Apply { offline } => {
            let network = if offline {
                apply::Network::Offline
            } else {
                apply::Network::Allowed
            };
            let report = apply::run(&paths, network)?;
            print_apply(&report);
            Ok(exit_code(&report.failures))
        }
        Command::Update {
            source_id,
            no_apply,
        } => {
            let report = update::run(&paths, source_id.as_deref(), no_apply)?;
            println!("updated {}", list(&report.updated));
            print_lock_changes(&report.lock, &report.changes);
            print_stored(&report.stored);
            if let Some(applied) = &report.applied {
                print_apply(applied);
            }
            Ok(exit_code(&report.failures))
        }
        Command::Hydrate => {
            let report = hydrate::run(&paths)?;
            match report.added.len() {
                0 => println!("store already complete"),
                count => println!("hydrated {count} skill(s)"),
            }
            for (name, path) in &report.added {
                println!("  + {name}  {}", path.display());
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
    if !report.hydrated.is_empty() {
        println!("hydrated {} skill(s) from the lock", report.hydrated.len());
        for (name, path) in &report.hydrated {
            println!("  + {name}  {}", path.display());
        }
    }
    if report.switched {
        println!(
            "  current: {} -> {}",
            report.current.display(),
            report.generation_path.display()
        );
    } else {
        println!(
            "  current: {} already points at {}",
            report.current.display(),
            report.generation_path.display()
        );
    }
    let mut unchanged = 0;
    for (name, change) in &report.skills {
        match change {
            Change::Added(_) => println!("    + {name}"),
            Change::Removed(_) => println!("    - {name}"),
            Change::Changed(_, _) => println!("    ~ {name}"),
            Change::Unchanged(_) => unchanged += 1,
        }
    }
    if unchanged > 0 {
        println!("    = {unchanged} skill(s) unchanged");
    }
    for target in &report.targets {
        let mut parts = Vec::new();
        if !target.linked.is_empty() {
            parts.push(format!("linked {}", list(&target.linked)));
        }
        if !target.removed.is_empty() {
            parts.push(format!("removed {}", list(&target.removed)));
        }
        if parts.is_empty() {
            parts.push("no changes".to_string());
        }
        println!("  {}: {}", target.path.display(), parts.join("; "));
    }
    for path in &report.pruned {
        println!("  pruned {}", path.display());
    }
    print_failures("failed", &report.failures);
}

fn print_lock_changes(lock: &std::path::Path, changes: &[SourceDiff]) {
    if changes.iter().any(SourceDiff::changed) {
        println!("lock: wrote {}", lock.display());
    } else {
        println!("lock: no changes to {}", lock.display());
    }
    for source in changes {
        println!("  source {}:", source.source_id);
        let mut unchanged = 0;
        for (name, change) in &source.skills {
            let name = namespaced(&source.source_id, name);
            match change {
                Change::Added(skill) => println!(
                    "    + {name}  {}  {}",
                    short(&skill.coordinate),
                    short(&skill.digest)
                ),
                Change::Removed(skill) => println!(
                    "    - {name}  {}  {}",
                    short(&skill.coordinate),
                    short(&skill.digest)
                ),
                Change::Changed(was, now) => println!("    ~ {name}  {}", skill_delta(was, now)),
                Change::Unchanged(_) => unchanged += 1,
            }
        }
        if unchanged > 0 {
            println!("    = {unchanged} skill(s) unchanged");
        }
    }
}

fn print_stored(stored: &[(String, std::path::PathBuf)]) {
    match stored.len() {
        0 => println!("store: no new entries"),
        1 => println!("store: wrote 1 new entry"),
        count => println!("store: wrote {count} new entries"),
    }
    for (name, path) in stored {
        println!("  + {name}  {}", path.display());
    }
}

/// Only the fields that moved, each as `before -> after`.
fn skill_delta(was: &LockedSkill, now: &LockedSkill) -> String {
    let mut parts = Vec::new();
    let mut field = |label: &str, before: &str, after: &str, abbreviate: bool| {
        if before != after {
            let (before, after) = if abbreviate {
                (short(before), short(after))
            } else {
                (before.to_string(), after.to_string())
            };
            parts.push(format!("{label} {before} -> {after}"));
        }
    };
    field("coordinate", &was.coordinate, &now.coordinate, true);
    field("digest", &was.digest, &now.digest, true);
    field("path", &was.path, &now.path, false);
    field(
        "upstream name",
        &was.upstream_name,
        &now.upstream_name,
        false,
    );
    if was.digest == now.digest {
        parts.push("content unchanged".to_string());
    }
    parts.join("; ")
}

/// `sha256:<64 hex>` or `git:<40 hex>` cut to a readable prefix.
fn short(value: &str) -> String {
    match value.split_once(':') {
        Some((scheme, rest)) if rest.chars().count() > 12 => {
            format!("{scheme}:{}", rest.chars().take(12).collect::<String>())
        }
        _ => value.to_string(),
    }
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
    if let Some(manifest) = &report.manifest {
        println!("manifest: wrote {}", manifest.display());
    }
    if let Some(lock) = &report.lock {
        print_lock_changes(lock, &report.lock_changes);
    }
    if !report.stored.is_empty() {
        print_stored(&report.stored);
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
