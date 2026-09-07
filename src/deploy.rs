//! One generated tree, symlinked into every target (ADR-0009).

use crate::error::{Error, IoContext, Result};
use crate::fsx;
use crate::generate::{self, Deployable};
use crate::paths::Paths;
use crate::tree::{Digest, Tree};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct TargetReport {
    pub path: PathBuf,
    pub linked: Vec<String>,
    pub removed: Vec<String>,
    /// Entries in a target that are not ours and block a deployed name.
    pub conflicts: Vec<String>,
}

#[derive(Debug)]
pub struct Applied {
    pub generation: Digest,
    /// False when `current` already pointed at this generation.
    pub switched: bool,
    pub targets: Vec<TargetReport>,
}

pub struct Deployment<'a> {
    paths: &'a Paths,
}

impl<'a> Deployment<'a> {
    pub fn new(paths: &'a Paths) -> Self {
        Deployment { paths }
    }

    pub fn apply(&self, deployables: &[Deployable]) -> Result<Applied> {
        let generation = self.materialize_generation(deployables)?;
        let switched = self.switch_current(&generation)?;
        let names: BTreeSet<String> = deployables
            .iter()
            .map(Deployable::deployed_name)
            .collect::<BTreeSet<_>>();
        let names = expected_names(names);

        let mut targets = Vec::new();
        for target in &self.paths.targets {
            targets.push(self.sync_target(target, &names)?);
        }
        self.prune_generations(&generation)?;

        Ok(Applied {
            generation,
            switched,
            targets,
        })
    }

    /// Builds the generation unless its digest is already present.
    fn materialize_generation(&self, deployables: &[Deployable]) -> Result<Digest> {
        fs::create_dir_all(&self.paths.generations).at(&self.paths.generations)?;
        let staging =
            tempfile::TempDir::new_in(&self.paths.generations).at(&self.paths.generations)?;
        let staged = staging.path().join("generation");
        generate::build(&staged, deployables)?;

        let digest = Tree::read(&staged)?.digest()?;
        let destination = self.paths.generation(&digest);
        if destination.is_dir() {
            return Ok(digest);
        }
        let parent = destination
            .parent()
            .expect("generation paths have a parent");
        fs::create_dir_all(parent).at(parent)?;
        match fs::rename(&staged, &destination) {
            Ok(()) => fsx::make_read_only(&destination).map(|()| digest),
            Err(_) if destination.is_dir() => Ok(digest),
            Err(error) => Err(Error::io(&destination, error)),
        }
    }

    fn switch_current(&self, generation: &Digest) -> Result<bool> {
        let destination = self.paths.generation(generation);
        if fs::read_link(&self.paths.current).is_ok_and(|current| current == destination) {
            return Ok(false);
        }
        fsx::replace_symlink_atomic(&self.paths.current, &destination)?;
        Ok(true)
    }

    fn sync_target(&self, target: &Path, expected: &BTreeSet<String>) -> Result<TargetReport> {
        let mut report = TargetReport {
            path: target.to_path_buf(),
            ..TargetReport::default()
        };
        fs::create_dir_all(target).at(target)?;

        for entry in fs::read_dir(target).at(target)? {
            let path = entry.at(target)?.path();
            let name = file_name(&path);
            if !self.is_ours(&path) {
                if expected.contains(&name) {
                    report.conflicts.push(name);
                }
                continue;
            }
            if !expected.contains(&name) {
                fs::remove_file(&path).at(&path)?;
                report.removed.push(name);
            }
        }

        for name in expected {
            let link = target.join(name);
            if report.conflicts.contains(name) {
                continue;
            }
            let wanted = self.paths.current.join(name);
            if fs::read_link(&link).is_ok_and(|existing| existing == wanted) {
                continue;
            }
            fsx::replace_symlink_atomic(&link, &wanted)?;
            report.linked.push(name.clone());
        }

        Ok(report)
    }

    /// Ownership is stateless: ours iff it is a symlink into the build directory.
    pub fn is_ours(&self, path: &Path) -> bool {
        fs::read_link(path).is_ok_and(|target| target.starts_with(&self.paths.deployed))
    }

    fn prune_generations(&self, keep: &Digest) -> Result<()> {
        let kept = self.paths.generation(keep);
        let algorithms = match fs::read_dir(&self.paths.generations) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(Error::io(&self.paths.generations, error)),
        };
        for algorithm in algorithms {
            let algorithm = algorithm.at(&self.paths.generations)?.path();
            if !algorithm.is_dir() {
                continue;
            }
            for generation in fs::read_dir(&algorithm).at(&algorithm)? {
                let generation = generation.at(&algorithm)?.path();
                if generation != kept {
                    fsx::remove_tree(&generation)?;
                }
            }
        }
        Ok(())
    }
}

