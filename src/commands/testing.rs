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
    source_id: &str,
    kind: &str,
    url: &str,
    git_ref: Option<&str>,
    skills: &[(&str, &str, &str, &str)],
) -> String {
    let mut out =
        format!("version = 1\n\n[sources.{source_id}]\nkind = \"{kind}\"\nurl = \"{url}\"\n");
    if let Some(git_ref) = git_ref {
        out.push_str(&format!("ref = \"{git_ref}\"\n"));
    }
    for (name, path, coordinate, digest) in skills {
        out.push_str(&format!(
            "\n[sources.{source_id}.skills.{name}]\npath = \"{path}\"\nupstream_name = \"{name}\"\ncoordinate = \"{coordinate}\"\ndigest = \"{digest}\"\n"
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

/// A gzipped tarball of one skill directory, as a well-known endpoint serves it.
pub fn skill_archive(name: &str, body: &str) -> Vec<u8> {
    use flate2::{Compression, write::GzEncoder};
    let contents = skill_text(name, body);
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, format!("{name}/SKILL.md"), contents.as_bytes())
        .unwrap();
    builder.into_inner().unwrap().finish().unwrap()
}

/// The content digest of what `archive` extracts to, as the lock records it.
pub fn extracted_digest(archive: &[u8], name: &str) -> String {
    let temp = TempDir::new().unwrap();
    crate::fetch::install_archive(archive, name, temp.path()).unwrap();
    Tree::read(temp.path().join(name))
        .unwrap()
        .digest()
        .unwrap()
        .to_string()
}

/// Serves `files` by request path over plain HTTP on localhost until the test
/// process exits, and returns the base URL.
pub fn serve(files: Vec<(&'static str, Vec<u8>)>) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let mut header = String::new();
            while reader.read_line(&mut header).unwrap() > 2 {
                header.clear();
            }
            let path = request.split_whitespace().nth(1).unwrap_or_default();
            let (status, body) = match files.iter().find(|(served, _)| *served == path) {
                Some((_, body)) => ("200 OK", body.as_slice()),
                None => ("404 Not Found", &b""[..]),
            };
            let head = format!(
                "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        }
    });
    base
}
