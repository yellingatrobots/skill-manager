//! Immutable, content-addressed storage of pristine upstream content (ADR-0003).

use crate::error::{Error, IoContext, Result};
use crate::fsx;
use crate::tree::{Digest, Tree};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Store { root: root.into() }
    }

    pub fn path_for(&self, digest: &Digest) -> PathBuf {
        self.root.join(digest.store_relative_path())
    }

    pub fn contains(&self, digest: &Digest) -> bool {
        self.path_for(digest).is_dir()
    }

    /// Copies a tree in under its own digest. Already-present content is left alone.
    pub fn insert(&self, tree: &Tree) -> Result<Digest> {
        let digest = tree.digest()?;
        let destination = self.path_for(&digest);
        if destination.is_dir() {
            return Ok(digest);
        }

        let parent = destination.parent().expect("digest paths have a parent");
        fs::create_dir_all(parent).at(parent)?;
        let staging = tempfile::TempDir::new_in(parent).at(parent)?;
        let staged = staging.path().join("entry");
        tree.materialize_into(&staged)?;

        // Renaming a directory rewrites its `..`, so it must stay writable until it lands.
        match fs::rename(&staged, &destination) {
            Ok(()) => fsx::make_read_only(&destination).map(|()| digest),
            // A concurrent writer won the race with identical content.
            Err(_) if destination.is_dir() => Ok(digest),
            Err(error) => Err(Error::io(&destination, error)),
        }
    }

    pub fn tree(&self, digest: &Digest) -> Result<Tree> {
        let path = self.path_for(digest);
        if !path.is_dir() {
            return Err(Error::MissingStoreEntry(digest.to_string()));
        }
        Tree::read(&path)
    }

    /// Rehashes an entry: content-addressed storage makes this the whole check.
    pub fn verify(&self, digest: &Digest) -> Result<bool> {
        Ok(&self.tree(digest)?.digest()? == digest)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fixture(contents: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("SKILL.md"), contents).unwrap();
        dir
    }

    #[test]
    fn insert_stores_content_under_its_digest() {
        let home = TempDir::new().unwrap();
        let store = Store::new(home.path().join("store"));
        let source = fixture("body");
        let tree = Tree::read(source.path()).unwrap();

        let digest = store.insert(&tree).unwrap();

        assert_eq!(digest, tree.digest().unwrap());
        assert!(store.contains(&digest));
        assert!(store.path_for(&digest).starts_with(home.path().join("store/sha256")));
        assert_eq!(
            fs::read_to_string(store.path_for(&digest).join("SKILL.md")).unwrap(),
            "body"
        );
    }

    #[test]
    fn stored_entries_are_read_only() {
        let home = TempDir::new().unwrap();
        let store = Store::new(home.path().join("store"));
        let source = fixture("body");

        let digest = store.insert(&Tree::read(source.path()).unwrap()).unwrap();

        assert!(fs::write(store.path_for(&digest).join("SKILL.md"), "tampered").is_err());
    }

    #[test]
    fn inserting_the_same_content_twice_is_a_no_op() {
        let home = TempDir::new().unwrap();
        let store = Store::new(home.path().join("store"));
        let source = fixture("body");
        let tree = Tree::read(source.path()).unwrap();

        let first = store.insert(&tree).unwrap();
        let second = store.insert(&tree).unwrap();

        assert_eq!(first, second);
        assert_eq!(fs::read_dir(store.root().join("sha256")).unwrap().count(), 1);
    }

    #[test]
    fn different_content_lands_in_different_entries() {
        let home = TempDir::new().unwrap();
        let store = Store::new(home.path().join("store"));
        let one = fixture("one");
        let two = fixture("two");

        let a = store.insert(&Tree::read(one.path()).unwrap()).unwrap();
        let b = store.insert(&Tree::read(two.path()).unwrap()).unwrap();

        assert_ne!(a, b);
        assert_eq!(fs::read_dir(store.root().join("sha256")).unwrap().count(), 2);
    }

    #[test]
    fn tree_reads_back_a_stored_entry() {
        let home = TempDir::new().unwrap();
        let store = Store::new(home.path().join("store"));
        let source = fixture("body");
        let digest = store.insert(&Tree::read(source.path()).unwrap()).unwrap();

        let stored = store.tree(&digest).unwrap();

        assert_eq!(stored.digest().unwrap(), digest);
        assert!(store.verify(&digest).unwrap());
    }

    #[test]
    fn missing_entries_are_reported_as_missing() {
        let home = TempDir::new().unwrap();
        let store = Store::new(home.path().join("store"));
        let digest = Digest::parse(&format!("sha256:{}", "a".repeat(64))).unwrap();

        assert!(!store.contains(&digest));
        assert!(matches!(store.tree(&digest), Err(Error::MissingStoreEntry(_))));
    }

    #[test]
    fn verify_detects_tampered_entries() {
        let home = TempDir::new().unwrap();
        let store = Store::new(home.path().join("store"));
        let source = fixture("body");
        let digest = store.insert(&Tree::read(source.path()).unwrap()).unwrap();

        fsx::make_writable(&store.path_for(&digest)).unwrap();
        fs::write(store.path_for(&digest).join("SKILL.md"), "tampered").unwrap();

        assert!(!store.verify(&digest).unwrap());
    }
}
