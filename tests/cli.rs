mod common;

use assert_cmd::Command;
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
    path: String,
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

    let worktree = repo.root.join("wt").join("test-branch");
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
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    let ticket = ws_dir_in(&repo.root, "test-branch")
        .join("tickets")
        .join("1.md");
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
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    repo.cmd()
        .args([
            "ticket",
            "done",
            "-w",
            "test-branch",
            "1",
            "--summary",
            "Done",
        ])
        .assert()
        .success();

    let ticket = ws_dir_in(&repo.root, "test-branch")
        .join("tickets")
        .join("1.md");
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
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "done", "-w", "test-branch", "1"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args([
            "ticket",
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
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "done", "-w", "test-branch", "1"])
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

    let worktree = repo.root.join("wt").join("test-branch");
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
        !repo.root.join("wt").join("test-branch").exists(),
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
fn wt_archive_removes_a_worktree_with_a_submodule() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat/sub"])
        .assert()
        .success();
    let worktree = repo.root.join("wt").join("feat-sub");

    let outside = repo.root.parent().unwrap().to_path_buf();
    let sub_src = outside.join("subsrc");
    git_in(&outside, &["init", "-b", "main", sub_src.to_str().unwrap()]);
    git_in(&sub_src, &["commit", "--allow-empty", "-m", "sub init"]);
    git_in(
        &worktree,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            sub_src.to_str().unwrap(),
            "sub",
        ],
    );
    git_in(&worktree, &["commit", "-m", "add submodule"]);

    repo.cmd()
        .args(["wt", "archive", "feat-sub"])
        .assert()
        .success();

    assert!(!worktree.exists(), "worktree should be removed");
    assert!(archived_ws_dir_in(&repo.root, "feat-sub").exists());
}

