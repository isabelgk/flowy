use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use crate::core::{
    cache::{Cache, WorktreeCache, load_cache, save_cache},
    meta::{Meta, WorktreeMeta, read_meta, write_meta},
    names::safe_name,
    repo::{archive_root, ensure_local_excluded, find_repo_root, worktrees_dir},
    ticket::{Status, load_tickets},
};
use crate::output::print_json;

pub fn dispatch(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("new", sub)) => new(sub),
        Some(("ls", sub)) => ls(sub),
        Some(("archive", sub)) => archive(sub),
        Some(("unarchive", sub)) => unarchive(sub),
        Some(("rename", sub)) => rename(sub),
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
    std::fs::create_dir_all(archive_root(&root)).context("creating notes/workstreams/archive/")?;
    ensure_local_excluded(&root)?;

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
    std::fs::create_dir_all(archive_root(&root)).context("creating notes/workstreams/archive/")?;
    ensure_local_excluded(&root)?;

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

    // Self-heal repos scaffolded before workstream notes moved into the worktree.
    ensure_local_excluded(&root)?;

    let branch_exists = Command::new("git")
        .args(["rev-parse", "--verify", &format!("refs/heads/{branch}")])
        .current_dir(root.join(".bare"))
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    let mut cmd = Command::new("git");
    cmd.arg("worktree").arg("add").arg(&worktree_path);
    if branch_exists {
        cmd.arg(branch);
    } else {
        cmd.arg("-b").arg(branch);
        if let Some(sp) = start_point {
            cmd.arg(sp);
        }
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
    // The worktree's own path, so consumers resolve notes from where they
    // actually live instead of rebuilding the path from the branch name.
    pub path: PathBuf,
    pub branch: String,
    pub dirty: bool,
    pub age_seconds: i64,
    pub tickets_pending: usize,
    pub tickets_total: usize,
}

pub fn list_worktrees(root: &Path) -> Result<Vec<WorktreeInfo>> {
    let mut cache = load_cache(root);
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(root.join(".bare"))
        .output()
        .context("running git worktree list")?;
    if !output.status.success() {
        anyhow::bail!("git worktree list failed");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let result = collect_worktree_info(&stdout, &mut cache);
    save_cache(root, &cache);
    Ok(result)
}

pub fn format_age(age_seconds: i64) -> String {
    format_age_inner(age_seconds)
}

fn ls(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");

    let worktrees = list_worktrees(&root)?;

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

struct WorktreeBlock {
    path: PathBuf,
    branch: String,
    head_sha: Option<String>,
}

fn parse_worktree_blocks(porcelain: &str) -> Vec<WorktreeBlock> {
    let mut blocks = Vec::new();

    for block in porcelain.split("\n\n") {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }

        let mut path: Option<PathBuf> = None;
        let mut branch: Option<String> = None;
        let mut head_sha: Option<String> = None;
        let mut is_bare = false;

        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(b) = line.strip_prefix("branch ") {
                branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            } else if let Some(h) = line.strip_prefix("HEAD ") {
                head_sha = Some(h.to_string());
            } else if line == "bare" {
                is_bare = true;
            }
        }

        if is_bare {
            continue;
        }

        let Some(path) = path else { continue };
        let branch = branch.unwrap_or_else(|| "(detached)".to_string());
        blocks.push(WorktreeBlock {
            path,
            branch,
            head_sha,
        });
    }

    blocks
}

