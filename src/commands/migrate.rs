use anyhow::{Context, Result};
use clap::ArgMatches;
use std::path::{Path, PathBuf};

use crate::core::{
    names::workstream_dir,
    repo::{find_repo_root, worktrees_dir},
};
use crate::output::print_json;

// What migrating one old-layout workstream would do, or did.
enum Outcome {
    Moved,
    // Notes with no worktree to move into; the user must archive or move them.
    Orphaned,
    // Destination already populated, so a previous run covered this one.
    Skipped,
}

impl Outcome {
    fn label(&self) -> &'static str {
        match self {
            Outcome::Moved => "moved",
            Outcome::Orphaned => "orphaned",
            Outcome::Skipped => "skipped",
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

    let mut results: Vec<(String, Outcome, PathBuf)> = Vec::new();

    for (name, old_dir) in old_layout_dirs(&root) {
        let worktree = worktrees_dir(&root).join(&name);
        let new_dir = workstream_dir(&root, &name);

        if new_dir.exists() {
            results.push((name, Outcome::Skipped, new_dir));
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
            Outcome::Skipped => println!("skipped {name} (already at {})", path.display()),
            Outcome::Orphaned => println!(
                "orphaned {name}: no worktree at {}; archive it or move {} by hand",
                worktrees_dir(&root).join(name).display(),
                path.display()
            ),
        }
    }

    Ok(())
}
