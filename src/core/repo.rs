use anyhow::{Context, Result, bail};
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

// The worktree directory cwd sits inside, if any. A workstream is keyed by its
// directory name, not by the checked-out branch: `git branch -m` moves the branch
// out from under the directory, and rebuilding the path from the branch then
// points at a directory that does not exist.
pub fn cwd_worktree(root: &Path) -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let worktrees = worktrees_dir(root);
    // Resolve both sides: current_dir() is always physical, while --repo-root may
    // arrive relative or by way of a symlink (macOS /tmp, /var/folders).
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    let worktrees = worktrees.canonicalize().unwrap_or(worktrees);
    let rel = cwd.strip_prefix(&worktrees).ok()?;
    rel.components()
        .next()
        .and_then(|c| c.as_os_str().to_str())
        .map(|s| s.to_string())
}

// Workstream state lives at <worktree>/.local/workstream, inside the working
// tree, so git must be told to ignore it or every worktree reads as dirty and
// `git worktree remove` refuses without --force. info/exclude covers all
// worktrees of the repo and is never committed.
pub fn ensure_local_excluded(root: &Path) -> Result<()> {
    let path = root.join(".bare").join("info").join("exclude");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == ".local/") {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("\n# flowy: workstream notes live here\n.local/\n");
    std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))
}
