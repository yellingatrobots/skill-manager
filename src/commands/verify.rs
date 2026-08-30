//! `verify` — read-only convergence check across store, generation, and targets.

use crate::commands::plan;
use crate::deploy::{Deployment, file_name};
use crate::error::{Error, Failures, Result};
use crate::generate;
use crate::lock::Lock;
use crate::manifest::Manifest;
use crate::paths::Paths;
use crate::store::Store;
use crate::tree::{Digest, Tree};
use std::collections::BTreeSet;
use std::fs;

#[derive(Debug)]
pub struct VerifyReport {
    pub checked: usize,
    pub problems: Failures,
}

pub fn run(paths: &Paths) -> Result<VerifyReport> {
    let manifest = Manifest::read(&paths.manifest)?;
    let lock = Lock::read(&paths.lock)?;
    let store = Store::new(&paths.store);
    let mut problems = Failures::new();

    if let Err(error) = plan::check_drift(&manifest, &lock) {
        problems.push(error);
    }
    verify_store(&manifest, &lock, &store, &mut problems);

    let plan = plan::build(&manifest, &lock, &store)?;
    for failure in plan.failures.iter() {
        problems.push(failure);
    }

    let expected_names: BTreeSet<String> = plan
        .deployables
        .iter()
        .map(|deployable| deployable.deployed_name())
        .chain([format!("{}:provenance", generate::BUILTIN_ALIAS)])
        .collect();

    let expected = expected_generation(paths, &plan.deployables)?;
    verify_current(paths, &expected, &mut problems)?;
    verify_targets(paths, &expected_names, &mut problems)?;
    verify_generations(paths, &expected, &mut problems)?;

    Ok(VerifyReport { checked: expected_names.len(), problems })
}

/// Only entries the current lock references are checked (ADR-0003).
fn verify_store(manifest: &Manifest, lock: &Lock, store: &Store, problems: &mut Failures) {
    for source in manifest.sources.values() {
        let Some(locked) = lock.sources.get(&source.alias) else {
            continue;
        };
        for (name, entry) in &locked.skills {
            if !source.selects(name) {
                continue;
            }
            let Some(digest) = Digest::parse(&entry.digest) else {
                problems.push(format!("{}:{name}: unusable digest `{}`", source.alias, entry.digest));
                continue;
            };
            match store.verify(&digest) {
                Ok(true) => {}
                Ok(false) => problems
                    .push(format!("{}:{name}: store entry {digest} no longer hashes to its digest", source.alias)),
                Err(error) => problems.push(format!("{}:{name}: {error}", source.alias)),
            }
        }
    }
}

/// What `apply` would produce right now, built in a temporary directory.
fn expected_generation(paths: &Paths, deployables: &[generate::Deployable]) -> Result<Digest> {
    let temp = tempfile::TempDir::new_in(std::env::temp_dir())
        .map_err(|error| Error::io(std::env::temp_dir(), error))?;
    let root = temp.path().join("generation");
    generate::build(&root, deployables)?;
    let _ = paths;
    Tree::read(&root)?.digest()
}

fn verify_current(paths: &Paths, expected: &Digest, problems: &mut Failures) -> Result<()> {
    let Ok(current) = fs::read_link(&paths.current) else {
        problems.push(format!("{}: no current generation; run `skill-manager apply`", paths.current.display()));
        return Ok(());
    };
    if current != paths.generation(expected) {
        problems.push(format!(
            "{}: points at {} but the manifest, lock, and store generate {expected}",
            paths.current.display(),
            file_name(&current)
        ));
    }
    if !current.is_dir() {
        problems.push(format!("{}: the current generation is missing", current.display()));
        return Ok(());
    }
    let actual = Tree::read(&current)?.digest()?;
    if file_name(&current) != actual.store_relative_path().file_name().unwrap().to_string_lossy() {
        problems.push(format!("{}: generated content no longer matches its digest", current.display()));
    }
    Ok(())
}

fn verify_targets(paths: &Paths, expected: &BTreeSet<String>, problems: &mut Failures) -> Result<()> {
    let deployment = Deployment::new(paths);
    for target in &paths.targets {
        let mut present = BTreeSet::new();
        let entries = match fs::read_dir(target) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                problems.push(format!("{}: target directory is missing", target.display()));
                continue;
            }
            Err(error) => return Err(Error::io(target, error)),
        };

        for entry in entries {
            let path = entry.map_err(|error| Error::io(target, error))?.path();
            let name = file_name(&path);
            if !deployment.is_ours(&path) {
                if expected.contains(&name) {
                    problems.push(format!("{}: `{name}` was not written by skill-manager", target.display()));
                }
                continue;
            }
            present.insert(name.clone());
            if !expected.contains(&name) {
                problems.push(format!("{}: `{name}` is deployed but no longer selected", target.display()));
                continue;
            }
            let wanted = paths.current.join(&name);
            if fs::read_link(&path).is_ok_and(|link| link != wanted) {
                problems.push(format!("{}: `{name}` does not point at the current generation", target.display()));
            }
        }

        for name in expected.difference(&present) {
            problems.push(format!("{}: `{name}` is not deployed", target.display()));
        }
    }
    Ok(())
}

