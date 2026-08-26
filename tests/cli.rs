mod common;

use common::{TestRepo, archived_ws_dir_in, git_in, git_out, ws_dir_in};
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

    let ws_dir = ws_dir_in(&repo.root, "test-branch");
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

    let ticket = ws_dir_in(&repo.root, "test-branch")
        .join("tickets")
        .join("001.md");
    assert!(ticket.exists(), "ticket file should exist");
    let content = fs::read_to_string(&ticket).unwrap();
    assert!(content.starts_with("+++\n"));
    assert!(content.contains("title = \"My ticket\""));
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

    let ticket = ws_dir_in(&repo.root, "test-branch")
        .join("tickets")
        .join("001.md");
    let content = fs::read_to_string(&ticket).unwrap();
    assert!(
        content.contains("status = \"complete\""),
        "frontmatter should have complete status"
    );
    assert!(content.contains("title = \"My ticket\""));
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

// Workstream notes live inside the working tree, so without an exclude entry
// every worktree reads as dirty and `git worktree remove` refuses. Asserted
// with core.excludesFile disabled so a developer's global ignore cannot mask a
// regression here.
#[test]
fn workstream_dir_is_excluded_from_git_status() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    let worktree = repo.root.join("worktrees").join("test-branch");
    assert!(repo.ws_dir("test-branch").join("meta.toml").exists());

    let status = git_out(
        &worktree,
        &[
            "-c",
            "core.excludesFile=",
            "status",
            "--porcelain",
            "--untracked-files=normal",
        ],
    );
    assert_eq!(status, "", "workstream dir must not show as untracked");
}

// Archiving without --force must not trip the dirty check on the workstream
// dir the worktree now contains; an unignored .local/ would make it prompt.
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
        !ws_dir_in(&repo.root, "test-branch").exists(),
        "notes should be moved out of workstreams"
    );
    assert!(
        archived_ws_dir_in(&repo.root, "test-branch").exists(),
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
        ws_dir_in(&repo.root, "test-branch").exists(),
        "notes should be restored"
    );
    assert!(
        !archived_ws_dir_in(&repo.root, "test-branch").exists(),
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
    assert!(stdout.contains("My ticket"));
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

    let ticket = repo.ticket("test-branch", 1);
    let content = fs::read_to_string(ticket).unwrap();
    assert!(content.contains("status = \"wont-implement\""));
    assert!(content.contains("title = \"My ticket\""));
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

    let ticket = repo.ticket("test-branch", 1);
    let content = fs::read_to_string(ticket).unwrap();
    assert!(content.contains("status = \"pending\""));
    assert!(content.contains("title = \"My ticket\""));
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

    let ticket = repo.ticket("test-branch", 1);
    let content = fs::read_to_string(ticket).unwrap();
    assert!(content.starts_with("+++\n"));
    assert!(content.contains("title = \"My ticket\""));
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

    let ticket = repo.ticket("test-branch", 2);
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

    let ticket = repo.ticket("test-branch", 1);
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

    let ticket = repo.ticket("test-branch", 1);
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

    assert!(repo.ticket("test-branch", 1).exists());
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
        repo.ticket("target-branch", 1).exists(),
        "ticket should land in target-branch, not other-branch"
    );
    assert!(
        !repo.ticket("other-branch", 1).exists(),
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

#[test]
fn init_creates_project_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("myproject");

    assert_cmd::Command::cargo_bin("flowy")
        .unwrap()
        .args(["init", project.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized"));

    assert!(project.join(".bare").is_dir(), ".bare should exist");
    assert!(project.join(".git").is_file(), ".git pointer should exist");
    assert!(
        project.join("worktrees").join("main").is_dir(),
        "worktrees/main should exist"
    );
    assert!(
        ws_dir_in(&project, "main").is_dir(),
        "notes/workstreams/main should exist"
    );
    assert!(
        project
            .join("notes")
            .join("workstreams")
            .join("archive")
            .is_dir(),
        "notes/workstreams/archive should exist"
    );

    let meta = read_meta(&ws_dir_in(&project, "main"));
    assert_eq!(meta.worktree.branch, "main");
    assert!(!meta.created.is_empty());
}

#[test]
fn init_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("myproject");

    assert_cmd::Command::cargo_bin("flowy")
        .unwrap()
        .args(["init", project.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\": true"));
}

#[test]
fn init_with_relative_path_creates_worktree() {
    let tmp = tempfile::tempdir().unwrap();

    assert_cmd::Command::cargo_bin("flowy")
        .unwrap()
        .current_dir(tmp.path())
        .args(["init", "myproject"])
        .assert()
        .success();

    assert!(
        tmp.path()
            .join("myproject")
            .join("worktrees")
            .join("main")
            .is_dir(),
        "worktrees/main should exist when init is called with a relative path"
    );
}

#[test]
fn init_fails_if_directory_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("existing");
    fs::create_dir_all(&project).unwrap();

    assert_cmd::Command::cargo_bin("flowy")
        .unwrap()
        .args(["init", project.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

// Bug fix: titles with special characters (tabs, newlines) that the old manual
// escaping silently corrupted now round-trip correctly through TOML frontmatter.
#[test]
fn t_add_title_with_special_chars_round_trips() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat"])
        .assert()
        .success();

    // Tab is the character the old manual escaping silently corrupted.
    let title = "first\tsecond";
    repo.cmd()
        .args(["t", "add", "-w", "feat", title])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["--json", "t", "show", "-w", "feat", "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["title"].as_str().unwrap(),
        title,
        "title should survive TOML round-trip unchanged"
    );
}

// Bug fix: running `done --summary` twice must not append a second ## Summary.
#[test]
fn t_done_summary_not_duplicated_on_second_call() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "feat", "A ticket"])
        .assert()
        .success();

    repo.cmd()
        .args(["t", "done", "-w", "feat", "1", "--summary", "First"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "done", "-w", "feat", "1", "--reopen"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "done", "-w", "feat", "1", "--summary", "Second"])
        .assert()
        .success();

    let ticket_path = ws_dir_in(&repo.root, "feat").join("tickets").join("001.md");
    let content = fs::read_to_string(&ticket_path).unwrap();
    assert_eq!(
        content.matches("## Summary").count(),
        1,
        "exactly one ## Summary section should exist"
    );
}