fn collect_worktree_info(porcelain: &str, cache: &mut Cache) -> Vec<WorktreeInfo> {
    // Per-worktree status comes from separate `git` invocations (status, log);
    // running them concurrently avoids paying process-spawn latency N times over.
    let jobs: Vec<_> = parse_worktree_blocks(porcelain)
        .into_iter()
        .map(|b| {
            // The directory name is the workstream's identity (`wt archive`,
            // `wt rename` and the notes path all key on it); the branch can be
            // renamed out from under it.
            let name = b
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| safe_name(&b.branch));
            let ws_dir = crate::core::names::workstream_subdir(&b.path);
            let old_entry = cache.worktrees.get(&name).cloned().unwrap_or_default();
            (name, b.path, b.branch, b.head_sha, ws_dir, old_entry)
        })
        .collect();

    let computed: Vec<(String, WorktreeInfo, WorktreeCache)> = thread::scope(|scope| {
        jobs.into_iter()
            .map(|(name, path, branch, head_sha, ws_dir, entry)| {
                scope.spawn(move || {
                    compute_worktree_entry(name, path, branch, head_sha, ws_dir, entry)
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().expect("worktree info thread panicked"))
            .collect()
    });

    let mut result = Vec::with_capacity(computed.len());
    for (name, info, entry) in computed {
        cache.worktrees.insert(name, entry);
        result.push(info);
    }
    result
}

fn compute_worktree_entry(
    name: String,
    path: PathBuf,
    branch: String,
    head_sha: Option<String>,
    ws_dir: PathBuf,
    mut entry: WorktreeCache,
) -> (String, WorktreeInfo, WorktreeCache) {
    let dirty = is_dirty(&path).unwrap_or(false);

    // Use cached commit timestamp if HEAD hasn't moved; else re-fetch from git.
    let commit_ts = match &head_sha {
        Some(sha) if *sha == entry.head_sha => entry.commit_ts,
        _ => {
            let ts = commit_timestamp(&path).unwrap_or(0);
            entry.head_sha = head_sha.unwrap_or_default();
            entry.commit_ts = ts;
            ts
        }
    };
    let age_seconds = (chrono::Utc::now().timestamp() - commit_ts).max(0);

    // Use cached ticket counts if the tickets directory mtime is unchanged.
    let mtime = tickets_dir_mtime(&ws_dir);
    let (tickets_pending, tickets_total) = if mtime != 0 && mtime == entry.tickets_dir_mtime {
        (entry.tickets_pending, entry.tickets_total)
    } else {
        let tickets = load_tickets(&ws_dir).unwrap_or_default();
        let pending = tickets
            .iter()
            .filter(|t| t.status == Status::Pending)
            .count();
        let total = tickets.len();
        entry.tickets_dir_mtime = mtime;
        entry.tickets_pending = pending;
        entry.tickets_total = total;
        (pending, total)
    };

    let info = WorktreeInfo {
        name: name.clone(),
        path,
        branch,
        dirty,
        age_seconds,
        tickets_pending,
        tickets_total,
    };
    (name, info, entry)
}

fn tickets_dir_mtime(ws_dir: &Path) -> u64 {
    // Max mtime across ticket files — directory mtime only changes on add/remove,
    // not on content edits (e.g. status changes written in-place). Nanoseconds,
    // because a status change landing in the same second as the previous count
    // would otherwise read as no change at all and keep the stale count forever.
    let dir = ws_dir.join("tickets");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| {
            e.metadata()
                .ok()?
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_nanos() as u64)
        })
        .max()
        .unwrap_or(0)
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

fn commit_timestamp(path: &Path) -> Result<i64> {
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

    ts_str.parse().context("parsing commit timestamp")
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

    // Reporting success while doing nothing hides a typo'd name, and hides
    // old-layout notes that `flowy migrate` could not place.
    if !worktree_path.exists() && !crate::core::names::workstream_dir(&root, name).exists() {
        if root.join("notes").join("workstreams").join(name).is_dir() {
            bail!(
                "'{name}' still uses the old notes layout and has no worktree; \
                 move notes/workstreams/{name} to notes/workstreams/archive/{name} by hand"
            );
        }
        bail!("no workstream named '{name}' to archive");
    }

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

    // Move notes out before removing the worktree they live in, so a partial
    // failure leaves them intact rather than deleted with the worktree.
    let ws_dir = crate::core::names::workstream_dir(&root, name);
    let archive_dir = crate::core::names::archived_workstream_dir(&root, name);
    std::fs::create_dir_all(archive_root(&root)).context("creating notes/workstreams/archive/")?;

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

    let mut cache = load_cache(&root);
    cache.worktrees.remove(name);
    save_cache(&root, &cache);

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

    let archive_dir = crate::core::names::archived_workstream_dir(&root, name);
    let meta = read_meta(&archive_dir)?;

    let branch = &meta.worktree.branch;
    let worktree_path = worktrees_dir(&root).join(name);
    let ws_dir = crate::core::names::workstream_dir(&root, name);

    // The worktree must exist before the notes can move into it, so create it
    // first and unwind it if the move fails.
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
        bail!("git worktree add failed");
    }

    let restore = (|| -> Result<()> {
        let parent = ws_dir
            .parent()
            .context("workstream dir has no parent")?
            .to_path_buf();
        std::fs::create_dir_all(&parent)
            .with_context(|| format!("creating {}", parent.display()))?;
        std::fs::rename(&archive_dir, &ws_dir).with_context(|| {
            format!(
                "restoring {} from {}",
                ws_dir.display(),
                archive_dir.display()
            )
        })
    })();

    if let Err(e) = restore {
        // Leave the workstream cleanly archived rather than half-restored.
        let _ = Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&worktree_path)
            .current_dir(root.join(".bare"))
            .status();
        return Err(e);
    }

    if json {
        return print_json(&serde_json::json!({"ok": true}));
    }
    println!("Unarchived {name}");
    Ok(())
}