fn expected_names(mut names: BTreeSet<String>) -> BTreeSet<String> {
    names.insert(generate::namespaced(
        generate::BUILTIN_SOURCE_ID,
        "provenance",
    ));
    names
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::SkillDoc;
    use tempfile::TempDir;

    fn deployable(source_id: &str, name: &str, body: &str) -> (TempDir, Deployable) {
        let source = TempDir::new().unwrap();
        let text = format!("---\nname: {name}\ndescription: does {name}\n---\n{body}");
        fs::write(source.path().join("SKILL.md"), &text).unwrap();
        let deployable = Deployable {
            source_id: source_id.to_string(),
            name: name.to_string(),
            doc: SkillDoc::parse(&text).unwrap(),
            tree: Tree::read(source.path()).unwrap(),
            origin: generate::Origin {
                source_kind: "git".to_string(),
                source_url: "https://example.test/skills.git".to_string(),
                upstream_name: name.to_string(),
                upstream_path: name.to_string(),
                upstream_coordinate: "git:abc".to_string(),
                digest: format!("sha256:{}", "a".repeat(64)),
            },
        };
        (source, deployable)
    }

    fn home() -> (TempDir, Paths) {
        let home = TempDir::new().unwrap();
        let paths = Paths::rooted_at(home.path());
        (home, paths)
    }

    #[test]
    fn apply_links_every_target_through_current() {
        let (_home, paths) = home();
        let (_source, skill) = deployable("mp", "handoff", "body\n");

        let applied = Deployment::new(&paths).apply(&[skill]).unwrap();

        assert!(applied.switched);
        for target in &paths.targets {
            let link = target.join("mp-handoff");
            assert_eq!(
                fs::read_link(&link).unwrap(),
                paths.current.join("mp-handoff")
            );
            assert!(link.join("SKILL.md").is_file());
            assert!(target.join("sm-provenance/SKILL.md").is_file());
        }
        assert_eq!(
            fs::read_link(&paths.current).unwrap(),
            paths.generation(&applied.generation)
        );
    }

    #[test]
    fn re_applying_identical_input_is_a_no_op() {
        let (_home, paths) = home();
        let (_source, skill) = deployable("mp", "handoff", "body\n");

        let first = Deployment::new(&paths)
            .apply(std::slice::from_ref(&skill))
            .unwrap();
        let second = Deployment::new(&paths).apply(&[skill]).unwrap();

        assert_eq!(first.generation, second.generation);
        assert!(!second.switched);
        assert!(second.targets.iter().all(|target| target.linked.is_empty()));
    }

    #[test]
    fn changed_content_switches_current_and_prunes_the_old_generation() {
        let (_home, paths) = home();
        let (_a, before) = deployable("mp", "handoff", "one\n");
        let (_b, after) = deployable("mp", "handoff", "two\n");

        let first = Deployment::new(&paths).apply(&[before]).unwrap();
        let second = Deployment::new(&paths).apply(&[after]).unwrap();

        assert_ne!(first.generation, second.generation);
        assert!(second.switched);
        assert!(!paths.generation(&first.generation).exists());
        assert_eq!(
            fs::read_to_string(paths.targets[0].join("mp-handoff/SKILL.md"))
                .unwrap()
                .lines()
                .last(),
            Some("two")
        );
    }

    #[test]
    fn deselected_skills_lose_their_target_symlinks() {
        let (_home, paths) = home();
        let (_a, one) = deployable("mp", "handoff", "body\n");
        let (_b, two) = deployable("mp", "tdd", "body\n");

        Deployment::new(&paths).apply(&[one.clone(), two]).unwrap();
        let applied = Deployment::new(&paths).apply(&[one]).unwrap();

        assert!(
            applied
                .targets
                .iter()
                .all(|target| target.removed == ["mp-tdd"])
        );
        assert!(!paths.targets[0].join("mp-tdd").exists());
        assert!(paths.targets[0].join("mp-handoff").exists());
    }

    #[test]
    fn foreign_target_entries_are_reported_and_left_alone() {
        let (_home, paths) = home();
        let (_source, skill) = deployable("mp", "handoff", "body\n");
        let foreign = paths.targets[0].join("mp-handoff");
        fs::create_dir_all(&foreign).unwrap();
        fs::write(foreign.join("SKILL.md"), "not ours").unwrap();

        let applied = Deployment::new(&paths).apply(&[skill]).unwrap();

        assert_eq!(applied.targets[0].conflicts, ["mp-handoff"]);
        assert_eq!(
            fs::read_to_string(foreign.join("SKILL.md")).unwrap(),
            "not ours"
        );
        assert!(applied.targets[1].conflicts.is_empty());
    }

    #[test]
    fn unrelated_target_entries_are_never_touched() {
        let (_home, paths) = home();
        let (_source, skill) = deployable("mp", "handoff", "body\n");
        let stranger = paths.targets[0].join("hand-written");
        fs::create_dir_all(&stranger).unwrap();
        fs::write(stranger.join("SKILL.md"), "mine").unwrap();

        Deployment::new(&paths).apply(&[skill]).unwrap();

        assert!(stranger.join("SKILL.md").exists());
    }

    #[test]
    fn removing_a_target_entry_does_not_recurse_through_the_symlink() {
        let (_home, paths) = home();
        let (_a, one) = deployable("mp", "handoff", "body\n");

        let applied = Deployment::new(&paths).apply(&[one]).unwrap();
        let generation = paths.generation(&applied.generation);
        Deployment::new(&paths).apply(&[]).unwrap();

        assert!(!paths.targets[0].join("mp-handoff").exists());
        assert!(!generation.exists() || generation.join("mp-handoff/SKILL.md").exists());
    }

    #[test]
    fn a_stale_symlink_into_the_build_directory_is_repointed() {
        let (_home, paths) = home();
        let (_source, skill) = deployable("mp", "handoff", "body\n");
        let link = paths.targets[0].join("mp-handoff");
        fs::create_dir_all(&paths.targets[0]).unwrap();
        std::os::unix::fs::symlink(
            paths.deployed.join("generations/sha256/old/mp-handoff"),
            &link,
        )
        .unwrap();

        let applied = Deployment::new(&paths).apply(&[skill]).unwrap();

        assert_eq!(
            fs::read_link(&link).unwrap(),
            paths.current.join("mp-handoff")
        );
        assert!(applied.targets[0].conflicts.is_empty());
    }
}
