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

pub fn notes_dir(root: &Path) -> PathBuf {
    root.join("notes").join("workstreams")
}

pub fn worktrees_dir(root: &Path) -> PathBuf {
    root.join("worktrees")
}
