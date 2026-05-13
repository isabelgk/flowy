use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::core::{
    meta::{Meta, WorktreeMeta, read_meta, write_meta},
    names::safe_name,
    repo::{find_repo_root, notes_dir, worktrees_dir},
    ticket::{Status, load_tickets},
};
use crate::output::print_json;

pub fn dispatch(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("new", sub)) => new(sub),
        Some(("ls", sub)) => ls(sub),
        Some(("archive", sub)) => archive(sub),
        Some(("unarchive", sub)) => unarchive(sub),
        _ => Ok(()),
    }
}

fn get_repo_root(matches: &ArgMatches) -> Result<PathBuf> {
    if let Some(path) = matches.get_one::<String>("repo-root") {
        Ok(PathBuf::from(path))
    } else {
        find_repo_root()
    }
}

pub fn init(matches: &ArgMatches) -> Result<()> {
    let json = matches.get_flag("json");
    let name = matches.get_one::<String>("name").unwrap();

    let root = PathBuf::from(name);
    if root.exists() {
        bail!("'{name}' already exists");
    }
    std::fs::create_dir_all(&root).with_context(|| format!("creating {name}"))?;
    let root = root
        .canonicalize()
        .with_context(|| format!("resolving path for {name}"))?;

    let init_src = root.join(".init-src");
    let status = Command::new("git")
        .args(["init", "-b", "main"])
        .arg(&init_src)
        .status()
        .context("running git init")?;
    if !status.success() {
        bail!("git init failed");
    }

    let status = Command::new("git")
        .args([
            "commit",
            "--allow-empty",
            "--allow-empty-message",
            "--message",
            "",
        ])
        .current_dir(&init_src)
        .status()
        .context("running git commit")?;
    if !status.success() {
        bail!("git commit failed");
    }

    let bare_dir = root.join(".bare");
    let status = Command::new("git")
        .args(["clone", "--bare"])
        .arg(&init_src)
        .arg(&bare_dir)
        .status()
        .context("running git clone --bare")?;
    if !status.success() {
        bail!("git clone --bare failed");
    }

    std::fs::remove_dir_all(&init_src).context("removing .init-src")?;

    let status = Command::new("git")
        .args(["remote", "remove", "origin"])
        .current_dir(&bare_dir)
        .status()
        .context("running git remote remove origin")?;
    if !status.success() {
        bail!("git remote remove origin failed");
    }

    std::fs::write(root.join(".git"), "gitdir: ./.bare\n").context("writing .git pointer")?;
    std::fs::create_dir_all(root.join("worktrees")).context("creating worktrees/")?;
    std::fs::create_dir_all(root.join("notes").join("workstreams").join("archive"))
        .context("creating notes/workstreams/archive/")?;

    let worktree_path = root.join("worktrees").join("main");
    let status = Command::new("git")
        .args(["worktree", "add"])
        .arg(&worktree_path)
        .arg("main")
        .current_dir(&bare_dir)
        .status()
        .context("running git worktree add")?;
    if !status.success() {
        bail!("git worktree add failed");
    }

    let ws_dir = crate::core::names::workstream_dir(&root, "main");
    std::fs::create_dir_all(ws_dir.join("tickets")).context("creating tickets dir")?;
    let meta = Meta {
        created: chrono::Utc::now().to_rfc3339(),
        worktree: WorktreeMeta {
            path: worktree_path.to_string_lossy().into_owned(),
            branch: "main".to_string(),
        },
    };
    write_meta(&ws_dir, &meta)?;

    if json {
        return print_json(&serde_json::json!({"ok": true}));
    }
    println!("Initialized {name}");
    Ok(())
}

