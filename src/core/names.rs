use std::path::{Path, PathBuf};

pub fn safe_name(branch: &str) -> String {
    branch.replace('/', "-")
}

// Clap value parser for a workstream name argument. Returns the safe name, since
// that is what names the directory; `.` and `..` would resolve outside wt/.
pub fn parse_workstream_name(raw: &str) -> Result<String, String> {
    let name = safe_name(raw);
    match name.as_str() {
        "" => Err("workstream name cannot be empty".into()),
        "." | ".." => Err(format!("'{raw}' is not a valid workstream name")),
        _ => Ok(name),
    }
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
    fn parse_workstream_name_rejects_path_components() {
        assert_eq!(parse_workstream_name("feat/foo").unwrap(), "feat-foo");
        assert!(parse_workstream_name("").is_err());
        assert!(parse_workstream_name(".").is_err());
        assert!(parse_workstream_name("..").is_err());
    }

    #[test]
    fn workstream_dir_constructs_path() {
        let root = PathBuf::from("/repo");
        assert_eq!(
            workstream_dir(&root, "my-branch"),
            PathBuf::from("/repo/wt/my-branch/.local/workstream")
        );
    }

    #[test]
    fn workstream_subdir_matches_workstream_dir() {
        let root = PathBuf::from("/repo");
        assert_eq!(
            workstream_subdir(&root.join("wt").join("my-branch")),
            workstream_dir(&root, "my-branch")
        );
    }

    #[test]
    fn archived_workstream_dir_constructs_path() {
        let root = PathBuf::from("/repo");
        assert_eq!(
            archived_workstream_dir(&root, "my-branch"),
            PathBuf::from("/repo/.flowy/archive/my-branch")
        );
    }
}
