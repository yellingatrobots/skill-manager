//! Discovery: a skill is any directory containing `SKILL.md` (ADR-0008).

use crate::error::{IoContext, Result};
use crate::frontmatter::SkillDoc;
use crate::tree::Tree;
use std::fs;
use std::path::{Path, PathBuf};

pub const SKILL_FILE: &str = "SKILL.md";

/// The shape the agent-skills spec allows for a `name:`, which a deployed
/// `<source-id>-<skill>` must still satisfy: `^[a-z0-9]+(-[a-z0-9]+)*$`.
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// A structurally valid skill found in a source.
#[derive(Debug, Clone)]
pub struct Skill {
    /// The directory name — the `<skill>` half of `<source-id>-<skill>`.
    pub name: String,
    /// The frontmatter `name:` that namespacing will overwrite.
    pub upstream_name: String,
    pub doc: SkillDoc,
    pub tree: Tree,
}

/// Provenance is injected into `metadata`, so that key must be a plain map we
/// can extend, and it must not already claim one of our keys (ADR-0013).
fn check_injectable(doc: &SkillDoc) -> std::result::Result<(), String> {
    let metadata = crate::generate::METADATA;
    if !doc.frontmatter.is_extensible_map(metadata) {
        return Err(format!(
            "frontmatter `{metadata}` is not a plain key-value map, so provenance cannot be added"
        ));
    }
    for key in crate::generate::INJECTED_KEYS {
        if doc.frontmatter.get_in(metadata, key).is_some() {
            return Err(format!(
                "frontmatter `{metadata}` already defines `{key}`, which skill-manager injects"
            ));
        }
    }
    Ok(())
}

/// Where a skill sits inside its source, for provenance.
pub fn relative_path(root: &Path, skill: &Skill) -> String {
    skill
        .tree
        .root()
        .strip_prefix(root)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| skill.name.clone())
}

#[derive(Debug, Clone)]
pub struct InvalidSkill {
    pub name: String,
    pub reason: String,
}

/// Everything one source walk found, valid and invalid.
#[derive(Debug, Default)]
pub struct Discovery {
    pub skills: Vec<Skill>,
    pub invalid: Vec<InvalidSkill>,
}

impl Discovery {
    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|skill| skill.name == name)
    }

    #[cfg(test)]
    pub fn names(&self) -> Vec<String> {
        self.skills.iter().map(|skill| skill.name.clone()).collect()
    }
}

/// Walks `root`, narrowed by an optional relative `dir`, for skill directories.
pub fn discover(root: &Path, dir: Option<&str>) -> Result<Discovery> {
    let start = match dir {
        Some(relative) => root.join(relative),
        None => root.to_path_buf(),
    };
    let mut discovery = Discovery::default();
    if !start.is_dir() {
        return Err(crate::error::Error::io(
            &start,
            std::io::Error::new(std::io::ErrorKind::NotFound, "source directory not found"),
        ));
    }
    if start.join(SKILL_FILE).is_file() {
        collect_skill(&start, &mut discovery);
    } else {
        walk(&start, &mut discovery)?;
    }
    discovery.skills.sort_by(|a, b| a.name.cmp(&b.name));
    discovery.invalid.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(discovery)
}

fn walk(dir: &Path, discovery: &mut Discovery) -> Result<()> {
    let mut children: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir).at(dir)? {
        let entry = entry.at(dir)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).at(&path)?;
        let hidden = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'));
        if !metadata.is_dir() || hidden {
            continue;
        }
        children.push(path);
    }

    for child in children {
        if child.join(SKILL_FILE).is_file() {
            collect_skill(&child, discovery);
        } else {
            walk(&child, discovery)?;
        }
    }
    Ok(())
}

fn collect_skill(path: &Path, discovery: &mut Discovery) {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    match read_skill(&name, path) {
        Ok(skill) => discovery.skills.push(skill),
        Err(reason) => discovery.invalid.push(InvalidSkill { name, reason }),
    }
}

