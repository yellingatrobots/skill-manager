//! `update` — the only command that resolves refs and fetches (ADR-0004).

use crate::commands::apply::{self, ApplyReport};
use crate::error::{Error, Failures, Result};
use crate::fetch;
use crate::lock::{Lock, LockedSkill, LockedSource};
use crate::manifest::{Manifest, Source};
use crate::paths::Paths;
use crate::skill::{self, Discovery, Skill};
use crate::store::Store;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug)]
pub struct UpdateReport {
    pub updated: Vec<String>,
    pub applied: Option<ApplyReport>,
    pub failures: Failures,
}

pub fn run(paths: &Paths, alias: Option<&str>, no_apply: bool) -> Result<UpdateReport> {
    let manifest = Manifest::read(&paths.manifest)?;
    let previous = Lock::read(&paths.lock)?;
    let store = Store::new(&paths.store);

    let selected = select_sources(&manifest, alias)?;
    let mut failures = Failures::new();
    let mut staged: BTreeMap<String, LockedSource> = BTreeMap::new();

    // Everything is staged before the lock changes: one fetch failure aborts.
    for source in &selected {
        let locked = stage(source, previous.sources.get(&source.alias), &store, &mut failures)?;
        staged.insert(source.alias.clone(), locked);
    }

    let lock = merge(&manifest, previous, staged, alias);
    lock.write(&paths.lock)?;

    let unlockable = unlockable_selections(&manifest, &lock);
    for missing in &unlockable {
        failures.push(format!("`{missing}` could not be locked; adjust the manifest"));
    }

    let applied = if no_apply || !unlockable.is_empty() {
        None
    } else {
        Some(apply::run_with(paths, &manifest, &lock)?)
    };
    if let Some(report) = &applied {
        for failure in report.failures.iter() {
            failures.push(failure);
        }
    }

    Ok(UpdateReport {
        updated: selected.iter().map(|source| source.alias.clone()).collect(),
        applied,
        failures,
    })
}

fn select_sources<'a>(manifest: &'a Manifest, alias: Option<&str>) -> Result<Vec<&'a Source>> {
    match alias {
        None => Ok(manifest.sources.values().filter(|s| !s.transport.is_local()).collect()),
        Some(alias) => {
            let source = manifest.source(alias)?;
            if source.transport.is_local() {
                return Err(Error::Configuration(format!(
                    "source `{alias}` is a local path; it is read directly by `apply`"
                )));
            }
            Ok(vec![source])
        }
    }
}

fn stage(
    source: &Source,
    previous: Option<&LockedSource>,
    store: &Store,
    failures: &mut Failures,
) -> Result<LockedSource> {
    let required: Vec<String> = source.skills.clone().unwrap_or_default();
    stage_selection(source, previous, store, failures, &|name| source.selects(name), &required)
}

/// Staging for an explicit set of skills, which `add` needs before the manifest
/// has been edited to select them.
pub(crate) fn stage_selection(
    source: &Source,
    previous: Option<&LockedSource>,
    store: &Store,
    failures: &mut Failures,
    selects: &dyn Fn(&str) -> bool,
    required: &[String],
) -> Result<LockedSource> {
    let mut locked = LockedSource {
        kind: source.transport.kind().to_string(),
        url: source.transport.url(),
        git_ref: source.git_ref.clone(),
        dir: source.dir.clone(),
        skills: BTreeMap::new(),
    };

    match &source.transport {
        crate::manifest::Transport::WellKnown { endpoint } => stage_well_known(
            source, endpoint, store, previous, &mut locked, failures, selects, required,
        )?,
        _ => stage_git(source, store, previous, &mut locked, failures, selects, required)?,
    }
    Ok(locked)
}