#[test]
fn wt_archive_restores_notes_when_removal_fails() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat/locked"])
        .assert()
        .success();
    let worktree = repo.root.join("wt").join("feat-locked");
    git_in(&worktree, &["worktree", "lock", worktree.to_str().unwrap()]);
    repo.cmd()
        .args(["ticket", "add", "-w", "feat-locked", "Survives"])
        .assert()
        .success();

    repo.cmd()
        .args(["wt", "archive", "feat-locked"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("left unarchived"));

    assert!(worktree.exists());
    assert!(!archived_ws_dir_in(&repo.root, "feat-locked").exists());
    repo.cmd()
        .args(["ticket", "list", "-w", "feat-locked"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Survives"));
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
        repo.root.join("wt").join("test-branch").is_dir(),
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
fn a_broken_ticket_does_not_hide_the_rest() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat"])
        .assert()
        .success();
    let tickets = repo.ws_dir("feat").join("tickets");
    fs::write(tickets.join("001.md"), "+++\ntitle = \"Readable\"\n+++\n").unwrap();
    fs::write(tickets.join("002.md"), "+++\ntitle = \"Unclosed\"\n").unwrap();
    fs::write(
        tickets.join("003.md"),
        "+++\r\ntitle = \"Windows\"\r\n+++\r\n",
    )
    .unwrap();
    fs::write(tickets.join("README.md"), "# Not a ticket\n").unwrap();

    repo.cmd()
        .args(["ticket", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Readable"))
        .stdout(predicate::str::contains("Windows"))
        .stderr(predicate::str::contains("002.md"))
        .stderr(predicate::str::contains("README").not());

    repo.cmd()
        .args(["ticket", "status", "-w", "feat"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2 pending / 2 total"));

    repo.cmd()
        .args(["ticket", "list", "-w", "feat"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("002.md"));
}

#[test]
fn wt_ls_counts_follow_deleted_and_back_dated_tickets() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat"])
        .assert()
        .success();
    for title in ["One", "Two"] {
        repo.cmd()
            .args(["ticket", "add", "-w", "feat", title])
            .assert()
            .success();
    }
    let total = || {
        let out = repo.cmd().args(["--json", "wt", "ls"]).output().unwrap();
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        json[0]["tickets_total"].as_u64().unwrap()
    };
    assert_eq!(total(), 2);

    // 001 is not the newest ticket, so the newest mtime does not move.
    fs::remove_file(repo.ticket("feat", 1)).unwrap();
    assert_eq!(total(), 1);

    let copied = repo.ticket("feat", 7);
    fs::write(&copied, "+++\ntitle = \"Copied\"\n+++\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&copied)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000))
        .unwrap();
    assert_eq!(total(), 2);
}

#[test]
fn w_names_an_archived_workstream() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "add", "-w", "feat", "Kept"])
        .assert()
        .success();
    repo.cmd()
        .args(["notes", "write", "-w", "feat", "planning"])
        .write_stdin("the plan\n")
        .assert()
        .success();
    repo.cmd()
        .args(["wt", "archive", "feat"])
        .assert()
        .success();

    repo.cmd()
        .args(["notes", "show", "-w", "feat", "planning"])
        .assert()
        .success()
        .stdout("the plan\n");
    repo.cmd()
        .args(["ticket", "list", "-w", "feat"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Kept"));
    repo.cmd()
        .args(["notes", "write", "-w", "feat", "next"])
        .write_stdin("later\n")
        .assert()
        .success();
    assert!(
        archived_ws_dir_in(&repo.root, "feat")
            .join("next.md")
            .exists()
    );

    // No stray worktree dir, so unarchive still has somewhere to go.
    assert!(!repo.root.join("wt").join("feat").exists());
    repo.cmd()
        .args(["wt", "unarchive", "feat"])
        .assert()
        .success();
}

#[test]
fn w_with_an_unknown_name_writes_nothing() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["notes", "write", "-w", "typo", "planning"])
        .write_stdin("lost\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("no workstream named 'typo'"));
    assert!(!repo.root.join("wt").join("typo").exists());
}

#[test]
fn tickets_are_addressed_by_their_actual_file() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat"])
        .assert()
        .success();
    let tickets = repo.ws_dir("feat").join("tickets");
    let seven = tickets.join("7.md");
    fs::write(
        &seven,
        "+++\nstatus = \"pending\"\ntitle = \"Seven\"\n+++\n",
    )
    .unwrap();

    repo.cmd()
        .args(["ticket", "done", "-w", "feat", "7"])
        .assert()
        .success();
    assert!(
        fs::read_to_string(&seven)
            .unwrap()
            .contains("status = \"complete\"")
    );
    assert!(!tickets.join("007.md").exists());

    // The editor is handed the file that exists, not a fresh zero-padded one.
    let scratch = tempfile::TempDir::new().unwrap();
    let opened = scratch.path().join("opened");
    let editor = scratch.path().join("editor.sh");
    fs::write(
        &editor,
        format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", opened.display()),
    )
    .unwrap();
    let mut perms = fs::metadata(&editor).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    fs::set_permissions(&editor, perms).unwrap();
    repo.cmd()
        .env("EDITOR", &editor)
        .args(["ticket", "edit", "-w", "feat", "7"])
        .assert()
        .success();
    assert_eq!(
        fs::read_to_string(&opened).unwrap(),
        seven.to_str().unwrap()
    );
    assert!(!tickets.join("007.md").exists());

    fs::write(tickets.join("007.md"), "+++\ntitle = \"Twin\"\n+++\n").unwrap();
    repo.cmd()
        .args(["ticket", "show", "-w", "feat", "7"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("ambiguous"));
}

#[test]
fn t_done_unknown_id_errors() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    repo.cmd()
        .args(["ticket", "done", "-w", "test-branch", "99"])
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
    let worktree = repo.root.join("wt").join("test-branch");
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
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "done", "-w", "test-branch", "1"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["ticket", "show", "-w", "test-branch", "1"])
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
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["ticket", "show", "-w", "test-branch", "1", "--json"])
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
        .args(["ticket", "show", "-w", "test-branch", "99"])
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
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    repo.cmd()
        .args([
            "ticket",
            "done",
            "-w",
            "test-branch",
            "1",
            "--wont-implement",
        ])
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
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "done", "-w", "test-branch", "1"])
        .assert()
        .success();

    repo.cmd()
        .args(["ticket", "done", "-w", "test-branch", "1", "--reopen"])
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
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["ticket", "done", "-w", "test-branch", "1", "--json"])
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
            "ticket",
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
        .args(["ticket", "add", "-w", "test-branch", "My ticket", "--json"])
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
        .args(["ticket", "add", "-w", "test-branch", "First"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "Second"])
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
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    repo.cmd()
        .args([
            "ticket",
            "journal",
            "-w",
            "test-branch",
            "1",
            "Started work",
        ])
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
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    repo.cmd()
        .args([
            "ticket",
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
        .args(["ticket", "add", "-w", "test-branch", "Fix auth bug"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "Improve dashboard"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["ticket", "search", "-w", "test-branch", "auth"])
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
        .args(["ticket", "add", "-w", "test-branch", "Fix auth bug"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "Improve dashboard"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["ticket", "search", "-w", "test-branch", "auth", "--json"])
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
        .args(["ticket", "add", "-w", "test-branch", "Fix auth bug"])
        .assert()
        .success();

    // In non-TTY (captured) mode, no results means empty stdout.
    repo.cmd()
        .args(["ticket", "search", "-w", "test-branch", "zzznomatch"])
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
        .args(["ticket", "add", "-w", "test-branch", "First"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "done", "-w", "test-branch", "1"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "Second"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["ticket", "status", "-w", "test-branch"])
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
        .args(["ticket", "add", "-w", "test-branch", "First"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "done", "-w", "test-branch", "1"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "Second"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["ticket", "status", "-w", "test-branch", "--json"])
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
fn t_infers_workstream_from_worktree_cwd() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    // No -w flag; the worktree directory cwd sits in names the workstream.
    repo.cmd()
        .args(["ticket", "add", "My ticket"])
        .current_dir(repo.root.join("wt").join("test-branch"))
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
        .args(["ticket", "add", "-w", "target-branch", "My ticket"])
        .current_dir(repo.root.join("wt").join("other-branch"))
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
    git_in(
        &repo.root,
        &["symbolic-ref", "HEAD", "refs/heads/no-such-worktree"],
    );
    // Run from a directory outside any git repo, which must not matter. The
    // ceiling keeps that true when TMPDIR itself sits inside a repo.
    let outside = tempfile::TempDir::new().unwrap();
    repo.cmd()
        .args(["ticket", "add", "My ticket"])
        .current_dir(outside.path())
        .env("GIT_CEILING_DIRECTORIES", outside.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "no workstream named 'no-such-worktree'",
        ));
}

#[test]
fn branch_fallback_reads_the_repo_root_not_the_cwd() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "main"])
        .assert()
        .success();

    let elsewhere = tempfile::TempDir::new().unwrap();
    git_in(elsewhere.path(), &["init", "-b", "unrelated"]);
    repo.cmd()
        .args(["notes", "path"])
        .current_dir(elsewhere.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("wt/main/.local/workstream"));
}

#[test]
fn branch_fallback_reports_a_failing_git() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().join("not-a-repo");
    fs::create_dir_all(root.join("wt")).unwrap();
    common::flowy()
        .arg("--repo-root")
        .arg(&root)
        .env("GIT_CEILING_DIRECTORIES", dir.path())
        .args(["notes", "path"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("git branch --show-current failed"));
}

fn stdout_json(cmd: &mut Command) -> serde_json::Value {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {:?}", String::from_utf8_lossy(&out.stdout)))
}

#[test]
fn wt_new_json() {
    let repo = TestRepo::new();
    let json = stdout_json(
        repo.cmd()
            .args(["wt", "new", "-b", "test/branch", "--json"]),
    );
    assert_eq!(json["ok"], true);
}

#[test]
fn json_stdout_is_only_json() {
    let repo = TestRepo::new();
    stdout_json(repo.cmd().args(["--json", "wt", "new", "-b", "feat"]));
    stdout_json(repo.cmd().args(["--json", "wt", "archive", "feat"]));
    stdout_json(repo.cmd().args(["--json", "wt", "unarchive", "feat"]));

    let tmp = tempfile::TempDir::new().unwrap();
    let mut init = common::flowy();
    init.current_dir(tmp.path())
        .args(["--json", "init", "fresh"]);
    stdout_json(&mut init);
}

#[test]
fn wt_archive_json_refuses_a_dirty_worktree_without_prompting() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat"])
        .assert()
        .success();
    fs::write(repo.root.join("wt/feat/wip.txt"), "wip\n").unwrap();

    repo.cmd()
        .args(["--json", "wt", "archive", "feat"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("--force"));
    assert!(repo.root.join("wt/feat").exists());
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

    common::flowy()
        .args(["init", project.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized"));

    assert!(project.join(".bare").is_dir(), ".bare should exist");
    assert!(project.join(".git").is_file(), ".git pointer should exist");
    assert!(
        project.join("wt").join("main").is_dir(),
        "wt/main should exist"
    );
    assert!(
        ws_dir_in(&project, "main").is_dir(),
        "notes/workstreams/main should exist"
    );

    let meta = read_meta(&ws_dir_in(&project, "main"));
    assert_eq!(meta.worktree.branch, "main");
    assert!(!meta.created.is_empty());
}

#[test]
fn init_json_output() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("myproject");

    common::flowy()
        .args(["init", project.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\": true"));
}

#[test]
fn init_with_relative_path_creates_worktree() {
    let tmp = tempfile::tempdir().unwrap();

    common::flowy()
        .current_dir(tmp.path())
        .args(["init", "myproject"])
        .assert()
        .success();

    assert!(
        tmp.path()
            .join("myproject")
            .join("wt")
            .join("main")
            .is_dir(),
        "wt/main should exist when init is called with a relative path"
    );
}

#[test]
fn init_fails_if_directory_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("existing");
    fs::create_dir_all(&project).unwrap();

    common::flowy()
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
        .args(["ticket", "add", "-w", "feat", title])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args(["--json", "ticket", "show", "-w", "feat", "1"])
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

// Running `done --summary` twice keeps one ## Summary section and both texts.
#[test]
fn t_done_summary_not_duplicated_on_second_call() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "add", "-w", "feat", "A ticket"])
        .assert()
        .success();

    repo.cmd()
        .args(["ticket", "done", "-w", "feat", "1", "--summary", "First"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "done", "-w", "feat", "1", "--reopen"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "done", "-w", "feat", "1", "--summary", "Second"])
        .assert()
        .success();

    let ticket_path = ws_dir_in(&repo.root, "feat").join("tickets").join("1.md");
    let content = fs::read_to_string(&ticket_path).unwrap();
    assert_eq!(
        content.matches("## Summary").count(),
        1,
        "exactly one ## Summary section should exist"
    );
    assert!(content.contains("First"), "{content}");
    assert!(content.contains("Second"), "{content}");
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
    git_in(&repo.root.join(".bare"), &["branch", "-D", "feat"]);

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

    git_in(tmp.path(), &["init", "-b", "main", src.to_str().unwrap()]);
    git_in(&src, &["commit", "--allow-empty", "-m", "init"]);

    common::flowy()
        .current_dir(tmp.path())
        .args(["clone", src.to_str().unwrap(), "project"])
        .assert()
        .success();

    let project = tmp.path().join("project");
    assert!(project.join(".bare").is_dir(), ".bare should exist");
    assert!(
        project.join("wt").join("main").is_dir(),
        "wt/main should exist after clone"
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
fn clone_tracks_origin_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    git_in(tmp.path(), &["init", "-b", "main", src.to_str().unwrap()]);
    git_in(&src, &["commit", "--allow-empty", "-m", "init"]);

    common::flowy()
        .current_dir(tmp.path())
        .args(["clone", src.to_str().unwrap(), "project"])
        .assert()
        .success();
    let project = tmp.path().join("project");
    let main_wt = project.join("wt").join("main");
    assert_eq!(
        git_out(&main_wt, &["rev-parse", "--abbrev-ref", "main@{upstream}"]).trim(),
        "origin/main"
    );

    // A branch pushed after the clone arrives with a fetch, and `wt new` checks
    // it out rather than branching fresh from HEAD.
    git_in(&src, &["checkout", "-q", "-b", "later"]);
    git_in(&src, &["commit", "--allow-empty", "-m", "later work"]);
    git_in(&project, &["fetch", "--quiet", "origin"]);
    common::flowy()
        .arg("--repo-root")
        .arg(&project)
        .args(["wt", "new", "-b", "later"])
        .assert()
        .success();

    let later_wt = project.join("wt").join("later");
    assert_eq!(
        git_out(&later_wt, &["rev-parse", "HEAD"]),
        git_out(&src, &["rev-parse", "HEAD"])
    );
    assert_eq!(
        git_out(&later_wt, &["rev-parse", "--abbrev-ref", "@{upstream}"]).trim(),
        "origin/later"
    );
}

