//! `info` — provenance for what is actually deployed.

use crate::error::{Error, Result};
use crate::frontmatter::SkillDoc;
use crate::generate::{BUILTIN_SOURCE_ID, METADATA, split_namespaced};
use crate::lock::Lock;
use crate::manifest::Manifest;
use crate::paths::Paths;
use crate::skill::SKILL_FILE;
use serde::Serialize;
use std::fs;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    pub name: String,
    pub source_id: String,
    pub skill: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    pub source_url: String,
    pub upstream_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_path: Option<String>,
    pub upstream_coordinate: String,
    /// The content digest of what the skill was generated from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// Where a local source currently resolves to on this machine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
    pub deployed_path: String,
}

/// Where this machine keeps every file the tool reads or writes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Locations {
    pub manifest: String,
    pub lock: String,
    pub store: String,
    pub deployed: String,
    /// The generation `current` points at, absent when nothing is deployed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    pub targets: Vec<String>,
}

/// What `info` reports: where things live, and what is deployed.
#[derive(Debug, Serialize)]
pub struct Report {
    pub locations: Locations,
    pub skills: Vec<Provenance>,
}

pub fn report(paths: &Paths, wanted: Option<&str>) -> Result<Report> {
    Ok(Report {
        locations: locations(paths),
        skills: run(paths, wanted)?,
    })
}

fn locations(paths: &Paths) -> Locations {
    Locations {
        manifest: display(&paths.manifest),
        lock: display(&paths.lock),
        store: display(&paths.store),
        deployed: display(&paths.deployed),
        generation: fs::read_link(&paths.current).ok().map(|to| display(&to)),
        targets: paths.targets.iter().map(display).collect(),
    }
}

fn display(path: impl AsRef<std::path::Path>) -> String {
    path.as_ref().display().to_string()
}

pub fn run(paths: &Paths, wanted: Option<&str>) -> Result<Vec<Provenance>> {
    let all = collect(paths)?;
    match wanted {
        None => Ok(all),
        Some(name) => all
            .into_iter()
            .find(|record| record.name == name)
            .map(|record| vec![record])
            .ok_or_else(|| Error::Configuration(format!("`{name}` is not deployed"))),
    }
}

fn collect(paths: &Paths) -> Result<Vec<Provenance>> {
    if !paths.current.exists() {
        return Err(Error::Configuration(
            "nothing is deployed; run `skill-manager apply`".to_string(),
        ));
    }
    let manifest = Manifest::read(&paths.manifest).ok();
    let lock = Lock::read(&paths.lock).unwrap_or_default();

    let mut records = Vec::new();
    for entry in fs::read_dir(&paths.current).map_err(|e| Error::io(&paths.current, e))? {
        let path = entry.map_err(|e| Error::io(&paths.current, e))?.path();
        let name = crate::deploy::file_name(&path);
        if !path.join(SKILL_FILE).is_file() {
            continue;
        }
        let text = fs::read_to_string(path.join(SKILL_FILE))
            .map_err(|e| Error::io(path.join(SKILL_FILE), e))?;
        let doc = SkillDoc::parse(&text)
            .map_err(|error| Error::Configuration(format!("{name}: {error}")))?;

        let (source_id, skill) =
            split_namespaced(&name).unwrap_or((BUILTIN_SOURCE_ID, name.as_str()));
        let source = manifest
            .as_ref()
            .and_then(|manifest| manifest.sources.get(source_id));
        records.push(Provenance {
            name: name.clone(),
            source_id: metadata(&doc, "sm-source-id").unwrap_or_else(|| source_id.to_string()),
            skill: skill.to_string(),
            description: field(&doc, "description").unwrap_or_default(),
            source_kind: metadata(&doc, "sm-source-kind")
                .or_else(|| source.map(|source| source.transport.kind().to_string())),
            source_url: metadata(&doc, "sm-source-url").unwrap_or_default(),
            upstream_name: metadata(&doc, "sm-upstream-name").unwrap_or_default(),
            upstream_path: metadata(&doc, "sm-upstream-path"),
            upstream_coordinate: metadata(&doc, "sm-upstream-coordinate").unwrap_or_default(),
            digest: metadata(&doc, "sm-digest").or_else(|| {
                lock.skill(source_id, skill)
                    .map(|entry| entry.digest.clone())
            }),
            resolved_path: source.and_then(|source| match &source.transport {
                crate::manifest::Transport::Path { resolved, .. } => {
                    Some(resolved.display().to_string())
                }
                _ => None,
            }),
            deployed_path: path.display().to_string(),
        });
    }
    records.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(records)
}

fn field(doc: &SkillDoc, key: &str) -> Option<String> {
    doc.frontmatter.get(key).map(str::to_string)
}

/// Provenance lives under the spec's `metadata:` key (ADR-0013).
fn metadata(doc: &SkillDoc, key: &str) -> Option<String> {
    doc.frontmatter.get_in(METADATA, key).map(str::to_string)
}

pub fn render_locations(locations: &Locations) -> String {
    let mut out = String::new();
    let mut line = |label: &str, value: &str| out.push_str(&format!("{label:<12}{value}\n"));
    line("manifest", &locations.manifest);
    line("lock", &locations.lock);
    line("store", &locations.store);
    line("deployed", &locations.deployed);
    if let Some(generation) = &locations.generation {
        line("generation", generation);
    }
    for target in &locations.targets {
        line("target", target);
    }
    out
}