// Bug fix: if `git worktree add` fails during unarchive the notes must be
// rolled back to the archive directory, not left in the active workstreams dir.
#[test]
fn wt_unarchive_rolls_back_notes_on_git_failure() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat"])
        .assert()
        .success();
    repo.cmd()
        .args(["wt", "archive", "feat"])
        .assert()
        .success();

    // Delete the branch from the bare repo so git worktree add will fail.
    std::process::Command::new("git")
        .args(["branch", "-D", "feat"])
        .current_dir(repo.root.join(".bare"))
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("git branch -D");

    repo.cmd()
        .args(["wt", "unarchive", "feat"])
        .assert()
        .failure();

    // Notes must be rolled back to archive, not stranded in workstreams.
    let archive_dir = archived_ws_dir_in(&repo.root, "feat");
    assert!(
        archive_dir.is_dir(),
        "notes should be rolled back to archive after failed unarchive"
    );
    let ws_dir = ws_dir_in(&repo.root, "feat");
    assert!(
        !ws_dir.exists(),
        "notes must not remain in workstreams after failed unarchive"
    );
}

// Bug fix: `clone` must scaffold the default branch worktree and notes dir,
// matching what `init` does for the initial main worktree.
#[test]
fn clone_scaffolds_default_branch_workstream() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");

    let git_env = [
        ("GIT_AUTHOR_NAME", "Test"),
        ("GIT_AUTHOR_EMAIL", "test@example.com"),
        ("GIT_COMMITTER_NAME", "Test"),
        ("GIT_COMMITTER_EMAIL", "test@example.com"),
    ];
    std::process::Command::new("git")
        .args(["init", "-b", "main", src.to_str().unwrap()])
        .envs(git_env.iter().cloned())
        .status()
        .expect("git init");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&src)
        .envs(git_env.iter().cloned())
        .status()
        .expect("git commit");

    assert_cmd::Command::cargo_bin("flowy")
        .unwrap()
        .current_dir(tmp.path())
        .args(["clone", src.to_str().unwrap(), "project"])
        .envs(git_env.iter().cloned())
        .assert()
        .success();

    let project = tmp.path().join("project");
    assert!(project.join(".bare").is_dir(), ".bare should exist");
    assert!(
        project.join("worktrees").join("main").is_dir(),
        "worktrees/main should exist after clone"
    );
    let ws_main = ws_dir_in(&project, "main");
    assert!(
        ws_main.is_dir(),
        "notes/workstreams/main should exist after clone"
    );
    let meta = read_meta(&ws_main);
    assert_eq!(meta.worktree.branch, "main");
}

