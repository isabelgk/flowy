mod common;

use common::TestRepo;
use std::fs;

#[derive(serde::Deserialize)]
struct Meta {
    created: String,
    worktree: WorktreeMeta,
}

#[derive(serde::Deserialize)]
struct WorktreeMeta {
    branch: String,
}

fn read_meta(dir: &std::path::Path) -> Meta {
    let content = fs::read_to_string(dir.join("meta.toml")).expect("meta.toml readable");
    toml::from_str(&content).expect("meta.toml parseable")
}

#[test]
fn wt_new_creates_worktree_and_meta() {
    let repo = TestRepo::new();

    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    let worktree = repo.root.join("worktrees").join("test-branch");
    assert!(worktree.is_dir(), "worktree directory should exist");

    let ws_dir = repo
        .root
        .join("notes")
        .join("workstreams")
        .join("test-branch");
    let meta = read_meta(&ws_dir);
    assert_eq!(meta.worktree.branch, "test/branch");
    assert!(!meta.created.is_empty());
}

#[test]
fn t_add_creates_ticket_with_h1() {
    let repo = TestRepo::new();

    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    let ticket = repo
        .root
        .join("notes")
        .join("workstreams")
        .join("test-branch")
        .join("tickets")
        .join("001-my-ticket.md");
    assert!(ticket.exists(), "ticket file should exist");
    let content = fs::read_to_string(&ticket).unwrap();
    assert!(content.starts_with("# My ticket\n"));
}

#[test]
fn t_done_rewrites_heading_and_appends_summary() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    repo.cmd()
        .args(["t", "done", "-w", "test-branch", "1", "--summary", "Done"])
        .assert()
        .success();

    let ticket = repo
        .root
        .join("notes")
        .join("workstreams")
        .join("test-branch")
        .join("tickets")
        .join("001-my-ticket.md");
    let content = fs::read_to_string(&ticket).unwrap();
    assert!(
        content.starts_with("# completed: My ticket\n"),
        "heading should have completed: prefix"
    );
    assert!(
        content.contains("## Summary"),
        "summary section should exist"
    );
}

#[test]
fn t_list_json_correct_fields() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "done", "-w", "test-branch", "1"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args([
            "t",
            "list",
            "-w",
            "test-branch",
            "--json",
            "--status",
            "all",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let arr = json.as_array().expect("JSON array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["number"], 1);
    assert_eq!(arr[0]["slug"], "my-ticket");
    assert_eq!(arr[0]["status"], "complete");
}

#[test]
fn wt_ls_json_correct_ticket_counts() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "done", "-w", "test-branch", "1"])
        .assert()
        .success();

    let output = repo.cmd().args(["wt", "ls", "--json"]).output().unwrap();
    assert!(output.status.success());

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid JSON output");
    let arr = json.as_array().expect("JSON array");
    let entry = arr
        .iter()
        .find(|e| e["name"] == "test-branch")
        .expect("entry for test-branch");
    assert_eq!(entry["tickets_pending"], 0);
    assert_eq!(entry["tickets_total"], 1);
}

#[test]
fn wt_archive_removes_worktree_and_moves_notes() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    repo.cmd()
        .args(["wt", "archive", "test-branch"])
        .assert()
        .success();

    assert!(
        !repo.root.join("worktrees").join("test-branch").exists(),
        "worktree should be removed"
    );
    assert!(
        !repo
            .root
            .join("notes")
            .join("workstreams")
            .join("test-branch")
            .exists(),
        "notes should be moved out of workstreams"
    );
    assert!(
        repo.root
            .join("notes")
            .join("workstreams")
            .join("archive")
            .join("test-branch")
            .exists(),
        "notes should exist in archive"
    );
}

#[test]
fn wt_unarchive_restores_worktree_and_notes() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["wt", "archive", "test-branch"])
        .assert()
        .success();

    repo.cmd()
        .args(["wt", "unarchive", "test-branch"])
        .assert()
        .success();

    assert!(
        repo.root.join("worktrees").join("test-branch").is_dir(),
        "worktree should be restored"
    );
    assert!(
        repo.root
            .join("notes")
            .join("workstreams")
            .join("test-branch")
            .exists(),
        "notes should be restored"
    );
    assert!(
        !repo
            .root
            .join("notes")
            .join("workstreams")
            .join("archive")
            .join("test-branch")
            .exists(),
        "archive entry should be removed"
    );
}

#[test]
fn t_done_unknown_id_errors() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    repo.cmd()
        .args(["t", "done", "-w", "test-branch", "99"])
        .assert()
        .failure();
}

#[test]
fn wt_archive_force_flag_bypasses_dirty_check() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    // Create an untracked file to make the worktree dirty.
    let worktree = repo.root.join("worktrees").join("test-branch");
    fs::write(worktree.join("untracked.txt"), "dirty").unwrap();

    repo.cmd()
        .args(["wt", "archive", "--force", "test-branch"])
        .assert()
        .success();

    assert!(!worktree.exists(), "worktree should be removed");
}
