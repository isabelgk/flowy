use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::get_repo_root;
use super::worktree::move_worktree;
use crate::core::{
    meta::{read_meta, write_meta},
    names::{workstream_dir, workstream_subdir},
    repo::{ensure_local_excluded, worktrees_dir},
};
use crate::output::print_json;

// What migrating one old-layout workstream would do, or did.
enum Outcome {
    Moved,
    // Notes with no worktree to move into; the user must archive or move them.
    Orphaned,
    // Something (a `ticket add`, say) created the new notes dir before the old
    // one was migrated. Both hold notes, and their ticket numbers can overlap,
    // so the user has to merge them.
    Conflict,
}

impl Outcome {
    fn label(&self) -> &'static str {
        match self {
            Outcome::Moved => "moved",
            Outcome::Orphaned => "orphaned",
            Outcome::Conflict => "conflict",
        }
    }
}

// Old-layout workstream dirs: everything directly under notes/workstreams/
// except the archive root, which does not move.
fn old_layout_dirs(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(root.join("notes").join("workstreams")) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.to_string();
            if name == "archive" {
                return None;
            }
            Some((name, p))
        })
        .collect()
}

// Worktrees under the project's old worktrees/ dir, which wt/ replaced. Only
// entries with a .git file are worktrees; anything else is left where it is.
fn old_worktrees(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(root.join("worktrees")) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(".git").is_file())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|p| Some((p.file_name()?.to_str()?.to_string(), p)))
        .collect()
}

// Moves each worktree on its own rather than renaming worktrees/ wholesale:
// git's admin pointers and any submodule's core.worktree name the old path, and
// a failure partway leaves a state that re-running picks up from.
fn migrate_worktrees(root: &Path, dry_run: bool) -> Result<Vec<(String, Outcome, PathBuf)>> {
    let old = old_worktrees(root);
    if old.is_empty() {
        return Ok(Vec::new());
    }
    let old_root = root.join("worktrees");
    if !dry_run
        && let (Ok(cwd), Ok(old_root)) = (std::env::current_dir(), old_root.canonicalize())
        && cwd
            .canonicalize()
            .map(|c| c.starts_with(&old_root))
            .unwrap_or(false)
    {
        bail!(
            "run this from outside the worktrees being moved (cwd is under {})",
            old_root.display()
        );
    }

    let mut results = Vec::new();
    for (name, old_path) in old {
        let new_path = worktrees_dir(root).join(&name);
        if new_path.exists() {
            results.push((name, Outcome::Conflict, old_path));
            continue;
        }
        if !dry_run {
            std::fs::create_dir_all(worktrees_dir(root))
                .with_context(|| format!("creating {}", worktrees_dir(root).display()))?;
            move_worktree(root, &old_path, &new_path).with_context(|| {
                format!(
                    "moving {} to {}; re-run migrate once resolved",
                    old_path.display(),
                    new_path.display()
                )
            })?;
        }
        results.push((name, Outcome::Moved, new_path));
    }
    if !dry_run {
        // Fails, harmlessly, while conflicts or non-worktree entries remain.
        let _ = std::fs::remove_dir(&old_root);
    }
    Ok(results)
}

// Renames each worktree's notes from .local/workstream, the name .local/flowy
// replaced. A dry run also looks in the worktrees/ entries it would have moved.
fn migrate_notes_dirs(
    root: &Path,
    dry_run: bool,
    pending_worktrees: &HashSet<&str>,
) -> Result<Vec<(String, Outcome, PathBuf)>> {
    let mut worktrees: Vec<(String, PathBuf)> = std::fs::read_dir(worktrees_dir(root))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join(".git").is_file())
                .filter_map(|p| Some((p.file_name()?.to_str()?.to_string(), p)))
                .collect()
        })
        .unwrap_or_default();
    worktrees.extend(
        pending_worktrees
            .iter()
            .map(|name| (name.to_string(), root.join("worktrees").join(name))),
    );
    worktrees.sort();

    let mut results = Vec::new();
    for (name, worktree) in worktrees {
        let old_dir = worktree.join(".local").join("workstream");
        if !old_dir.is_dir() {
            continue;
        }
        let new_dir = workstream_subdir(&worktree);
        if new_dir.exists() {
            results.push((name, Outcome::Conflict, old_dir));
            continue;
        }
        if !dry_run {
            std::fs::rename(&old_dir, &new_dir).with_context(|| {
                format!("moving {} to {}", old_dir.display(), new_dir.display())
            })?;
        }
        results.push((name.clone(), Outcome::Moved, workstream_dir(root, &name)));
    }
    Ok(results)
}

