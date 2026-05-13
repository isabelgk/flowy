mod common;

use common::TestRepo;
use predicates::prelude::*;
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

#[test]
fn t_show_displays_ticket_content() {
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
        .args(["t", "show", "-w", "test-branch", "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("completed: My ticket"));
}

#[test]
fn t_show_json() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["t", "show", "-w", "test-branch", "1", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["number"], 1);
    assert_eq!(json["slug"], "my-ticket");
    assert_eq!(json["title"], "My ticket");
    assert_eq!(json["status"], "pending");
}

#[test]
fn t_show_unknown_id_errors() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    repo.cmd()
        .args(["t", "show", "-w", "test-branch", "99"])
        .assert()
        .failure();
}

#[test]
fn t_done_wont_implement() {
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
        .args(["t", "done", "-w", "test-branch", "1", "--wont-implement"])
        .assert()
        .success();

    let ticket = repo
        .root
        .join("notes/workstreams/test-branch/tickets/001-my-ticket.md");
    let content = fs::read_to_string(ticket).unwrap();
    assert!(content.starts_with("# wont-implement: My ticket"));
}

#[test]
fn t_done_reopen_complete_ticket() {
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

    repo.cmd()
        .args(["t", "done", "-w", "test-branch", "1", "--reopen"])
        .assert()
        .success();

    let ticket = repo
        .root
        .join("notes/workstreams/test-branch/tickets/001-my-ticket.md");
    let content = fs::read_to_string(ticket).unwrap();
    assert!(content.starts_with("# My ticket\n"));
}

#[test]
fn t_done_json() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["t", "done", "-w", "test-branch", "1", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["number"], 1);
    assert_eq!(json["status"], "complete");
}

#[test]
fn t_add_with_description() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    repo.cmd()
        .args([
            "t",
            "add",
            "-w",
            "test-branch",
            "My ticket",
            "-d",
            "Some details",
        ])
        .assert()
        .success();

    let ticket = repo
        .root
        .join("notes/workstreams/test-branch/tickets/001-my-ticket.md");
    let content = fs::read_to_string(ticket).unwrap();
    assert!(content.starts_with("# My ticket\n"));
    assert!(content.contains("Some details"));
}

#[test]
fn t_add_json() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["t", "add", "-w", "test-branch", "My ticket", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["number"], 1);
    assert_eq!(json["slug"], "my-ticket");
    assert!(!json["path"].as_str().unwrap_or("").is_empty());
}

#[test]
fn t_add_increments_ticket_number() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "First"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "Second"])
        .assert()
        .success();

    let ticket = repo
        .root
        .join("notes/workstreams/test-branch/tickets/002-second.md");
    assert!(ticket.exists());
}

#[test]
fn t_journal_appends_entry() {
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
        .args(["t", "journal", "-w", "test-branch", "1", "Started work"])
        .assert()
        .success();

    let ticket = repo
        .root
        .join("notes/workstreams/test-branch/tickets/001-my-ticket.md");
    let content = fs::read_to_string(ticket).unwrap();
    assert!(content.contains("## Journal"));
    assert!(content.contains("Started work"));
}

#[test]
fn t_journal_with_heading() {
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
        .args([
            "t",
            "journal",
            "-w",
            "test-branch",
            "1",
            "Progress",
            "--heading",
            "Day 1",
        ])
        .assert()
        .success();

    let ticket = repo
        .root
        .join("notes/workstreams/test-branch/tickets/001-my-ticket.md");
    let content = fs::read_to_string(ticket).unwrap();
    assert!(content.contains("Day 1"));
    assert!(content.contains("Progress"));
}

#[test]
fn t_search_returns_matching_tickets() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "Fix auth bug"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "Improve dashboard"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["t", "search", "-w", "test-branch", "auth"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("auth"));
}

#[test]
fn t_search_json() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "Fix auth bug"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "Improve dashboard"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["t", "search", "-w", "test-branch", "auth", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = json.as_array().unwrap();
    assert!(!arr.is_empty());
    assert_eq!(arr[0]["slug"], "fix-auth-bug");
}

#[test]
fn t_search_no_results() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "Fix auth bug"])
        .assert()
        .success();

    // In non-TTY (captured) mode, no results means empty stdout.
    repo.cmd()
        .args(["t", "search", "-w", "test-branch", "zzznomatch"])
        .assert()
        .success()
        .stdout("");
}

#[test]
fn t_status_shows_counts() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "First"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "done", "-w", "test-branch", "1"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "Second"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["t", "status", "-w", "test-branch"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // "1 pending / 2 total"
    assert!(stdout.contains("1 pending"));
    assert!(stdout.contains("2 total"));
}

#[test]
fn t_status_json() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "First"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "done", "-w", "test-branch", "1"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "test-branch", "Second"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["t", "status", "-w", "test-branch", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["pending"], 1);
    assert_eq!(json["total"], 2);
    assert_eq!(json["workstream"], "test-branch");
    assert!(!json["branch"].as_str().unwrap_or("").is_empty());
}

#[test]
fn t_edit_opens_ticket_in_editor() {
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
        .args(["t", "edit", "-w", "test-branch", "1"])
        .env("EDITOR", "true")
        .assert()
        .success();
}

#[test]
fn t_infers_workstream_from_worktree_branch() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    // No -w flag; workstream should be inferred from the worktree's checked-out branch.
    repo.cmd()
        .args(["t", "add", "My ticket"])
        .current_dir(repo.root.join("worktrees").join("test-branch"))
        .assert()
        .success();

    assert!(
        repo.root
            .join("notes/workstreams/test-branch/tickets/001-my-ticket.md")
            .exists()
    );
}

#[test]
fn t_w_flag_overrides_inferred_branch() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "target/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["wt", "new", "-b", "other/branch"])
        .assert()
        .success();

    // Run from other-branch worktree but write to a different workstream via -w.
    repo.cmd()
        .args(["t", "add", "-w", "target-branch", "My ticket"])
        .current_dir(repo.root.join("worktrees").join("other-branch"))
        .assert()
        .success();

    assert!(
        repo.root
            .join("notes/workstreams/target-branch/tickets/001-my-ticket.md")
            .exists(),
        "ticket should land in target-branch, not other-branch"
    );
    assert!(
        !repo
            .root
            .join("notes/workstreams/other-branch/tickets/001-my-ticket.md")
            .exists(),
        "ticket should not land in other-branch"
    );
}

#[test]
fn t_no_branch_errors_with_helpful_message() {
    let repo = TestRepo::new();
    // Run from a directory outside any git repo; git branch --show-current returns empty.
    let outside = tempfile::TempDir::new().unwrap();
    repo.cmd()
        .args(["t", "add", "My ticket"])
        .current_dir(outside.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot infer workstream"));
}

#[test]
fn wt_new_json() {
    let repo = TestRepo::new();

    // git worktree add writes "Preparing worktree..." to inherited stdout,
    // so we check for the JSON fragment rather than parsing the whole buffer.
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\": true"));
}

#[test]
fn wt_archive_json() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["wt", "archive", "test-branch", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
}

#[test]
fn wt_unarchive_json() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["wt", "archive", "test-branch"])
        .assert()
        .success();

    // Same as wt_new_json: git worktree add pollutes stdout with status lines.
    repo.cmd()
        .args(["wt", "unarchive", "test-branch", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\": true"));
}
