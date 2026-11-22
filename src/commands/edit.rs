//! `add` and `remove` — the only commands that write the manifest.

use crate::commands::apply::{self, ApplyReport};
use crate::commands::update;
use crate::error::{Error, Failures, Result};
use crate::fsx;
use crate::generate::{namespaced, split_namespaced};
use crate::lock::{self, Lock, SourceDiff};
use crate::manifest::{Manifest, Source};
use crate::manifest_edit::{self, Edit};
use crate::paths::Paths;
use crate::skill;
use crate::store::Store;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug)]
pub struct EditReport {
    pub changed: Vec<String>,
    /// Selections that were already in the requested state.
    pub unchanged: Vec<String>,
    /// The manifest file written, when any selection changed.
    pub manifest: Option<PathBuf>,
    /// The lock file written; only `add` writes it.
    pub lock: Option<PathBuf>,
    /// How the written lock differs, for the sources `add` resolved.
    pub lock_changes: Vec<SourceDiff>,
    /// Store entries the preflight fetch added, by namespaced skill.
    pub stored: Vec<(String, PathBuf)>,
    pub applied: Option<ApplyReport>,
    pub failures: Failures,
}

/// `<source-id>-<skill>`, as typed on the command line.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Selection {
    pub source_id: String,
    pub skill: String,
}

impl std::fmt::Display for Selection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", namespaced(&self.source_id, &self.skill))
    }
}

pub fn parse_selection(text: &str) -> Result<Selection> {
    let (source_id, skill) = split_namespaced(text).ok_or_else(|| {
        Error::Configuration(format!(
            "`{text}` is not a namespaced name; expected <source-id>-<skill>"
        ))
    })?;
    Ok(Selection {
        source_id: source_id.to_string(),
        skill: skill.to_string(),
    })
}

pub fn add(paths: &Paths, requested: &[String]) -> Result<EditReport> {
    let selections = parse_all(requested)?;
    let text = read_manifest(paths)?;
    let manifest = Manifest::parse(&text)?;
    let lock = Lock::read(&paths.lock)?;
    let store = Store::new(&paths.store);

    let mut unchanged = Vec::new();
    let mut wanted: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for selection in &selections {
        let source = manifest.source(&selection.source_id)?;
        if manifest_edit::selects_all(&text, &source.source_id)? {
            unchanged.push(format!(
                "{selection}: source `{}` already selects every skill",
                source.source_id
            ));
            continue;
        }
        wanted
            .entry(selection.source_id.clone())
            .or_default()
            .push(selection.skill.clone());
    }

    // Preflight: confirm every requested skill exists before either file changes.
    let mut failures = Failures::new();
    let mut staged = lock.clone();
    for (source_id, skills) in &wanted {
        let source = manifest.source(source_id)?;
        confirm_available(source, skills, &lock, &store, &mut staged, &mut failures)?;
    }

    let mut edited = text.clone();
    let mut changed = Vec::new();
    for selection in &selections {
        if !wanted
            .get(&selection.source_id)
            .is_some_and(|skills| skills.contains(&selection.skill))
        {
            continue;
        }
        match manifest_edit::add_skill(&edited, &selection.source_id, &selection.skill)? {
            Edit::Changed(next) => {
                edited = next;
                changed.push(selection.to_string());
            }
            Edit::Unchanged => unchanged.push(format!("{selection}: already selected")),
        }
    }

    let stored = update::stored_entries(&store, &staged);
    if changed.is_empty() {
        return Ok(EditReport {
            changed,
            unchanged,
            manifest: None,
            lock: None,
            lock_changes: Vec::new(),
            stored,
            applied: None,
            failures,
        });
    }

    let manifest = Manifest::parse(&edited)?;
    let manifest_written = fsx::write_atomic(&paths.manifest, edited.as_bytes())?;
    let lock_written = staged.write(&paths.lock)?;
    let lock_changes = lock::diff(&lock, &staged)
        .into_iter()
        .filter(|source| source.changed() || wanted.contains_key(&source.source_id))
        .collect();
    let applied = apply::run_with(paths, &manifest, &staged, apply::Network::Allowed)?;
    for failure in applied.failures.iter() {
        failures.push(failure);
    }

    Ok(EditReport {
        changed,
        unchanged,
        manifest: Some(manifest_written),
        lock: Some(lock_written),
        lock_changes,
        stored,
        applied: Some(applied),
        failures,
    })
}