#[allow(clippy::too_many_arguments)]
fn stage_git(
    source: &Source,
    store: &Store,
    previous: Option<&LockedSource>,
    locked: &mut LockedSource,
    failures: &mut Failures,
    selects: &dyn Fn(&str) -> bool,
    required: &[String],
) -> Result<()> {
    let checkout = fetch::checkout(source, None)?;
    let discovery = skill::discover(&checkout.root, source.dir.as_deref())?;

    for found in &discovery.skills {
        if !selects(&found.name) {
            continue;
        }
        let digest = store.insert(&found.tree)?;
        locked.skills.insert(
            found.name.clone(),
            LockedSkill {
                path: relative_path(&checkout.root, found),
                upstream_name: found.upstream_name.clone(),
                coordinate: checkout.coordinate.clone(),
                digest: digest.to_string(),
            },
        );
    }

    retain_previous(source, &discovery, previous, locked, failures, selects, required);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn stage_well_known(
    source: &Source,
    endpoint: &str,
    store: &Store,
    previous: Option<&LockedSource>,
    locked: &mut LockedSource,
    failures: &mut Failures,
    selects: &dyn Fn(&str) -> bool,
    required: &[String],
) -> Result<()> {
    let index = fetch::well_known_index(endpoint)?;
    let mut discovery = Discovery::default();

    for entry in index.iter().filter(|entry| selects(&entry.name)) {
        let fetched = fetch::archive_skill(endpoint, &entry.name, &entry.url, Some(&entry.digest))?;
        let mut found = skill::discover(&fetched.root, None)?;
        for skill in found.skills.drain(..) {
            let digest = store.insert(&skill.tree)?;
            locked.skills.insert(
                skill.name.clone(),
                LockedSkill {
                    path: fetched.url_path.clone(),
                    upstream_name: skill.upstream_name.clone(),
                    coordinate: fetched.coordinate.clone(),
                    digest: digest.to_string(),
                },
            );
            discovery.skills.push(skill);
        }
        discovery.invalid.extend(found.invalid);
    }

    retain_previous(source, &discovery, previous, locked, failures, selects, required);
    Ok(())
}

/// A structurally invalid skill keeps its previous valid lock entry (ADR-0004).
fn retain_previous(
    source: &Source,
    discovery: &Discovery,
    previous: Option<&LockedSource>,
    locked: &mut LockedSource,
    failures: &mut Failures,
    selects: &dyn Fn(&str) -> bool,
    required: &[String],
) {
    for invalid in &discovery.invalid {
        if !selects(&invalid.name) {
            continue;
        }
        failures.push(format!("{}:{}: {}", source.alias, invalid.name, invalid.reason));
        if let Some(entry) = previous.and_then(|previous| previous.skills.get(&invalid.name)) {
            locked.skills.insert(invalid.name.clone(), entry.clone());
        }
    }

    for selected in required {
        if locked.skills.contains_key(selected)
            || discovery.invalid.iter().any(|invalid| &invalid.name == selected)
        {
            continue;
        }
        failures.push(format!("{}:{selected}: not found in the source", source.alias));
        if let Some(entry) = previous.and_then(|previous| previous.skills.get(selected)) {
            locked.skills.insert(selected.clone(), entry.clone());
        }
    }
}

fn relative_path(root: &Path, skill: &Skill) -> String {
    skill
        .tree
        .root()
        .strip_prefix(root)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| skill.name.clone())
}

/// Keeps untouched sources, drops sources the manifest no longer declares.
fn merge(
    manifest: &Manifest,
    previous: Lock,
    staged: BTreeMap<String, LockedSource>,
    alias: Option<&str>,
) -> Lock {
    let mut sources = previous.sources;
    if alias.is_none() {
        sources.retain(|alias, _| {
            manifest.sources.get(alias).is_some_and(|source| !source.transport.is_local())
        });
    }
    sources.extend(staged);
    Lock { sources }
}