#[test]
fn clone_scaffolds_non_main_default_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");

    git_in(tmp.path(), &["init", "-b", "trunk", src.to_str().unwrap()]);
    git_in(&src, &["commit", "--allow-empty", "-m", "init"]);

    common::flowy()
        .current_dir(tmp.path())
        .args(["clone", src.to_str().unwrap(), "project"])
        .assert()
        .success();

    let project = tmp.path().join("project");
    assert!(
        project.join("wt").join("trunk").is_dir(),
        "wt/trunk should exist after clone"
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
        .args(["ticket", "add", "Carry me", "-w", "feat-old"])
        .assert()
        .success();

    repo.cmd()
        .args(["wt", "rename", "feat-old", "feat/new"])
        .assert()
        .success();

    assert!(!repo.root.join("wt").join("feat-old").exists());
    assert!(!repo.ws_dir("feat-old").exists());

    let new_wt = repo.root.join("wt").join("feat-new");
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
    assert!(repo.root.join("wt").join("keep-me").is_dir());
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
    rename_worktree_with_submodule(false);
}

// git 2.48+ can write the worktree's gitdir pointers relative to each other.
#[test]
fn wt_rename_moves_worktree_with_submodule_and_relative_paths() {
    rename_worktree_with_submodule(true);
}

fn rename_worktree_with_submodule(relative_paths: bool) {
    let repo = TestRepo::new();
    if relative_paths {
        git_in(
            &repo.root.join(".bare"),
            &["config", "worktree.useRelativePaths", "true"],
        );
    }

    repo.cmd()
        .args(["wt", "new", "-b", "feat/sub"])
        .assert()
        .success();
    let old_wt = repo.root.join("wt").join("feat-sub");

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
    std::fs::write(old_wt.join("sub").join("wip.txt"), "unsaved\n").unwrap();

    repo.cmd()
        .args(["wt", "rename", "feat-sub", "feat/renamed"])
        .assert()
        .success();

    let new_wt = repo.root.join("wt").join("feat-renamed");
    assert!(!old_wt.exists(), "old worktree dir should be gone");
    assert_eq!(
        std::fs::read_to_string(new_wt.join("sub").join("wip.txt")).unwrap(),
        "unsaved\n",
        "untracked submodule work should move with the worktree"
    );

    // The submodule's core.worktree follows the move.
    let sub_status = git_out(&new_wt.join("sub"), &["status", "--porcelain"]);
    assert_eq!(sub_status.trim(), "?? wip.txt");

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
fn wt_rename_honors_a_worktree_lock() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat/locked"])
        .assert()
        .success();
    let old_wt = repo.root.join("wt").join("feat-locked");
    git_in(&old_wt, &["worktree", "lock", old_wt.to_str().unwrap()]);

    repo.cmd()
        .args(["wt", "rename", "feat-locked", "feat/moved"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("locked"));
    assert!(old_wt.exists());
    assert!(!repo.root.join("wt").join("feat-moved").exists());
}

#[test]
fn wt_rename_refuses_from_inside_worktree() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat/x"])
        .assert()
        .success();

    let wt = repo.root.join("wt").join("feat-x");
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
                root.join("wt").join(name).to_str().unwrap(),
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
        repo.ws_dir("legacy").join("tickets/001.md").exists(),
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
        .args(["ticket", "list", "-w", "legacy", "--json"])
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
fn migrate_reports_a_workstream_started_before_migrating() {
    let repo = TestRepo::new();
    let old = seed_old_layout(&repo.root, "legacy", true);
    repo.cmd()
        .args(["ticket", "add", "-w", "legacy", "Filed too early"])
        .assert()
        .success();

    repo.cmd()
        .arg("migrate")
        .assert()
        .success()
        .stdout(predicate::str::contains("conflict legacy"));

    assert!(
        old.join("tickets").join("001.md").exists(),
        "old notes stay put"
    );
    let json: serde_json::Value = serde_json::from_slice(
        &repo
            .cmd()
            .args(["--json", "migrate"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(json["workstreams"][0]["outcome"], "conflict");
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
    assert!(repo.ws_dir("legacy").join("tickets/001.md").exists());
}

// A worktree where flowy put it before wt/ replaced worktrees/, with its notes
// already inside it.
fn seed_old_worktree(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    let old = root.join("worktrees").join(name);
    git_in(
        &root.join(".bare"),
        &["worktree", "add", old.to_str().unwrap(), "-b", name],
    );
    let ws = old.join(".local").join("workstream");
    fs::create_dir_all(ws.join("tickets")).unwrap();
    fs::write(
        ws.join("tickets").join("1.md"),
        "+++\ntitle = \"legacy\"\nstatus = \"pending\"\n+++\nbody\n",
    )
    .unwrap();
    fs::write(
        ws.join("meta.toml"),
        format!(
            "created = \"x\"\n[worktree]\npath = \"{}\"\nbranch = \"{name}\"\n",
            old.display()
        ),
    )
    .unwrap();
    old
}

#[test]
fn migrate_moves_worktrees_into_wt() {
    let repo = TestRepo::new();
    let old = seed_old_worktree(&repo.root, "legacy");

    repo.cmd()
        .arg("migrate")
        .assert()
        .success()
        .stdout(predicate::str::contains("moved worktree legacy"));

    let new = repo.root.join("wt").join("legacy");
    assert!(!old.exists(), "old worktree should be gone");
    assert!(
        !repo.root.join("worktrees").exists(),
        "empty worktrees/ should be removed"
    );
    assert!(
        git_out(&repo.root.join(".bare"), &["worktree", "list"])
            .contains(new.canonicalize().unwrap().to_str().unwrap()),
        "git should know the worktree's new path"
    );
    git_in(&new, &["status"]);
    assert_eq!(
        read_meta(&repo.ws_dir("legacy")).worktree.path,
        new.to_string_lossy(),
        "meta should name the new path"
    );
    let output = repo
        .cmd()
        .args(["ticket", "list", "-w", "legacy", "--json"])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 1);
}

#[test]
fn migrate_dry_run_plans_worktree_and_notes_moves_together() {
    let repo = TestRepo::new();
    let old_notes = seed_old_layout(&repo.root, "legacy", false);
    git_in(
        &repo.root.join(".bare"),
        &[
            "worktree",
            "add",
            repo.root.join("worktrees/legacy").to_str().unwrap(),
            "-b",
            "legacy",
        ],
    );

    repo.cmd()
        .args(["migrate", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("would move worktree legacy"))
        .stdout(predicate::str::contains("would move legacy"))
        .stdout(predicate::str::contains("orphaned").not());

    assert!(repo.root.join("worktrees/legacy").is_dir());
    assert!(old_notes.is_dir());

    repo.cmd().arg("migrate").assert().success();
    assert!(repo.ws_dir("legacy").join("tickets/001.md").exists());
}

#[test]
fn migrate_leaves_a_worktree_whose_target_exists() {
    let repo = TestRepo::new();
    let old = seed_old_worktree(&repo.root, "legacy");
    fs::create_dir_all(repo.root.join("wt/legacy")).unwrap();

    repo.cmd()
        .arg("migrate")
        .assert()
        .success()
        .stdout(predicate::str::contains("conflict worktree legacy"));

    assert!(old.is_dir(), "the old worktree must stay put");
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
        .args(["ticket", "add", "-w", "feat-foo", "First"])
        .assert()
        .success();
    git_in(
        &repo.root.join("wt").join("feat-foo"),
        &["branch", "-m", "feat/bar"],
    );
    repo
}

#[test]
fn t_list_follows_the_worktree_dir_when_the_branch_is_renamed() {
    let repo = repo_with_renamed_branch();

    let output = repo
        .cmd()
        .args(["ticket", "list", "--json"])
        .current_dir(repo.root.join("wt").join("feat-foo"))
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
        .args(["ticket", "status", "--json"])
        .current_dir(repo.root.join("wt").join("feat-foo"))
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
        .args(["ticket", "add", "Second"])
        .current_dir(repo.root.join("wt").join("feat-foo"))
        .assert()
        .success();

    assert!(repo.ticket("feat-foo", 2).exists());
    // A branch-derived path would scaffold a directory that is not a worktree,
    // stranding tickets where nothing reads them.
    assert!(
        !repo.root.join("wt").join("feat-bar").exists(),
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
            .join("wt")
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
                .envs(common::GIT_ENV)
                .arg("--repo-root")
                .arg(&repo.root)
                .args(["ticket", "add", "-w", "test-branch", &format!("Ticket {i}")])
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

#[test]
fn t_add_body_file_reads_a_file() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    let body = repo.root.join("body.md");
    let prose = "## Scope\n\nHandles `foo --bar` and \"quoted\" text.\n\n- one\n- two\n";
    fs::write(&body, prose).unwrap();

    repo.cmd()
        .args([
            "ticket",
            "add",
            "-w",
            "test-branch",
            "My ticket",
            "--body-file",
        ])
        .arg(&body)
        .assert()
        .success();

    let content = fs::read_to_string(repo.ticket("test-branch", 1)).unwrap();
    assert!(content.contains(prose.trim_end()));
}

#[test]
fn t_add_body_file_dash_reads_stdin() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    repo.cmd()
        .args([
            "ticket",
            "add",
            "-w",
            "test-branch",
            "My ticket",
            "--body-file",
            "-",
        ])
        .write_stdin("Body with a $VAR and `backticks`.\n")
        .assert()
        .success();

    let content = fs::read_to_string(repo.ticket("test-branch", 1)).unwrap();
    assert!(content.contains("Body with a $VAR and `backticks`."));
}

#[test]
fn t_add_rejects_description_and_body_file_together() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    repo.cmd()
        .args([
            "ticket",
            "add",
            "-w",
            "test-branch",
            "My ticket",
            "-d",
            "inline",
            "--body-file",
            "-",
        ])
        .assert()
        .failure();
}

#[test]
fn t_add_missing_body_file_fails_without_creating_a_ticket() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    repo.cmd()
        .args([
            "ticket",
            "add",
            "-w",
            "test-branch",
            "My ticket",
            "--body-file",
            "/nonexistent/body.md",
        ])
        .assert()
        .failure();

    assert!(!repo.ticket("test-branch", 1).exists());
}

#[test]
fn t_journal_body_file_reads_stdin() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    repo.cmd()
        .args([
            "ticket",
            "journal",
            "-w",
            "test-branch",
            "1",
            "--body-file",
            "-",
            "--heading",
            "Findings",
        ])
        .write_stdin("Measured with `perf stat`:\n\n    3.2s -> 0.8s\n")
        .assert()
        .success();

    let content = fs::read_to_string(repo.ticket("test-branch", 1)).unwrap();
    assert!(content.contains("] Findings"));
    assert!(content.contains("3.2s -> 0.8s"));
}

#[test]
fn t_journal_requires_text_or_body_file() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    repo.cmd()
        .args(["ticket", "journal", "-w", "test-branch", "1"])
        .assert()
        .failure();
}

