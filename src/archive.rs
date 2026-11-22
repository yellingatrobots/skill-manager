//! Tarball extraction that refuses anything a skill may not contain (ADR-0010).

use crate::error::{Error, IoContext, Result};
use flate2::read::GzDecoder;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Extracts a gzipped tar into `dest`, validating every path before writing it.
pub fn extract_tar_gz(bytes: &[u8], dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).at(dest)?;
    let mut archive = tar::Archive::new(GzDecoder::new(bytes));
    let entries = archive
        .entries()
        .map_err(|error| Error::Archive(format!("unreadable archive: {error}")))?;

    for entry in entries {
        let mut entry =
            entry.map_err(|error| Error::Archive(format!("unreadable archive entry: {error}")))?;
        let path = entry
            .path()
            .map_err(|error| Error::Archive(format!("unreadable archive path: {error}")))?
            .into_owned();
        let relative = safe_relative_path(&path)?;

        let kind = entry.header().entry_type();
        let destination = dest.join(&relative);
        if kind.is_dir() {
            fs::create_dir_all(&destination).at(&destination)?;
            continue;
        }
        if !kind.is_file() {
            return Err(Error::Archive(format!(
                "{}: archives may contain only regular files and directories",
                path.display()
            )));
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).at(parent)?;
        }
        let mut contents = Vec::new();
        entry
            .read_to_end(&mut contents)
            .map_err(|error| Error::Archive(format!("{}: {error}", path.display())))?;
        fs::write(&destination, contents).at(&destination)?;
    }
    Ok(())
}

/// Collapses a single wrapping directory so `dest` is the skill directory itself.
pub fn skill_root(dest: &Path) -> Result<PathBuf> {
    if dest.join(crate::skill::SKILL_FILE).is_file() {
        return Ok(dest.to_path_buf());
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(dest)
        .at(dest)?
        .map(|entry| entry.at(dest).map(|e| e.path()))
        .collect::<Result<_>>()?;
    entries.retain(|entry| entry.is_dir());
    match entries.as_slice() {
        [only] if only.join(crate::skill::SKILL_FILE).is_file() => Ok(only.clone()),
        _ => Ok(dest.to_path_buf()),
    }
}

fn safe_relative_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Err(Error::Archive(format!(
            "{}: absolute archive path",
            path.display()
        )));
    }
    let mut relative = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => relative.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(Error::Archive(format!(
                    "{}: archive path escapes the extraction root",
                    path.display()
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(Error::Archive(format!(
                    "{}: absolute archive path",
                    path.display()
                )));
            }
        }
    }
    if relative.as_os_str().is_empty() {
        return Err(Error::Archive(
            "archive entry has an empty path".to_string(),
        ));
    }
    Ok(relative)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tempfile::TempDir;

    fn tar_gz(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        for (path, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, contents.as_bytes())
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    /// The tar builder refuses malicious paths, so the header name is written directly.
    fn tar_gz_raw_path(path: &str, contents: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        let name = &mut header.as_old_mut().name;
        name[..path.len()].copy_from_slice(path.as_bytes());
        header.set_cksum();
        builder.append(&header, contents.as_bytes()).unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }

    fn tar_gz_with_link(link: &str, target: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        builder.append_link(&mut header, link, target).unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn extracts_files_and_directories() {
        let dir = TempDir::new().unwrap();
        extract_tar_gz(
            &tar_gz(&[("SKILL.md", "body"), ("ref/one.md", "one")]),
            dir.path(),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("SKILL.md")).unwrap(),
            "body"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("ref/one.md")).unwrap(),
            "one"
        );
    }

    #[test]
    fn rejects_paths_that_escape_the_root() {
        let dir = TempDir::new().unwrap();
        let error = extract_tar_gz(&tar_gz_raw_path("../escape.md", "x"), dir.path()).unwrap_err();
        assert!(error.to_string().contains("escapes"));
        assert!(!dir.path().parent().unwrap().join("escape.md").exists());
    }

    #[test]
    fn rejects_absolute_paths() {
        let dir = TempDir::new().unwrap();
        let error = extract_tar_gz(&tar_gz_raw_path("/etc/passwd", "x"), dir.path()).unwrap_err();
        assert!(error.to_string().contains("absolute"));
    }

    #[test]
    fn rejects_links() {
        let dir = TempDir::new().unwrap();
        let error =
            extract_tar_gz(&tar_gz_with_link("link.md", "/etc/passwd"), dir.path()).unwrap_err();
        assert!(error.to_string().contains("regular files"));
    }
}
