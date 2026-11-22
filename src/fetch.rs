//! Bringing upstream content onto disk. One adapter per transport (ADR-0011).

use crate::archive;
use crate::error::{Error, IoContext, Result};
use crate::git;
use crate::http;
use crate::manifest::{Source, Transport};
use crate::tree;
use crate::wellknown::{self, IndexEntry};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A git or github source on disk, at one commit.
#[derive(Debug)]
pub struct Checkout {
    pub root: PathBuf,
    pub coordinate: String,
    _temp: TempDir,
}

/// One archive skill on disk, unwrapped so its directory is named for the index.
#[derive(Debug)]
pub struct ArchiveSkill {
    /// The parent to walk: it contains exactly the index name.
    pub root: PathBuf,
    pub coordinate: String,
    /// The archive path recorded in the lock, so `hydrate` can refetch it.
    pub url_path: String,
    _temp: TempDir,
}

pub fn git_coordinate(commit: &str) -> String {
    format!("git:{commit}")
}

pub fn commit_of(coordinate: &str) -> Option<&str> {
    coordinate.strip_prefix("git:")
}

/// Checks out `git_ref`, or the source's declared intent when `None`.
pub fn checkout(source: &Source, git_ref: Option<&str>) -> Result<Checkout> {
    let url = match &source.transport {
        Transport::Git { .. } | Transport::GitHub { .. } => source.transport.url(),
        other => {
            return Err(Error::Configuration(format!(
                "source `{}` is a {} source and cannot be checked out",
                source.source_id,
                other.kind()
            )));
        }
    };
    let temp = TempDir::new().at(std::env::temp_dir())?;
    let root = temp.path().join("checkout");
    let wanted = git_ref.or(source.git_ref.as_deref());
    let commit = git::fetch_into(&url, wanted, &root)?;
    Ok(Checkout {
        root,
        coordinate: git_coordinate(&commit),
        _temp: temp,
    })
}

pub fn well_known_index(endpoint: &str) -> Result<Vec<IndexEntry>> {
    wellknown::parse_index(&http::get_string(&wellknown::index_url(endpoint))?)
}

/// Downloads and extracts one archive, verifying its digest when one is expected.
pub fn archive_skill(
    endpoint: &str,
    name: &str,
    url_path: &str,
    expected_digest: Option<&str>,
) -> Result<ArchiveSkill> {
    let url = wellknown::archive_url(endpoint, url_path);
    let bytes = http::get(&url)?;
    let digest = tree::sha256(&bytes).to_string();
    if let Some(expected) = expected_digest
        && digest != expected
    {
        return Err(Error::Network(format!(
            "{url}: archive digest {digest} does not match the locked {expected}"
        )));
    }

    let temp = TempDir::new().at(std::env::temp_dir())?;
    let root = temp.path().join("source");
    install_archive(&bytes, name, &root)?;
    Ok(ArchiveSkill {
        root,
        coordinate: digest,
        url_path: url_path.to_string(),
        _temp: temp,
    })
}

/// Extracts an archive so that `root/<name>` is the skill directory.
pub fn install_archive(bytes: &[u8], name: &str, root: &Path) -> Result<()> {
    let raw = root.join(".extracted");
    archive::extract_tar_gz(bytes, &raw)?;
    let skill_root = archive::skill_root(&raw)?;
    let destination = root.join(name);
    if skill_root == raw {
        // The archive is the skill itself; give it the name the index published.
        fs::create_dir_all(&destination).at(&destination)?;
        for entry in fs::read_dir(&raw).at(&raw)? {
            let path = entry.at(&raw)?.path();
            let target = destination.join(crate::deploy::file_name(&path));
            fs::rename(&path, &target).at(&target)?;
        }
        fs::remove_dir(&raw).at(&raw)?;
    } else {
        fs::rename(&skill_root, &destination).at(&destination)?;
        crate::fsx::remove_tree(&raw)?;
    }
    Ok(())
}

/// A local source is read straight from its working tree (ADR-0005).
///
/// `None` when the configured path is absent: one manifest is shared across
/// machines, and a local directory need not exist on all of them. A path that
/// exists but is not a directory stays an error — that is a manifest mistake.
pub fn local_root(source: &Source) -> Result<Option<PathBuf>> {
    let Transport::Path {
        resolved,
        configured,
    } = &source.transport
    else {
        return Err(Error::Configuration(format!(
            "source `{}` is a {} source, not a local path",
            source.source_id,
            source.transport.kind()
        )));
    };
    match fs::metadata(resolved) {
        Ok(metadata) if metadata.is_dir() => Ok(Some(resolved.clone())),
        Ok(_) => Err(Error::Configuration(format!(
            "source `{}`: {configured} is not a directory",
            source.source_id
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::io(resolved, error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;

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

    #[test]
    fn an_archive_wrapped_in_a_directory_is_renamed_to_the_index_name() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("source");

        install_archive(
            &tar_gz(&[("dsl/SKILL.md", "body"), ("dsl/ref/a.md", "a")]),
            "likec4-dsl",
            &root,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(root.join("likec4-dsl/SKILL.md")).unwrap(),
            "body"
        );
        assert_eq!(
            fs::read_to_string(root.join("likec4-dsl/ref/a.md")).unwrap(),
            "a"
        );
        assert!(!root.join(".extracted").exists());
    }

    #[test]
    fn an_archive_rooted_at_the_skill_is_given_the_index_name() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("source");

        install_archive(
            &tar_gz(&[("SKILL.md", "body"), ("ref/a.md", "a")]),
            "likec4-dsl",
            &root,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(root.join("likec4-dsl/SKILL.md")).unwrap(),
            "body"
        );
        assert_eq!(
            fs::read_to_string(root.join("likec4-dsl/ref/a.md")).unwrap(),
            "a"
        );
        assert!(!root.join(".extracted").exists());
    }
}