pub fn remove(paths: &Paths, requested: &[String]) -> Result<EditReport> {
    let selections = parse_all(requested)?;
    let text = read_manifest(paths)?;
    let manifest = Manifest::parse(&text)?;

    for selection in &selections {
        let source = manifest.source(&selection.source_id)?;
        if manifest_edit::selects_all(&text, &source.source_id)? {
            return Err(Error::Configuration(format!(
                "source `{}` omits `skills`, so `{selection}` cannot be removed: the manifest has no `all except` form",
                source.source_id
            )));
        }
    }

    let mut edited = text.clone();
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    for selection in &selections {
        match manifest_edit::remove_skill(&edited, &selection.source_id, &selection.skill)? {
            Edit::Changed(next) => {
                edited = next;
                changed.push(selection.to_string());
            }
            Edit::Unchanged => unchanged.push(format!("{selection}: not selected")),
        }
    }

    if changed.is_empty() {
        return Ok(EditReport {
            changed,
            unchanged,
            manifest: None,
            lock: None,
            lock_changes: Vec::new(),
            stored: Vec::new(),
            applied: None,
            failures: Failures::new(),
        });
    }

    let manifest = Manifest::parse(&edited)?;
    let manifest_written = fsx::write_atomic(&paths.manifest, edited.as_bytes())?;
    let lock = Lock::read(&paths.lock)?;
    let applied = apply::run_with(paths, &manifest, &lock, apply::Network::Allowed)?;
    let mut failures = Failures::new();
    for failure in applied.failures.iter() {
        failures.push(failure);
    }

    Ok(EditReport {
        changed,
        unchanged,
        manifest: Some(manifest_written),
        lock: None,
        lock_changes: Vec::new(),
        stored: Vec::new(),
        applied: Some(applied),
        failures,
    })
}

/// Resolves only the affected remote source_ids; local sources need no resolution.
fn confirm_available(
    source: &Source,
    skills: &[String],
    lock: &Lock,
    store: &Store,
    staged: &mut Lock,
    failures: &mut Failures,
) -> Result<()> {
    if source.transport.is_local() {
        // `add` and `remove` name this source outright, so an absent path is fatal
        // here even though `apply` skips it.
        let root = crate::fetch::local_root(source)?.ok_or_else(|| {
            Error::Configuration(format!(
                "source `{}`: {} does not exist",
                source.source_id,
                source.transport.url()
            ))
        })?;
        let discovery = skill::discover(&root, source.dir.as_deref())?;
        for skill in skills {
            if discovery.get(skill).is_none() {
                return Err(Error::Configuration(format!(
                    "`{}` was not found in {}",
                    namespaced(&source.source_id, skill),
                    root.display()
                )));
            }
        }
        return Ok(());
    }

    let previous = lock.sources.get(&source.source_id);
    let wanted: Vec<String> = skills.to_vec();
    let locked = update::stage_selection(
        source,
        previous,
        store,
        failures,
        &|name| wanted.iter().any(|skill| skill == name),
        &wanted,
    )?;

    for skill in skills {
        if !locked.skills.contains_key(skill) {
            return Err(Error::Configuration(format!(
                "`{}` was not found in {}",
                namespaced(&source.source_id, skill),
                source.transport.url()
            )));
        }
    }

    let entry = staged
        .sources
        .entry(source.source_id.clone())
        .or_insert_with(|| locked.clone());
    entry.kind = locked.kind.clone();
    entry.url = locked.url.clone();
    entry.git_ref = locked.git_ref.clone();
    entry.dir = locked.dir.clone();
    entry.skills.extend(locked.skills);
    Ok(())
}

fn parse_all(requested: &[String]) -> Result<Vec<Selection>> {
    if requested.is_empty() {
        return Err(Error::Configuration(
            "expected at least one <source-id>-<skill>".to_string(),
        ));
    }
    requested.iter().map(|text| parse_selection(text)).collect()
}

