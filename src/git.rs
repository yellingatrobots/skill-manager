//! Git and GitHub sources go through the installed `git` executable (ADR-0011).

use crate::error::{Error, IoContext, Result};
use std::path::Path;
use std::process::Command;

/// Fetches `git_ref` (or the remote default) into `dest` and returns the commit.
pub fn fetch_into(url: &str, git_ref: Option<&str>, dest: &Path) -> Result<String> {
    std::fs::create_dir_all(dest).at(dest)?;
    run(dest, &["init", "--quiet"])?;
    run(dest, &["remote", "add", "origin", url])?;

    let wanted = git_ref.unwrap_or("HEAD");
    if run(dest, &["fetch", "--quiet", "--depth", "1", "origin", wanted]).is_err() {
        // Servers that refuse a shallow fetch of a bare commit still serve everything.
        run(dest, &["fetch", "--quiet", "origin"])
            .map_err(|error| Error::Git(format!("{url}: cannot fetch `{wanted}`: {error}")))?;
        run(dest, &["checkout", "--quiet", "--detach", wanted])
            .map_err(|error| Error::Git(format!("{url}: cannot check out `{wanted}`: {error}")))?;
    } else {
        run(dest, &["checkout", "--quiet", "--detach", "FETCH_HEAD"])?;
    }

    let commit = run(dest, &["rev-parse", "HEAD"])?.trim().to_string();
    crate::fsx::remove_tree(&dest.join(".git"))?;
    Ok(commit)
}

fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::Git("`git` was not found on PATH; it is required for git and github sources".into())
        } else {
            Error::Git(format!("git {}: {error}", args.join(" ")))
        }
    })?;
    if !output.status.success() {
        return Err(Error::Git(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn origin(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        run(dir.path(), &["init", "--quiet", "--initial-branch", "main"]).unwrap();
        run(dir.path(), &["config", "user.email", "test@example.test"]).unwrap();
        run(dir.path(), &["config", "user.name", "Test"]).unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        run(dir.path(), &["add", "."]).unwrap();
        run(dir.path(), &["commit", "--quiet", "-m", "initial"]).unwrap();
        dir
    }

    #[test]
    fn fetches_the_default_branch_and_reports_the_commit() {
        let upstream = origin(&[("skills/handoff/SKILL.md", "---\nname: handoff\ndescription: d\n---\n")]);
        let dest = TempDir::new().unwrap();
        let checkout = dest.path().join("checkout");

        let commit = fetch_into(upstream.path().to_str().unwrap(), None, &checkout).unwrap();

        assert_eq!(commit.len(), 40);
        assert!(checkout.join("skills/handoff/SKILL.md").is_file());
    }

    #[test]
    fn removes_the_git_directory_from_the_checkout() {
        let upstream = origin(&[("SKILL.md", "body")]);
        let dest = TempDir::new().unwrap();
        let checkout = dest.path().join("checkout");

        fetch_into(upstream.path().to_str().unwrap(), None, &checkout).unwrap();

        assert!(!checkout.join(".git").exists());
    }

    #[test]
    fn fetches_a_named_ref() {
        let upstream = origin(&[("SKILL.md", "one")]);
        run(upstream.path(), &["tag", "v1"]).unwrap();
        fs::write(upstream.path().join("SKILL.md"), "two").unwrap();
        run(upstream.path(), &["commit", "--quiet", "-am", "second"]).unwrap();
        let dest = TempDir::new().unwrap();
        let checkout = dest.path().join("checkout");

        fetch_into(upstream.path().to_str().unwrap(), Some("v1"), &checkout).unwrap();

        assert_eq!(fs::read_to_string(checkout.join("SKILL.md")).unwrap(), "one");
    }

    #[test]
    fn fetches_a_pinned_commit() {
        let upstream = origin(&[("SKILL.md", "one")]);
        let first = run(upstream.path(), &["rev-parse", "HEAD"]).unwrap().trim().to_string();
        fs::write(upstream.path().join("SKILL.md"), "two").unwrap();
        run(upstream.path(), &["commit", "--quiet", "-am", "second"]).unwrap();
        let dest = TempDir::new().unwrap();
        let checkout = dest.path().join("checkout");

        let resolved = fetch_into(upstream.path().to_str().unwrap(), Some(&first), &checkout).unwrap();

        assert_eq!(resolved, first);
        assert_eq!(fs::read_to_string(checkout.join("SKILL.md")).unwrap(), "one");
    }

    #[test]
    fn reports_an_unresolvable_ref() {
        let upstream = origin(&[("SKILL.md", "one")]);
        let dest = TempDir::new().unwrap();

        let error = fetch_into(upstream.path().to_str().unwrap(), Some("nope"), &dest.path().join("c"))
            .unwrap_err();

        assert!(error.to_string().contains("nope"));
    }
}