#[test]
fn t_done_summary_from_body_file() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    repo.cmd()
        .args([
            "ticket",
            "done",
            "-w",
            "test-branch",
            "1",
            "--body-file",
            "-",
        ])
        .write_stdin("Landed in `parser.rs`.\n\nThe \"fast path\" was the fix.\n")
        .assert()
        .success();

    let content = fs::read_to_string(repo.ticket("test-branch", 1)).unwrap();
    assert!(content.contains("status = \"complete\""));
    assert!(content.contains("## Summary"));
    assert!(content.contains("The \"fast path\" was the fix."));
}

#[test]
fn notes_write_then_show() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    let plan = "# Goal\n\nShip the `--body-file` flag.\n";
    repo.cmd()
        .args(["notes", "write", "-w", "test-branch", "planning"])
        .write_stdin(plan)
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(repo.ws_dir("test-branch").join("planning.md")).unwrap(),
        plan
    );

    let out = repo
        .cmd()
        .args(["notes", "show", "-w", "test-branch", "planning"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), plan);
}

#[test]
fn notes_write_creates_nested_paths() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    repo.cmd()
        .args([
            "notes",
            "write",
            "-w",
            "test-branch",
            "milestones/001-parser/summary.md",
        ])
        .write_stdin("# Milestone 1\n")
        .assert()
        .success();

    assert!(
        repo.ws_dir("test-branch")
            .join("milestones/001-parser/summary.md")
            .is_file()
    );
}

