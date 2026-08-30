//! `info` — provenance for what is actually deployed.

use crate::error::{Error, Result};
use crate::frontmatter::SkillDoc;
use crate::generate::BUILTIN_ALIAS;
use crate::lock::Lock;
use crate::manifest::Manifest;
use crate::paths::Paths;
use crate::skill::SKILL_FILE;
use serde::Serialize;
use std::fs;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    pub name: String,
    pub source_alias: String,
    pub skill: String,
    pub description: String,
    pub upstream_name: String,
    pub upstream_url: String,
    pub upstream_coordinate: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    /// The store content digest, for skills that have a lock entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    /// Where a local source currently resolves to on this machine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_path: Option<String>,
    pub deployed_path: String,
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

        let (alias, skill) = name
            .split_once(':')
            .unwrap_or((BUILTIN_ALIAS, name.as_str()));
        let source = manifest
            .as_ref()
            .and_then(|manifest| manifest.sources.get(alias));
        records.push(Provenance {
            name: name.clone(),
            source_alias: field(&doc, "source_alias").unwrap_or_else(|| alias.to_string()),
            skill: skill.to_string(),
            description: field(&doc, "description").unwrap_or_default(),
            upstream_name: field(&doc, "upstream_name").unwrap_or_default(),
            upstream_url: field(&doc, "upstream_url").unwrap_or_default(),
            upstream_coordinate: field(&doc, "upstream_coordinate").unwrap_or_default(),
            source_kind: source.map(|source| source.transport.kind().to_string()),
            digest: lock.skill(alias, skill).map(|entry| entry.digest.clone()),
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

/// The concise human listing.
pub fn render_line(record: &Provenance) -> String {
    let coordinate = short(&record.upstream_coordinate);
    format!("{}  {}  {coordinate}", record.name, record.upstream_url)
}

pub fn render_detail(record: &Provenance) -> String {
    let mut out = format!(
        "{}\n  description         {}\n  source_alias        {}\n  upstream_name       {}\n  upstream_url        {}\n  upstream_coordinate {}\n",
        record.name,
        record.description,
        record.source_alias,
        record.upstream_name,
        record.upstream_url,
        record.upstream_coordinate
    );
    if let Some(kind) = &record.source_kind {
        out.push_str(&format!("  source_kind         {kind}\n"));
    }
    if let Some(path) = &record.resolved_path {
        out.push_str(&format!("  resolved_path       {path}\n"));
    }
    if let Some(digest) = &record.digest {
        out.push_str(&format!("  digest              {digest}\n"));
    }
    out.push_str(&format!("  deployed_path       {}\n", record.deployed_path));
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
        apply::run(&home.paths).unwrap();

        let records = run(&home.paths, None).unwrap();

        let names: Vec<&str> = records.iter().map(|record| record.name.as_str()).collect();
        assert_eq!(names, ["me:codemode", "sm:provenance"]);
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
        apply::run(&home.paths).unwrap();

        let record = &run(&home.paths, Some("me:codemode")).unwrap()[0];

        assert_eq!(record.source_alias, "me");
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
        assert!(record.digest.is_none());
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

        let record = &run(&home.paths, Some("mp:handoff")).unwrap()[0];

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
    fn serializes_a_stable_json_record() {
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
        apply::run(&home.paths).unwrap();

        let records = run(&home.paths, Some("me:codemode")).unwrap();
        let json = serde_json::to_value(&records).unwrap();

        let record = &json[0];
        assert_eq!(record["name"], "me:codemode");
        assert_eq!(record["source_alias"], "me");
        assert_eq!(record["source_kind"], "path");
        assert!(record["digest"].is_null());
        assert!(
            record["deployed_path"]
                .as_str()
                .unwrap()
                .contains("me:codemode")
        );
    }

    #[test]
    fn an_unknown_name_is_an_error() {
        let home = home();
        write_manifest(&home, "version = 1\n");
        apply::run(&home.paths).unwrap();
        assert!(
            run(&home.paths, Some("me:absent"))
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
        apply::run(&home.paths).unwrap();

        let record = &run(&home.paths, Some("me:codemode")).unwrap()[0];

        assert_eq!(render_line(record).lines().count(), 1);
        assert!(render_line(record).starts_with("me:codemode  "));
        assert!(render_detail(record).contains("upstream_coordinate sha256:"));
    }
}