#[test]
fn clone_scaffolds_non_main_default_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");

    let git_env = [
        ("GIT_AUTHOR_NAME", "Test"),
        ("GIT_AUTHOR_EMAIL", "test@example.com"),
        ("GIT_COMMITTER_NAME", "Test"),
        ("GIT_COMMITTER_EMAIL", "test@example.com"),
    ];
    std::process::Command::new("git")
        .args(["init", "-b", "trunk", src.to_str().unwrap()])
        .envs(git_env.iter().cloned())
        .status()
        .expect("git init");
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(&src)
        .envs(git_env.iter().cloned())
        .status()
        .expect("git commit");

    assert_cmd::Command::cargo_bin("flowy")
        .unwrap()
        .current_dir(tmp.path())
        .args(["clone", src.to_str().unwrap(), "project"])
        .envs(git_env.iter().cloned())
        .assert()
        .success();

    let project = tmp.path().join("project");
    assert!(
        project.join("worktrees").join("trunk").is_dir(),
        "worktrees/trunk should exist after clone"
    );
    let ws = ws_dir_in(&project, "trunk");
    assert!(
        ws.is_dir(),
        "notes/workstreams/trunk should exist after clone"
    );
    let meta = read_meta(&ws);
    assert_eq!(meta.worktree.branch, "trunk");
}

#[test]
fn wt_rename_moves_branch_dir_and_notes() {
    let repo = TestRepo::new();

    repo.cmd()
        .args(["wt", "new", "-b", "feat/old"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "Carry me", "-w", "feat-old"])
        .assert()
        .success();

    repo.cmd()
        .args(["wt", "rename", "feat-old", "feat/new"])
        .assert()
        .success();

    assert!(!repo.root.join("worktrees").join("feat-old").exists());
    assert!(!repo.ws_dir("feat-old").exists());

    let new_wt = repo.root.join("worktrees").join("feat-new");
    assert!(new_wt.is_dir(), "renamed worktree dir should exist");
    let new_ws = repo.ws_dir("feat-new");
    assert_eq!(read_meta(&new_ws).worktree.branch, "feat/new");

    // Ticket carried over inside the worktree.
    let tickets: Vec<_> = fs::read_dir(new_ws.join("tickets")).unwrap().collect();
    assert_eq!(tickets.len(), 1, "ticket should move with the workstream");

    // git sees the new branch at the new path, and the old branch is gone.
    let list = git_out(&repo.root, &["worktree", "list", "--porcelain"]);
    assert!(list.contains(new_wt.to_str().unwrap()));
    assert!(!list.contains("feat-old"));
}

#[test]
fn wt_rename_branch_only_keeps_dirs() {
    let repo = TestRepo::new();

    // safe_name("keep-me") == safe_name("keep/me"), so only the branch changes.
    repo.cmd()
        .args(["wt", "new", "-b", "keep-me"])
        .assert()
        .success();

    repo.cmd()
        .args(["wt", "rename", "keep-me", "keep/me"])
        .assert()
        .success();

    let ws = ws_dir_in(&repo.root, "keep-me");
    assert!(repo.root.join("worktrees").join("keep-me").is_dir());
    assert_eq!(read_meta(&ws).worktree.branch, "keep/me");

    let list = git_out(&repo.root, &["worktree", "list", "--porcelain"]);
    assert!(list.contains("branch refs/heads/keep/me"));
}