pub fn clone(matches: &ArgMatches) -> Result<()> {
    let json = matches.get_flag("json");
    let url = matches.get_one::<String>("url").unwrap();

    let name = if let Some(n) = matches.get_one::<String>("name") {
        n.clone()
    } else {
        // Derive from URL: take last path component, strip .git suffix
        url.trim_end_matches('/')
            .rsplit('/')
            .next()
            .or_else(|| url.rsplit(':').next())
            .unwrap_or(url)
            .trim_end_matches(".git")
            .to_string()
    };

    let status = Command::new("git")
        .args(["clone", "--bare", url])
        .arg(PathBuf::from(&name).join(".bare"))
        .status()
        .context("running git clone")?;
    if !status.success() {
        bail!("git clone --bare failed");
    }

    let root = PathBuf::from(&name)
        .canonicalize()
        .with_context(|| format!("resolving absolute path for {name}"))?;
    let bare_dir = root.join(".bare");

    // .git pointer so git commands work from project root
    std::fs::write(root.join(".git"), "gitdir: ./.bare\n").context("writing .git pointer")?;

    std::fs::create_dir_all(root.join("worktrees")).context("creating worktrees/")?;
    std::fs::create_dir_all(root.join("notes").join("workstreams").join("archive"))
        .context("creating notes/workstreams/archive/")?;

    // Detect the default branch and scaffold its worktree and notes.
    let head_out = Command::new("git")
        .args(["symbolic-ref", "HEAD"])
        .current_dir(&bare_dir)
        .output()
        .context("running git symbolic-ref HEAD")?;
    let default_branch = if head_out.status.success() {
        String::from_utf8_lossy(&head_out.stdout)
            .trim()
            .strip_prefix("refs/heads/")
            .unwrap_or("main")
            .to_string()
    } else {
        "main".to_string()
    };

    let safe = safe_name(&default_branch);
    let worktree_path = root.join("worktrees").join(&safe);
    let status = Command::new("git")
        .args(["worktree", "add"])
        .arg(&worktree_path)
        .arg(&default_branch)
        .current_dir(&bare_dir)
        .status()
        .context("running git worktree add")?;
    if !status.success() {
        bail!("git worktree add failed");
    }

    let ws_dir = crate::core::names::workstream_dir(&root, &safe);
    std::fs::create_dir_all(ws_dir.join("tickets")).context("creating tickets dir")?;
    let meta = Meta {
        created: chrono::Utc::now().to_rfc3339(),
        worktree: WorktreeMeta {
            path: worktree_path.to_string_lossy().into_owned(),
            branch: default_branch,
        },
    };
    write_meta(&ws_dir, &meta)?;

    if json {
        return print_json(&serde_json::json!({"ok": true}));
    }
    println!("Set up {name}");
    Ok(())
}

fn new(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let branch = matches.get_one::<String>("branch").unwrap();
    let start_point = matches.get_one::<String>("start-point");

    let name = safe_name(branch);
    let worktree_path = worktrees_dir(&root).join(&name);

    let mut cmd = Command::new("git");
    cmd.arg("worktree")
        .arg("add")
        .arg(&worktree_path)
        .arg("-b")
        .arg(branch);
    if let Some(sp) = start_point {
        cmd.arg(sp);
    }
    cmd.current_dir(root.join(".bare"));

    let status = cmd.status().context("running git worktree add")?;
    if !status.success() {
        bail!("git worktree add failed");
    }

    let ws_dir = crate::core::names::workstream_dir(&root, &name);
    std::fs::create_dir_all(ws_dir.join("tickets"))
        .with_context(|| format!("creating {}", ws_dir.display()))?;

    let meta = Meta {
        created: chrono::Utc::now().to_rfc3339(),
        worktree: WorktreeMeta {
            path: worktree_path.to_string_lossy().into_owned(),
            branch: branch.clone(),
        },
    };
    write_meta(&ws_dir, &meta)?;

    if json {
        return print_json(&serde_json::json!({"ok": true}));
    }
    println!("Created worktree {name}");
    Ok(())
}

#[derive(Debug, serde::Serialize)]
pub struct WorktreeInfo {
    pub name: String,
    pub branch: String,
    pub dirty: bool,
    pub age_seconds: i64,
    pub tickets_pending: usize,
    pub tickets_total: usize,
}

