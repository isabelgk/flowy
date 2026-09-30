use anyhow::{Context, Result};
use clap::ArgMatches;
use std::path::{Path, PathBuf};

use crate::core::{
    names::workstream_dir,
    repo::{ensure_local_excluded, find_repo_root, worktrees_dir},
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

fn get_repo_root(matches: &ArgMatches) -> Result<PathBuf> {
    if let Some(path) = matches.get_one::<String>("repo-root") {
        Ok(PathBuf::from(path))
    } else {
        find_repo_root()
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

pub fn run(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let dry_run = matches.get_flag("dry-run");

    // Repos scaffolded before the move have no exclude entry for .local/.
    if !dry_run {
        ensure_local_excluded(&root)?;
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
        if !worktree.is_dir() {
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
        let val: Vec<serde_json::Value> = results
            .iter()
            .map(|(name, outcome, path)| {
                serde_json::json!({
                    "workstream": name,
                    "outcome": outcome.label(),
                    "path": path.to_string_lossy(),
                })
            })
            .collect();
        return print_json(&serde_json::json!({"dry_run": dry_run, "workstreams": val}));
    }

    if results.is_empty() {
        println!("Nothing to migrate.");
        return Ok(());
    }

    let prefix = if dry_run { "would move" } else { "moved" };
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