pub fn run(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let dry_run = matches.get_flag("dry-run");

    // Repos scaffolded before the move have no exclude entry for .local/.
    if !dry_run {
        ensure_local_excluded(&root)?;
    }

    let worktree_results = migrate_worktrees(&root, dry_run)?;
    // A dry run leaves worktrees under worktrees/, so the notes step has to
    // count the ones it would have moved as present.
    let pending_worktrees: HashSet<&str> = worktree_results
        .iter()
        .filter(|(_, outcome, _)| dry_run && matches!(outcome, Outcome::Moved))
        .map(|(name, _, _)| name.as_str())
        .collect();
    let notes_dir_results = migrate_notes_dirs(&root, dry_run, &pending_worktrees)?;

    // Waits for the notes rename, since meta.toml moves with it.
    if !dry_run {
        for (name, outcome, path) in &worktree_results {
            let ws_dir = workstream_dir(&root, name);
            if matches!(outcome, Outcome::Moved) && ws_dir.join("meta.toml").exists() {
                let mut meta = read_meta(&ws_dir)?;
                meta.worktree.path = path.to_string_lossy().into_owned();
                write_meta(&ws_dir, &meta)?;
            }
        }
    }

    let mut results: Vec<(String, Outcome, PathBuf)> = Vec::new();

    for (name, old_dir) in old_layout_dirs(&root) {
        let worktree = worktrees_dir(&root).join(&name);
        let new_dir = workstream_dir(&root, &name);

        // A completed migration moves the old dir away, so finding both means
        // the new one was started independently.
        if new_dir.exists() {
            results.push((name, Outcome::Conflict, old_dir));
            continue;
        }
        if !worktree.is_dir() && !pending_worktrees.contains(name.as_str()) {
            results.push((name, Outcome::Orphaned, old_dir));
            continue;
        }

        if !dry_run {
            let parent = new_dir.parent().context("workstream dir has no parent")?;
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
            std::fs::rename(&old_dir, &new_dir).with_context(|| {
                format!("moving {} to {}", old_dir.display(), new_dir.display())
            })?;
        }
        results.push((name, Outcome::Moved, new_dir));
    }

    if json {
        let to_json = |results: &[(String, Outcome, PathBuf)]| -> Vec<serde_json::Value> {
            results
                .iter()
                .map(|(name, outcome, path)| {
                    serde_json::json!({
                        "workstream": name,
                        "outcome": outcome.label(),
                        "path": path.to_string_lossy(),
                    })
                })
                .collect()
        };
        return print_json(&serde_json::json!({
            "dry_run": dry_run,
            "worktrees": to_json(&worktree_results),
            "notes_dirs": to_json(&notes_dir_results),
            "workstreams": to_json(&results),
        }));
    }

    if worktree_results.is_empty() && notes_dir_results.is_empty() && results.is_empty() {
        println!("Nothing to migrate.");
        return Ok(());
    }

    let prefix = if dry_run { "would move" } else { "moved" };
    for (name, outcome, path) in &worktree_results {
        match outcome {
            Outcome::Moved => println!("{prefix} worktree {name} -> {}", path.display()),
            Outcome::Conflict => println!(
                "conflict worktree {name}: both {} and {} exist\n  \
                 remove or rename one by hand, then re-run migrate",
                path.display(),
                worktrees_dir(&root).join(name).display()
            ),
            Outcome::Orphaned => unreachable!("worktree moves are never orphaned"),
        }
    }
    for (name, outcome, path) in &notes_dir_results {
        match outcome {
            Outcome::Moved => println!("{prefix} notes {name} -> {}", path.display()),
            Outcome::Conflict => println!(
                "conflict notes {name}: both {} and {} exist\n  \
                 merge the old notes into the new dir by hand, then remove the old dir",
                path.display(),
                workstream_dir(&root, name).display()
            ),
            Outcome::Orphaned => unreachable!("notes renames are never orphaned"),
        }
    }
    for (name, outcome, path) in &results {
        match outcome {
            Outcome::Moved => println!("{prefix} {name} -> {}", path.display()),
            Outcome::Conflict => println!(
                "conflict {name}: notes exist at both {} and {}\n  \
                 merge the old notes into the new dir by hand (ticket numbers may overlap), \
                 then remove the old dir",
                path.display(),
                workstream_dir(&root, name).display()
            ),
            // `wt archive` cannot help here: it moves notes out of a worktree,
            // and there is no worktree. Name the destination instead.
            Outcome::Orphaned => println!(
                "orphaned {name}: no worktree at {}\n  \
                 move {} to {} by hand, or recreate the worktree and re-run migrate",
                worktrees_dir(&root).join(name).display(),
                path.display(),
                crate::core::names::archived_workstream_dir(&root, name).display()
            ),
        }
    }

    Ok(())
}