pub fn list_worktrees(root: &Path) -> Result<Vec<WorktreeInfo>> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(root.join(".bare"))
        .output()
        .context("running git worktree list")?;
    if !output.status.success() {
        anyhow::bail!("git worktree list failed");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(collect_worktree_info(&stdout, root))
}

pub fn format_age(age_seconds: i64) -> String {
    format_age_inner(age_seconds)
}

fn ls(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");

    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(root.join(".bare"))
        .output()
        .context("running git worktree list")?;
    if !output.status.success() {
        bail!("git worktree list failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let worktrees = collect_worktree_info(&stdout, &root);

    if json {
        return print_json(&worktrees);
    }

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(["Branch", "", "Last commit", "Pending tickets"]);

    for wt in &worktrees {
        let branch_cell = if wt.dirty {
            Cell::new(&wt.branch).fg(Color::Cyan)
        } else {
            Cell::new(&wt.branch)
        };
        let dirty_cell = if wt.dirty {
            Cell::new("*")
                .fg(Color::Yellow)
                .add_attribute(Attribute::Bold)
        } else {
            Cell::new("")
        };
        let tickets_label = if wt.tickets_pending > 0 {
            Cell::new(format!("{}/{}", wt.tickets_pending, wt.tickets_total)).fg(Color::Green)
        } else {
            Cell::new(format!("{}/{}", wt.tickets_pending, wt.tickets_total))
        };
        table.add_row([
            branch_cell,
            dirty_cell,
            Cell::new(format_age_inner(wt.age_seconds)),
            tickets_label,
        ]);
    }

    println!("{table}");
    Ok(())
}

fn collect_worktree_info(porcelain: &str, root: &Path) -> Vec<WorktreeInfo> {
    let mut result = Vec::new();

    for block in porcelain.split("\n\n") {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }

        let mut path: Option<PathBuf> = None;
        let mut branch: Option<String> = None;
        let mut is_bare = false;

        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(b) = line.strip_prefix("branch ") {
                branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            } else if line == "bare" {
                is_bare = true;
            }
        }

        if is_bare {
            continue;
        }

        let path = match path {
            Some(p) => p,
            None => continue,
        };

        let branch = branch.unwrap_or_else(|| "(detached)".to_string());
        let dirty = is_dirty(&path).unwrap_or(false);
        let age_seconds = branch_age(&path).unwrap_or(0);

        let name = safe_name(&branch);
        let ws_dir = crate::core::names::workstream_dir(root, &name);
        let tickets = load_tickets(&ws_dir).unwrap_or_default();
        let tickets_pending = tickets
            .iter()
            .filter(|t| t.status == Status::Pending)
            .count();
        let tickets_total = tickets.len();

        result.push(WorktreeInfo {
            name,
            branch,
            dirty,
            age_seconds,
            tickets_pending,
            tickets_total,
        });
    }

    result
}

fn is_dirty(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .context("running git status --porcelain")?;
    Ok(!out.stdout.is_empty())
}

fn branch_age(path: &Path) -> Result<i64> {
    let out = Command::new("git")
        .args(["log", "-1", "--format=%ct", "HEAD"])
        .current_dir(path)
        .output()
        .context("running git log")?;

    let ts_str = String::from_utf8_lossy(&out.stdout);
    let ts_str = ts_str.trim();
    if ts_str.is_empty() {
        return Ok(0);
    }

    let ts: i64 = ts_str.parse().context("parsing commit timestamp")?;
    Ok((chrono::Utc::now().timestamp() - ts).max(0))
}

fn format_age_inner(age_seconds: i64) -> String {
    let age = age_seconds.max(0);
    let minutes = age / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    let weeks = days / 7;
    let months = days / 30;
    let years = days / 365;

    if years > 0 {
        format!("{years}y")
    } else if months > 0 {
        format!("{months}mo")
    } else if weeks > 0 {
        format!("{weeks}w")
    } else if days > 0 {
        format!("{days}d")
    } else if hours > 0 {
        format!("{hours}h")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "now".to_string()
    }
}

