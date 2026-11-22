//! Generating the deployed tree: namespaced directories whose `SKILL.md` is
//! produced from the pristine original (ADR-0002).

use crate::error::{IoContext, Result};
use crate::frontmatter::SkillDoc;
use crate::skill::SKILL_FILE;
use crate::tree::Tree;
use std::fs;
use std::path::Path;

/// The built-in skill deployed on every `apply`.
pub const BUILTIN_SOURCE_ID: &str = "sm";
/// Separates source_id from skill in a deployed name. A hyphen keeps that name
/// inside the shape the agent-skills spec allows for `name:` (ADR-0013).
pub const SEPARATOR: char = '-';
/// The frontmatter key the agent-skills spec reserves for custom keys. It is
/// a flat map of string to string, so our keys are prefixed rather than nested
/// (ADR-0013).
pub const METADATA: &str = "metadata";
/// The `metadata` keys this tool writes. A skill that already defines one is
/// reported and skipped rather than overwritten.
pub const INJECTED_KEYS: [&str; 7] = [
    "sm-source-id",
    "sm-source-kind",
    "sm-source-url",
    "sm-upstream-name",
    "sm-upstream-path",
    "sm-upstream-coordinate",
    "sm-digest",
];
/// The agent-skills cap on a `name:`, which the deployed name must respect.
pub const MAX_NAME_LENGTH: usize = 64;
const PROVENANCE_SKILL: &str = include_str!("builtin/provenance/SKILL.md");

pub fn namespaced(source_id: &str, skill: &str) -> String {
    format!("{source_id}{SEPARATOR}{skill}")
}

/// Splits at the first separator: an source_id never contains one (ADR-0013).
pub fn split_namespaced(name: &str) -> Option<(&str, &str)> {
    let (source_id, skill) = name.split_once(SEPARATOR)?;
    (!source_id.is_empty() && !skill.is_empty()).then_some((source_id, skill))
}

/// Where a deployed skill came from, injected under its `metadata:` key.
#[derive(Debug, Clone, Default)]
pub struct Origin {
    pub source_kind: String,
    pub source_url: String,
    pub upstream_name: String,
    /// Where the skill sat inside its source.
    pub upstream_path: String,
    pub upstream_coordinate: String,
    /// The content digest of the skill tree this was generated from.
    pub digest: String,
}

/// One skill about to be written into a generation, with its provenance.
#[derive(Debug, Clone)]
pub struct Deployable {
    pub source_id: String,
    pub name: String,
    pub doc: SkillDoc,
    pub tree: Tree,
    pub origin: Origin,
}

impl Deployable {
    pub fn deployed_name(&self) -> String {
        namespaced(&self.source_id, &self.name)
    }

    /// The generated `SKILL.md`: namespaced `name:` plus injected provenance.
    pub fn rendered_skill_file(&self) -> String {
        let mut doc = self.doc.clone();
        doc.frontmatter.set("name", self.deployed_name());
        for (key, value) in self.metadata() {
            doc.frontmatter.set_in(METADATA, key, value);
        }
        doc.render()
    }

    /// The provenance keys, in `INJECTED_KEYS` order. Empty values are omitted.
    fn metadata(&self) -> Vec<(&'static str, String)> {
        let origin = &self.origin;
        [
            ("sm-source-id", self.source_id.clone()),
            ("sm-source-kind", origin.source_kind.clone()),
            ("sm-source-url", origin.source_url.clone()),
            ("sm-upstream-name", origin.upstream_name.clone()),
            ("sm-upstream-path", origin.upstream_path.clone()),
            ("sm-upstream-coordinate", origin.upstream_coordinate.clone()),
            ("sm-digest", origin.digest.clone()),
        ]
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .collect()
    }

    pub fn write_into(&self, generation_root: &Path) -> Result<()> {
        let destination = generation_root.join(self.deployed_name());
        self.tree.materialize_into(&destination)?;
        let skill_file = destination.join(SKILL_FILE);
        fs::write(&skill_file, self.rendered_skill_file()).at(&skill_file)
    }
}

/// Writes the built-in `sm-provenance` skill into a generation.
pub fn write_builtin(generation_root: &Path) -> Result<()> {
    let destination = generation_root.join(namespaced(BUILTIN_SOURCE_ID, "provenance"));
    fs::create_dir_all(&destination).at(&destination)?;
    let skill_file = destination.join(SKILL_FILE);
    fs::write(&skill_file, PROVENANCE_SKILL).at(&skill_file)
}

