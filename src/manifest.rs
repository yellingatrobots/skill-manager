//! The manifest at `~/etc/ai/skills.toml` — the only file a human edits.

use crate::error::{Error, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Reserved for the built-in `sm:provenance` skill (ADR-0002).
pub const RESERVED_ALIAS: &str = "sm";
pub const MANIFEST_VERSION: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    Git { url: String },
    GitHub { repo: String },
    WellKnown { endpoint: String },
    Path { configured: String, resolved: PathBuf },
}

impl Transport {
    pub fn kind(&self) -> &'static str {
        match self {
            Transport::Git { .. } => "git",
            Transport::GitHub { .. } => "github",
            Transport::WellKnown { .. } => "well_known",
            Transport::Path { .. } => "path",
        }
    }

    /// The identity recorded in the lock, or the configured path for local sources.
    pub fn url(&self) -> String {
        match self {
            Transport::Git { url } => url.clone(),
            Transport::GitHub { repo } => format!("https://github.com/{repo}.git"),
            Transport::WellKnown { endpoint } => endpoint.clone(),
            Transport::Path { configured, .. } => configured.clone(),
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, Transport::Path { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub alias: String,
    pub transport: Transport,
    pub git_ref: Option<String>,
    pub dir: Option<String>,
    /// `None` selects every discovered skill.
    pub skills: Option<Vec<String>>,
}

impl Source {
    pub fn selects(&self, skill: &str) -> bool {
        match &self.skills {
            None => true,
            Some(selected) => selected.iter().any(|name| name == skill),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub sources: BTreeMap<String, Source>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self> {
        let raw: RawManifest = toml::from_str(text).map_err(|error| invalid(error.message()))?;
        if raw.version != MANIFEST_VERSION {
            return Err(invalid(format!(
                "unsupported manifest version {}; expected {MANIFEST_VERSION}",
                raw.version
            )));
        }

        let mut sources = BTreeMap::new();
        for (alias, raw_source) in raw.sources {
            let source = build_source(&alias, raw_source)?;
            sources.insert(alias, source);
        }
        Ok(Manifest { sources })
    }

    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .map_err(|source| Error::io(path, source))?;
        Manifest::parse(&text)
    }

    pub fn source(&self, alias: &str) -> Result<&Source> {
        self.sources
            .get(alias)
            .ok_or_else(|| invalid(format!("no source `{alias}` in the manifest")))
    }
}

fn invalid(message: impl std::fmt::Display) -> Error {
    Error::Manifest(message.to_string())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    version: i64,
    #[serde(default)]
    sources: BTreeMap<String, RawSource>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSource {
    git: Option<String>,
    github: Option<String>,
    well_known: Option<String>,
    path: Option<String>,
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    dir: Option<String>,
    skills: Option<Vec<String>>,
}

fn build_source(alias: &str, raw: RawSource) -> Result<Source> {
    validate_alias(alias)?;

    let declared: Vec<(&str, String)> = [
        ("git", raw.git),
        ("github", raw.github),
        ("well_known", raw.well_known),
        ("path", raw.path),
    ]
    .into_iter()
    .filter_map(|(key, value)| value.map(|value| (key, value)))
    .collect();

    let transport = match declared.as_slice() {
        [] => {
            return Err(invalid(format!(
                "source `{alias}` declares no transport; expected one of git, github, well_known, path"
            )));
        }
        [(key, value)] => build_transport(alias, key, value.clone())?,
        _ => {
            let keys: Vec<&str> = declared.iter().map(|(key, _)| *key).collect();
            return Err(invalid(format!(
                "source `{alias}` declares more than one transport: {}",
                keys.join(", ")
            )));
        }
    };

    if raw.git_ref.is_some() && !matches!(transport, Transport::Git { .. } | Transport::GitHub { .. })
    {
        return Err(invalid(format!(
            "source `{alias}` sets `ref`, which only applies to git and github sources"
        )));
    }
    if raw.dir.is_some() && matches!(transport, Transport::WellKnown { .. }) {
        return Err(invalid(format!(
            "source `{alias}` sets `dir`, which does not apply to well_known sources"
        )));
    }
    if let Some(dir) = &raw.dir {
        validate_relative_dir(alias, dir)?;
    }
    if let Some(skills) = &raw.skills {
        validate_skills(alias, skills)?;
    }

    Ok(Source {
        alias: alias.to_string(),
        transport,
        git_ref: raw.git_ref,
        dir: raw.dir,
        skills: raw.skills,
    })
}

fn build_transport(alias: &str, key: &str, value: String) -> Result<Transport> {
    if value.trim().is_empty() {
        return Err(invalid(format!("source `{alias}` has an empty `{key}`")));
    }
    match key {
        "git" => Ok(Transport::Git { url: value }),
        "github" => {
            validate_github_shorthand(alias, &value)?;
            Ok(Transport::GitHub { repo: value.trim_end_matches('/').to_string() })
        }
        "well_known" => {
            if !value.starts_with("https://") {
                return Err(invalid(format!(
                    "source `{alias}` has a non-HTTPS `well_known` endpoint: {value}"
                )));
            }
            Ok(Transport::WellKnown { endpoint: value })
        }
        "path" => Ok(Transport::Path { resolved: expand_tilde(&value), configured: value }),
        _ => unreachable!("transport keys are enumerated above"),
    }
}

fn validate_alias(alias: &str) -> Result<()> {
    if alias == RESERVED_ALIAS {
        return Err(invalid(format!("`{RESERVED_ALIAS}` is a reserved source alias")));
    }
    if alias.is_empty() {
        return Err(invalid("a source alias may not be empty"));
    }
    let allowed = alias
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if !allowed {
        return Err(invalid(format!(
            "source alias `{alias}` may contain only letters, digits, `-`, `_`, and `.`"
        )));
    }
    Ok(())
}

fn validate_github_shorthand(alias: &str, value: &str) -> Result<()> {
    let trimmed = value.trim_end_matches('/');
    let shaped = trimmed.split('/').count() == 2
        && trimmed.split('/').all(|part| !part.is_empty())
        && !trimmed.contains(':');
    if !shaped {
        return Err(invalid(format!(
            "source `{alias}` has a `github` value that is not `owner/repository`: {value}"
        )));
    }
    Ok(())
}

fn validate_relative_dir(alias: &str, dir: &str) -> Result<()> {
    let path = Path::new(dir);
    if path.is_absolute() || path.components().any(|c| c.as_os_str() == "..") {
        return Err(invalid(format!(
            "source `{alias}` has a `dir` that is not a relative path inside the source: {dir}"
        )));
    }
    Ok(())
}

fn validate_skills(alias: &str, skills: &[String]) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for skill in skills {
        if skill.is_empty() {
            return Err(invalid(format!("source `{alias}` selects an empty skill name")));
        }
        if skill.contains(':') || skill.contains('/') {
            return Err(invalid(format!(
                "source `{alias}` selects an invalid skill name `{skill}`"
            )));
        }
        if !seen.insert(skill) {
            return Err(invalid(format!("source `{alias}` selects `{skill}` twice")));
        }
    }
    Ok(())
}

pub fn expand_tilde(path: &str) -> PathBuf {
    let Some(rest) = path.strip_prefix('~') else {
        return PathBuf::from(path);
    };
    let Some(home) = std::env::var_os("HOME") else {
        return PathBuf::from(path);
    };
    match rest.strip_prefix('/') {
        Some(rest) => PathBuf::from(home).join(rest),
        None if rest.is_empty() => PathBuf::from(home),
        None => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Manifest {
        Manifest::parse(text).unwrap()
    }

    fn error(text: &str) -> String {
        Manifest::parse(text).unwrap_err().to_string()
    }

    #[test]
    fn reads_every_transport() {
        let manifest = parse(
            r#"
            version = 1
            [sources.mp]
            github = "mattpocock/skills"
            ref = "main"
            skills = ["handoff", "tdd"]

            [sources.kepano]
            git = "git@github.com:kepano/obsidian-skills.git"
            dir = "skills"

            [sources.likec4]
            well_known = "https://likec4.dev/"

            [sources.me]
            path = "~/etc/ai/skills"
            "#,
        );

        let mp = &manifest.sources["mp"];
        assert_eq!(mp.transport, Transport::GitHub { repo: "mattpocock/skills".into() });
        assert_eq!(mp.transport.url(), "https://github.com/mattpocock/skills.git");
        assert_eq!(mp.git_ref.as_deref(), Some("main"));
        assert_eq!(mp.skills.as_deref(), Some(["handoff".to_string(), "tdd".to_string()].as_slice()));

        assert_eq!(manifest.sources["kepano"].dir.as_deref(), Some("skills"));
        assert_eq!(manifest.sources["likec4"].transport.kind(), "well_known");
        assert!(manifest.sources["me"].transport.is_local());
        assert!(manifest.sources["me"].selects("anything"));
    }

    #[test]
    fn expands_a_leading_tilde_in_paths() {
        unsafe { std::env::set_var("HOME", "/home/tester") };
        let manifest = parse("version = 1\n[sources.me]\npath = \"~/etc/ai/skills\"\n");
        let Transport::Path { configured, resolved } = &manifest.sources["me"].transport else {
            panic!("expected a path transport");
        };
        assert_eq!(configured, "~/etc/ai/skills");
        assert_eq!(resolved, Path::new("/home/tester/etc/ai/skills"));
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(error("version = 1\n[sources.mp]\ngithub = \"a/b\"\nbranch = \"main\"\n")
            .contains("branch"));
    }

    #[test]
    fn rejects_unsupported_versions() {
        assert!(error("version = 2\n").contains("version 2"));
    }

    #[test]
    fn rejects_the_reserved_alias() {
        assert!(error("version = 1\n[sources.sm]\ngithub = \"a/b\"\n").contains("reserved"));
    }

    #[test]
    fn requires_exactly_one_transport() {
        assert!(error("version = 1\n[sources.mp]\nref = \"main\"\n").contains("no transport"));
        assert!(
            error("version = 1\n[sources.mp]\ngithub = \"a/b\"\ngit = \"https://x/y.git\"\n")
                .contains("more than one transport")
        );
    }

    #[test]
    fn restricts_ref_and_dir_to_the_transports_that_support_them() {
        assert!(error("version = 1\n[sources.me]\npath = \"/x\"\nref = \"main\"\n").contains("`ref`"));
        assert!(
            error("version = 1\n[sources.l]\nwell_known = \"https://x/\"\ndir = \"skills\"\n")
                .contains("`dir`")
        );
    }

    #[test]
    fn rejects_dir_escaping_the_source() {
        assert!(error("version = 1\n[sources.mp]\ngithub = \"a/b\"\ndir = \"../x\"\n").contains("relative"));
        assert!(error("version = 1\n[sources.mp]\ngithub = \"a/b\"\ndir = \"/x\"\n").contains("relative"));
    }

    #[test]
    fn rejects_malformed_github_shorthand() {
        assert!(error("version = 1\n[sources.mp]\ngithub = \"mattpocock\"\n").contains("owner/repository"));
        assert!(
            error("version = 1\n[sources.mp]\ngithub = \"git@github.com:a/b.git\"\n")
                .contains("owner/repository")
        );
    }

    #[test]
    fn rejects_non_https_well_known_endpoints() {
        assert!(error("version = 1\n[sources.l]\nwell_known = \"http://x/\"\n").contains("HTTPS"));
    }

    #[test]
    fn rejects_invalid_skill_selections() {
        assert!(error("version = 1\n[sources.mp]\ngithub = \"a/b\"\nskills = [\"mp:handoff\"]\n")
            .contains("invalid skill name"));
        assert!(error("version = 1\n[sources.mp]\ngithub = \"a/b\"\nskills = [\"x\", \"x\"]\n")
            .contains("twice"));
    }

    #[test]
    fn selects_only_listed_skills_when_skills_is_present() {
        let manifest = parse("version = 1\n[sources.mp]\ngithub = \"a/b\"\nskills = [\"handoff\"]\n");
        assert!(manifest.sources["mp"].selects("handoff"));
        assert!(!manifest.sources["mp"].selects("tdd"));
    }
}
