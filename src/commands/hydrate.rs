//! `hydrate` — populate the store from the lock without resolving anything new.

use crate::error::{Error, Failures, Result};
use crate::fetch;
use crate::generate::namespaced;
use crate::git;
use crate::lock::{self, LockedSkill, LockedSource};
use crate::paths::Paths;
use crate::store::Store;
use crate::tree::{Digest, Tree};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct HydrateReport {
    /// Each skill written to the store, with the entry it landed in.
    pub added: Vec<(String, PathBuf)>,
    pub failures: Failures,
}

pub fn run(paths: &Paths) -> Result<HydrateReport> {
    let locked = lock::read_required(&paths.lock)?;
    let store = Store::new(&paths.store);
    let mut report = HydrateReport::default();

    for (source_id, source) in &locked.sources {
        let missing = missing_skills(source, &store, &mut report);
        if missing.is_empty() {
            continue;
        }
        let outcome = match source.kind.as_str() {
            "well_known" => hydrate_archives(source_id, source, &missing, &store, &mut report),
            _ => hydrate_git(source_id, source, &missing, &store, &mut report),
        };
        if let Err(error) = outcome {
            report.failures.push(format!("{source_id}: {error}"));
        }
    }

    Ok(report)
}

/// Skills whose store entry is absent; unusable digests are reported at once.
fn missing_skills<'a>(
    source: &'a LockedSource,
    store: &Store,
    report: &mut HydrateReport,
) -> Vec<(&'a String, &'a LockedSkill, Digest)> {
    let mut missing = Vec::new();
    for (name, skill) in &source.skills {
        match Digest::parse(&skill.digest) {
            None => report
                .failures
                .push(format!("{name}: unusable digest `{}`", skill.digest)),
            Some(digest) if !store.contains(&digest) => missing.push((name, skill, digest)),
            Some(_) => {}
        }
    }
    missing
}

fn hydrate_git(
    source_id: &str,
    source: &LockedSource,
    missing: &[(&String, &LockedSkill, Digest)],
    store: &Store,
    report: &mut HydrateReport,
) -> Result<()> {
    // Skills may sit at different commits, so one checkout per coordinate.
    let mut by_coordinate: BTreeMap<&str, Vec<&(&String, &LockedSkill, Digest)>> = BTreeMap::new();
    for entry in missing {
        by_coordinate
            .entry(entry.1.coordinate.as_str())
            .or_default()
            .push(entry);
    }

    for (coordinate, entries) in by_coordinate {
        let Some(commit) = fetch::commit_of(coordinate) else {
            report
                .failures
                .push(format!("{source_id}: unusable coordinate `{coordinate}`"));
            continue;
        };
        let temp =
            tempfile::TempDir::new().map_err(|error| Error::io(std::env::temp_dir(), error))?;
        let checkout = temp.path().join("checkout");
        match git::fetch_into(&source.url, Some(commit), &checkout) {
            Err(error) => report.failures.push(format!("{source_id}: {error}")),
            Ok(_) => {
                for (name, skill, digest) in entries {
                    let path = checkout.join(&skill.path);
                    match store_verified(&path, digest, store) {
                        Ok(entry) => report.added.push((namespaced(source_id, name), entry)),
                        Err(error) => report
                            .failures
                            .push(format!("{}: {error}", namespaced(source_id, name))),
                    }
                }
            }
        }
    }
    Ok(())
}

fn hydrate_archives(
    source_id: &str,
    source: &LockedSource,
    missing: &[(&String, &LockedSkill, Digest)],
    store: &Store,
    report: &mut HydrateReport,
) -> Result<()> {
    for (name, skill, digest) in missing {
        let fetched = fetch::archive_skill(&source.url, name, &skill.path, Some(&skill.coordinate));
        match fetched {
            Err(error) => report
                .failures
                .push(format!("{}: {error}", namespaced(source_id, name))),
            Ok(fetched) => {
                let path = fetched.root.join(name.as_str());
                match store_verified(&path, digest, store) {
                    Ok(entry) => report.added.push((namespaced(source_id, name), entry)),
                    Err(error) => report
                        .failures
                        .push(format!("{}: {error}", namespaced(source_id, name))),
                }
            }
        }
    }
    Ok(())
}

