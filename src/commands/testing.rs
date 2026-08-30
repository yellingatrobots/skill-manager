//! Fixtures shared by the command tests.

use crate::paths::Paths;
use crate::store::Store;
use crate::tree::Tree;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub struct Home {
    pub paths: Paths,
    temp: TempDir,
}

impl Home {
    pub fn path(&self) -> PathBuf {
        self.temp.path().to_path_buf()
    }
}

pub fn home() -> Home {
    let temp = TempDir::new().unwrap();
    Home {
        paths: Paths::rooted_at(temp.path()),
        temp,
    }
}

pub fn write_skill(dir: &Path, name: &str, body: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: does {name}\n---\n{body}"),
    )
    .unwrap();
}

/// `{home}` in `contents` expands to this home, so tests never depend on `$HOME`.
pub fn write_manifest(home: &Home, contents: &str) {
    let contents = &contents.replace("{home}", &home.path().display().to_string());
    let path = &home.paths.manifest;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

pub fn write_lock(home: &Home, contents: &str) {
    let path = &home.paths.lock;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Puts a pristine skill in the store and returns its content digest.
pub fn store_skill(home: &Home, name: &str, body: &str) -> String {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join(name);
    write_skill(&dir, name, body);
    Store::new(&home.paths.store)
        .insert(&Tree::read(&dir).unwrap())
        .unwrap()
        .to_string()
}

pub fn lock_toml(
    alias: &str,
    kind: &str,
    url: &str,
    git_ref: Option<&str>,
    skills: &[(&str, &str, &str, &str)],
) -> String {
    let mut out = format!("version = 1\n\n[sources.{alias}]\nkind = \"{kind}\"\nurl = \"{url}\"\n");
    if let Some(git_ref) = git_ref {
        out.push_str(&format!("ref = \"{git_ref}\"\n"));
    }
    for (name, path, coordinate, digest) in skills {
        out.push_str(&format!(
            "\n[sources.{alias}.skills.{name}]\npath = \"{path}\"\nupstream_name = \"{name}\"\ncoordinate = \"{coordinate}\"\ndigest = \"{digest}\"\n"
        ));
    }
    out
}

pub fn skill_text(name: &str, body: &str) -> String {
    format!("---\nname: {name}\ndescription: does {name}\n---\n{body}")
}

fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git is available in tests");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A real repository on disk, used as a git source URL.
pub fn git_origin(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "--quiet", "--initial-branch", "main"]);
    git(dir.path(), &["config", "user.email", "test@example.test"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    git_commit(&dir, files);
    dir
}

pub fn git_commit(origin: &TempDir, files: &[(&str, &str)]) {
    for (path, contents) in files {
        let full = origin.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
    }
    git(origin.path(), &["add", "."]);
    git(
        origin.path(),
        &["commit", "--quiet", "-m", "change", "--allow-empty"],
    );
}

pub fn git_tag(origin: &TempDir, tag: &str) {
    git(origin.path(), &["tag", tag]);
}
