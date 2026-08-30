//! Semantic file tree: reading, hashing (ADR-0012), and materialization.

use crate::error::{Error, IoContext, Result};
use sha2::{Digest as _, Sha256};
use std::fmt;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Versions the canonical stream independently from the lock schema.
const STREAM_MARKER: &[u8] = b"skill-tree-v1\n";

const FILE_MODE: u32 = 0o644;
const EXECUTABLE_MODE: u32 = 0o755;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest(String);

impl Digest {
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.strip_prefix("sha256:")?;
        let well_formed = hex.len() == 64
            && hex
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
        well_formed.then(|| Digest(text.to_string()))
    }

    /// The store lays entries out as `sha256/<hex>`.
    pub fn store_relative_path(&self) -> PathBuf {
        let (algorithm, hex) = self
            .0
            .split_once(':')
            .expect("digest is algorithm-prefixed");
        Path::new(algorithm).join(hex)
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct TreeFile {
    pub rel: String,
    pub executable: bool,
    pub abs: PathBuf,
}

/// The regular files physically inside one directory, in canonical order.
#[derive(Debug, Clone)]
pub struct Tree {
    root: PathBuf,
    files: Vec<TreeFile>,
}

impl Tree {
    pub fn read(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let mut files = Vec::new();
        collect(root, root, &mut files)?;
        files.sort_by(|a, b| a.rel.as_bytes().cmp(b.rel.as_bytes()));
        Ok(Tree {
            root: root.to_path_buf(),
            files,
        })
    }

    pub fn digest(&self) -> Result<Digest> {
        let mut hasher = Sha256::new();
        hasher.update(STREAM_MARKER);
        for file in &self.files {
            let contents = fs::read(&file.abs).at(&file.abs)?;
            hasher.update((file.rel.len() as u64).to_le_bytes());
            hasher.update(file.rel.as_bytes());
            hasher.update([u8::from(file.executable)]);
            hasher.update((contents.len() as u64).to_le_bytes());
            hasher.update(&contents);
        }
        Ok(Digest(format!("sha256:{}", hex(&hasher.finalize()))))
    }

    #[cfg(test)]
    pub fn files(&self) -> &[TreeFile] {
        &self.files
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn read_file(&self, rel: &str) -> Result<Option<Vec<u8>>> {
        match self.files.iter().find(|file| file.rel == rel) {
            Some(file) => fs::read(&file.abs).at(&file.abs).map(Some),
            None => Ok(None),
        }
    }

    pub fn materialize_into(&self, dest: impl AsRef<Path>) -> Result<()> {
        let dest = dest.as_ref();
        fs::create_dir_all(dest).at(dest)?;
        for file in &self.files {
            let target = dest.join(&file.rel);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).at(parent)?;
            }
            fs::copy(&file.abs, &target).at(&target)?;
            let mode = if file.executable {
                EXECUTABLE_MODE
            } else {
                FILE_MODE
            };
            fs::set_permissions(&target, fs::Permissions::from_mode(mode)).at(&target)?;
        }
        Ok(())
    }
}