#[test]
fn notes_write_rejects_paths_outside_the_workstream() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    let outside = tempfile::TempDir::new().unwrap();
    let absolute = outside.path().join("escaped.md");
    let relative = repo.ws_dir("test-branch").join("..").join("escaped.md");
    for (name, target) in [
        ("../escaped.md", &relative),
        (absolute.to_str().unwrap(), &absolute),
    ] {
        repo.cmd()
            .args(["notes", "write", "-w", "test-branch", name])
            .write_stdin("nope\n")
            .assert()
            .failure()
            .stderr(predicate::str::contains("inside the workstream"));
        assert!(!target.exists(), "{name} was written");
    }
}

#[test]
fn notes_write_refuses_an_empty_body() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    let plan = repo.ws_dir("test-branch").join("planning.md");
    fs::write(&plan, "the plan\n").unwrap();

    repo.cmd()
        .args(["notes", "write", "-w", "test-branch", "planning"])
        .write_stdin("")
        .assert()
        .failure();

    assert_eq!(fs::read_to_string(&plan).unwrap(), "the plan\n");
}

#[test]
fn notes_append_separates_blocks() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    repo.cmd()
        .args(["notes", "write", "-w", "test-branch", "planning"])
        .write_stdin("First.")
        .assert()
        .success();
    repo.cmd()
        .args(["notes", "append", "-w", "test-branch", "planning"])
        .write_stdin("## Amendment\n\nSecond.")
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(repo.ws_dir("test-branch").join("planning.md")).unwrap(),
        "First.\n\n## Amendment\n\nSecond.\n"
    );
}

