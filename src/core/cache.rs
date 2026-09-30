use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::atomic::write_atomic;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WorktreeCache {
    pub head_sha: String,
    pub commit_ts: i64,
    pub tickets_dir_mtime: u64,
    pub tickets_pending: usize,
    pub tickets_total: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Cache {
    pub worktrees: HashMap<String, WorktreeCache>,
}

pub fn cache_path(root: &Path) -> PathBuf {
    root.join(".flowy").join("cache.json")
}

pub fn load_cache(root: &Path) -> Cache {
    let path = cache_path(root);
    let Ok(bytes) = std::fs::read(&path) else {
        return Cache::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn save_cache(root: &Path, cache: &Cache) {
    let path = cache_path(root);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_vec_pretty(cache) {
        let _ = write_atomic(&path, json);
    }
}
