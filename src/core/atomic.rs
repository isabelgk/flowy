use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

// Writes through a sibling temp file and a rename, so a crash or a full disk
// leaves either the old contents or the new, never a truncated file. It does not
// serialize writers: two read-modify-writes that overlap still lose one update.
pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    // Distinguishes temp files from threads of one process writing the same path.
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    // Renaming over a symlink would replace the link rather than its target.
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let name = path.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no file name")
    })?;
    let tmp = path.with_file_name(format!(
        ".{}.{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));

    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        if let Ok(existing) = std::fs::metadata(&path) {
            file.set_permissions(existing.permissions())?;
        }
        file.write_all(contents.as_ref())?;
        file.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_contents_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("001.md");
        std::fs::write(&path, "old contents that are longer\n").unwrap();

        write_atomic(&path, "new\n").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["001.md"]);
    }

    #[test]
    fn creates_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meta.toml");
        write_atomic(&path, "x = 1\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x = 1\n");
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real");
        let link = dir.path().join("link");
        std::fs::write(&target, "old\n").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        write_atomic(&link, "new\n").unwrap();

        assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new\n");
    }
}