#[test]
fn notes_log_appends_a_timestamped_bullet() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    repo.cmd()
        .args([
            "notes",
            "log",
            "-w",
            "test-branch",
            "Planning complete. 4 tickets created.",
        ])
        .assert()
        .success();

    let status = fs::read_to_string(repo.ws_dir("test-branch").join("status.md")).unwrap();
    let last = status.lines().last().unwrap();
    assert!(last.starts_with("- 20"), "got {last:?}");
    assert!(last.ends_with("Planning complete. 4 tickets created."));
}

#[test]
fn notes_ls_marks_absent_files() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    let output = repo
        .cmd()
        .args(["--json", "notes", "ls", "-w", "test-branch"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    let notes = json["notes"].as_array().unwrap();
    let by_name = |n: &str| {
        notes
            .iter()
            .find(|e| e["name"] == n)
            .unwrap_or_else(|| panic!("{n} missing from notes ls"))
            .clone()
    };
    assert_eq!(by_name("meta.toml")["exists"], true);
    assert_eq!(by_name("planning.md")["exists"], false);
}

#[test]
fn notes_path_prints_the_workstream_dir() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    let out = repo
        .cmd()
        .args(["notes", "path", "-w", "test-branch"])
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(printed, repo.ws_dir("test-branch").display().to_string());
}

