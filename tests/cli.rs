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
        let deployed = sandbox.path().join(target).join("me-codemode/SKILL.md");
        assert!(
            fs::read_to_string(deployed)
                .unwrap()
                .contains("name: me-codemode\n")
        );
    }
    assert!(sandbox.target("sm-provenance").exists());
}

#[test]
fn apply_reports_every_deployed_change_and_where_it_was_made() {
    let sandbox = Sandbox::new();
    sandbox.write_skill("codemode", "one\n");
    sandbox.write_skill("old", "one\n");
    sandbox.write_manifest("version = 1\n[sources.me]\npath = \"~/etc/ai/skills\"\n");

    let first = sandbox.run(&["apply"]);

    assert_eq!(code(&first), 0, "{}", stderr(&first));
    let text = stdout(&first);
    assert!(text.contains("+ me-codemode"), "{text}");
    assert!(
        text.contains(".local/share/skill-manager/deployed/current -> "),
        "{text}"
    );
    assert!(text.contains(".claude/skills: linked "), "{text}");

    sandbox.write_skill("codemode", "two\n");
    fs::remove_dir_all(sandbox.path().join("etc/ai/skills/old")).unwrap();

    let second = sandbox.run(&["apply"]);

    assert_eq!(code(&second), 0, "{}", stderr(&second));
    let text = stdout(&second);
    assert!(text.contains("~ me-codemode"), "{text}");
    assert!(text.contains("- me-old"), "{text}");
    assert!(text.contains("= 1 skill(s) unchanged"), "{text}");
    assert!(text.contains(".agents/skills: removed me-old"), "{text}");
    assert!(text.contains("pruned "), "{text}");

    let third = sandbox.run(&["apply"]);

    let text = stdout(&third);
    assert!(text.contains("unchanged"), "{text}");
    assert!(text.contains(".claude/skills: no changes"), "{text}");
}

#[test]
fn apply_skips_a_local_source_whose_path_is_absent_and_still_succeeds() {
    let sandbox = Sandbox::new();
    sandbox.write_skill("codemode", "principles\n");
    sandbox.write_manifest(
        "version = 1\n\
         [sources.me]\npath = \"~/etc/ai/skills\"\n\n\
         [sources.work]\npath = \"~/not/on/this/machine\"\n",
    );

    let output = sandbox.run(&["apply"]);

    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stdout(&output).contains("source `work`"));
    assert!(stdout(&output).contains("does not exist"));
    assert!(stdout(&output).contains("deployed 1 skill(s)"));
    assert!(sandbox.target("me-codemode").exists());

    let verified = sandbox.run(&["verify"]);

    assert_eq!(code(&verified), 0, "{}", stderr(&verified));
    assert!(stdout(&verified).contains("source `work`"));
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
    assert!(stderr(&output).contains("me-bad"));
    assert!(sandbox.target("me-good").exists());
}

#[test]
fn apply_hydrates_a_missing_store_unless_offline() {
    let sandbox = Sandbox::new();
    let upstream = git_origin(&[("handoff/SKILL.md", &skill_text("handoff", "body\n"))]);
    sandbox.write_manifest(&format!(
        "version = 1\n[sources.mp]\ngit = \"{}\"\n",
        upstream.path().display()
    ));
    assert_eq!(code(&sandbox.run(&["update", "--no-apply"])), 0);
    let store = sandbox.path().join(".local/share/skill-manager/store");
    remove_read_only_tree(&store);

    let offline = sandbox.run(&["apply", "--offline"]);

    assert_eq!(code(&offline), 2, "{}", stdout(&offline));
    assert!(
        stderr(&offline).contains("--offline"),
        "{}",
        stderr(&offline)
    );
    assert!(!sandbox.target("mp-handoff").exists());

    let online = sandbox.run(&["apply"]);

    assert_eq!(code(&online), 0, "{}", stderr(&online));
    let text = stdout(&online);
    assert!(text.contains("hydrated 1 skill(s)"), "{text}");
    assert!(text.contains("+ mp-handoff  "), "{text}");
    assert!(sandbox.target("mp-handoff/SKILL.md").is_file());
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
    assert!(sandbox.target("mp-handoff").exists());

    let store = sandbox.path().join(".local/share/skill-manager/store");
    remove_read_only_tree(&store);
    let hydrated = sandbox.run(&["hydrate"]);

    assert_eq!(code(&hydrated), 0, "{}", stderr(&hydrated));
    assert!(stdout(&hydrated).contains("mp-handoff"));
    assert!(
        stdout(&hydrated).contains(".local/share/skill-manager/store/sha256/"),
        "{}",
        stdout(&hydrated)
    );
    assert!(store.exists());
}