/// The digest of a byte stream, for archives fetched whole.
pub fn sha256(bytes: &[u8]) -> Digest {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Digest(format!("sha256:{}", hex(&hasher.finalize())))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<TreeFile>) -> Result<()> {
    let entries = fs::read_dir(dir).at(dir)?;
    for entry in entries {
        let entry = entry.at(dir)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).at(&path)?;
        if metadata.is_dir() {
            collect(root, &path, files)?;
        } else if metadata.is_file() {
            let rel = path
                .strip_prefix(root)
                .expect("walked path is under root")
                .to_str()
                .ok_or_else(|| Error::NonUtf8Path(path.clone()))?
                .to_string();
            files.push(TreeFile {
                rel,
                executable: metadata.permissions().mode() & 0o111 != 0,
                abs: path,
            });
        } else {
            return Err(Error::IrregularEntry(path));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn write(root: &Path, rel: &str, contents: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn set_executable(root: &Path, rel: &str) {
        let path = root.join(rel);
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(perms.mode() | 0o111);
        fs::set_permissions(path, perms).unwrap();
    }

    fn digest_of(root: &Path) -> Digest {
        Tree::read(root).unwrap().digest().unwrap()
    }

    fn fixture(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (rel, contents) in files {
            write(dir.path(), rel, contents);
        }
        dir
    }

    #[test]
    fn digest_is_prefixed_hex_sha256() {
        let dir = fixture(&[("SKILL.md", "body")]);
        let Digest(text) = digest_of(dir.path());
        let hex = text.strip_prefix("sha256:").expect("sha256 prefix");
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
        );
    }

    #[test]
    fn identical_content_digests_identically() {
        let a = fixture(&[("SKILL.md", "body"), ("ref/one.md", "one")]);
        let b = fixture(&[("ref/one.md", "one"), ("SKILL.md", "body")]);
        assert_eq!(digest_of(a.path()), digest_of(b.path()));
    }

    #[test]
    fn content_change_changes_digest() {
        let a = fixture(&[("SKILL.md", "body")]);
        let b = fixture(&[("SKILL.md", "other")]);
        assert_ne!(digest_of(a.path()), digest_of(b.path()));
    }

    #[test]
    fn rename_changes_digest() {
        let a = fixture(&[("one.md", "x")]);
        let b = fixture(&[("two.md", "x")]);
        assert_ne!(digest_of(a.path()), digest_of(b.path()));
    }

    #[test]
    fn moving_content_between_files_changes_digest() {
        let a = fixture(&[("a.md", "xy"), ("b.md", "z")]);
        let b = fixture(&[("a.md", "x"), ("b.md", "yz")]);
        assert_ne!(digest_of(a.path()), digest_of(b.path()));
    }

    #[test]
    fn executable_bit_changes_digest() {
        let a = fixture(&[("run.sh", "#!/bin/sh")]);
        let b = fixture(&[("run.sh", "#!/bin/sh")]);
        let before = digest_of(b.path());
        set_executable(b.path(), "run.sh");
        assert_eq!(digest_of(a.path()), before);
        assert_ne!(digest_of(a.path()), digest_of(b.path()));
    }

    #[test]
    fn empty_directories_do_not_contribute() {
        let a = fixture(&[("SKILL.md", "body")]);
        let b = fixture(&[("SKILL.md", "body")]);
        fs::create_dir_all(b.path().join("empty/nested")).unwrap();
        assert_eq!(digest_of(a.path()), digest_of(b.path()));
    }

    #[test]
    fn non_executable_permission_bits_do_not_contribute() {
        let a = fixture(&[("SKILL.md", "body")]);
        let b = fixture(&[("SKILL.md", "body")]);
        fs::set_permissions(b.path().join("SKILL.md"), fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(digest_of(a.path()), digest_of(b.path()));
    }

    #[test]
    fn symlinks_are_rejected() {
        let dir = fixture(&[("SKILL.md", "body")]);
        std::os::unix::fs::symlink(dir.path().join("SKILL.md"), dir.path().join("link.md"))
            .unwrap();
        assert!(Tree::read(dir.path()).is_err());
    }

    #[test]
    fn symlinked_directories_are_rejected() {
        let dir = fixture(&[("SKILL.md", "body"), ("ref/one.md", "one")]);
        std::os::unix::fs::symlink(dir.path().join("ref"), dir.path().join("alias")).unwrap();
        assert!(Tree::read(dir.path()).is_err());
    }

    #[test]
    fn files_are_listed_in_bytewise_relative_path_order() {
        let dir = fixture(&[("b.md", ""), ("a/z.md", ""), ("a.md", ""), ("SKILL.md", "")]);
        let tree = Tree::read(dir.path()).unwrap();
        let rels: Vec<&str> = tree.files().iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(rels, ["SKILL.md", "a.md", "a/z.md", "b.md"]);
    }

    #[test]
    fn materialize_reproduces_files_and_executable_bits() {
        let src = fixture(&[("SKILL.md", "body"), ("ref/run.sh", "#!/bin/sh")]);
        set_executable(src.path(), "ref/run.sh");
        let dest = TempDir::new().unwrap();
        let out = dest.path().join("out");

        Tree::read(src.path())
            .unwrap()
            .materialize_into(&out)
            .unwrap();

        assert_eq!(fs::read_to_string(out.join("SKILL.md")).unwrap(), "body");
        assert_eq!(
            fs::read_to_string(out.join("ref/run.sh")).unwrap(),
            "#!/bin/sh"
        );
        let mode = fs::metadata(out.join("ref/run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert!(mode & 0o111 != 0);
        assert!(
            fs::metadata(out.join("SKILL.md"))
                .unwrap()
                .permissions()
                .mode()
                & 0o111
                == 0
        );
        assert_eq!(digest_of(&out), digest_of(src.path()));
    }
}
