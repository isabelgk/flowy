use std::path::{Path, PathBuf};

pub fn safe_name(branch: &str) -> String {
    branch.replace('/', "-")
}

// Live workstream state lives inside its worktree so tickets and notes sit next
// to the code they describe.
pub fn workstream_dir(root: &Path, name: &str) -> PathBuf {
    workstream_subdir(&crate::core::repo::worktrees_dir(root).join(name))
}

// Same path relative to an already-known worktree, for callers that have the
// worktree path but not the workstream name.
pub fn workstream_subdir(worktree: &Path) -> PathBuf {
    worktree.join(".local").join("workstream")
}

pub fn archived_workstream_dir(root: &Path, name: &str) -> PathBuf {
    crate::core::repo::archive_root(root).join(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn safe_name_replaces_slashes() {
        assert_eq!(safe_name("feat/foo"), "feat-foo");
    }

    #[test]
    fn safe_name_replaces_multiple_slashes() {
        assert_eq!(safe_name("a/b/c"), "a-b-c");
    }

    #[test]
    fn safe_name_no_slash_unchanged() {
        assert_eq!(safe_name("main"), "main");
    }

    #[test]
    fn safe_name_empty_string() {
        assert_eq!(safe_name(""), "");
    }

    #[test]
    fn workstream_dir_constructs_path() {
        let root = PathBuf::from("/repo");
        assert_eq!(
            workstream_dir(&root, "my-branch"),
            PathBuf::from("/repo/worktrees/my-branch/.local/workstream")
        );
    }

    #[test]
    fn workstream_subdir_matches_workstream_dir() {
        let root = PathBuf::from("/repo");
        assert_eq!(
            workstream_subdir(&root.join("worktrees").join("my-branch")),
            workstream_dir(&root, "my-branch")
        );
    }

    #[test]
    fn archived_workstream_dir_constructs_path() {
        let root = PathBuf::from("/repo");
        assert_eq!(
            archived_workstream_dir(&root, "my-branch"),
            PathBuf::from("/repo/notes/workstreams/archive/my-branch")
        );
    }
}
