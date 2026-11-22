//! `apply` — offline materialization of the deployed tree (ADR-0004).

use crate::change::Change;
use crate::commands::{hydrate, plan};
use crate::deploy::{Deployment, TargetReport};
use crate::error::{Error, Failures, Result};
use crate::lock::{Lock, LockedSource};
use crate::manifest::Manifest;
use crate::paths::Paths;
use crate::store::Store;
use crate::tree::Digest;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug)]
pub struct ApplyReport {
    pub generation: Digest,
    /// Where the generation lives on disk.
    pub generation_path: PathBuf,
    /// The `current` symlink, repointed when `switched`.
    pub current: PathBuf,
    pub switched: bool,
    /// Each deployed name in either generation, compared by content digest.
    pub skills: BTreeMap<String, Change<Digest>>,
    /// Old generation directories deleted.
    pub pruned: Vec<PathBuf>,
    /// Store entries fetched because the lock referenced them and they were absent.
    pub hydrated: Vec<(String, PathBuf)>,
    pub deployed: Vec<String>,
    pub targets: Vec<TargetReport>,
    pub failures: Failures,
    pub skipped: Vec<String>,
}

/// Whether `apply` may fetch locked store entries it lacks (ADR-0014).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Allowed,
    Offline,
}

pub fn run(paths: &Paths, network: Network) -> Result<ApplyReport> {
    let manifest = Manifest::read(&paths.manifest)?;
    let lock = Lock::read(&paths.lock)?;
    run_with(paths, &manifest, &lock, network)
}

/// Shared with `update`, `add`, and `remove`, which already hold both files.
pub fn run_with(
    paths: &Paths,
    manifest: &Manifest,
    lock: &Lock,
    network: Network,
) -> Result<ApplyReport> {
    plan::check_drift(manifest, lock)?;
    let store = Store::new(&paths.store);
    let hydrated = match network {
        Network::Offline => Vec::new(),
        Network::Allowed => hydrate_selected(manifest, lock, &store)?,
    };
    let mut plan = plan::build(manifest, lock, &store)?;

    let applied = Deployment::new(paths).apply(&plan.deployables)?;
    for target in &applied.targets {
        for conflict in &target.conflicts {
            plan.failures.push(format!(
                "{}: `{conflict}` already exists and was not written by skill-manager",
                target.path.display()
            ));
        }
    }

    Ok(ApplyReport {
        generation_path: paths.generation(&applied.generation),
        generation: applied.generation,
        current: paths.current.clone(),
        switched: applied.switched,
        skills: applied.skills,
        pruned: applied.pruned,
        hydrated,
        deployed: plan.deployables.iter().map(|d| d.deployed_name()).collect(),
        targets: applied.targets,
        failures: plan.failures,
        skipped: plan.skipped,
    })
}