fn rename(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    // Absolute root so fs::rename / `git -C` don't depend on cwd.
    let root = root
        .canonicalize()
        .with_context(|| format!("resolving repo root {}", root.display()))?;
    let json = matches.get_flag("json");
    let old_name = matches.get_one::<String>("old").unwrap();
    let new_branch = matches.get_one::<String>("new-branch").unwrap();
    let new_name = safe_name(new_branch);

    // Notes live inside the worktree, so an archived workstream has nothing to
    // rename in place; it must be unarchived first.
    let old_ws_dir = crate::core::names::workstream_dir(&root, old_name);
    if !old_ws_dir.exists() {
        if crate::core::names::archived_workstream_dir(&root, old_name).exists() {
            bail!("workstream '{old_name}' is archived; unarchive it before renaming");
        }
        bail!(
            "no workstream named '{old_name}' (looked in {})",
            old_ws_dir.display()
        );
    }
    let old_meta = read_meta(&old_ws_dir)?;
    let old_branch = old_meta.worktree.branch.clone();

    let old_worktree = worktrees_dir(&root).join(old_name);
    let new_worktree = worktrees_dir(&root).join(&new_name);
    let new_ws_dir = crate::core::names::workstream_dir(&root, &new_name);

    let renaming_dirs = new_name != *old_name;
    let renaming_branch = *new_branch != old_branch;

    if !renaming_dirs && !renaming_branch {
        bail!("'{old_name}' is already named '{new_branch}'; nothing to do");
    }

    // Only guard against clobbering when the safe-name actually changes; a
    // branch-only rename (same safe-name) legitimately keeps its dirs.
    if renaming_dirs {
        if new_worktree.exists() {
            bail!("target worktree already exists: {}", new_worktree.display());
        }
        // Renaming the dir out from under the shell's cwd breaks the move and
        // every `git -C` after it; refuse rather than half-move.
        if old_worktree.exists()
            && let (Ok(cwd), Ok(owt)) = (std::env::current_dir(), old_worktree.canonicalize())
            && cwd
                .canonicalize()
                .map(|c| c.starts_with(&owt))
                .unwrap_or(false)
        {
            bail!(
                "run this from outside the worktree being renamed (cwd is under {})",
                owt.display()
            );
        }
    }

    // Track progress so a mid-sequence failure reports what already moved.
    let mut moved_branch = false;

    if renaming_branch {
        let status = Command::new("git")
            .args(["branch", "-m", &old_branch, new_branch])
            .current_dir(root.join(".bare"))
            .status()
            .context("running git branch -m")?;
        if !status.success() {
            bail!("git branch -m {old_branch} {new_branch} failed");
        }
        moved_branch = true;
    }

    if renaming_dirs
        && old_worktree.exists()
        && let Err(e) = move_worktree(&root, &old_worktree, &new_worktree)
    {
        // move_worktree may fail after the dir is already renamed (repair or
        // submodule step); report based on where the dir actually is.
        report_partial(moved_branch, &old_branch, new_branch, new_worktree.exists());
        return Err(e);
    }

    // Notes live inside the worktree, so move_worktree already carried them.

    let target_ws_dir = if renaming_dirs {
        &new_ws_dir
    } else {
        &old_ws_dir
    };
    let meta = Meta {
        created: old_meta.created,
        worktree: WorktreeMeta {
            path: new_worktree.to_string_lossy().into_owned(),
            branch: new_branch.clone(),
        },
    };
    write_meta(target_ws_dir, &meta)?;

    // Stale entries under the old key; rebuilt on the next `ls`.
    let mut cache = load_cache(&root);
    cache.worktrees.remove(old_name);
    cache.worktrees.remove(&new_name);
    save_cache(&root, &cache);

    if json {
        return print_json(
            &serde_json::json!({"ok": true, "name": new_name, "branch": new_branch}),
        );
    }
    println!("Renamed {old_name} -> {new_name} (branch {new_branch})");
    Ok(())
}

