use std::path::{Path, PathBuf};

pub fn safe_name(branch: &str) -> String {
    branch.replace('/', "-")
}

pub fn workstream_dir(root: &Path, name: &str) -> PathBuf {
    crate::core::repo::notes_dir(root).join(name)
}