#[test]
fn notes_milestone_freezes_the_plan_and_keeps_tickets() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");
    let ws = repo.ws_dir("test-branch");

    fs::write(ws.join("planning.md"), "the plan\n").unwrap();
    fs::write(ws.join("next.md"), "the board\n").unwrap();
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "Done work"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "done", "-w", "test-branch", "1"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args([
            "--json",
            "notes",
            "milestone",
            "-w",
            "test-branch",
            "No Lua",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(json["number"], 1);
    assert_eq!(json["slug"], "no-lua");
    assert_eq!(json["pending_tickets"], 0);

    let dir = ws.join("milestones").join("1-no-lua");
    assert_eq!(
        fs::read_to_string(dir.join("planning.md")).unwrap(),
        "the plan\n"
    );
    assert_eq!(
        fs::read_to_string(dir.join("next.md")).unwrap(),
        "the board\n"
    );
    assert!(
        !ws.join("planning.md").exists(),
        "the plan must move, not copy"
    );
    assert!(!ws.join("next.md").exists());
    assert!(repo.ticket("test-branch", 1).exists(), "tickets stay put");
    assert!(
        fs::read_to_string(ws.join("status.md"))
            .unwrap()
            .contains("milestone 1-no-lua frozen")
    );
}

#[test]
fn notes_milestone_numbers_past_the_highest() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");
    let ws = repo.ws_dir("test-branch");

    fs::create_dir_all(ws.join("milestones").join("001-first")).unwrap();
    fs::write(ws.join("planning.md"), "the plan\n").unwrap();

    repo.cmd()
        .args(["notes", "milestone", "-w", "test-branch", "second"])
        .assert()
        .success();

    assert!(ws.join("milestones").join("2-second").is_dir());
}

#[test]
fn notes_milestone_reports_pending_tickets() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");
    let ws = repo.ws_dir("test-branch");

    fs::write(ws.join("planning.md"), "the plan\n").unwrap();
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "Still open"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args([
            "--json",
            "notes",
            "milestone",
            "-w",
            "test-branch",
            "partial",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["pending_tickets"], 1);
}

#[test]
fn notes_milestone_without_a_plan_fails() {
    let repo = TestRepo::new();
    new_workstream(&repo, "test/branch");

    repo.cmd()
        .args(["notes", "milestone", "-w", "test-branch", "nothing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("nothing to freeze"));
}

#[test]
fn t_add_writes_priority_frontmatter() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "-p", "high", "Urgent"])
        .assert()
        .success();

    let content = fs::read_to_string(repo.ticket("test-branch", 1)).unwrap();
    assert!(content.contains("priority = \"high\""));
}