fn move_worktree(root: &Path, old: &Path, new: &Path) -> Result<()> {
    // Fast path: git moves the dir and rewrites its own admin files.
    let moved = Command::new("git")
        .arg("worktree")
        .arg("move")
        .arg(old)
        .arg(new)
        .current_dir(root.join(".bare"))
        .status()
        .context("running git worktree move")?;
    if moved.success() {
        return Ok(());
    }

    // git refuses to move a worktree containing submodules, and the check is on
    // the gitlinks in the tree (not on init state), so deinit alone won't let
    // `git worktree move` succeed. Move it by hand and fix the admin pointers.
    deinit_submodules(old);
    std::fs::rename(old, new)
        .with_context(|| format!("moving {} to {}", old.display(), new.display()))?;
    repair_worktree_gitdir(new)?;

    // Restore submodule working trees; this also refreshes each core.worktree.
    let status = Command::new("git")
        .args(["submodule", "update", "--init", "--recursive"])
        .current_dir(new)
        .status()
        .context("running git submodule update --init --recursive")?;
    if !status.success() {
        bail!("git submodule update --init --recursive failed after manual move");
    }
    Ok(())
}

fn deinit_submodules(worktree: &Path) {
    // Best-effort: a repo with no submodules has nothing to deinit.
    let _ = Command::new("git")
        .args(["submodule", "deinit", "--all", "-f"])
        .current_dir(worktree)
        .status();
}

fn repair_worktree_gitdir(new_worktree: &Path) -> Result<()> {
    // The worktree's own .git file points (absolutely) at its admin dir under
    // .bare/worktrees/<id>, unaffected by the move. Only the reverse pointer
    // (admin/gitdir -> worktree/.git) still names the pre-move path.
    let git_file = new_worktree.join(".git");
    let content = std::fs::read_to_string(&git_file)
        .with_context(|| format!("reading {}", git_file.display()))?;
    let admin_dir = content
        .lines()
        .find_map(|l| l.strip_prefix("gitdir: "))
        .map(|p| PathBuf::from(p.trim()))
        .with_context(|| format!("no gitdir pointer in {}", git_file.display()))?;

    let pointer = admin_dir.join("gitdir");
    std::fs::write(&pointer, format!("{}\n", git_file.display()))
        .with_context(|| format!("writing {}", pointer.display()))
}

fn report_partial(moved_branch: bool, old_branch: &str, new_branch: &str, moved_worktree: bool) {
    eprintln!("rename left a partial state:");
    if moved_branch {
        eprintln!("  - branch renamed {old_branch} -> {new_branch} (not reverted)");
    }
    if moved_worktree {
        eprintln!("  - worktree dir moved; notes dir and meta.toml not yet updated");
    }
    eprintln!("finish the remaining moves by hand, or re-run once resolved.");
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
