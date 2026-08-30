//! End-to-end runs of the built binary against a temporary HOME.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

struct Sandbox {
    home: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        Sandbox {
            home: TempDir::new().unwrap(),
        }
    }

    fn path(&self) -> &Path {
        self.home.path()
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_skill-manager"))
            .args(args)
            .env("HOME", self.path())
            .env("XDG_CONFIG_HOME", self.path().join(".config"))
            .env("XDG_DATA_HOME", self.path().join(".local/share"))
            .output()
            .expect("the binary runs")
    }

    fn manifest(&self) -> PathBuf {
        self.path().join(".config/skill-manager/skills.toml")
    }

    fn write_manifest(&self, contents: &str) {
        let path = self.manifest();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            contents.replace("{home}", &self.path().display().to_string()),
        )
        .unwrap();
    }

    fn write_skill(&self, name: &str, body: &str) {
        let dir = self.path().join("etc/ai/skills").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("SKILL.md"), skill_text(name, body)).unwrap();
    }

    fn target(&self, name: &str) -> PathBuf {
        self.path().join(".claude/skills").join(name)
    }
}

fn skill_text(name: &str, body: &str) -> String {
    format!("---\nname: {name}\ndescription: does {name}\n---\n{body}")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("the process exits normally")
}

/// Store entries are read-only, so write permission is restored before removal.
fn remove_read_only_tree(path: &Path) {
    let output = Command::new("chmod")
        .args(["-R", "u+w"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    fs::remove_dir_all(path).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_origin(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init", "--quiet", "--initial-branch", "main"]);
    git(dir.path(), &["config", "user.email", "test@example.test"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    for (path, contents) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, contents).unwrap();
    }
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "--quiet", "-m", "initial"]);
    dir
}

#[test]
fn the_first_run_scaffolds_a_manifest_instead_of_running_the_command() {
    let sandbox = Sandbox::new();

    let first = sandbox.run(&["apply"]);

    assert_eq!(code(&first), 0, "{}", stderr(&first));
    assert!(stdout(&first).contains(".config/skill-manager/skills.toml"));
    assert!(stdout(&first).contains("skill-manager add"));
    let manifest = fs::read_to_string(sandbox.manifest()).unwrap();
    assert!(manifest.contains("# [sources.mp]"));

    let second = sandbox.run(&["apply"]);

    assert_eq!(code(&second), 0, "{}", stderr(&second));
    assert!(stdout(&second).contains("deployed"));
}

#[test]
fn apply_deploys_a_local_source_into_both_targets() {
    let sandbox = Sandbox::new();
    sandbox.write_skill("codemode", "principles\n");
    sandbox.write_manifest("version = 1\n[sources.me]\npath = \"~/etc/ai/skills\"\n");

    let output = sandbox.run(&["apply"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stdout(&output).contains("deployed 1 skill(s)"));
    for target in [".claude/skills", ".agents/skills"] {
        let deployed = sandbox.path().join(target).join("me:codemode/SKILL.md");
        assert!(
            fs::read_to_string(deployed)
                .unwrap()
                .contains("name: me:codemode\n")
        );
    }
    assert!(sandbox.target("sm:provenance").exists());
}

#[test]
fn apply_without_a_lock_for_a_remote_source_says_to_update() {
    let sandbox = Sandbox::new();
    sandbox.write_manifest("version = 1\n[sources.mp]\ngithub = \"mattpocock/skills\"\n");

    let output = sandbox.run(&["apply"]);

    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("skill-manager update"));
}

#[test]
fn a_malformed_manifest_aborts() {
    let sandbox = Sandbox::new();
    sandbox.write_manifest("version = 1\n[sources.mp]\nnope = \"x\"\n");

    let output = sandbox.run(&["apply"]);

    assert_eq!(code(&output), 2);
    assert!(stderr(&output).contains("invalid manifest"));
}

#[test]
fn an_invalid_skill_is_skipped_and_the_command_exits_non_zero() {
    let sandbox = Sandbox::new();
    sandbox.write_skill("good", "body\n");
    let bad = sandbox.path().join("etc/ai/skills/bad");
    fs::create_dir_all(&bad).unwrap();
    fs::write(bad.join("SKILL.md"), "no frontmatter\n").unwrap();
    sandbox.write_manifest("version = 1\n[sources.me]\npath = \"~/etc/ai/skills\"\n");

    let output = sandbox.run(&["apply"]);

    assert_eq!(code(&output), 1);
    assert!(stderr(&output).contains("me:bad"));
    assert!(sandbox.target("me:good").exists());
}

#[test]
fn update_then_hydrate_rebuilds_the_store_from_the_lock() {
    let sandbox = Sandbox::new();
    let upstream = git_origin(&[("handoff/SKILL.md", &skill_text("handoff", "body\n"))]);
    sandbox.write_manifest(&format!(
        "version = 1\n[sources.mp]\ngit = \"{}\"\n",
        upstream.path().display()
    ));

    let updated = sandbox.run(&["update"]);
    assert_eq!(code(&updated), 0, "{}", stderr(&updated));
    assert!(sandbox.target("mp:handoff").exists());

    let store = sandbox.path().join(".local/share/skill-manager/store");
    remove_read_only_tree(&store);
    let hydrated = sandbox.run(&["hydrate"]);

    assert_eq!(code(&hydrated), 0, "{}", stderr(&hydrated));
    assert!(stdout(&hydrated).contains("mp:handoff"));
    assert!(store.exists());
}

#[test]
fn info_reports_provenance_as_text_and_json() {
    let sandbox = Sandbox::new();
    sandbox.write_skill("codemode", "body\n");
    sandbox.write_manifest("version = 1\n[sources.me]\npath = \"~/etc/ai/skills\"\n");
    sandbox.run(&["apply"]);

    let listing = sandbox.run(&["info"]);
    assert_eq!(code(&listing), 0);
    assert!(stdout(&listing).contains("me:codemode"));
    assert!(stdout(&listing).contains("sm:provenance"));

    let detail = sandbox.run(&["info", "me:codemode", "--json"]);
    assert_eq!(code(&detail), 0, "{}", stderr(&detail));
    let json: serde_json::Value = serde_json::from_str(&stdout(&detail)).unwrap();
    let record = &json["skills"][0];
    assert_eq!(record["name"], "me:codemode");
    assert_eq!(record["source_kind"], "path");
    assert!(
        record["upstream_coordinate"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
}

#[test]
fn info_lists_only_skills_until_full_is_asked_for() {
    let sandbox = Sandbox::new();
    sandbox.write_skill("codemode", "body\n");
    sandbox.write_manifest("version = 1\n[sources.me]\npath = \"~/etc/ai/skills\"\n");
    sandbox.run(&["apply"]);

    let listing = sandbox.run(&["info"]);
    assert_eq!(code(&listing), 0, "{}", stderr(&listing));
    let text = stdout(&listing);
    assert!(text.contains("me:codemode"));
    assert!(!text.contains(&sandbox.manifest().display().to_string()));
    assert!(!text.contains("upstream_coordinate"));

    let one = sandbox.run(&["info", "me:codemode"]);
    assert_eq!(stdout(&one).lines().count(), 1);
}

#[test]
fn info_full_reports_where_everything_lives_and_every_field() {
    let sandbox = Sandbox::new();
    sandbox.write_skill("codemode", "body\n");
    sandbox.write_manifest("version = 1\n[sources.me]\npath = \"~/etc/ai/skills\"\n");
    sandbox.run(&["apply"]);

    let listing = sandbox.run(&["info", "--full"]);

    assert_eq!(code(&listing), 0, "{}", stderr(&listing));
    let text = stdout(&listing);
    assert!(text.contains(&sandbox.manifest().display().to_string()));
    assert!(text.contains(".local/share/skill-manager/store"));
    assert!(text.contains(".claude/skills"));
    assert!(text.contains(".agents/skills"));
    assert!(text.contains("upstream_coordinate"));

    let json = sandbox.run(&["info", "--json"]);
    let value: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(
        value["locations"]["manifest"],
        sandbox.manifest().display().to_string()
    );
    assert!(
        value["locations"]["generation"]
            .as_str()
            .unwrap()
            .contains("generations/")
    );
}

#[test]
fn add_and_remove_edit_the_manifest_in_place() {
    let sandbox = Sandbox::new();
    sandbox.write_skill("codemode", "body\n");
    sandbox.write_skill("tdd", "body\n");
    sandbox.write_manifest(
        "version = 1\n# mine\n[sources.me]\npath = \"~/etc/ai/skills\"\nskills = [\"codemode\"]\n",
    );

    let added = sandbox.run(&["add", "me:tdd"]);
    assert_eq!(code(&added), 0, "{}", stderr(&added));
    let manifest = fs::read_to_string(sandbox.manifest()).unwrap();
    assert!(manifest.contains("# mine"));
    assert!(manifest.contains(r#"skills = ["codemode", "tdd"]"#));
    assert!(sandbox.target("me:tdd").exists());

    let removed = sandbox.run(&["remove", "me:tdd"]);
    assert_eq!(code(&removed), 0, "{}", stderr(&removed));
    assert!(!sandbox.target("me:tdd").exists());
    assert!(sandbox.target("me:codemode").exists());
}

#[test]
fn verify_passes_after_apply_and_fails_after_tampering() {
    let sandbox = Sandbox::new();
    sandbox.write_skill("codemode", "body\n");
    sandbox.write_manifest("version = 1\n[sources.me]\npath = \"~/etc/ai/skills\"\n");
    sandbox.run(&["apply"]);

    let clean = sandbox.run(&["verify"]);
    assert_eq!(code(&clean), 0, "{}", stderr(&clean));
    assert!(stdout(&clean).contains("verified 2 deployed skill(s)"));

    fs::remove_file(sandbox.target("me:codemode")).unwrap();
    let tampered = sandbox.run(&["verify"]);

    assert_eq!(code(&tampered), 1);
    assert!(stderr(&tampered).contains("me:codemode"));
}

#[test]
fn help_lists_every_command() {
    let sandbox = Sandbox::new();
    let output = sandbox.run(&["--help"]);
    let text = stdout(&output);
    for command in [
        "apply", "update", "hydrate", "add", "remove", "info", "verify",
    ] {
        assert!(text.contains(command), "`{command}` missing from help");
    }
}