#[test]
fn wt_rename_missing_workstream_errors() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "rename", "nope", "feat/x"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no workstream named 'nope'"));
}

#[test]
fn wt_rename_target_exists_errors() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat/a"])
        .assert()
        .success();
    repo.cmd()
        .args(["wt", "new", "-b", "feat/b"])
        .assert()
        .success();

    repo.cmd()
        .args(["wt", "rename", "feat-a", "feat/b"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn wt_rename_json() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat/old"])
        .assert()
        .success();

    repo.cmd()
        .args(["--json", "wt", "rename", "feat-old", "feat/new"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"branch\": \"feat/new\""));
}

// The motivating case: `git worktree move` refuses worktrees containing
// submodules, so rename must fall back to a manual move + admin repair.
#[test]
fn wt_rename_moves_worktree_with_submodule() {
    let repo = TestRepo::new();

    repo.cmd()
        .args(["wt", "new", "-b", "feat/sub"])
        .assert()
        .success();
    let old_wt = repo.root.join("worktrees").join("feat-sub");

    // A standalone repo to serve as the submodule's origin, outside the tree.
    let outside = repo.root.parent().unwrap().to_path_buf();
    let sub_src = outside.join("subsrc");
    git_in(&outside, &["init", "-b", "main", sub_src.to_str().unwrap()]);
    git_in(&sub_src, &["commit", "--allow-empty", "-m", "sub init"]);

    // file:// submodules are blocked by default since git 2.38; allow them here.
    git_in(
        &old_wt,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            sub_src.to_str().unwrap(),
            "sub",
        ],
    );
    git_in(&old_wt, &["commit", "-m", "add submodule"]);

    // Propagate the same allowance to the tool's own `submodule update`.
    repo.cmd()
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "protocol.file.allow")
        .env("GIT_CONFIG_VALUE_0", "always")
        .args(["wt", "rename", "feat-sub", "feat/renamed"])
        .assert()
        .success();

    let new_wt = repo.root.join("worktrees").join("feat-renamed");
    assert!(!old_wt.exists(), "old worktree dir should be gone");
    assert!(
        new_wt.join("sub").join(".git").exists(),
        "submodule should be restored at the new path"
    );

    // git tracks the worktree at its new path only.
    let list = git_out(&repo.root, &["worktree", "list", "--porcelain"]);
    assert!(list.contains(new_wt.to_str().unwrap()));
    assert!(!list.contains("feat-sub"));

    // Submodule pointers are healthy: a leading space means in-sync (no +/-/U).
    let status = git_out(&new_wt, &["submodule", "status", "--recursive"]);
    assert!(!status.trim().is_empty(), "expected a submodule entry");
    for line in status.lines() {
        assert!(line.starts_with(' '), "submodule out of sync: {line:?}");
    }

    assert_eq!(
        read_meta(&ws_dir_in(&repo.root, "feat-renamed"))
            .worktree
            .branch,
        "feat/renamed"
    );
}

#[test]
fn wt_rename_refuses_from_inside_worktree() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat/x"])
        .assert()
        .success();

    let wt = repo.root.join("worktrees").join("feat-x");
    repo.cmd()
        .current_dir(&wt)
        .args(["wt", "rename", "feat-x", "feat/y"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("outside the worktree"));
}