#[test]
fn update_prints_why_a_selected_skill_could_not_be_locked() {
    let sandbox = Sandbox::new();
    let upstream = git_origin(&[("bad/SKILL.md", "no frontmatter\n")]);
    sandbox.write_manifest(&format!(
        "version = 1\n[sources.mp]\ngit = \"{}\"\nskills = [\"bad\"]\n",
        upstream.path().display()
    ));

    let output = sandbox.run(&["update"]);

    assert_eq!(code(&output), 1, "{}", stderr(&output));
    let errors = stderr(&output);
    assert!(errors.contains("failed: "), "{errors}");
    assert!(errors.contains("frontmatter"), "{errors}");
    assert!(errors.contains("could not be locked"), "{errors}");
}

#[test]
fn update_reports_each_lock_change_and_where_it_was_written() {
    let sandbox = Sandbox::new();
    let upstream = git_origin(&[
        ("handoff/SKILL.md", &skill_text("handoff", "one\n")),
        ("retired/SKILL.md", &skill_text("retired", "one\n")),
    ]);
    sandbox.write_manifest(&format!(
        "version = 1\n[sources.mp]\ngit = \"{}\"\n",
        upstream.path().display()
    ));

    let first = sandbox.run(&["update"]);

    assert_eq!(code(&first), 0, "{}", stderr(&first));
    let text = stdout(&first);
    assert!(text.contains(".config/skill-manager/skills.lock"), "{text}");
    assert!(text.contains("+ mp-handoff"), "{text}");
    assert!(text.contains("+ mp-retired"), "{text}");
    assert!(text.contains("store: wrote 2 new entries"), "{text}");
    assert!(
        text.contains(".local/share/skill-manager/store/sha256/"),
        "{text}"
    );

    fs::write(
        upstream.path().join("handoff/SKILL.md"),
        skill_text("handoff", "two\n"),
    )
    .unwrap();
    fs::remove_dir_all(upstream.path().join("retired")).unwrap();
    fs::create_dir_all(upstream.path().join("tdd")).unwrap();
    fs::write(
        upstream.path().join("tdd/SKILL.md"),
        skill_text("tdd", "one\n"),
    )
    .unwrap();
    git(upstream.path(), &["add", "-A"]);
    git(upstream.path(), &["commit", "--quiet", "-m", "second"]);

    let second = sandbox.run(&["update"]);

    assert_eq!(code(&second), 0, "{}", stderr(&second));
    let text = stdout(&second);
    assert!(text.contains("~ mp-handoff"), "{text}");
    assert!(text.contains("- mp-retired"), "{text}");
    assert!(text.contains("+ mp-tdd"), "{text}");
    assert!(text.contains(" -> "), "{text}");

    let third = sandbox.run(&["update"]);

    let text = stdout(&third);
    assert!(text.contains("no changes"), "{text}");
    assert!(!text.contains("~ mp-handoff"), "{text}");
    assert!(text.contains("store: no new entries"), "{text}");
}

