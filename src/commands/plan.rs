//! Turning manifest plus lock plus store into the set of skills to deploy.

use crate::error::{Error, Failures, Result};
use crate::fetch;
use crate::frontmatter::SkillDoc;
use crate::generate::{Deployable, MAX_NAME_LENGTH, Origin, namespaced};
use crate::lock::Lock;
use crate::manifest::{Manifest, Source};
use crate::skill::{self, SKILL_FILE};
use crate::store::Store;
use crate::tree::{Digest, Tree};

#[derive(Debug, Default)]
pub struct Plan {
    pub deployables: Vec<Deployable>,
    pub failures: Failures,
}

/// Remote lock entries must still describe the manifest's declared intent.
pub fn check_drift(manifest: &Manifest, lock: &Lock) -> Result<()> {
    for source in manifest.sources.values() {
        if source.transport.is_local() {
            continue;
        }
        let source_id = &source.source_id;
        let Some(locked) = lock.sources.get(source_id) else {
            return Err(drift(format!("source `{source_id}` has no lock entry")));
        };
        let expected = (
            source.transport.kind().to_string(),
            source.transport.url(),
            source.git_ref.clone(),
            source.dir.clone(),
        );
        let found = (
            locked.kind.clone(),
            locked.url.clone(),
            locked.git_ref.clone(),
            locked.dir.clone(),
        );
        if expected != found {
            return Err(drift(format!(
                "source `{source_id}` no longer matches its lock entry ({} {} ref={:?} dir={:?} locked as {} {} ref={:?} dir={:?})",
                expected.0, expected.1, expected.2, expected.3, found.0, found.1, found.2, found.3
            )));
        }
        for selected in source.skills.iter().flatten() {
            if !locked.skills.contains_key(selected) {
                return Err(drift(format!(
                    "`{}` has no lock entry",
                    namespaced(source_id, selected)
                )));
            }
        }
    }
    Ok(())
}

fn drift(what: impl std::fmt::Display) -> Error {
    Error::Drift(format!("{what}; run `skill-manager update`"))
}

/// Builds every deployable offline. Invalid individual skills are collected,
/// not fatal; a missing store entry is fatal.
pub fn build(manifest: &Manifest, lock: &Lock, store: &Store) -> Result<Plan> {
    let mut plan = Plan::default();
    for source in manifest.sources.values() {
        if source.transport.is_local() {
            add_local(source, &mut plan)?;
        } else {
            add_remote(source, lock, store, &mut plan)?;
        }
    }
    plan.deployables.sort_by_key(Deployable::deployed_name);
    reject_oversized_names(&mut plan);
    Ok(plan)
}

/// The deployed name still has to fit the agent-skills cap (ADR-0013).
fn reject_oversized_names(plan: &mut Plan) {
    let (kept, oversized): (Vec<Deployable>, Vec<Deployable>) = plan
        .deployables
        .drain(..)
        .partition(|deployable| deployable.deployed_name().chars().count() <= MAX_NAME_LENGTH);
    for deployable in oversized {
        let name = deployable.deployed_name();
        plan.failures.push(format!(
            "{name}: deployed name is {} characters; the limit is {MAX_NAME_LENGTH}",
            name.chars().count()
        ));
    }
    plan.deployables = kept;
}

fn add_remote(source: &Source, lock: &Lock, store: &Store, plan: &mut Plan) -> Result<()> {
    let Some(locked) = lock.sources.get(&source.source_id) else {
        return Ok(());
    };
    for (name, entry) in &locked.skills {
        if !source.selects(name) {
            continue;
        }
        let digest = Digest::parse(&entry.digest).ok_or_else(|| {
            Error::Lock(format!(
                "`{}` has an unusable digest",
                namespaced(&source.source_id, name)
            ))
        })?;
        let tree = store.tree(&digest)?;
        match read_doc(&tree) {
            Ok(doc) => plan.deployables.push(Deployable {
                source_id: source.source_id.clone(),
                name: name.clone(),
                doc,
                tree,
                origin: Origin {
                    source_kind: source.transport.kind().to_string(),
                    source_url: locked.url.clone(),
                    upstream_name: entry.upstream_name.clone(),
                    upstream_path: entry.path.clone(),
                    upstream_coordinate: entry.coordinate.clone(),
                    digest: entry.digest.clone(),
                },
            }),
            Err(reason) => plan
                .failures
                .push(format!("{}: {reason}", namespaced(&source.source_id, name))),
        }
    }
    Ok(())
}

fn add_local(source: &Source, plan: &mut Plan) -> Result<()> {
    let root = fetch::local_root(source)?;
    let discovery = skill::discover(&root, source.dir.as_deref())?;
    for invalid in &discovery.invalid {
        if source.selects(&invalid.name) {
            plan.failures.push(format!(
                "{}: {}",
                namespaced(&source.source_id, &invalid.name),
                invalid.reason
            ));
        }
    }

    let mut selected_names: Vec<&String> = Vec::new();
    for found in &discovery.skills {
        if !source.selects(&found.name) {
            continue;
        }
        selected_names.push(&found.name);
        let digest = found.tree.digest()?.to_string();
        plan.deployables.push(Deployable {
            source_id: source.source_id.clone(),
            name: found.name.clone(),
            doc: found.doc.clone(),
            tree: found.tree.clone(),
            origin: Origin {
                source_kind: source.transport.kind().to_string(),
                source_url: source.transport.url(),
                upstream_name: found.upstream_name.clone(),
                upstream_path: skill::relative_path(&root, found),
                // A local source's content digest is its only honest version (ADR-0003).
                upstream_coordinate: digest.clone(),
                digest,
            },
        });
    }

    for missing in source.skills.iter().flatten() {
        if !selected_names.contains(&missing)
            && !discovery
                .invalid
                .iter()
                .any(|invalid| &invalid.name == missing)
        {
            plan.failures.push(format!(
                "{}: not found in {}",
                namespaced(&source.source_id, missing),
                root.display()
            ));
        }
    }
    Ok(())
}

fn read_doc(tree: &Tree) -> std::result::Result<SkillDoc, String> {
    let bytes = tree
        .read_file(SKILL_FILE)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("{SKILL_FILE} is missing"))?;
    let text = String::from_utf8(bytes).map_err(|_| format!("{SKILL_FILE} is not valid UTF-8"))?;
    SkillDoc::parse(&text).map_err(|error| error.to_string())
}