// Hand-build the pre-move layout: notes at <root>/notes/workstreams/<name>/
// beside an existing worktree.
fn seed_old_layout(root: &std::path::Path, name: &str, with_worktree: bool) -> std::path::PathBuf {
    let old = root.join("notes").join("workstreams").join(name);
    fs::create_dir_all(old.join("tickets")).unwrap();
    fs::write(
        old.join("tickets").join("001.md"),
        "+++\ntitle = \"legacy\"\nstatus = \"pending\"\n+++\nbody\n",
    )
    .unwrap();
    fs::write(
        old.join("meta.toml"),
        format!("created = \"x\"\n[worktree]\npath = \"p\"\nbranch = \"{name}\"\n"),
    )
    .unwrap();
    if with_worktree {
        git_in(
            &root.join(".bare"),
            &[
                "worktree",
                "add",
                root.join("worktrees").join(name).to_str().unwrap(),
                "-b",
                name,
            ],
        );
    }
    old
}

#[test]
fn migrate_moves_notes_into_worktree() {
    let repo = TestRepo::new();
    let old = seed_old_layout(&repo.root, "legacy", true);

    repo.cmd().arg("migrate").assert().success();

    assert!(!old.exists(), "old notes dir should be gone");
    assert!(
        repo.ticket("legacy", 1).exists(),
        "ticket should land inside the worktree"
    );
    assert_eq!(
        read_meta(&repo.ws_dir("legacy")).worktree.branch,
        "legacy",
        "meta should move intact"
    );

    // The migrated workstream is now visible to the normal commands.
    let output = repo
        .cmd()
        .args(["t", "list", "-w", "legacy", "--json"])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 1);
}

#[test]
fn migrate_dry_run_moves_nothing() {
    let repo = TestRepo::new();
    let old = seed_old_layout(&repo.root, "legacy", true);

    repo.cmd()
        .args(["migrate", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("would move legacy"));

    assert!(old.is_dir(), "dry run must leave the old dir in place");
    assert!(!repo.ws_dir("legacy").exists());
}

#[test]
fn migrate_reports_notes_without_a_worktree() {
    let repo = TestRepo::new();
    let old = seed_old_layout(&repo.root, "orphan", false);

    repo.cmd()
        .arg("migrate")
        .assert()
        .success()
        .stdout(predicate::str::contains("orphaned orphan"));

    assert!(old.is_dir(), "orphaned notes must be left alone");
}

#[test]
fn migrate_leaves_archive_alone_and_is_idempotent() {
    let repo = TestRepo::new();
    seed_old_layout(&repo.root, "legacy", true);
    let archived = archived_ws_dir_in(&repo.root, "old-thing");
    fs::create_dir_all(&archived).unwrap();

    repo.cmd().arg("migrate").assert().success();
    assert!(archived.is_dir(), "archive entries must not be migrated");

    // Second run has nothing left to do and must not fail.
    repo.cmd()
        .arg("migrate")
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to migrate"));
    assert!(repo.ticket("legacy", 1).exists());
}

#[test]
fn migrate_adds_the_exclude_entry() {
    let repo = TestRepo::new();
    let exclude = repo.root.join(".bare").join("info").join("exclude");
    fs::write(&exclude, "# pre-existing\n*.tmp\n").unwrap();

    repo.cmd().arg("migrate").assert().success();

    let content = fs::read_to_string(&exclude).unwrap();
    assert!(content.contains(".local/"), "exclude entry should be added");
    assert!(
        content.contains("*.tmp"),
        "pre-existing exclude rules must survive"
    );
}

// Reporting "Archived <name>" while moving nothing hid both typos and
// old-layout notes that migrate could not place.
#[test]
fn wt_archive_unknown_workstream_errors() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "archive", "nope"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no workstream named 'nope'"));
}