#[test]
fn info_lists_skills_tersely_and_reports_everything_on_request() {
    let sandbox = Sandbox::new();
    sandbox.write_skill("codemode", "body\n");
    sandbox.write_manifest("version = 1\n[sources.me]\npath = \"~/etc/ai/skills\"\n");
    sandbox.run(&["apply"]);
    let manifest = sandbox.manifest().display().to_string();

    let listing = sandbox.run(&["info"]);
    assert_eq!(code(&listing), 0, "{}", stderr(&listing));
    let text = stdout(&listing);
    assert!(text.contains("me-codemode"));
    assert!(text.contains("sm-provenance"));
    assert!(!text.contains(&manifest));
    assert!(!text.contains("upstream_coordinate"));

    let one = sandbox.run(&["info", "me-codemode"]);
    assert_eq!(stdout(&one).lines().count(), 1);

    let full = sandbox.run(&["info", "--full"]);
    assert_eq!(code(&full), 0, "{}", stderr(&full));
    let text = stdout(&full);
    assert!(text.contains(&manifest));
    assert!(text.contains(".local/share/skill-manager/store"));
    assert!(text.contains(".claude/skills"));
    assert!(text.contains(".agents/skills"));
    assert!(text.contains("upstream_coordinate"));

    let json = sandbox.run(&["info", "me-codemode", "--json"]);
    assert_eq!(code(&json), 0, "{}", stderr(&json));
    let value: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(value["locations"]["manifest"], manifest);
    assert!(
        value["locations"]["generation"]
            .as_str()
            .unwrap()
            .contains("generations/")
    );
    let record = &value["skills"][0];
    assert_eq!(record["name"], "me-codemode");
    assert_eq!(record["source_kind"], "path");
    assert!(
        record["upstream_coordinate"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
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

    assert_eq!(code(&sandbox.run(&["apply"])), 0);
    let added = sandbox.run(&["add", "me-tdd"]);
    assert_eq!(code(&added), 0, "{}", stderr(&added));
    let manifest = fs::read_to_string(sandbox.manifest()).unwrap();
    assert!(manifest.contains("# mine"));
    assert!(manifest.contains(r#"skills = ["codemode", "tdd"]"#));
    assert!(sandbox.target("me-tdd").exists());
    let text = stdout(&added);
    let wrote = format!("manifest: wrote {}", sandbox.manifest().display());
    assert!(text.contains(&wrote), "{text}");
    assert!(text.contains("+ me-tdd"), "{text}");
    assert!(text.contains(".claude/skills: linked me-tdd"), "{text}");

    let removed = sandbox.run(&["remove", "me-tdd"]);
    assert_eq!(code(&removed), 0, "{}", stderr(&removed));
    let text = stdout(&removed);
    assert!(text.contains(&wrote), "{text}");
    assert!(!text.contains("lock:"), "{text}");
    assert!(text.contains("- me-tdd"), "{text}");
    assert!(text.contains(".claude/skills: removed me-tdd"), "{text}");
    assert!(!sandbox.target("me-tdd").exists());
    assert!(sandbox.target("me-codemode").exists());
}

#[test]
fn add_reports_why_a_present_skill_is_unusable() {
    let sandbox = Sandbox::new();
    sandbox.write_skill("good", "body\n");
    let local_bad = sandbox.path().join("etc/ai/skills/bad");
    fs::create_dir_all(&local_bad).unwrap();
    fs::write(local_bad.join("SKILL.md"), "no frontmatter\n").unwrap();
    let upstream = git_origin(&[("bad/SKILL.md", "no frontmatter\n")]);
    sandbox.write_manifest(&format!(
        "version = 1\n[sources.me]\npath = \"~/etc/ai/skills\"\nskills = [\"good\"]\n\
         [sources.mp]\ngit = \"{}\"\nskills = []\n",
        upstream.path().display()
    ));

    for selection in ["me-bad", "mp-bad"] {
        let output = sandbox.run(&["add", selection]);

        assert_ne!(code(&output), 0, "{selection}");
        let errors = stderr(&output);
        assert!(errors.contains("frontmatter"), "{selection}: {errors}");
        assert!(!errors.contains("was not found"), "{selection}: {errors}");
    }
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

    fs::remove_file(sandbox.target("me-codemode")).unwrap();
    let tampered = sandbox.run(&["verify"]);

    assert_eq!(code(&tampered), 1);
    assert!(stderr(&tampered).contains("me-codemode"));
}
