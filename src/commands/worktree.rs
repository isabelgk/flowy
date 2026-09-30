use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use super::get_repo_root;
use crate::core::{
    cache::{Cache, WorktreeCache, load_cache, save_cache},
    meta::{Meta, WorktreeMeta, read_meta, write_meta},
    names::safe_name,
    repo::{archive_root, ensure_local_excluded, worktrees_dir},
    ticket::{Status, load_tickets_lenient},
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
    let status = git()
        .args(["init", "-b", "main"])
        .arg(&init_src)
        .status()
        .context("running git init")?;
    if !status.success() {
        bail!("git init failed");
    }

    let status = git()
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
    let status = git()
        .args(["clone", "--bare"])
        .arg(&init_src)
        .arg(&bare_dir)
        .status()
        .context("running git clone --bare")?;
    if !status.success() {
        bail!("git clone --bare failed");
    }

    std::fs::remove_dir_all(&init_src).context("removing .init-src")?;

    let status = git()
        .args(["remote", "remove", "origin"])
        .current_dir(&bare_dir)
        .status()
        .context("running git remote remove origin")?;
    if !status.success() {
        bail!("git remote remove origin failed");
    }

    std::fs::write(root.join(".git"), "gitdir: ./.bare\n").context("writing .git pointer")?;
    std::fs::create_dir_all(root.join("worktrees")).context("creating worktrees/")?;
    ensure_local_excluded(&root)?;

    let worktree_path = root.join("worktrees").join("main");
    let status = git()
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

// The last component of a URL or scp-style `host:path` remote, without `.git`.
fn clone_dir_name(url: &str) -> Result<String> {
    let last = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("");
    let name = last.strip_suffix(".git").unwrap_or(last);
    if name.is_empty() || name == "." || name == ".." {
        bail!("cannot derive a directory name from '{url}'; pass NAME");
    }
    Ok(name.to_string())
}

pub fn clone(matches: &ArgMatches) -> Result<()> {
    let json = matches.get_flag("json");
    let url = matches.get_one::<String>("url").unwrap();

    let name = match matches.get_one::<String>("name") {
        Some(n) => n.clone(),
        None => clone_dir_name(url)?,
    };
    if Path::new(&name).exists() {
        bail!("'{name}' already exists");
    }

    let status = git()
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
    track_origin(&bare_dir)?;

    // .git pointer so git commands work from project root
    std::fs::write(root.join(".git"), "gitdir: ./.bare\n").context("writing .git pointer")?;

    std::fs::create_dir_all(root.join("worktrees")).context("creating worktrees/")?;
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
    let status = git()
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
    // `a/b` and `a-b` share a directory; say so rather than surface git's error.
    if worktree_path.exists() {
        bail!(
            "worktree directory {} already exists; branch '{branch}' maps to workstream '{name}'",
            worktree_path.display()
        );
    }

    // Self-heal repos scaffolded before workstream notes moved into the worktree.
    ensure_local_excluded(&root)?;

    let ref_exists = |refname: &str| {
        Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", refname])
            .current_dir(root.join(".bare"))
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    let branch_exists = ref_exists(&format!("refs/heads/{branch}"));
    if branch_exists && let Some(sp) = start_point {
        bail!("branch '{branch}' already exists; START_POINT '{sp}' only applies to a new branch");
    }
    let on_origin = !branch_exists
        && start_point.is_none()
        && ref_exists(&format!("refs/remotes/origin/{branch}"));

    let mut cmd = git();
    cmd.arg("worktree").arg("add").arg(&worktree_path);
    if branch_exists {
        cmd.arg(branch);
    } else if on_origin {
        cmd.args(["--track", "-b", branch])
            .arg(format!("origin/{branch}"));
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorktreeInfo {
    pub name: String,
    // The worktree's own path, so consumers resolve notes from where they
    // actually live instead of rebuilding the path from the branch name.
    pub path: PathBuf,
    pub branch: String,
    pub dirty: bool,
    // None when HEAD has no commit (unborn branch) or git could not report one.
    pub age_seconds: Option<i64>,
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

pub fn format_age(age_seconds: Option<i64>) -> String {
    age_seconds.map_or_else(|| "-".to_string(), format_age_inner)
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
            Cell::new(format_age(wt.age_seconds)),
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
        Some(sha) if *sha == entry.head_sha && entry.commit_ts.is_some() => entry.commit_ts,
        _ => {
            let ts = commit_timestamp(&path);
            entry.head_sha = head_sha.unwrap_or_default();
            entry.commit_ts = ts;
            ts
        }
    };
    let age_seconds = commit_ts.map(|ts| (chrono::Utc::now().timestamp() - ts).max(0));

    let fingerprint = tickets_fingerprint(&ws_dir);
    let (tickets_pending, tickets_total) =
        if fingerprint != 0 && fingerprint == entry.tickets_fingerprint {
            (entry.tickets_pending, entry.tickets_total)
        } else {
            // A ticket that does not parse is left out of the counts.
            let (tickets, _) = load_tickets_lenient(&ws_dir);
            let pending = tickets
                .iter()
                .filter(|t| t.status == Status::Pending)
                .count();
            let total = tickets.len();
            entry.tickets_fingerprint = fingerprint;
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

// Changes whenever a ticket is added, removed, renamed, or rewritten: every
// entry's name, size, and mtime go into it, with the directory's own mtime. File
// mtimes alone miss a deleted ticket, or one copied in with an old mtime. Hashed
// with DefaultHasher, which can change between builds; a mismatch only costs a
// recount. 0 means there is no tickets directory.
fn tickets_fingerprint(ws_dir: &Path) -> u64 {
    use std::hash::{Hash, Hasher};

    let dir = ws_dir.join("tickets");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    let nanos = |m: &std::fs::Metadata| {
        m.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos())
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((e.file_name(), m.len(), nanos(&m)))
        })
        .collect();
    files.sort();

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::fs::metadata(&dir)
        .map(|m| nanos(&m))
        .unwrap_or(0)
        .hash(&mut hasher);
    files.hash(&mut hasher);
    hasher.finish().max(1)
}

// For git commands whose output is not captured. Some report on stdout ("HEAD is
// now at ..."), which would land ahead of --json output, so stdout goes to
// stderr with the rest of git's chatter.
fn git() -> Command {
    let mut cmd = Command::new("git");
    cmd.stdout(std::io::stderr());
    cmd
}

// A bare clone configures no fetch refspec, so `git fetch` would bring in no
// origin/* refs and no branch could track its remote. Sets the refspec, fetches
// once, and points every cloned branch at its origin counterpart.
fn track_origin(bare_dir: &Path) -> Result<()> {
    run_git(
        bare_dir,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    )?;
    run_git(bare_dir, &["fetch", "--quiet", "origin"])?;
    let branches = run_git(
        bare_dir,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )?;
    for branch in branches.lines().filter(|b| !b.is_empty()) {
        // Written as config rather than `branch --set-upstream-to`, which reports
        // on stdout.
        run_git(
            bare_dir,
            &["config", &format!("branch.{branch}.remote"), "origin"],
        )?;
        run_git(
            bare_dir,
            &[
                "config",
                &format!("branch.{branch}.merge"),
                &format!("refs/heads/{branch}"),
            ],
        )?;
    }
    Ok(())
}

// Stdout of a git command that has to succeed.
fn run_git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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

// None on an unborn HEAD or any git failure.
fn commit_timestamp(path: &Path) -> Option<i64> {
    let out = Command::new("git")
        .args(["log", "-1", "--format=%ct", "HEAD"])
        .current_dir(path)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
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
                 move notes/workstreams/{name} to .flowy/archive/{name} by hand"
            );
        }
        bail!("no workstream named '{name}' to archive");
    }

    // Check dirty status; if dirty and not forced, prompt the user
    let mut needs_force = force;
    if !force && worktree_path.exists() && is_dirty(&worktree_path)? {
        // A prompt would corrupt JSON output, and nobody is there to answer it.
        if json || !io::stdin().is_terminal() {
            bail!("worktree '{name}' has uncommitted changes; pass --force to archive anyway");
        }
        eprint!("Worktree '{name}' has uncommitted changes. Archive anyway? [y/N] ");
        io::stderr().flush()?;
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
    std::fs::create_dir_all(archive_root(&root)).context("creating .flowy/archive/")?;

    // git refuses to remove a worktree holding submodules unless forced. The
    // dirty check above already covered their contents.
    if worktree_path.exists() && !needs_force && has_submodules(&worktree_path)? {
        needs_force = true;
    }

    let moved_notes = ws_dir.exists();
    if moved_notes {
        std::fs::rename(&ws_dir, &archive_dir)
            .with_context(|| format!("moving {} to {}", ws_dir.display(), archive_dir.display()))?;
    }

    if worktree_path.exists() {
        let mut cmd = git();
        cmd.arg("worktree").arg("remove");
        if needs_force {
            cmd.arg("--force");
        }
        cmd.arg(&worktree_path).current_dir(root.join(".bare"));

        let status = cmd.status().context("running git worktree remove")?;
        if !status.success() {
            // A worktree that survives with its notes in archive/ is neither live
            // nor archived, and neither `unarchive` nor `rename` can recover it.
            if moved_notes {
                std::fs::rename(&archive_dir, &ws_dir).with_context(|| {
                    format!(
                        "git worktree remove failed, and moving notes back from {} also failed",
                        archive_dir.display()
                    )
                })?;
            }
            bail!("git worktree remove failed; {name} is left unarchived");
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
    let status = git()
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
        let _ = git()
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
        let status = git()
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
        .output()
        .context("running git worktree move")?;
    if moved.status.success() {
        return Ok(());
    }
    let git_error = String::from_utf8_lossy(&moved.stderr).trim().to_string();

    // git refuses to move a worktree containing submodules, and that is the only
    // refusal worth working around; a lock, for one, has to stay honored.
    let submodules = submodule_git_dirs(old)?;
    if submodules.is_empty() || admin_dir(old)?.join("locked").exists() {
        bail!("git worktree move failed: {git_error}");
    }

    // Moving the tree as-is keeps untracked and uncommitted submodule work. The
    // submodules' own `.git` files are relative and stay at the same depth, so
    // only the admin pointer and each submodule's core.worktree go stale.
    std::fs::rename(old, new)
        .with_context(|| format!("moving {} to {}", old.display(), new.display()))?;
    repair_worktree_gitdir(root, new)?;
    for (path, git_dir) in submodules {
        let status = git()
            .args(["config", "--file"])
            .arg(git_dir.join("config"))
            .arg("core.worktree")
            .arg(new.join(&path))
            .status()
            .context("running git config core.worktree")?;
        if !status.success() {
            bail!("updating core.worktree for submodule {path} failed");
        }
    }
    Ok(())
}

// (path relative to the worktree, absolute git dir) for every checked-out
// submodule, nested ones included.
fn submodule_git_dirs(worktree: &Path) -> Result<Vec<(String, PathBuf)>> {
    let out = Command::new("git")
        .args([
            "submodule",
            "foreach",
            "--quiet",
            "--recursive",
            "echo \"$displaypath\"",
        ])
        .current_dir(worktree)
        .output()
        .context("running git submodule foreach")?;
    if !out.status.success() {
        bail!(
            "listing submodules failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|path| {
            let git_dir = Command::new("git")
                .args(["rev-parse", "--absolute-git-dir"])
                .current_dir(worktree.join(path))
                .output()
                .with_context(|| format!("resolving the git dir of submodule {path}"))?;
            if !git_dir.status.success() {
                bail!("resolving the git dir of submodule {path} failed");
            }
            let git_dir = String::from_utf8_lossy(&git_dir.stdout).trim().to_string();
            Ok((path.to_string(), PathBuf::from(git_dir)))
        })
        .collect()
}

// A deinitialized submodule leaves no checkout but keeps its git dir under the
// admin dir's modules/, and git still counts it.
fn has_submodules(worktree: &Path) -> Result<bool> {
    Ok(admin_dir(worktree)?.join("modules").is_dir() || !submodule_git_dirs(worktree)?.is_empty())
}

// The worktree's admin dir under .bare/worktrees/<id>, read from its .git file.
fn admin_dir(worktree: &Path) -> Result<PathBuf> {
    let git_file = worktree.join(".git");
    let content = std::fs::read_to_string(&git_file)
        .with_context(|| format!("reading {}", git_file.display()))?;
    content
        .lines()
        .find_map(|l| l.strip_prefix("gitdir: "))
        // Relative under worktree.useRelativePaths; join keeps an absolute one as is.
        .map(|p| worktree.join(p.trim()))
        .with_context(|| format!("no gitdir pointer in {}", git_file.display()))
}

// After a manual move the admin dir's gitdir still names the old path. git
// rewrites it in whichever form worktree.useRelativePaths asks for.
fn repair_worktree_gitdir(root: &Path, new_worktree: &Path) -> Result<()> {
    let status = git()
        .args(["worktree", "repair"])
        .arg(new_worktree)
        .current_dir(root.join(".bare"))
        .status()
        .context("running git worktree repair")?;
    if !status.success() {
        bail!("git worktree repair failed for {}", new_worktree.display());
    }
    Ok(())
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
    fn clone_dir_name_handles_urls_and_scp_remotes() {
        assert_eq!(clone_dir_name("https://h/u/repo.git").unwrap(), "repo");
        assert_eq!(clone_dir_name("https://h/u/repo/").unwrap(), "repo");
        assert_eq!(clone_dir_name("git@h:u/repo.git").unwrap(), "repo");
        assert_eq!(clone_dir_name("host:repo.git").unwrap(), "repo");
        assert!(clone_dir_name("host:").is_err());
    }

    #[test]
    fn unborn_head_has_no_age() {
        let dir = tempfile::TempDir::new().unwrap();
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(commit_timestamp(dir.path()), None);
        assert_eq!(format_age(None), "-");
    }

    #[test]
    fn format_age_under_minute_is_now() {
        assert_eq!(format_age_inner(0), "now");
        assert_eq!(format_age_inner(59), "now");
        assert_eq!(format_age_inner(-10), "now");
    }

    #[test]
    fn format_age_minutes() {
        assert_eq!(format_age_inner(60), "1m");
        assert_eq!(format_age_inner(3599), "59m");
    }

    #[test]
    fn format_age_hours() {
        assert_eq!(format_age_inner(3600), "1h");
        assert_eq!(format_age_inner(86399), "23h");
    }

    #[test]
    fn format_age_days() {
        assert_eq!(format_age_inner(86400), "1d");
        assert_eq!(format_age_inner(7 * 86400 - 1), "6d");
    }

    #[test]
    fn format_age_weeks() {
        assert_eq!(format_age_inner(7 * 86400), "1w");
    }

    #[test]
    fn format_age_months() {
        assert_eq!(format_age_inner(30 * 86400), "1mo");
    }

    #[test]
    fn format_age_years() {
        assert_eq!(format_age_inner(365 * 86400), "1y");
    }
}