#[test]
fn wt_archive_old_layout_without_worktree_errors() {
    let repo = TestRepo::new();
    let old = seed_old_layout(&repo.root, "lonely", false);

    repo.cmd()
        .args(["wt", "archive", "lonely"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("old notes layout"));

    assert!(old.is_dir(), "notes must not be silently left half-handled");
}

// A branch renamed with plain `git branch -m` leaves the worktree directory
// keeping its original name. Everything below asserts flowy follows the
// directory, which is what actually holds the notes.
fn repo_with_renamed_branch() -> TestRepo {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat/foo"])
        .assert()
        .success();
    repo.cmd()
        .args(["t", "add", "-w", "feat-foo", "First"])
        .assert()
        .success();
    git_in(
        &repo.root.join("worktrees").join("feat-foo"),
        &["branch", "-m", "feat/bar"],
    );
    repo
}

#[test]
fn t_list_follows_the_worktree_dir_when_the_branch_is_renamed() {
    let repo = repo_with_renamed_branch();

    let output = repo
        .cmd()
        .args(["t", "list", "--json"])
        .current_dir(repo.root.join("worktrees").join("feat-foo"))
        .output()
        .unwrap();
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = json.as_array().expect("JSON array");
    assert_eq!(arr.len(), 1, "the workstream's own ticket must be listed");
    assert_eq!(arr[0]["title"], "First");
}

#[test]
fn t_status_counts_tickets_when_the_branch_is_renamed() {
    let repo = repo_with_renamed_branch();

    let output = repo
        .cmd()
        .args(["t", "status", "--json"])
        .current_dir(repo.root.join("worktrees").join("feat-foo"))
        .output()
        .unwrap();
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["workstream"], "feat-foo");
    assert_eq!(json["pending"], 1);
    assert_eq!(json["total"], 1);
}

#[test]
fn t_add_writes_into_the_worktree_when_the_branch_is_renamed() {
    let repo = repo_with_renamed_branch();

    repo.cmd()
        .args(["t", "add", "Second"])
        .current_dir(repo.root.join("worktrees").join("feat-foo"))
        .assert()
        .success();

    assert!(repo.ticket("feat-foo", 2).exists());
    // A branch-derived path would scaffold a directory that is not a worktree,
    // stranding tickets where nothing reads them.
    assert!(
        !repo.root.join("worktrees").join("feat-bar").exists(),
        "must not create a phantom worktree dir named after the branch"
    );
}

#[test]
fn wt_ls_json_names_the_worktree_dir_not_the_branch() {
    let repo = repo_with_renamed_branch();

    let output = repo.cmd().args(["wt", "ls", "--json"]).output().unwrap();
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = json.as_array().expect("JSON array");
    let entry = arr
        .iter()
        .find(|e| e["name"] == "feat-foo")
        .expect("entry keyed by the worktree dir name");
    assert_eq!(entry["branch"], "feat/bar");
    assert_eq!(entry["tickets_pending"], 1);
    // wt archive / wt rename key on this name, so it must name a real directory.
    assert!(
        repo.root
            .join("worktrees")
            .join(entry["name"].as_str().unwrap())
            .is_dir()
    );
}

fn new_workstream(repo: &TestRepo, branch: &str) {
    repo.cmd()
        .args(["wt", "new", "-b", branch])
        .assert()
        .success();
}

// The allocator claims a number with create_new, so simultaneous adds must land
// on distinct files rather than the last writer overwriting the rest.
#[test]
fn t_add_concurrent_adds_get_distinct_numbers() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    const N: usize = 8;
    let children: Vec<_> = (0..N)
        .map(|i| {
            std::process::Command::new(env!("CARGO_BIN_EXE_flowy"))
                .arg("--repo-root")
                .arg(&repo.root)
                .args(["t", "add", "-w", "test-branch", &format!("Ticket {i}")])
                .stdout(std::process::Stdio::null())
                .spawn()
                .expect("spawn flowy")
        })
        .collect();
    for mut child in children {
        assert!(child.wait().expect("wait").success());
    }

    let tickets_dir = repo.ws_dir("test-branch").join("tickets");
    let mut numbers: Vec<String> = fs::read_dir(&tickets_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    numbers.sort();
    assert_eq!(
        numbers.len(),
        N,
        "expected {N} distinct tickets, got {numbers:?}"
    );

    let mut titles: Vec<String> = numbers
        .iter()
        .map(|n| {
            let content = fs::read_to_string(tickets_dir.join(n)).unwrap();
            content
                .lines()
                .find(|l| l.starts_with("title = "))
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    titles.sort();
    titles.dedup();
    assert_eq!(titles.len(), N, "every add should keep its own title");
}