/// Every fetched skill is verified against its locked content digest.
fn store_verified(path: &Path, expected: &Digest, store: &Store) -> Result<PathBuf> {
    if !path.is_dir() {
        return Err(Error::Lock(format!(
            "{} is not in the fetched content; run `skill-manager update`",
            path.display()
        )));
    }
    let tree = Tree::read(path)?;
    let digest = tree.digest()?;
    if &digest != expected {
        return Err(Error::Lock(format!(
            "content digest {digest} does not match the locked {expected}; run `skill-manager update`"
        )));
    }
    store.insert(&tree)?;
    Ok(store.path_for(expected))
}

#[cfg(test)]
fn locked_digests(lock: &crate::lock::Lock) -> Vec<(String, Digest)> {
    let mut digests = Vec::new();
    for (source_id, source) in &lock.sources {
        for (name, skill) in &source.skills {
            if let Some(digest) = Digest::parse(&skill.digest) {
                digests.push((namespaced(source_id, name), digest));
            }
        }
    }
    digests
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::*;
    use crate::commands::update;
    use crate::fsx;
    use crate::lock::Lock;
    use std::fs;

    fn locked_home() -> (Home, tempfile::TempDir) {
        let home = home();
        let upstream = git_origin(&[
            ("handoff/SKILL.md", &skill_text("handoff", "one\n")),
            ("tdd/SKILL.md", &skill_text("tdd", "one\n")),
        ]);
        write_manifest(
            &home,
            &format!(
                "version = 1\n[sources.mp]\ngit = \"{}\"\n",
                upstream.path().display()
            ),
        );
        update::run(&home.paths, None, true).unwrap();
        (home, upstream)
    }

    #[test]
    fn refills_an_empty_store_from_the_lock() {
        let (home, _upstream) = locked_home();
        fsx::remove_tree(&home.paths.store).unwrap();

        let report = run(&home.paths).unwrap();

        assert!(report.failures.is_empty(), "{:?}", report.failures);
        let lock = Lock::read(&home.paths.lock).unwrap();
        let store = Store::new(&home.paths.store);
        let expected: Vec<(String, std::path::PathBuf)> = locked_digests(&lock)
            .into_iter()
            .map(|(name, digest)| (name, store.path_for(&digest)))
            .collect();
        assert_eq!(report.added, expected);
        for (_, digest) in locked_digests(&lock) {
            assert!(store.verify(&digest).unwrap());
        }
    }

    #[test]
    fn present_entries_are_left_alone() {
        let (home, _upstream) = locked_home();

        let report = run(&home.paths).unwrap();

        assert!(report.added.is_empty());
        assert!(report.failures.is_empty());
    }

    #[test]
    fn never_writes_the_deployed_tree() {
        let (home, _upstream) = locked_home();
        fsx::remove_tree(&home.paths.store).unwrap();

        run(&home.paths).unwrap();

        assert!(!home.paths.current.exists());
    }

    #[test]
    fn a_digest_mismatch_is_reported_with_an_instruction_to_update() {
        let (home, _upstream) = locked_home();
        fsx::remove_tree(&home.paths.store).unwrap();
        let lock = fs::read_to_string(&home.paths.lock).unwrap();
        let wrong = format!("digest = \"sha256:{}\"", "0".repeat(64));
        let doctored: String = lock
            .lines()
            .map(|line| {
                if line.starts_with("digest = ") {
                    wrong.as_str()
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        write_lock(&home, &doctored);

        let report = run(&home.paths).unwrap();

        assert!(
            report
                .failures
                .iter()
                .any(|failure| failure.contains("does not match the locked"))
        );
        assert!(
            report
                .failures
                .iter()
                .any(|failure| failure.contains("skill-manager update"))
        );
    }

    #[test]
    fn other_skills_are_stored_when_one_fails() {
        let (home, _upstream) = locked_home();
        fsx::remove_tree(&home.paths.store).unwrap();
        let lock = fs::read_to_string(&home.paths.lock).unwrap();
        write_lock(
            &home,
            &lock.replace("path = \"handoff\"", "path = \"absent\""),
        );

        let report = run(&home.paths).unwrap();

        let names: Vec<&str> = report.added.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["mp-tdd"]);
        assert!(
            report
                .failures
                .iter()
                .any(|failure| failure.contains("mp-handoff"))
        );
    }

    #[test]
    fn a_missing_lock_points_at_update() {
        let home = home();
        let error = run(&home.paths).unwrap_err().to_string();
        assert!(error.contains("skill-manager update"));
    }
}
