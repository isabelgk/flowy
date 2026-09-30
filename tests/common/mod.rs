// Shared by several test binaries; each uses a different subset.
#![allow(dead_code)]

use assert_cmd::Command;
use std::path::{Path, PathBuf};
use std::process;
use tempfile::TempDir;

/// Hermetic git environment: a fixed identity, and none of the developer's
/// global or system config (commit.gpgsign, core.hooksPath) or default global
/// ignore file, which git finds through HOME and XDG_CONFIG_HOME.
pub const GIT_ENV: [(&str, &str); 8] = [
    ("GIT_AUTHOR_NAME", "Test"),
    ("GIT_AUTHOR_EMAIL", "test@example.com"),
    ("GIT_COMMITTER_NAME", "Test"),
    ("GIT_COMMITTER_EMAIL", "test@example.com"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("HOME", "/nonexistent"),
    ("XDG_CONFIG_HOME", "/nonexistent"),
];

/// The flowy binary with the hermetic git environment.
pub fn flowy() -> Command {
    let mut c = Command::cargo_bin("flowy").expect("flowy binary");
    c.envs(GIT_ENV);
    c
}

/// Run git in `dir` with the hermetic identity, asserting success.
pub fn git_in(dir: &Path, args: &[&str]) {
    git_run(args, dir, &GIT_ENV);
}

/// Run git in `dir`, asserting success, and return its stdout.
pub fn git_out(dir: &Path, args: &[&str]) -> String {
    let mut cmd = process::Command::new("git");
    cmd.args(args).current_dir(dir);
    for (k, v) in GIT_ENV {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("git command");
    assert!(
        out.status.success(),
        "git {args:?} failed in {}",
        dir.display()
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Live workstream dir under an arbitrary project root, for tests that scaffold
/// a project themselves (clone, init) rather than using TestRepo.
pub fn ws_dir_in(root: &Path, name: &str) -> PathBuf {
    root.join("worktrees")
        .join(name)
        .join(".local")
        .join("workstream")
}

pub fn archived_ws_dir_in(root: &Path, name: &str) -> PathBuf {
    root.join("notes")
        .join("workstreams")
        .join("archive")
        .join(name)
}

pub struct TestRepo {
    // Keep TempDir alive for the duration of the test.
    _dir: TempDir,
    pub root: PathBuf,
}

impl TestRepo {
    pub fn new() -> Self {
        let dir = TempDir::new().expect("create tempdir");
        let src = dir.path().join("src");
        let root = dir.path().join("repo");

        let git_env = GIT_ENV;

        // Init source repo with an empty commit so it has a HEAD to clone.
        git_run(
            &["init", "-b", "main", src.to_str().unwrap()],
            dir.path(),
            &git_env,
        );
        git_run(&["commit", "--allow-empty", "-m", "init"], &src, &git_env);

        // Clone bare.
        let bare_dir = root.join(".bare");
        git_run(
            &[
                "clone",
                "--bare",
                src.to_str().unwrap(),
                bare_dir.to_str().unwrap(),
            ],
            dir.path(),
            &git_env,
        );

        // .git pointer so git commands work from repo root.
        std::fs::write(root.join(".git"), "gitdir: ./.bare\n").unwrap();

        // Scaffold directories.
        std::fs::create_dir_all(root.join("worktrees")).unwrap();
        std::fs::create_dir_all(root.join("notes").join("workstreams").join("archive")).unwrap();

        TestRepo { _dir: dir, root }
    }

    /// Live workstream dir for `name`. Routed through a helper so a future
    /// layout change is one edit rather than one per assertion.
    pub fn ws_dir(&self, name: &str) -> PathBuf {
        ws_dir_in(&self.root, name)
    }

    pub fn ticket(&self, name: &str, number: u32) -> PathBuf {
        self.ws_dir(name)
            .join("tickets")
            .join(format!("{number:03}.md"))
    }

    pub fn cmd(&self) -> Command {
        let mut c = flowy();
        c.arg("--repo-root").arg(&self.root);
        c
    }
}

fn git_run(args: &[&str], cwd: &Path, env: &[(&str, &str)]) {
    let mut cmd = process::Command::new("git");
    cmd.args(args).current_dir(cwd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let status = cmd.status().expect("git command");
    assert!(status.success(), "git {args:?} failed in {}", cwd.display());
}