/// Builds a complete generation under `root`, which must not already exist.
pub fn build(root: &Path, deployables: &[Deployable]) -> Result<()> {
    fs::create_dir_all(root).at(root)?;
    for deployable in deployables {
        deployable.write_into(root)?;
    }
    write_builtin(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn deployable(source_id: &str, name: &str, body: &str) -> (TempDir, Deployable) {
        let source = TempDir::new().unwrap();
        let text = format!("---\nname: upstream-{name}\ndescription: does {name}\n---\n{body}");
        fs::write(source.path().join(SKILL_FILE), &text).unwrap();
        fs::create_dir(source.path().join("ref")).unwrap();
        fs::write(source.path().join("ref/extra.md"), "extra").unwrap();

        let deployable = Deployable {
            source_id: source_id.to_string(),
            name: name.to_string(),
            doc: SkillDoc::parse(&text).unwrap(),
            tree: Tree::read(source.path()).unwrap(),
            origin: Origin {
                source_kind: "github".to_string(),
                source_url: "https://github.com/mattpocock/skills.git".to_string(),
                upstream_name: format!("upstream-{name}"),
                upstream_path: format!("skills/{name}"),
                upstream_coordinate: "git:abc123".to_string(),
                digest: format!("sha256:{}", "a".repeat(64)),
            },
        };
        (source, deployable)
    }

    #[test]
    fn generated_frontmatter_is_namespaced_and_carries_provenance() {
        let (_source, deployable) = deployable("mp", "handoff", "# body\n");
        let rendered = deployable.rendered_skill_file();

        assert!(rendered.starts_with("---\nname: mp-handoff\n"));
        assert!(rendered.contains("description: does handoff\n"));
        assert!(rendered.contains("metadata:\n"));
        assert!(rendered.contains("\n  sm-source-id: mp\n"));
        assert!(rendered.contains("\n  sm-source-kind: github\n"));
        assert!(rendered.contains("\n  sm-source-url: https://github.com/mattpocock/skills.git\n"));
        assert!(rendered.contains("\n  sm-upstream-name: upstream-handoff\n"));
        assert!(rendered.contains("\n  sm-upstream-path: skills/handoff\n"));
        assert!(rendered.contains("\n  sm-upstream-coordinate: git:abc123\n"));
        assert!(rendered.contains("\n  sm-digest: sha256:"));
        assert!(rendered.ends_with("---\n# body\n"));
    }

    #[test]
    fn the_injected_keys_constant_matches_what_is_written() {
        let (_source, deployable) = deployable("mp", "handoff", "body\n");
        let written: Vec<&str> = deployable
            .metadata()
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert_eq!(written, INJECTED_KEYS);
    }

    #[test]
    fn generated_frontmatter_carries_no_timestamp() {
        let (_source, deployable) = deployable("mp", "handoff", "body\n");
        let rendered = deployable.rendered_skill_file();
        for word in ["date", "time", "deployed_at", "generated_at"] {
            assert!(
                !rendered.contains(word),
                "{word} appears in generated frontmatter"
            );
        }
    }

    #[test]
    fn metadata_extends_an_upstream_metadata_block_without_dropping_its_keys() {
        let (source, mut deployable) = deployable("mp", "handoff", "# body\n");
        let text = "---\nname: handoff\ndescription: d\nmetadata:\n  author: matt\n---\nbody\n";
        fs::write(source.path().join(SKILL_FILE), text).unwrap();
        deployable.doc = SkillDoc::parse(text).unwrap();

        let rendered = deployable.rendered_skill_file();

        assert!(rendered.contains("\n  author: matt\n"));
        assert!(rendered.contains("\n  sm-source-id: mp\n"));
        assert_eq!(
            SkillDoc::parse(&rendered)
                .unwrap()
                .frontmatter
                .get_in("metadata", "author"),
            Some("matt")
        );
    }

    #[test]
    fn build_writes_namespaced_directories_and_bundled_files() {
        let (_source, deployable) = deployable("mp", "handoff", "body\n");
        let out = TempDir::new().unwrap();
        let root = out.path().join("generation");

        build(&root, std::slice::from_ref(&deployable)).unwrap();

        assert_eq!(
            fs::read_to_string(root.join("mp-handoff/ref/extra.md")).unwrap(),
            "extra"
        );
        let skill_file = fs::read_to_string(root.join("mp-handoff/SKILL.md")).unwrap();
        assert_eq!(skill_file, deployable.rendered_skill_file());
    }

    #[test]
    fn build_always_writes_the_builtin_provenance_skill() {
        let out = TempDir::new().unwrap();
        let root = out.path().join("generation");

        build(&root, &[]).unwrap();

        let skill_file = fs::read_to_string(root.join("sm-provenance/SKILL.md")).unwrap();
        let doc = SkillDoc::parse(&skill_file).unwrap();
        assert_eq!(doc.name(), "sm-provenance");
        assert!(!doc.frontmatter.get("description").unwrap().is_empty());
    }

    #[test]
    fn identical_inputs_generate_an_identically_digested_tree() {
        let (_source, deployable) = deployable("mp", "handoff", "body\n");
        let out = TempDir::new().unwrap();
        let (a, b) = (out.path().join("a"), out.path().join("b"));

        build(&a, std::slice::from_ref(&deployable)).unwrap();
        build(&b, std::slice::from_ref(&deployable)).unwrap();

        assert_eq!(
            Tree::read(&a).unwrap().digest().unwrap(),
            Tree::read(&b).unwrap().digest().unwrap()
        );
    }

    #[test]
    fn a_different_source_id_generates_a_different_tree() {
        let (_a_source, one) = deployable("mp", "handoff", "body\n");
        let (_b_source, two) = deployable("me", "handoff", "body\n");
        let out = TempDir::new().unwrap();
        let (a, b) = (out.path().join("a"), out.path().join("b"));

        build(&a, &[one]).unwrap();
        build(&b, &[two]).unwrap();

        assert_ne!(
            Tree::read(&a).unwrap().digest().unwrap(),
            Tree::read(&b).unwrap().digest().unwrap()
        );
    }
}