fn archive(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let name = matches.get_one::<String>("name").unwrap();
    let force = matches.get_flag("force");

    let worktree_path = worktrees_dir(&root).join(name);

    // Check dirty status; if dirty and not forced, prompt the user
    let mut needs_force = force;
    if !force && worktree_path.exists() && is_dirty(&worktree_path)? {
        print!("Worktree '{name}' has uncommitted changes. Archive anyway? [y/N] ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            bail!("archive cancelled");
        }
        // git worktree remove also refuses dirty worktrees without --force
        needs_force = true;
    }

    // Move notes dir before removing worktree so partial failure leaves notes intact
    let ws_dir = crate::core::names::workstream_dir(&root, name);
    let archive_dir = notes_dir(&root).join("archive").join(name);
    std::fs::create_dir_all(notes_dir(&root).join("archive"))
        .context("creating notes/workstreams/archive/")?;

    if ws_dir.exists() {
        std::fs::rename(&ws_dir, &archive_dir)
            .with_context(|| format!("moving {} to {}", ws_dir.display(), archive_dir.display()))?;
    }

    if worktree_path.exists() {
        let mut cmd = Command::new("git");
        cmd.arg("worktree").arg("remove");
        if needs_force {
            cmd.arg("--force");
        }
        cmd.arg(&worktree_path).current_dir(root.join(".bare"));

        let status = cmd.status().context("running git worktree remove")?;
        if !status.success() {
            bail!("git worktree remove failed");
        }
    }

    if json {
        return print_json(&serde_json::json!({"ok": true}));
    }
    println!("Archived {name}");
    Ok(())
}

fn unarchive(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let name = matches.get_one::<String>("name").unwrap();

    let archive_dir = notes_dir(&root).join("archive").join(name);
    let meta = read_meta(&archive_dir)?;

    let branch = &meta.worktree.branch;
    let worktree_path = worktrees_dir(&root).join(name);
    let ws_dir = crate::core::names::workstream_dir(&root, name);

    // Move notes first so that if worktree add fails we can roll back and leave
    // the workstream cleanly in the archive.
    std::fs::rename(&archive_dir, &ws_dir).with_context(|| {
        format!(
            "restoring {} from {}",
            ws_dir.display(),
            archive_dir.display()
        )
    })?;

    // Branch already exists in the bare repo; no -b flag needed.
    let status = Command::new("git")
        .arg("worktree")
        .arg("add")
        .arg(&worktree_path)
        .arg(branch)
        .current_dir(root.join(".bare"))
        .status()
        .context("running git worktree add")?;
    if !status.success() {
        let _ = std::fs::rename(&ws_dir, &archive_dir);
        bail!("git worktree add failed");
    }

    if json {
        return print_json(&serde_json::json!({"ok": true}));
    }
    println!("Unarchived {name}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_age_under_minute_is_now() {
        assert_eq!(format_age(0), "now");
        assert_eq!(format_age(59), "now");
        assert_eq!(format_age(-10), "now");
    }

    #[test]
    fn format_age_minutes() {
        assert_eq!(format_age(60), "1m");
        assert_eq!(format_age(3599), "59m");
    }

    #[test]
    fn format_age_hours() {
        assert_eq!(format_age(3600), "1h");
        assert_eq!(format_age(86399), "23h");
    }

    #[test]
    fn format_age_days() {
        assert_eq!(format_age(86400), "1d");
        assert_eq!(format_age(7 * 86400 - 1), "6d");
    }

    #[test]
    fn format_age_weeks() {
        assert_eq!(format_age(7 * 86400), "1w");
    }

    #[test]
    fn format_age_months() {
        assert_eq!(format_age(30 * 86400), "1mo");
    }

    #[test]
    fn format_age_years() {
        assert_eq!(format_age(365 * 86400), "1y");
    }
}
