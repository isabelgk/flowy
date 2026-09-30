use crate::core::atomic::write_atomic;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct Meta {
    pub created: String,
    pub worktree: WorktreeMeta,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorktreeMeta {
    pub path: String,
    pub branch: String,
}

pub fn read_meta(dir: &Path) -> Result<Meta> {
    let path = dir.join("meta.toml");
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("parsing {}", path.display()))
}

pub fn write_meta(dir: &Path, meta: &Meta) -> Result<()> {
    let path = dir.join("meta.toml");
    let content = toml::to_string(meta).context("serializing meta.toml")?;
    write_atomic(&path, content).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let meta = Meta {
            created: "2024-01-01T00:00:00Z".to_string(),
            worktree: WorktreeMeta {
                path: "/worktrees/my-branch".to_string(),
                branch: "feat/my-branch".to_string(),
            },
        };
        write_meta(dir.path(), &meta).unwrap();
        let loaded = read_meta(dir.path()).unwrap();
        assert_eq!(loaded.created, meta.created);
        assert_eq!(loaded.worktree.path, meta.worktree.path);
        assert_eq!(loaded.worktree.branch, meta.worktree.branch);
        assert!(dir.path().join("meta.toml").exists());
    }

    #[test]
    fn read_meta_missing_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_meta(dir.path()).is_err());
    }
}
