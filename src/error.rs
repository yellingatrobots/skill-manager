use std::fmt;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{0}: skills may contain only regular files and directories")]
    IrregularEntry(PathBuf),

    #[error("{0}: path is not valid UTF-8")]
    NonUtf8Path(PathBuf),

    #[error("invalid manifest: {0}")]
    Manifest(String),

    #[error("invalid lock: {0}")]
    Lock(String),

    #[error("{0}")]
    Configuration(String),

    #[error("store entry {0} is missing; run `skill-manager hydrate`")]
    MissingStoreEntry(String),
}

impl Error {
    pub fn io(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Error::Io { path: path.as_ref().to_path_buf(), source }
    }
}

/// Wraps an `io::Result` with the path it concerns.
pub trait IoContext<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: impl AsRef<Path>) -> Result<T> {
        self.map_err(|source| Error::io(path, source))
    }
}

/// Collects failures that must all be reported before a command exits non-zero.
#[derive(Debug, Default)]
pub struct Failures(Vec<String>);

impl Failures {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, message: impl fmt::Display) {
        self.0.push(message.to_string());
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }
}