fn read_manifest(paths: &Paths) -> Result<String> {
    std::fs::read_to_string(&paths.manifest).map_err(|error| Error::io(&paths.manifest, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::testing::*;
    use std::fs;

    fn local_home(skills: &[&str], selected: &str) -> Home {
        let home = home();
        for name in skills {
            write_skill(
                &home.path().join("etc/ai/skills").join(name),
                name,
                "body\n",
            );
        }
        write_manifest(
            &home,
            &format!(
                "version = 1\n# mine\n[sources.me]\npath = \"{{home}}/etc/ai/skills\"\nskills = [{selected}]\n"
            ),
        );
        home
    }

    #[test]
    fn add_selects_a_local_skill_and_deploys_it() {
        let home = local_home(&["codemode", "tdd"], "\"codemode\"");

        let report = add(&home.paths, &["me-tdd".to_string()]).unwrap();

        assert_eq!(report.changed, ["me-tdd"]);
        let text = fs::read_to_string(&home.paths.manifest).unwrap();
        assert!(text.contains("# mine"));
        assert!(text.contains(r#"skills = ["codemode", "tdd"]"#));
        assert!(home.paths.targets[0].join("me-tdd").exists());
    }

    #[test]
    fn add_writes_through_a_symlinked_manifest() {
        let home = local_home(&["codemode", "tdd"], "\"codemode\"");
        let repo = home.path().join("etc/skills.toml");
        fs::rename(&home.paths.manifest, &repo).unwrap();
        std::os::unix::fs::symlink(&repo, &home.paths.manifest).unwrap();

        let report = add(&home.paths, &["me-tdd".to_string()]).unwrap();

        assert_eq!(report.manifest, Some(repo.clone()));
        assert!(
            fs::symlink_metadata(&home.paths.manifest)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            fs::read_to_string(&repo)
                .unwrap()
                .contains(r#"skills = ["codemode", "tdd"]"#)
        );
    }

    #[test]
    fn add_leaves_both_files_unchanged_when_a_skill_does_not_exist() {
        let home = local_home(&["codemode"], "\"codemode\"");
        let before = fs::read_to_string(&home.paths.manifest).unwrap();

        let error = add(&home.paths, &["me-absent".to_string()])
            .unwrap_err()
            .to_string();

        assert!(error.contains("me-absent"));
        assert_eq!(fs::read_to_string(&home.paths.manifest).unwrap(), before);
        assert!(!home.paths.lock.exists());
    }

    #[test]
    fn add_is_a_reported_no_op_when_the_source_selects_everything() {
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

        let report = add(&home.paths, &["me-codemode".to_string()]).unwrap();

        assert!(report.changed.is_empty());
        assert!(report.unchanged[0].contains("already selects every skill"));
        assert!(report.applied.is_none());
    }

    #[test]
    fn add_reports_an_already_selected_skill() {
        let home = local_home(&["codemode"], "\"codemode\"");

        let report = add(&home.paths, &["me-codemode".to_string()]).unwrap();

        assert!(report.changed.is_empty());
        assert!(report.unchanged[0].contains("already selected"));
    }

    #[test]
    fn add_writes_a_lock_entry_for_a_remote_skill() {
        let home = home();
        let upstream = git_origin(&[
            ("handoff/SKILL.md", &skill_text("handoff", "body\n")),
            ("tdd/SKILL.md", &skill_text("tdd", "body\n")),
        ]);
        write_manifest(
            &home,
            &format!(
                "version = 1\n[sources.mp]\ngit = \"{}\"\nskills = [\"handoff\"]\n",
                upstream.path().display()
            ),
        );
        crate::commands::update::run(&home.paths, None, false).unwrap();

        let report = add(&home.paths, &["mp-tdd".to_string()]).unwrap();

        assert_eq!(report.changed, ["mp-tdd"]);
        assert_eq!(
            report.manifest.as_deref(),
            Some(home.paths.manifest.as_path())
        );
        assert_eq!(report.lock.as_deref(), Some(home.paths.lock.as_path()));
        let mp = &report.lock_changes[0];
        assert!(matches!(mp.skills["tdd"], crate::change::Change::Added(_)));
        assert!(matches!(
            mp.skills["handoff"],
            crate::change::Change::Unchanged(_)
        ));
        let stored: Vec<&str> = report
            .stored
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(stored, ["mp-tdd"]);
        let lock = Lock::read(&home.paths.lock).unwrap();
        assert!(lock.skill("mp", "handoff").is_some());
        assert!(lock.skill("mp", "tdd").is_some());
        assert!(home.paths.targets[0].join("mp-tdd/SKILL.md").is_file());
    }

    #[test]
    fn add_does_not_create_sources() {
        let home = local_home(&["codemode"], "\"codemode\"");
        assert!(
            add(&home.paths, &["other-x".to_string()])
                .unwrap_err()
                .to_string()
                .contains("no source")
        );
    }

    #[test]
    fn remove_deselects_and_undeploys() {
        let home = local_home(&["codemode", "tdd"], "\"codemode\", \"tdd\"");
        apply::run(&home.paths, apply::Network::Offline).unwrap();

        let report = remove(&home.paths, &["me-tdd".to_string()]).unwrap();

        assert_eq!(report.changed, ["me-tdd"]);
        assert_eq!(
            report.manifest.as_deref(),
            Some(home.paths.manifest.as_path())
        );
        assert_eq!(report.lock, None);
        assert!(
            fs::read_to_string(&home.paths.manifest)
                .unwrap()
                .contains(r#"skills = ["codemode"]"#)
        );
        assert!(!home.paths.targets[0].join("me-tdd").exists());
        assert!(home.paths.targets[0].join("me-codemode").exists());
    }

    #[test]
    fn remove_refuses_a_source_that_selects_everything() {
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

        let error = remove(&home.paths, &["me-codemode".to_string()])
            .unwrap_err()
            .to_string();

        assert!(error.contains("all except"));
    }

    #[test]
    fn remove_reports_a_skill_that_was_not_selected() {
        let home = local_home(&["codemode"], "\"codemode\"");

        let report = remove(&home.paths, &["me-other".to_string()]).unwrap();

        assert!(report.changed.is_empty());
        assert!(report.unchanged[0].contains("not selected"));
    }

    #[test]
    fn selections_must_be_namespaced() {
        assert!(parse_selection("handoff").is_err());
        assert!(parse_selection(":handoff").is_err());
        assert!(parse_selection("mp:").is_err());
        assert_eq!(
            parse_selection("mp-handoff").unwrap(),
            Selection {
                source_id: "mp".into(),
                skill: "handoff".into()
            }
        );
    }
}
