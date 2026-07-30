use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

pub fn find_repo_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join(".bare").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("could not find repo root (no .bare directory found in any parent)");
        }
    }
}

// Archived workstreams stay at the repo root: archiving removes the worktree,
// so notes kept inside it would be destroyed by the thing meant to preserve them.
pub fn archive_root(root: &Path) -> PathBuf {
    root.join("notes").join("workstreams").join("archive")
}

pub fn worktrees_dir(root: &Path) -> PathBuf {
    root.join("worktrees")
}