/// The concise human listing.
pub fn render_line(record: &Provenance) -> String {
    if record.source_url.is_empty() {
        return format!("{}  built in", record.name);
    }
    format!(
        "{}  {}  {}",
        record.name,
        record.source_url,
        short(&record.upstream_coordinate)
    )
}

pub fn render_detail(record: &Provenance) -> String {
    let mut out = format!("{}\n", record.name);
    let mut line = |label: &str, value: &str| out.push_str(&format!("  {label:<20}{value}\n"));
    line("description", &record.description);
    line("source_id", &record.source_id);
    if let Some(kind) = &record.source_kind {
        line("source_kind", kind);
    }
    line("source_url", &record.source_url);
    line("upstream_name", &record.upstream_name);
    if let Some(path) = &record.upstream_path {
        line("upstream_path", path);
    }
    line("upstream_coordinate", &record.upstream_coordinate);
    if let Some(path) = &record.resolved_path {
        line("resolved_path", path);
    }
    if let Some(digest) = &record.digest {
        line("digest", digest);
    }
    line("deployed_path", &record.deployed_path);
    out
}

fn short(coordinate: &str) -> String {
    match coordinate.split_once(':') {
        Some((prefix, rest)) if rest.len() > 12 => format!("{prefix}:{}", &rest[..12]),
        _ => coordinate.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::apply;
    use crate::commands::testing::*;
    use crate::commands::update;

    #[test]
    fn lists_every_deployed_skill_including_the_builtin() {
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
        apply::run(&home.paths, apply::Network::Offline).unwrap();

        let records = run(&home.paths, None).unwrap();

        let names: Vec<&str> = records.iter().map(|record| record.name.as_str()).collect();
        assert_eq!(names, ["me-codemode", "sm-provenance"]);
    }

    #[test]
    fn reports_a_local_source_from_the_manifest_and_working_tree() {
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
        apply::run(&home.paths, apply::Network::Offline).unwrap();

        let records = run(&home.paths, Some("me-codemode")).unwrap();
        let record = &records[0];

        assert_eq!(record.source_id, "me");
        assert_eq!(record.upstream_name, "codemode");
        assert_eq!(record.source_kind.as_deref(), Some("path"));
        assert!(record.upstream_coordinate.starts_with("sha256:"));
        assert_eq!(
            record.resolved_path.as_deref(),
            Some(
                home.path()
                    .join("etc/ai/skills")
                    .display()
                    .to_string()
                    .as_str()
            )
        );
        assert!(
            record
                .digest
                .as_deref()
                .is_some_and(|digest| digest.starts_with("sha256:"))
        );
        assert_eq!(record.upstream_path.as_deref(), Some("codemode"));

        let json = &serde_json::to_value(&records).unwrap()[0];
        assert_eq!(json["name"], "me-codemode");
        assert_eq!(json["source_id"], "me");
        assert_eq!(json["source_kind"], "path");
        assert!(
            json["deployed_path"]
                .as_str()
                .unwrap()
                .contains("me-codemode")
        );
    }

    #[test]
    fn reports_a_remote_source_with_its_locked_digest() {
        let home = home();
        let upstream = git_origin(&[("handoff/SKILL.md", &skill_text("handoff", "body\n"))]);
        write_manifest(
            &home,
            &format!(
                "version = 1\n[sources.mp]\ngit = \"{}\"\n",
                upstream.path().display()
            ),
        );
        update::run(&home.paths, None, false).unwrap();

        let record = &run(&home.paths, Some("mp-handoff")).unwrap()[0];

        assert!(record.upstream_coordinate.starts_with("git:"));
        assert!(
            record
                .digest
                .as_deref()
                .is_some_and(|digest| digest.starts_with("sha256:"))
        );
        assert_eq!(record.source_kind.as_deref(), Some("git"));
        assert!(record.resolved_path.is_none());
    }

    #[test]
    fn reports_where_the_manifest_lock_store_and_targets_live() {
        let home = home();
        write_manifest(&home, "version = 1\n");
        apply::run(&home.paths, apply::Network::Offline).unwrap();

        let report = report(&home.paths, None).unwrap();
        let locations = &report.locations;

        assert_eq!(
            locations.generation,
            Some(display(std::fs::read_link(&home.paths.current).unwrap()))
        );
        assert!(report.skills.iter().any(|s| s.name == "sm-provenance"));

        let text = render_locations(locations);
        assert!(text.contains(&format!("manifest    {}", locations.manifest)));
        assert!(text.contains(&format!("target      {}", locations.targets[0])));
    }

    #[test]
    fn an_unknown_name_is_an_error() {
        let home = home();
        write_manifest(&home, "version = 1\n");
        apply::run(&home.paths, apply::Network::Offline).unwrap();
        assert!(
            run(&home.paths, Some("me-absent"))
                .unwrap_err()
                .to_string()
                .contains("not deployed")
        );
    }

    #[test]
    fn nothing_deployed_points_at_apply() {
        let home = home();
        assert!(
            run(&home.paths, None)
                .unwrap_err()
                .to_string()
                .contains("skill-manager apply")
        );
    }

    #[test]
    fn human_output_is_concise() {
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
        apply::run(&home.paths, apply::Network::Offline).unwrap();

        let record = &run(&home.paths, Some("me-codemode")).unwrap()[0];

        let builtin = &run(&home.paths, Some("sm-provenance")).unwrap()[0];
        assert_eq!(render_line(builtin), "sm-provenance  built in");
        assert_eq!(render_line(record).lines().count(), 1);
        assert!(render_line(record).starts_with("me-codemode  "));
        assert!(render_detail(record).contains("upstream_coordinate sha256:"));
    }
}
