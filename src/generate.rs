//! Generating the deployed tree: namespaced directories whose `SKILL.md` is
//! produced from the pristine original (ADR-0002).

use crate::error::{IoContext, Result};
use crate::frontmatter::SkillDoc;
use crate::skill::SKILL_FILE;
use crate::tree::Tree;
use std::fs;
use std::path::Path;

/// The built-in skill deployed on every `apply`.
pub const BUILTIN_ALIAS: &str = "sm";
const PROVENANCE_SKILL: &str = include_str!("builtin/provenance/SKILL.md");

/// One skill about to be written into a generation, with its provenance.
#[derive(Debug, Clone)]
pub struct Deployable {
    pub alias: String,
    pub name: String,
    pub doc: SkillDoc,
    pub tree: Tree,
    pub upstream_name: String,
    pub upstream_url: String,
    pub upstream_coordinate: String,
}

impl Deployable {
    pub fn deployed_name(&self) -> String {
        format!("{}:{}", self.alias, self.name)
    }

    /// The generated `SKILL.md`: namespaced `name:` plus injected provenance.
    pub fn rendered_skill_file(&self) -> String {
        let mut doc = self.doc.clone();
        doc.frontmatter.set("name", self.deployed_name());
        doc.frontmatter.set("source_alias", self.alias.clone());
        doc.frontmatter.set("upstream_name", self.upstream_name.clone());
        doc.frontmatter.set("upstream_url", self.upstream_url.clone());
        doc.frontmatter.set("upstream_coordinate", self.upstream_coordinate.clone());
        doc.render()
    }

    pub fn write_into(&self, generation_root: &Path) -> Result<()> {
        let destination = generation_root.join(self.deployed_name());
        self.tree.materialize_into(&destination)?;
        let skill_file = destination.join(SKILL_FILE);
        fs::write(&skill_file, self.rendered_skill_file()).at(&skill_file)
    }
}

/// Writes the built-in `sm:provenance` skill into a generation.
pub fn write_builtin(generation_root: &Path) -> Result<()> {
    let destination = generation_root.join(format!("{BUILTIN_ALIAS}:provenance"));
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

    fn deployable(alias: &str, name: &str, body: &str) -> (TempDir, Deployable) {
        let source = TempDir::new().unwrap();
        let text = format!("---\nname: upstream-{name}\ndescription: does {name}\n---\n{body}");
        fs::write(source.path().join(SKILL_FILE), &text).unwrap();
        fs::create_dir(source.path().join("ref")).unwrap();
        fs::write(source.path().join("ref/extra.md"), "extra").unwrap();

        let deployable = Deployable {
            alias: alias.to_string(),
            name: name.to_string(),
            doc: SkillDoc::parse(&text).unwrap(),
            tree: Tree::read(source.path()).unwrap(),
            upstream_name: format!("upstream-{name}"),
            upstream_url: "https://github.com/mattpocock/skills.git".to_string(),
            upstream_coordinate: "git:abc123".to_string(),
        };
        (source, deployable)
    }

    #[test]
    fn generated_frontmatter_is_namespaced_and_carries_provenance() {
        let (_source, deployable) = deployable("mp", "handoff", "# body\n");
        let rendered = deployable.rendered_skill_file();

        assert!(rendered.starts_with("---\nname: mp:handoff\n"));
        assert!(rendered.contains("description: does handoff\n"));
        assert!(rendered.contains("source_alias: mp\n"));
        assert!(rendered.contains("upstream_name: upstream-handoff\n"));
        assert!(rendered.contains("upstream_url: https://github.com/mattpocock/skills.git\n"));
        assert!(rendered.contains("upstream_coordinate: git:abc123\n"));
        assert!(rendered.ends_with("---\n# body\n"));
    }

    #[test]
    fn generated_frontmatter_carries_no_timestamp() {
        let (_source, deployable) = deployable("mp", "handoff", "body\n");
        let rendered = deployable.rendered_skill_file();
        for word in ["date", "time", "deployed_at", "generated_at"] {
            assert!(!rendered.contains(word), "{word} appears in generated frontmatter");
        }
    }

    #[test]
    fn build_writes_namespaced_directories_and_bundled_files() {
        let (_source, deployable) = deployable("mp", "handoff", "body\n");
        let out = TempDir::new().unwrap();
        let root = out.path().join("generation");

        build(&root, std::slice::from_ref(&deployable)).unwrap();

        assert_eq!(fs::read_to_string(root.join("mp:handoff/ref/extra.md")).unwrap(), "extra");
        let skill_file = fs::read_to_string(root.join("mp:handoff/SKILL.md")).unwrap();
        assert_eq!(skill_file, deployable.rendered_skill_file());
    }

    #[test]
    fn build_always_writes_the_builtin_provenance_skill() {
        let out = TempDir::new().unwrap();
        let root = out.path().join("generation");

        build(&root, &[]).unwrap();

        let skill_file = fs::read_to_string(root.join("sm:provenance/SKILL.md")).unwrap();
        assert!(skill_file.starts_with("---\nname: sm:provenance\n"));
        assert!(skill_file.contains("skill-manager info"));
    }

    #[test]
    fn the_builtin_skill_parses_as_a_skill() {
        let doc = SkillDoc::parse(PROVENANCE_SKILL).unwrap();
        assert_eq!(doc.name(), "sm:provenance");
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
    fn a_different_alias_generates_a_different_tree() {
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
