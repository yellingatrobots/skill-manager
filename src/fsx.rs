//! Filesystem operations that must not be observed half-done.

use crate::error::{Error, IoContext, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Matches the kernel's own limit on a symlink chain.
const MAX_SYMLINK_HOPS: usize = 40;

/// Writes `contents` to a temporary sibling, then renames it over `path`.
/// Returns the file written: `path` with any symlink chain followed.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<PathBuf> {
    let path = &resolve_symlinks(path)?;
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).at(parent)?;
    let temporary = tempfile::NamedTempFile::new_in(parent).at(parent)?;
    fs::write(temporary.path(), contents).at(temporary.path())?;
    temporary
        .persist(path)
        .map_err(|error| crate::error::Error::io(path, error.error))?;
    Ok(path.clone())
}

/// Follows a symlink chain to the path that holds the content.
///
/// Renaming over a link would replace it: a manifest linked into a dotfiles repo
/// would lose the link and strand the edit outside the repo.
fn resolve_symlinks(path: &Path) -> Result<PathBuf> {
    let mut resolved = path.to_path_buf();
    for _ in 0..MAX_SYMLINK_HOPS {
        let is_symlink =
            fs::symlink_metadata(&resolved).is_ok_and(|metadata| metadata.file_type().is_symlink());
        if !is_symlink {
            return Ok(resolved);
        }
        let target = fs::read_link(&resolved).at(&resolved)?;
        let parent = resolved.parent().unwrap_or(Path::new("."));
        resolved = parent.join(target);
    }
    Err(Error::Configuration(format!(
        "{}: too many levels of symbolic links",
        path.display()
    )))
}

/// Replaces `link` with a symlink to `target` without an interval where it is absent.
pub fn replace_symlink_atomic(link: &Path, target: &Path) -> Result<()> {
    let parent = link.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).at(parent)?;
    let staged = parent.join(format!(
        ".{}.staging",
        link.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = fs::remove_file(&staged);
    std::os::unix::fs::symlink(target, &staged).at(&staged)?;
    fs::rename(&staged, link).at(link)
}

/// Recursively makes a tree read-only, deepest entries first.
pub fn make_read_only(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).at(path)?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path).at(path)? {
            make_read_only(&entry.at(path)?.path())?;
        }
        set_mode(path, 0o555)
    } else {
        let executable = metadata.permissions().mode() & 0o111 != 0;
        set_mode(path, if executable { 0o555 } else { 0o444 })
    }
}

/// Restores write permission so a tree can be removed.
pub fn make_writable(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).at(path)?;
    if metadata.is_dir() {
        set_mode(path, metadata.permissions().mode() | 0o700)?;
        for entry in fs::read_dir(path).at(path)? {
            make_writable(&entry.at(path)?.path())?;
        }
        Ok(())
    } else {
        set_mode(path, metadata.permissions().mode() | 0o200)
    }
}

/// Removes a tree that may be read-only, never following symlinks out of it.
pub fn remove_tree(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(crate::error::Error::io(path, error)),
        Ok(metadata) if !metadata.is_dir() => return fs::remove_file(path).at(path),
        Ok(_) => {}
    }
    make_writable(path)?;
    fs::remove_dir_all(path).at(path)
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).at(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_atomic_creates_parents_and_replaces_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested/file.toml");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    }

    #[test]
    fn write_atomic_writes_through_a_symlink() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("repo/skills.toml");
        write_atomic(&target, b"first").unwrap();
        let hop = dir.path().join("hop");
        let link = dir.path().join("skills.toml");
        std::os::unix::fs::symlink(&target, &hop).unwrap();
        std::os::unix::fs::symlink("hop", &link).unwrap();

        assert_eq!(write_atomic(&link, b"second").unwrap(), target);

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "second");
    }

    #[test]
    fn write_atomic_creates_the_target_of_a_dangling_symlink() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("repo/skills.toml");
        let link = dir.path().join("skills.toml");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        write_atomic(&link, b"scaffold").unwrap();

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "scaffold");
    }

    #[test]
    fn write_atomic_refuses_a_symlink_loop() {
        let dir = TempDir::new().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::os::unix::fs::symlink(&b, &a).unwrap();
        std::os::unix::fs::symlink(&a, &b).unwrap();

        assert!(write_atomic(&a, b"never").is_err());
    }

    #[test]
    fn replace_symlink_atomic_repoints_an_existing_link() {
        let dir = TempDir::new().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        let link = dir.path().join("current");

        replace_symlink_atomic(&link, &a).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), a);
        replace_symlink_atomic(&link, &b).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), b);
    }

    #[test]
    fn read_only_trees_can_still_be_removed() {
        let dir = TempDir::new().unwrap();
        let tree = dir.path().join("store-entry");
        fs::create_dir_all(tree.join("ref")).unwrap();
        fs::write(tree.join("ref/one.md"), "one").unwrap();
        make_read_only(&tree).unwrap();

        assert!(fs::write(tree.join("ref/one.md"), "two").is_err());
        remove_tree(&tree).unwrap();
        assert!(!tree.exists());
    }

    #[test]
    fn remove_tree_removes_a_symlink_without_following_it() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.md"), "keep").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        remove_tree(&link).unwrap();

        assert!(!link.exists());
        assert!(target.join("keep.md").exists());
    }

    #[test]
    fn remove_tree_is_a_no_op_for_missing_paths() {
        let dir = TempDir::new().unwrap();
        remove_tree(&dir.path().join("absent")).unwrap();
    }
}