fn read_skill(name: &str, path: &Path) -> std::result::Result<Skill, String> {
    if !is_valid_name(name) {
        return Err(format!(
            "directory name `{name}` is not a valid skill name: lowercase letters and digits, separated by single `-`"
        ));
    }
    let tree = Tree::read(path).map_err(|error| error.to_string())?;
    let source = tree
        .read_file(SKILL_FILE)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("{SKILL_FILE} is not a regular file"))?;
    let text = String::from_utf8(source).map_err(|_| format!("{SKILL_FILE} is not valid UTF-8"))?;
    let doc = SkillDoc::parse(&text).map_err(|error| error.to_string())?;
    check_injectable(&doc)?;

    Ok(Skill {
        name: name.to_string(),
        upstream_name: doc.name().to_string(),
        doc,
        tree,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn skill_md(name: &str) -> String {
        format!("---\nname: {name}\ndescription: does {name}\n---\nbody\n")
    }

    fn write(root: &Path, rel: &str, contents: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn finds_nested_skill_directories() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "skills/handoff/SKILL.md", &skill_md("handoff"));
        write(dir.path(), "skills/tdd/SKILL.md", &skill_md("tdd"));
        write(dir.path(), "README.md", "ignored");

        let found = discover(dir.path(), None).unwrap();

        assert_eq!(found.names(), ["handoff", "tdd"]);
        assert!(found.invalid.is_empty());
    }

    #[test]
    fn narrows_the_walk_to_dir() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "skills/handoff/SKILL.md", &skill_md("handoff"));
        write(dir.path(), "examples/demo/SKILL.md", &skill_md("demo"));

        let found = discover(dir.path(), Some("skills")).unwrap();

        assert_eq!(found.names(), ["handoff"]);
    }

    #[test]
    fn treats_a_root_holding_skill_md_as_one_skill() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("handoff");
        write(&root, "SKILL.md", &skill_md("upstream-handoff"));

        let found = discover(&root, None).unwrap();

        assert_eq!(found.names(), ["handoff"]);
        assert_eq!(found.skills[0].upstream_name, "upstream-handoff");
    }

    #[test]
    fn does_not_recurse_inside_a_skill() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "handoff/SKILL.md", &skill_md("handoff"));
        write(dir.path(), "handoff/nested/SKILL.md", &skill_md("nested"));

        let found = discover(dir.path(), None).unwrap();

        assert_eq!(found.names(), ["handoff"]);
    }

    #[test]
    fn skips_hidden_directories() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), ".git/modules/x/SKILL.md", &skill_md("x"));
        write(dir.path(), "handoff/SKILL.md", &skill_md("handoff"));

        let found = discover(dir.path(), None).unwrap();

        assert_eq!(found.names(), ["handoff"]);
    }

    #[test]
    fn reports_skills_with_unusable_skill_md_without_dropping_valid_ones() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "good/SKILL.md", &skill_md("good"));
        write(dir.path(), "bad/SKILL.md", "no frontmatter here\n");

        let found = discover(dir.path(), None).unwrap();

        assert_eq!(found.names(), ["good"]);
        assert_eq!(found.invalid.len(), 1);
        assert_eq!(found.invalid[0].name, "bad");
    }

    #[test]
    fn rejects_directory_names_outside_the_spec_shape() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "mp:handoff/SKILL.md", &skill_md("handoff"));
        write(dir.path(), "Hand_off/SKILL.md", &skill_md("handoff"));
        write(dir.path(), "hand--off/SKILL.md", &skill_md("handoff"));

        let found = discover(dir.path(), None).unwrap();

        assert!(found.skills.is_empty());
        assert_eq!(found.invalid.len(), 3);
        assert!(found.invalid[0].reason.contains("valid skill name"));
    }

    #[test]
    fn accepts_hyphenated_lowercase_names() {
        assert!(is_valid_name("tdd"));
        assert!(is_valid_name("likec4-dsl"));
        assert!(!is_valid_name("-tdd"));
        assert!(!is_valid_name("mp:tdd"));
        assert!(!is_valid_name(""));
    }

    #[test]
    fn rejects_a_metadata_block_that_cannot_take_injected_keys() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "nested/SKILL.md",
            "---\nname: nested\ndescription: d\nmetadata:\n  a:\n    b: c\n---\nbody\n",
        );

        let found = discover(dir.path(), None).unwrap();

        assert!(found.skills.is_empty());
        assert!(found.invalid[0].reason.contains("plain key-value map"));
    }

    #[test]
    fn rejects_a_skill_that_already_claims_an_injected_key() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "claimed/SKILL.md",
            "---\nname: claimed\ndescription: d\nmetadata:\n  sm-source-id: theirs\n---\nbody\n",
        );

        let found = discover(dir.path(), None).unwrap();

        assert!(found.skills.is_empty());
        assert!(found.invalid[0].reason.contains("sm-source-id"));
    }

    #[test]
    fn keeps_a_metadata_block_that_uses_other_sm_keys() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "tagged/SKILL.md",
            "---\nname: tagged\ndescription: d\nmetadata:\n  sm-tags: rust\n---\nbody\n",
        );

        let found = discover(dir.path(), None).unwrap();

        assert_eq!(found.names(), ["tagged"]);
    }

    #[test]
    fn rejects_skills_containing_irregular_entries() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "handoff/SKILL.md", &skill_md("handoff"));
        std::os::unix::fs::symlink("/etc/passwd", dir.path().join("handoff/link")).unwrap();

        let found = discover(dir.path(), None).unwrap();

        assert!(found.skills.is_empty());
        assert_eq!(found.invalid[0].name, "handoff");
    }

    #[test]
    fn errors_when_the_source_directory_is_missing() {
        let dir = TempDir::new().unwrap();
        assert!(discover(dir.path(), Some("nope")).is_err());
    }
}
