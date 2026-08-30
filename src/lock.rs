//! The lock at `~/etc/ai/skills.lock` — the record of resolution (ADR-0007).

use crate::error::{Error, IoContext, Result};
use crate::fsx;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

pub const LOCK_VERSION: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedSkill {
    /// Where the skill sat inside its source, for provenance.
    pub path: String,
    /// The frontmatter `name:` that namespacing overwrote.
    pub upstream_name: String,
    /// What identifies the content at its origin: a commit SHA, an archive digest.
    pub coordinate: String,
    /// The content digest of what was extracted into the store.
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedSource {
    pub kind: String,
    pub url: String,
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    pub dir: Option<String>,
    #[serde(default)]
    pub skills: BTreeMap<String, LockedSkill>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lock {
    pub sources: BTreeMap<String, LockedSource>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLock {
    version: i64,
    #[serde(default)]
    sources: BTreeMap<String, LockedSource>,
}

impl Lock {
    pub fn parse(text: &str) -> Result<Self> {
        let raw: RawLock =
            toml::from_str(text).map_err(|error| Error::Lock(error.message().to_string()))?;
        if raw.version != LOCK_VERSION {
            return Err(Error::Lock(format!(
                "unsupported lock version {}; expected {LOCK_VERSION}",
                raw.version
            )));
        }
        Ok(Lock {
            sources: raw.sources,
        })
    }

    /// An absent lock file is an empty lock; a malformed one is an error.
    pub fn read(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Lock::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Lock::default()),
            Err(error) => Err(Error::io(path, error)),
        }
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        fsx::write_atomic(path, self.render().as_bytes())
    }

    pub fn render(&self) -> String {
        let mut out = format!("version = {LOCK_VERSION}\n");
        for (alias, source) in &self.sources {
            out.push_str(&format!("\n[sources.{}]\n", key(alias)));
            out.push_str(&entry("kind", &source.kind));
            out.push_str(&entry("url", &source.url));
            if let Some(git_ref) = &source.git_ref {
                out.push_str(&entry("ref", git_ref));
            }
            if let Some(dir) = &source.dir {
                out.push_str(&entry("dir", dir));
            }
            for (name, skill) in &source.skills {
                out.push_str(&format!(
                    "\n[sources.{}.skills.{}]\n",
                    key(alias),
                    key(name)
                ));
                out.push_str(&entry("path", &skill.path));
                out.push_str(&entry("upstream_name", &skill.upstream_name));
                out.push_str(&entry("coordinate", &skill.coordinate));
                out.push_str(&entry("digest", &skill.digest));
            }
        }
        out
    }

    pub fn skill(&self, alias: &str, skill: &str) -> Option<&LockedSkill> {
        self.sources.get(alias)?.skills.get(skill)
    }
}

fn entry(key: &str, value: &str) -> String {
    format!("{key} = {}\n", string(value))
}

fn string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

/// TOML bare keys are unquoted; anything else is quoted.
fn key(value: &str) -> String {
    let bare = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if bare {
        value.to_string()
    } else {
        string(value)
    }
}

/// Reads a lock, or fails with the actionable message the commands share.
pub fn read_required(path: &Path) -> Result<Lock> {
    if !path.exists() {
        return Err(Error::Lock(format!(
            "{} does not exist; run `skill-manager update` to create it",
            path.display()
        )));
    }
    let text = std::fs::read_to_string(path).at(path)?;
    Lock::parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(digest: &str) -> LockedSkill {
        LockedSkill {
            path: "skills/handoff".into(),
            upstream_name: "handoff".into(),
            coordinate: "git:abc123".into(),
            digest: digest.into(),
        }
    }

    fn sample() -> Lock {
        let mut skills = BTreeMap::new();
        skills.insert("handoff".to_string(), skill("sha256:aa"));
        let mut sources = BTreeMap::new();
        sources.insert(
            "mp".to_string(),
            LockedSource {
                kind: "github".into(),
                url: "https://github.com/mattpocock/skills.git".into(),
                git_ref: Some("main".into()),
                dir: None,
                skills,
            },
        );
        Lock { sources }
    }

    #[test]
    fn renders_the_documented_shape() {
        assert_eq!(
            sample().render(),
            "version = 1\n\
             \n[sources.mp]\n\
             kind = \"github\"\n\
             url = \"https://github.com/mattpocock/skills.git\"\n\
             ref = \"main\"\n\
             \n[sources.mp.skills.handoff]\n\
             path = \"skills/handoff\"\n\
             upstream_name = \"handoff\"\n\
             coordinate = \"git:abc123\"\n\
             digest = \"sha256:aa\"\n"
        );
    }

    #[test]
    fn round_trips_through_toml() {
        let lock = sample();
        assert_eq!(Lock::parse(&lock.render()).unwrap(), lock);
    }

    #[test]
    fn renders_deterministically_regardless_of_insertion_order() {
        let mut a = Lock::default();
        let mut b = Lock::default();
        for alias in ["zz", "aa"] {
            a.sources
                .insert(alias.into(), sample().sources["mp"].clone());
        }
        for alias in ["aa", "zz"] {
            b.sources
                .insert(alias.into(), sample().sources["mp"].clone());
        }
        assert_eq!(a.render(), b.render());
        assert!(a.render().find("[sources.aa]") < a.render().find("[sources.zz]"));
    }

    #[test]
    fn writes_no_timestamps() {
        assert!(!sample().render().contains("time"));
    }

    #[test]
    fn quotes_keys_that_are_not_bare() {
        let mut lock = Lock::default();
        let mut source = sample().sources["mp"].clone();
        let entry = source.skills.remove("handoff").unwrap();
        source.skills.insert("hand.off".into(), entry);
        lock.sources.insert("mp".into(), source);
        assert!(lock.render().contains("[sources.mp.skills.\"hand.off\"]"));
        assert_eq!(Lock::parse(&lock.render()).unwrap(), lock);
    }

    #[test]
    fn rejects_unsupported_versions_and_unknown_keys() {
        assert!(Lock::parse("version = 2\n").is_err());
        assert!(
            Lock::parse("version = 1\n[sources.mp]\nkind = \"git\"\nurl = \"u\"\nnope = 1\n")
                .is_err()
        );
    }

    #[test]
    fn a_missing_lock_file_reads_as_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(
            Lock::read(&dir.path().join("skills.lock")).unwrap(),
            Lock::default()
        );
    }

    #[test]
    fn read_required_points_at_update() {
        let dir = tempfile::TempDir::new().unwrap();
        let error = read_required(&dir.path().join("skills.lock"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("skill-manager update"));
    }
}