#[test]
fn t_add_without_priority_omits_the_field() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();

    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "Plain"])
        .assert()
        .success();

    let content = fs::read_to_string(repo.ticket("test-branch", 1)).unwrap();
    assert!(!content.contains("priority"));
}

#[test]
fn t_priority_sets_the_level_on_an_existing_ticket() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "Later"])
        .assert()
        .success();

    repo.cmd()
        .args(["ticket", "priority", "-w", "test-branch", "1", "low"])
        .assert()
        .success();

    let content = fs::read_to_string(repo.ticket("test-branch", 1)).unwrap();
    assert!(content.contains("priority = \"low\""));

    repo.cmd()
        .args(["ticket", "priority", "-w", "test-branch", "1", "high"])
        .assert()
        .success();

    let content = fs::read_to_string(repo.ticket("test-branch", 1)).unwrap();
    assert!(content.contains("priority = \"high\""));
    assert!(!content.contains("low"));
}

#[test]
fn t_priority_rejects_an_unknown_level() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "T"])
        .assert()
        .success();

    repo.cmd()
        .args(["ticket", "priority", "-w", "test-branch", "1", "urgent"])
        .assert()
        .failure();
}

#[test]
fn t_list_orders_by_priority_then_number() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    for (title, priority) in [
        ("Low one", "low"),
        ("High one", "high"),
        ("Plain", "medium"),
    ] {
        repo.cmd()
            .args(["ticket", "add", "-w", "test-branch", "-p", priority, title])
            .assert()
            .success();
    }

    let output = repo
        .cmd()
        .args(["ticket", "list", "-w", "test-branch", "--json"])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = json.as_array().unwrap();
    let order: Vec<u64> = arr.iter().map(|t| t["number"].as_u64().unwrap()).collect();
    assert_eq!(order, vec![2, 3, 1]);
    assert_eq!(arr[0]["priority"], "high");
}

#[test]
fn t_list_filters_by_priority() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "-p", "high", "Urgent"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "Ordinary"])
        .assert()
        .success();

    let output = repo
        .cmd()
        .args([
            "ticket",
            "list",
            "-w",
            "test-branch",
            "--json",
            "-p",
            "high",
        ])
        .output()
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["title"], "Urgent");
}

// A ticket written before priorities existed has no `priority` key; setting one
// must leave the rest of the frontmatter and the body intact.
#[test]
fn t_priority_preserves_an_existing_ticket_body() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args([
            "ticket",
            "add",
            "-w",
            "test-branch",
            "-d",
            "Some details",
            "Titled",
        ])
        .assert()
        .success();

    repo.cmd()
        .args(["ticket", "priority", "-w", "test-branch", "1", "high"])
        .assert()
        .success();

    let content = fs::read_to_string(repo.ticket("test-branch", 1)).unwrap();
    assert!(content.contains("title = \"Titled\""));
    assert!(content.contains("status = \"pending\""));
    assert!(content.contains("Some details"));
}

#[test]
fn workstream_name_rejects_dot_components() {
    let repo = TestRepo::new();
    for bad in [".", ".."] {
        repo.cmd()
            .args(["ticket", "ls", "-w", bad])
            .assert()
            .failure()
            .stderr(predicate::str::contains("not a valid workstream name"));
        repo.cmd().args(["wt", "archive", bad]).assert().failure();
    }
}

#[test]
fn wt_new_reports_safe_name_collision() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "a/b"])
        .assert()
        .success();
    repo.cmd()
        .args(["wt", "new", "-b", "a-b"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("maps to workstream 'a-b'"));
}

#[test]
fn t_edit_runs_editor_with_arguments() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "test/branch"])
        .assert()
        .success();
    repo.cmd()
        .args(["ticket", "add", "-w", "test-branch", "My ticket"])
        .assert()
        .success();

    repo.cmd()
        .args(["ticket", "edit", "-w", "test-branch", "1"])
        .env("EDITOR", "perl -pi -e s/My/Your/")
        .assert()
        .success();

    let ticket = ws_dir_in(&repo.root, "test-branch")
        .join("tickets")
        .join("1.md");
    assert!(fs::read_to_string(ticket).unwrap().contains("Your ticket"));
}

#[test]
fn ticket_flags_reject_bad_combinations() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["ticket", "ls", "--status", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value 'bogus'"));
    repo.cmd()
        .args(["ticket", "done", "1", "--reopen", "--wont-implement"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn wt_new_refuses_start_point_for_existing_branch() {
    let repo = TestRepo::new();
    git_in(&repo.root.join(".bare"), &["branch", "existing", "main"]);
    repo.cmd()
        .args(["wt", "new", "-b", "existing", "main"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
    assert!(!repo.root.join("wt").join("existing").exists());
}

#[test]
fn clone_refuses_an_existing_target() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir(tmp.path().join("project")).unwrap();
    common::flowy()
        .current_dir(tmp.path())
        .args(["clone", "https://example.invalid/repo.git", "project"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("'project' already exists"));
    assert!(!tmp.path().join("project").join(".bare").exists());
}
