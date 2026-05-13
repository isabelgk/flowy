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
    std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))
}