/// Fetches the locked content of selected remote skills missing from the store.
/// Any failure aborts: deploying without a selected skill is not an option.
fn hydrate_selected(
    manifest: &Manifest,
    lock: &Lock,
    store: &Store,
) -> Result<Vec<(String, PathBuf)>> {
    let mut selected = Lock::default();
    for source in manifest.remote() {
        if let Some(locked) = lock.sources.get(&source.source_id) {
            let skills = locked
                .skills
                .iter()
                .filter(|(name, _)| source.selects(name))
                .map(|(name, skill)| (name.clone(), skill.clone()))
                .collect();
            selected.sources.insert(
                source.source_id.clone(),
                LockedSource {
                    skills,
                    ..locked.clone()
                },
            );
        }
    }
    let report = hydrate::fill(&selected, store);
    if report.failures.is_empty() {
        return Ok(report.added);
    }
    let failures: Vec<&str> = report.failures.iter().collect();
    Err(Error::Hydrate(failures.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::*;
    use std::fs;

    #[test]
    fn an_absent_local_path_is_skipped_and_the_rest_still_deploys() {
        let home = home();
        write_skill(&home.path().join("etc/ai/skills/here"), "here", "body\n");
        write_manifest(
            &home,
            "version = 1\n             [sources.me]\npath = \"{home}/etc/ai/skills\"\n\n             [sources.other]\npath = \"{home}/not/on/this/machine\"\n",
        );

        let report = run(&home.paths, Network::Offline).unwrap();

        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(report.deployed, ["me-here"]);
        assert_eq!(report.skipped.len(), 1, "{:?}", report.skipped);
        assert!(report.skipped[0].contains("source `other`"));
        assert!(report.skipped[0].contains("does not exist"));
    }

    #[test]
    fn a_local_path_that_is_not_a_directory_still_aborts() {
        let home = home();
        fs::create_dir_all(home.path().join("etc")).unwrap();
        fs::write(home.path().join("etc/skills"), "not a directory").unwrap();
        write_manifest(
            &home,
            "version = 1\n[sources.me]\npath = \"{home}/etc/skills\"\n",
        );

        let error = run(&home.paths, Network::Offline).unwrap_err().to_string();

        assert!(error.contains("is not a directory"), "{error}");
    }

    #[test]
    fn deploys_a_local_source_into_every_target() {
        let home = home();
        write_skill(
            &home.path().join("etc/ai/skills/codemode"),
            "codemode",
            "principles\n",
        );
        write_manifest(
            &home,
            "version = 1\n[sources.me]\npath = \"{home}/etc/ai/skills\"\n",
        );

        let report = run(&home.paths, Network::Offline).unwrap();

        assert!(report.failures.is_empty());
        assert_eq!(report.deployed, ["me-codemode"]);
        for target in &home.paths.targets {
            let text = fs::read_to_string(target.join("me-codemode/SKILL.md")).unwrap();
            // A path source's provenance: its kind, its directory, and a content digest
            // standing in for an upstream coordinate.
            assert!(text.contains("\n  sm-source-kind: path\n"));
            assert!(text.contains(&format!(
                "\n  sm-source-url: {}/etc/ai/skills\n",
                home.path().display()
            )));
            assert!(text.contains("\n  sm-upstream-coordinate: sha256:"));
        }
    }

    #[test]
    fn reports_a_deployed_name_over_the_spec_limit_and_skips_it() {
        let home = home();
        let long = "a".repeat(62);
        write_skill(
            &home.path().join("etc/ai/skills").join(&long),
            &long,
            "body\n",
        );
        write_skill(&home.path().join("etc/ai/skills/short"), "short", "body\n");
        write_manifest(
            &home,
            "version = 1\n[sources.me]\npath = \"{home}/etc/ai/skills\"\n",
        );

        let report = run(&home.paths, Network::Offline).unwrap();

        assert_eq!(report.deployed, ["me-short"]);
        assert!(
            report
                .failures
                .iter()
                .any(|failure| failure.contains("65 characters"))
        );
    }

    #[test]
    fn keeps_deploying_valid_skills_when_one_is_invalid() {
        let home = home();
        write_skill(&home.path().join("etc/ai/skills/good"), "good", "body\n");
        let bad = home.path().join("etc/ai/skills/bad");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join("SKILL.md"), "no frontmatter\n").unwrap();
        write_manifest(
            &home,
            "version = 1\n[sources.me]\npath = \"{home}/etc/ai/skills\"\n",
        );

        let report = run(&home.paths, Network::Offline).unwrap();

        assert_eq!(report.deployed, ["me-good"]);
        assert!(
            report
                .failures
                .iter()
                .any(|failure| failure.starts_with("me-bad:"))
        );
    }

    #[test]
    fn reports_selected_skills_that_do_not_exist() {
        let home = home();
        write_skill(&home.path().join("etc/ai/skills/good"), "good", "body\n");
        write_manifest(
            &home,
            "version = 1\n[sources.me]\npath = \"{home}/etc/ai/skills\"\nskills = [\"good\", \"absent\"]\n",
        );

        let report = run(&home.paths, Network::Offline).unwrap();

        assert_eq!(report.deployed, ["me-good"]);
        assert!(
            report
                .failures
                .iter()
                .any(|failure| failure.contains("me-absent"))
        );
    }

    #[test]
    fn deploys_a_remote_skill_from_the_lock_and_store() {
        let home = home();
        let digest = store_skill(&home, "handoff", "hand off work\n");
        write_manifest(
            &home,
            "version = 1\n[sources.mp]\ngithub = \"mattpocock/skills\"\nref = \"main\"\n",
        );
        write_lock(
            &home,
            &lock_toml(
                "mp",
                "github",
                "https://github.com/mattpocock/skills.git",
                Some("main"),
                &[("handoff", "skills/handoff", "git:abc123", &digest)],
            ),
        );

        let report = run(&home.paths, Network::Offline).unwrap();

        assert!(report.failures.is_empty());
        assert_eq!(report.deployed, ["mp-handoff"]);
        let text = fs::read_to_string(home.paths.targets[1].join("mp-handoff/SKILL.md")).unwrap();
        assert!(text.contains("name: mp-handoff\n"));
        assert!(text.contains("\n  sm-source-url: https://github.com/mattpocock/skills.git\n"));
        assert!(text.contains("\n  sm-upstream-path: skills/handoff\n"));
        assert!(text.contains("\n  sm-upstream-coordinate: git:abc123\n"));
    }

    #[test]
    fn aborts_when_the_manifest_no_longer_matches_the_lock() {
        let home = home();
        let digest = store_skill(&home, "handoff", "body\n");
        write_manifest(
            &home,
            "version = 1\n[sources.mp]\ngithub = \"mattpocock/skills\"\nref = \"next\"\n",
        );
        write_lock(
            &home,
            &lock_toml(
                "mp",
                "github",
                "https://github.com/mattpocock/skills.git",
                Some("main"),
                &[("handoff", "skills/handoff", "git:abc", &digest)],
            ),
        );

        let error = run(&home.paths, Network::Offline).unwrap_err().to_string();

        assert!(error.contains("skill-manager update"));
    }

    #[test]
    fn aborts_when_a_selected_remote_skill_has_no_lock_entry() {
        let home = home();
        let digest = store_skill(&home, "handoff", "body\n");
        write_manifest(
            &home,
            "version = 1\n[sources.mp]\ngithub = \"mattpocock/skills\"\nskills = [\"handoff\", \"tdd\"]\n",
        );
        write_lock(
            &home,
            &lock_toml(
                "mp",
                "github",
                "https://github.com/mattpocock/skills.git",
                None,
                &[("handoff", "skills/handoff", "git:abc", &digest)],
            ),
        );

        let error = run(&home.paths, Network::Offline).unwrap_err().to_string();

        assert!(error.contains("mp-tdd"));
        assert!(error.contains("skill-manager update"));
    }

    #[test]
    fn ignores_locked_skills_that_are_no_longer_selected() {
        let home = home();
        let digest = store_skill(&home, "handoff", "body\n");
        write_manifest(
            &home,
            "version = 1\n[sources.mp]\ngithub = \"mattpocock/skills\"\nskills = [\"handoff\"]\n",
        );
        write_lock(
            &home,
            &lock_toml(
                "mp",
                "github",
                "https://github.com/mattpocock/skills.git",
                None,
                &[
                    ("handoff", "skills/handoff", "git:abc", &digest),
                    ("retired", "skills/retired", "git:abc", &digest),
                ],
            ),
        );

        let report = run(&home.paths, Network::Offline).unwrap();

        assert_eq!(report.deployed, ["mp-handoff"]);
    }

    fn mp_lock(entries: &[(&str, &str, &str, &str)]) -> String {
        lock_toml(
            "mp",
            "github",
            "https://github.com/mattpocock/skills.git",
            None,
            entries,
        )
    }

    #[test]
    fn hydrates_a_missing_store_entry_before_deploying() {
        let home = home();
        let upstream = git_origin(&[("handoff/SKILL.md", &skill_text("handoff", "body\n"))]);
        write_manifest(
            &home,
            &format!(
                "version = 1\n[sources.mp]\ngit = \"{}\"\n",
                upstream.path().display()
            ),
        );
        crate::commands::update::run(&home.paths, None, true).unwrap();
        crate::fsx::remove_tree(&home.paths.store).unwrap();

        let report = run(&home.paths, Network::Allowed).unwrap();

        assert!(report.failures.is_empty(), "{:?}", report.failures);
        let names: Vec<&str> = report
            .hydrated
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(names, ["mp-handoff"]);
        assert!(report.hydrated[0].1.is_dir());
        assert!(home.paths.targets[0].join("mp-handoff/SKILL.md").is_file());
    }

    #[test]
    fn offline_refuses_to_fetch_a_missing_store_entry() {
        let home = home();
        let absent = format!("sha256:{}", "b".repeat(64));
        write_manifest(
            &home,
            "version = 1\n[sources.mp]\ngithub = \"mattpocock/skills\"\n",
        );
        write_lock(
            &home,
            &mp_lock(&[("handoff", "skills/handoff", "git:abc", &absent)]),
        );

        let error = run(&home.paths, Network::Offline).unwrap_err().to_string();

        assert!(error.contains("skill-manager hydrate"), "{error}");
        assert!(error.contains("--offline"), "{error}");
        assert!(!home.paths.current.exists());
    }

    #[test]
    fn a_failed_hydrate_aborts_before_deploying() {
        let home = home();
        let absent = format!("sha256:{}", "b".repeat(64));
        write_manifest(
            &home,
            "version = 1\n[sources.mp]\ngit = \"/nonexistent/repo.git\"\n",
        );
        write_lock(
            &home,
            &lock_toml(
                "mp",
                "git",
                "/nonexistent/repo.git",
                None,
                &[(
                    "handoff",
                    "handoff",
                    &format!("git:{}", "a".repeat(40)),
                    &absent,
                )],
            ),
        );

        let error = run(&home.paths, Network::Allowed).unwrap_err().to_string();

        assert!(error.contains("hydrate"), "{error}");
        assert!(error.contains("mp"), "{error}");
        assert!(!home.paths.current.exists());
    }

    #[test]
    fn hydrates_only_the_skills_the_manifest_selects() {
        let home = home();
        let digest = store_skill(&home, "handoff", "body\n");
        let absent = format!("sha256:{}", "b".repeat(64));
        write_manifest(
            &home,
            "version = 1\n[sources.mp]\ngithub = \"mattpocock/skills\"\nskills = [\"handoff\"]\n",
        );
        write_lock(
            &home,
            &mp_lock(&[
                ("handoff", "skills/handoff", "git:abc", &digest),
                ("retired", "skills/retired", "git:abc", &absent),
            ]),
        );

        let report = run(&home.paths, Network::Allowed).unwrap();

        assert!(report.hydrated.is_empty());
        assert_eq!(report.deployed, ["mp-handoff"]);
    }

    #[test]
    fn re_running_changes_nothing() {
        let home = home();
        write_skill(
            &home.path().join("etc/ai/skills/codemode"),
            "codemode",
            "body\n",
        );
        write_manifest(
            &home,
            "version = 1\n[sources.me]\npath = \"{home}/etc/ai/skills\"\n",
        );

        let first = run(&home.paths, Network::Offline).unwrap();
        let second = run(&home.paths, Network::Offline).unwrap();

        assert_eq!(first.generation, second.generation);
        assert!(!second.switched);
    }
}