fn verify_generations(paths: &Paths, expected: &Digest, problems: &mut Failures) -> Result<()> {
    let kept = paths.generation(expected);
    let algorithms = match fs::read_dir(&paths.generations) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(Error::io(&paths.generations, error)),
    };
    for algorithm in algorithms {
        let algorithm = algorithm.map_err(|error| Error::io(&paths.generations, error))?.path();
        if !algorithm.is_dir() {
            continue;
        }
        for generation in fs::read_dir(&algorithm).map_err(|error| Error::io(&algorithm, error))? {
            let generation = generation.map_err(|error| Error::io(&algorithm, error))?.path();
            if generation != kept {
                problems.push(format!("{}: stale generation", generation.display()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::apply;
    use crate::commands::testing::*;
    use crate::commands::update;
    use crate::fsx;

    fn local_home() -> Home {
        let home = home();
        write_skill(&home.path().join("etc/ai/skills/codemode"), "codemode", "body\n");
        write_manifest(&home, "version = 1\n[sources.me]\npath = \"{home}/etc/ai/skills\"\n");
        apply::run(&home.paths).unwrap();
        home
    }

    #[test]
    fn a_freshly_applied_tree_verifies() {
        let home = local_home();
        let report = run(&home.paths).unwrap();
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(report.checked, 2);
    }

    #[test]
    fn an_edited_local_source_no_longer_matches() {
        let home = local_home();
        write_skill(&home.path().join("etc/ai/skills/codemode"), "codemode", "edited\n");

        let report = run(&home.paths).unwrap();

        assert!(report.problems.iter().any(|problem| problem.contains("generate sha256:")));
    }

    #[test]
    fn a_missing_target_symlink_is_reported() {
        let home = local_home();
        std::fs::remove_file(home.paths.targets[0].join("me:codemode")).unwrap();

        let report = run(&home.paths).unwrap();

        assert!(report.problems.iter().any(|problem| problem.contains("`me:codemode` is not deployed")));
    }

    #[test]
    fn a_foreign_entry_occupying_a_deployed_name_is_reported() {
        let home = local_home();
        let path = home.paths.targets[0].join("me:codemode");
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir_all(&path).unwrap();

        let report = run(&home.paths).unwrap();

        assert!(report.problems.iter().any(|problem| problem.contains("not written by skill-manager")));
    }

    #[test]
    fn an_extra_owned_entry_is_reported() {
        let home = local_home();
        let stale = home.paths.targets[1].join("me:retired");
        std::os::unix::fs::symlink(home.paths.current.join("me:retired"), &stale).unwrap();

        let report = run(&home.paths).unwrap();

        assert!(report.problems.iter().any(|problem| problem.contains("no longer selected")));
    }

    #[test]
    fn tampered_generated_content_is_reported() {
        let home = local_home();
        let current = std::fs::read_link(&home.paths.current).unwrap();
        fsx::make_writable(&current).unwrap();
        std::fs::write(current.join("me:codemode/SKILL.md"), "tampered\n").unwrap();

        let report = run(&home.paths).unwrap();

        assert!(report.problems.iter().any(|problem| problem.contains("no longer matches its digest")));
    }

    #[test]
    fn a_stale_generation_is_reported() {
        let home = local_home();
        let stale = home.paths.generations.join("sha256").join("a".repeat(64));
        std::fs::create_dir_all(&stale).unwrap();

        let report = run(&home.paths).unwrap();

        assert!(report.problems.iter().any(|problem| problem.contains("stale generation")));
    }

    #[test]
    fn a_tampered_store_entry_is_reported() {
        let home = home();
        let upstream = git_origin(&[("handoff/SKILL.md", &skill_text("handoff", "body\n"))]);
        write_manifest(&home, &format!("version = 1\n[sources.mp]\ngit = \"{}\"\n", upstream.path().display()));
        update::run(&home.paths, None, false).unwrap();
        let lock = Lock::read(&home.paths.lock).unwrap();
        let digest = Digest::parse(&lock.skill("mp", "handoff").unwrap().digest).unwrap();
        let entry = Store::new(&home.paths.store).path_for(&digest);
        fsx::make_writable(&entry).unwrap();
        std::fs::write(entry.join("SKILL.md"), skill_text("handoff", "tampered\n")).unwrap();

        let report = run(&home.paths).unwrap();

        assert!(report.problems.iter().any(|problem| problem.contains("no longer hashes")));
    }

    #[test]
    fn a_manifest_that_no_longer_matches_the_lock_is_reported() {
        let home = home();
        let upstream = git_origin(&[("handoff/SKILL.md", &skill_text("handoff", "body\n"))]);
        write_manifest(&home, &format!("version = 1\n[sources.mp]\ngit = \"{}\"\n", upstream.path().display()));
        update::run(&home.paths, None, false).unwrap();
        write_manifest(
            &home,
            &format!("version = 1\n[sources.mp]\ngit = \"{}\"\nref = \"main\"\n", upstream.path().display()),
        );

        let report = run(&home.paths).unwrap();

        assert!(report.problems.iter().any(|problem| problem.contains("skill-manager update")));
    }
}
