//! `apply` — offline materialization of the deployed tree (ADR-0004).

use crate::commands::plan;
use crate::deploy::{Deployment, TargetReport};
use crate::error::{Failures, Result};
use crate::lock::Lock;
use crate::manifest::Manifest;
use crate::paths::Paths;
use crate::store::Store;
use crate::tree::Digest;

#[derive(Debug)]
pub struct ApplyReport {
    pub generation: Digest,
    pub switched: bool,
    pub deployed: Vec<String>,
    pub targets: Vec<TargetReport>,
    pub failures: Failures,
}

pub fn run(paths: &Paths) -> Result<ApplyReport> {
    let manifest = Manifest::read(&paths.manifest)?;
    let lock = Lock::read(&paths.lock)?;
    run_with(paths, &manifest, &lock)
}

/// Shared with `update`, `add`, and `remove`, which already hold both files.
pub fn run_with(paths: &Paths, manifest: &Manifest, lock: &Lock) -> Result<ApplyReport> {
    plan::check_drift(manifest, lock)?;
    let store = Store::new(&paths.store);
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
        generation: applied.generation,
        switched: applied.switched,
        deployed: plan.deployables.iter().map(|d| d.deployed_name()).collect(),
        targets: applied.targets,
        failures: plan.failures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::*;
    use std::fs;

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

        let report = run(&home.paths).unwrap();

        assert!(report.failures.is_empty());
        assert_eq!(report.deployed, ["me-codemode"]);
        let deployed = home.paths.targets[0].join("me-codemode/SKILL.md");
        let text = fs::read_to_string(deployed).unwrap();
        assert!(text.contains("name: me-codemode\n"));
        assert!(text.contains("metadata:\n"));
        assert!(text.contains("\n  sm-source-id: me\n"));
        assert!(text.contains("\n  sm-source-kind: path\n"));
        assert!(text.contains("\n  sm-upstream-name: codemode\n"));
        assert!(text.contains(&format!(
            "\n  sm-source-url: {}/etc/ai/skills\n",
            home.path().display()
        )));
        assert!(text.contains("\n  sm-upstream-path: codemode\n"));
        assert!(text.contains("\n  sm-upstream-coordinate: sha256:"));
        assert!(text.contains("\n  sm-digest: sha256:"));
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

        let report = run(&home.paths).unwrap();

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

        let report = run(&home.paths).unwrap();

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

        let report = run(&home.paths).unwrap();

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

        let report = run(&home.paths).unwrap();

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

        let error = run(&home.paths).unwrap_err().to_string();

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

        let error = run(&home.paths).unwrap_err().to_string();

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

        let report = run(&home.paths).unwrap();

        assert_eq!(report.deployed, ["mp-handoff"]);
    }

    #[test]
    fn fails_loudly_when_the_store_entry_is_missing() {
        let home = home();
        let absent = format!("sha256:{}", "b".repeat(64));
        write_manifest(
            &home,
            "version = 1\n[sources.mp]\ngithub = \"mattpocock/skills\"\n",
        );
        write_lock(
            &home,
            &lock_toml(
                "mp",
                "github",
                "https://github.com/mattpocock/skills.git",
                None,
                &[("handoff", "skills/handoff", "git:abc", &absent)],
            ),
        );

        let error = run(&home.paths).unwrap_err().to_string();

        assert!(error.contains("hydrate"));
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

        let first = run(&home.paths).unwrap();
        let second = run(&home.paths).unwrap();

        assert_eq!(first.generation, second.generation);
        assert!(!second.switched);
    }
}
