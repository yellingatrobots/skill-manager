//! Where the manifest, lock, store, generations, and targets live.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Fixed in v1: Codex discovers through `~/.agents/skills` (ADR-0009).
const TARGET_DIRECTORIES: [&str; 2] = [".claude/skills", ".agents/skills"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub manifest: PathBuf,
    pub lock: PathBuf,
    pub store: PathBuf,
    /// The build directory: a target entry is ours iff it is a symlink into it.
    pub deployed: PathBuf,
    pub generations: PathBuf,
    pub current: PathBuf,
    pub targets: Vec<PathBuf>,
}

impl Paths {
    pub fn from_env() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| Error::Configuration("HOME is not set".to_string()))?;
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        Ok(Paths::new(&home, &data))
    }

    pub fn new(home: &Path, data_home: &Path) -> Self {
        let deployed = data_home.join("skill-manager/deployed");
        Paths {
            manifest: home.join("etc/ai/skills.toml"),
            lock: home.join("etc/ai/skills.lock"),
            store: data_home.join("skill-manager/store"),
            deployed: deployed.clone(),
            generations: deployed.join("generations"),
            current: deployed.join("current"),
            targets: TARGET_DIRECTORIES.iter().map(|target| home.join(target)).collect(),
        }
    }

    /// Everything under one root, for tests and for `--root`-style isolation.
    pub fn rooted_at(root: &Path) -> Self {
        Paths::new(root, &root.join(".local/share"))
    }

    pub fn generation(&self, digest: &crate::tree::Digest) -> PathBuf {
        self.generations.join(digest.store_relative_path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_the_documented_locations() {
        let paths = Paths::new(Path::new("/home/t"), Path::new("/home/t/.local/share"));
        assert_eq!(paths.manifest, Path::new("/home/t/etc/ai/skills.toml"));
        assert_eq!(paths.lock, Path::new("/home/t/etc/ai/skills.lock"));
        assert_eq!(paths.store, Path::new("/home/t/.local/share/skill-manager/store"));
        assert_eq!(paths.current, Path::new("/home/t/.local/share/skill-manager/deployed/current"));
        assert_eq!(paths.deployed, Path::new("/home/t/.local/share/skill-manager/deployed"));
        assert_eq!(
            paths.targets,
            [Path::new("/home/t/.claude/skills"), Path::new("/home/t/.agents/skills")]
        );
    }

    #[test]
    fn honours_xdg_data_home() {
        let paths = Paths::new(Path::new("/home/t"), Path::new("/data"));
        assert_eq!(paths.store, Path::new("/data/skill-manager/store"));
        assert_eq!(paths.manifest, Path::new("/home/t/etc/ai/skills.toml"));
    }
}