fn unlockable_selections(manifest: &Manifest, lock: &Lock) -> Vec<String> {
    let mut missing = Vec::new();
    for source in manifest.sources.values() {
        if source.transport.is_local() {
            continue;
        }
        for selected in source.skills.iter().flatten() {
            if lock.skill(&source.alias, selected).is_none() {
                missing.push(format!("{}:{selected}", source.alias));
            }
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::*;
    use std::fs;

    #[test]
    fn resolves_a_git_source_into_the_lock_and_deploys_it() {
        let home = home();
        let upstream = git_origin(&[("skills/handoff/SKILL.md", &skill_text("handoff", "body\n"))]);
        write_manifest(
            &home,
            &format!("version = 1\n[sources.mp]\ngit = \"{}\"\ndir = \"skills\"\n", upstream.path().display()),
        );

        let report = run(&home.paths, None, false).unwrap();

        assert!(report.failures.is_empty(), "{:?}", report.failures);
        let lock = Lock::read(&home.paths.lock).unwrap();
        let entry = lock.skill("mp", "handoff").unwrap();
        assert_eq!(entry.path, "skills/handoff");
        assert_eq!(entry.upstream_name, "handoff");
        assert!(entry.coordinate.starts_with("git:"));
        assert!(entry.digest.starts_with("sha256:"));
        assert!(home.paths.targets[0].join("mp:handoff/SKILL.md").is_file());
    }

    #[test]
    fn no_apply_leaves_the_deployed_tree_alone() {
        let home = home();
        let upstream = git_origin(&[("handoff/SKILL.md", &skill_text("handoff", "body\n"))]);
        write_manifest(&home, &format!("version = 1\n[sources.mp]\ngit = \"{}\"\n", upstream.path().display()));

        let report = run(&home.paths, None, true).unwrap();

        assert!(report.applied.is_none());
        assert!(!home.paths.current.exists());
        assert!(Lock::read(&home.paths.lock).unwrap().skill("mp", "handoff").is_some());
    }

    #[test]
    fn a_fetch_failure_leaves_the_lock_unchanged() {
        let home = home();
        write_manifest(&home, "version = 1\n[sources.mp]\ngit = \"/nonexistent/repo.git\"\n");
        write_lock(&home, "version = 1\n");

        let error = run(&home.paths, None, false).unwrap_err();

        assert!(matches!(error, Error::Git(_)));
        assert_eq!(fs::read_to_string(&home.paths.lock).unwrap(), "version = 1\n");
    }

    #[test]
    fn an_invalid_skill_keeps_its_previous_entry_while_siblings_advance() {
        let home = home();
        let upstream = git_origin(&[
            ("handoff/SKILL.md", &skill_text("handoff", "one\n")),
            ("tdd/SKILL.md", &skill_text("tdd", "one\n")),
        ]);
        write_manifest(&home, &format!("version = 1\n[sources.mp]\ngit = \"{}\"\n", upstream.path().display()));
        run(&home.paths, None, false).unwrap();
        let before = Lock::read(&home.paths.lock).unwrap();

        git_commit(&upstream, &[
            ("handoff/SKILL.md", "broken, no frontmatter\n"),
            ("tdd/SKILL.md", &skill_text("tdd", "two\n")),
        ]);
        let report = run(&home.paths, None, false).unwrap();

        assert!(report.failures.iter().any(|failure| failure.starts_with("mp:handoff:")));
        let after = Lock::read(&home.paths.lock).unwrap();
        assert_eq!(after.skill("mp", "handoff"), before.skill("mp", "handoff"));
        assert_ne!(after.skill("mp", "tdd"), before.skill("mp", "tdd"));
        assert!(home.paths.targets[0].join("mp:handoff").exists());
        assert_eq!(
            fs::read_to_string(home.paths.targets[0].join("mp:tdd/SKILL.md")).unwrap().lines().last(),
            Some("two")
        );
    }

    #[test]
    fn a_selected_skill_that_does_not_exist_is_reported_and_blocks_apply() {
        let home = home();
        let upstream = git_origin(&[("handoff/SKILL.md", &skill_text("handoff", "body\n"))]);
        write_manifest(
            &home,
            &format!(
                "version = 1\n[sources.mp]\ngit = \"{}\"\nskills = [\"handoff\", \"absent\"]\n",
                upstream.path().display()
            ),
        );

        let report = run(&home.paths, None, false).unwrap();

        assert!(report.applied.is_none());
        assert!(report.failures.iter().any(|failure| failure.contains("mp:absent")));
        assert!(Lock::read(&home.paths.lock).unwrap().skill("mp", "handoff").is_some());
    }

    #[test]
    fn updating_one_alias_leaves_other_sources_untouched() {
        let home = home();
        let one = git_origin(&[("handoff/SKILL.md", &skill_text("handoff", "one\n"))]);
        let two = git_origin(&[("tdd/SKILL.md", &skill_text("tdd", "one\n"))]);
        write_manifest(
            &home,
            &format!(
                "version = 1\n[sources.mp]\ngit = \"{}\"\n\n[sources.kp]\ngit = \"{}\"\n",
                one.path().display(),
                two.path().display()
            ),
        );
        run(&home.paths, None, false).unwrap();
        let before = Lock::read(&home.paths.lock).unwrap();

        git_commit(&one, &[("handoff/SKILL.md", &skill_text("handoff", "two\n"))]);
        git_commit(&two, &[("tdd/SKILL.md", &skill_text("tdd", "two\n"))]);
        run(&home.paths, Some("mp"), false).unwrap();

        let after = Lock::read(&home.paths.lock).unwrap();
        assert_ne!(after.skill("mp", "handoff"), before.skill("mp", "handoff"));
        assert_eq!(after.skill("kp", "tdd"), before.skill("kp", "tdd"));
    }

    #[test]
    fn a_source_removed_from_the_manifest_leaves_the_lock() {
        let home = home();
        let one = git_origin(&[("handoff/SKILL.md", &skill_text("handoff", "one\n"))]);
        let two = git_origin(&[("tdd/SKILL.md", &skill_text("tdd", "one\n"))]);
        let both = format!(
            "version = 1\n[sources.mp]\ngit = \"{}\"\n\n[sources.kp]\ngit = \"{}\"\n",
            one.path().display(),
            two.path().display()
        );
        write_manifest(&home, &both);
        run(&home.paths, None, false).unwrap();

        write_manifest(&home, &format!("version = 1\n[sources.mp]\ngit = \"{}\"\n", one.path().display()));
        run(&home.paths, None, false).unwrap();

        let lock = Lock::read(&home.paths.lock).unwrap();
        assert!(lock.sources.contains_key("mp"));
        assert!(!lock.sources.contains_key("kp"));
        assert!(!home.paths.targets[0].join("kp:tdd").exists());
    }

    #[test]
    fn a_local_alias_cannot_be_updated() {
        let home = home();
        write_manifest(&home, "version = 1\n[sources.me]\npath = \"{home}/etc/ai/skills\"\n");
        assert!(run(&home.paths, Some("me"), false).is_err());
    }

    #[test]
    fn a_pinned_ref_resolves_to_the_same_coordinate_every_time() {
        let home = home();
        let upstream = git_origin(&[("handoff/SKILL.md", &skill_text("handoff", "one\n"))]);
        git_tag(&upstream, "v1");
        git_commit(&upstream, &[("handoff/SKILL.md", &skill_text("handoff", "two\n"))]);
        write_manifest(
            &home,
            &format!("version = 1\n[sources.mp]\ngit = \"{}\"\nref = \"v1\"\n", upstream.path().display()),
        );

        run(&home.paths, None, false).unwrap();
        let first = Lock::read(&home.paths.lock).unwrap();
        run(&home.paths, None, false).unwrap();
        let second = Lock::read(&home.paths.lock).unwrap();

        assert_eq!(first, second);
        assert_eq!(
            fs::read_to_string(home.paths.targets[0].join("mp:handoff/SKILL.md")).unwrap().lines().last(),
            Some("one")
        );
    }
}
