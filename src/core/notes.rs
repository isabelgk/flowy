use anyhow::{Context, Result, bail};
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};

// A note addressed by one of these aliases resolves to the same file whether the
// caller spells it "planning" or "planning.md".
const ALIASES: [(&str, &str); 4] = [
    ("planning", "planning.md"),
    ("next", "next.md"),
    ("status", "status.md"),
    ("meta", "meta.toml"),
];

// The files a workstream is expected to have, in the order `notes ls` reports
// them. Absence is normal: a workstream has no planning.md until `plan` runs,
// and none again after a milestone freezes it.
pub const WELL_KNOWN: [&str; 4] = ["meta.toml", "planning.md", "next.md", "status.md"];

#[derive(Debug, serde::Serialize)]
pub struct Milestone {
    pub number: u32,
    pub slug: String,
    pub dir: PathBuf,
    pub frozen: Vec<String>,
}

// status.md is an append-only log written by every command that changes a
// workstream, so the entry format lives here rather than at each call site.
pub fn append_log(ws_dir: &Path, entry: &str) -> Result<()> {
    let path = ws_dir.join("status.md");
    let line = format!(
        "- {} {entry}\n",
        chrono::Utc::now().format("%Y-%m-%dT%H:%MZ")
    );
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))?
        .write_all(line.as_bytes())?;
    Ok(())
}

// Notes are addressed by alias or by a path relative to the workstream
// directory ("milestones/001-parser/summary.md"). Absolute names and `..` are
// refused: the workstream directory is the write boundary, and a traversing
// name would put caller-supplied text anywhere on disk.
pub fn resolve_note(ws_dir: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty() {
        bail!("note name is empty");
    }
    if let Some((_, file)) = ALIASES.iter().find(|(alias, _)| *alias == name) {
        return Ok(ws_dir.join(file));
    }
    let rel = Path::new(name);
    if !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        bail!("note name must be a relative path inside the workstream, got {name:?}");
    }
    Ok(ws_dir.join(rel))
}

pub fn write_note(path: &Path, body: &str) -> Result<()> {
    let body = normalize(body)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, &body).with_context(|| format!("writing {}", path.display()))
}

pub fn append_note(path: &Path, body: &str) -> Result<()> {
    let body = normalize(body)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    // A blank line between blocks, so appended prose does not run into the last
    // paragraph of what was already there.
    let separator = match () {
        _ if existing.is_empty() => "",
        _ if existing.ends_with("\n\n") => "",
        _ if existing.ends_with('\n') => "\n",
        _ => "\n\n",
    };
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .write_all(format!("{separator}{body}").as_bytes())
        .with_context(|| format!("writing {}", path.display()))
}

// Markdown files end with exactly one newline. An empty body is refused rather
// than truncating a note, since it almost always means an empty pipe.
fn normalize(body: &str) -> Result<String> {
    let trimmed = body.trim_end();
    if trimmed.is_empty() {
        bail!("refusing to write an empty note body");
    }
    Ok(format!("{trimmed}\n"))
}

pub fn milestones_dir(ws_dir: &Path) -> PathBuf {
    ws_dir.join("milestones")
}

// One past the highest `<nnn>-<slug>` directory present. Names that do not
// start with a number are ignored so a README or stray directory cannot stall
// numbering.
pub fn next_milestone_number(ws_dir: &Path) -> u32 {
    let Ok(entries) = std::fs::read_dir(milestones_dir(ws_dir)) else {
        return 1;
    };
    let highest = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let digits: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
            digits.parse::<u32>().ok()
        })
        .max()
        .unwrap_or(0);
    highest + 1
}

pub fn slugify(raw: &str) -> Result<String> {
    let mut out = String::new();
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let slug = out.trim_matches('-').to_string();
    if slug.is_empty() {
        bail!("slug {raw:?} has no alphanumeric characters");
    }
    Ok(slug)
}

// Freezes the current plan under `milestones/<nnn>-<slug>/`. The plan is moved
// rather than copied: two live copies of a finished plan is the confusion the
// milestone boundary exists to prevent, and the next planning run would
// overwrite the top-level one anyway. The summary is left to the caller, and
// tickets/ deliberately does not move - one numbering sequence spans the life
// of the workstream, and the plans and journals cite those numbers.
pub fn freeze_milestone(ws_dir: &Path, slug: &str) -> Result<Milestone> {
    let slug = slugify(slug)?;
    let planning = ws_dir.join("planning.md");
    if !planning.exists() {
        bail!("no planning.md in {} - nothing to freeze", ws_dir.display());
    }

    let number = next_milestone_number(ws_dir);
    let dir = milestones_dir(ws_dir).join(format!("{number:03}-{slug}"));
    if dir.exists() {
        bail!("{} already exists", dir.display());
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let mut frozen = Vec::new();
    for name in ["planning.md", "next.md"] {
        let src = ws_dir.join(name);
        if !src.exists() {
            continue;
        }
        std::fs::rename(&src, dir.join(name))
            .with_context(|| format!("moving {} into {}", src.display(), dir.display()))?;
        frozen.push(name.to_string());
    }

    append_log(ws_dir, &format!("milestone {number:03}-{slug} frozen"))?;

    Ok(Milestone {
        number,
        slug,
        dir,
        frozen,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_note_expands_aliases() {
        let ws = Path::new("/ws");
        assert_eq!(
            resolve_note(ws, "planning").unwrap(),
            ws.join("planning.md")
        );
        assert_eq!(resolve_note(ws, "next").unwrap(), ws.join("next.md"));
        assert_eq!(resolve_note(ws, "status").unwrap(), ws.join("status.md"));
        assert_eq!(resolve_note(ws, "meta").unwrap(), ws.join("meta.toml"));
    }

    #[test]
    fn resolve_note_accepts_relative_paths() {
        let ws = Path::new("/ws");
        assert_eq!(
            resolve_note(ws, "milestones/001-parser/summary.md").unwrap(),
            ws.join("milestones/001-parser/summary.md")
        );
        assert_eq!(
            resolve_note(ws, "planning.md").unwrap(),
            ws.join("planning.md")
        );
    }

    #[test]
    fn resolve_note_rejects_traversal() {
        let ws = Path::new("/ws");
        for name in [
            "..",
            "../outside.md",
            "a/../../b.md",
            "/etc/passwd",
            "./a.md",
            "",
        ] {
            assert!(
                resolve_note(ws, name).is_err(),
                "{name:?} should be rejected"
            );
        }
    }

    #[test]
    fn write_note_creates_parents_and_one_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("milestones/001-x/summary.md");
        write_note(&path, "# Summary\n\n\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# Summary\n");
    }

    #[test]
    fn write_note_refuses_empty_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("planning.md");
        std::fs::write(&path, "keep me\n").unwrap();
        assert!(write_note(&path, "  \n\n").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep me\n");
    }

    #[test]
    fn append_note_separates_blocks_with_one_blank_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("planning.md");
        write_note(&path, "First.").unwrap();
        append_note(&path, "Second.").unwrap();
        append_note(&path, "Third.").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "First.\n\nSecond.\n\nThird.\n"
        );
    }

    #[test]
    fn append_note_creates_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("next.md");
        append_note(&path, "Only entry.").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Only entry.\n");
    }

    #[test]
    fn slugify_normalizes() {
        assert_eq!(slugify("No Lua").unwrap(), "no-lua");
        assert_eq!(slugify("parser/rewrite").unwrap(), "parser-rewrite");
        assert_eq!(slugify("--Foo__Bar--").unwrap(), "foo-bar");
        assert!(slugify("///").is_err());
    }

    #[test]
    fn next_milestone_number_skips_non_numeric_names() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        assert_eq!(next_milestone_number(ws), 1);
        std::fs::create_dir_all(ws.join("milestones/001-a")).unwrap();
        std::fs::create_dir_all(ws.join("milestones/002-b")).unwrap();
        std::fs::write(ws.join("milestones/README.md"), "index\n").unwrap();
        assert_eq!(next_milestone_number(ws), 3);
    }

    #[test]
    fn freeze_milestone_moves_plan_and_logs() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        std::fs::write(ws.join("planning.md"), "the plan\n").unwrap();
        std::fs::write(ws.join("next.md"), "the board\n").unwrap();

        let ms = freeze_milestone(ws, "No Lua").unwrap();

        assert_eq!(ms.number, 1);
        assert_eq!(ms.slug, "no-lua");
        assert_eq!(ms.frozen, vec!["planning.md", "next.md"]);
        assert_eq!(
            std::fs::read_to_string(ms.dir.join("planning.md")).unwrap(),
            "the plan\n"
        );
        assert!(!ws.join("planning.md").exists(), "plan must move, not copy");
        assert!(!ws.join("next.md").exists());
        assert!(
            std::fs::read_to_string(ws.join("status.md"))
                .unwrap()
                .contains("milestone 001-no-lua frozen")
        );
    }

    #[test]
    fn freeze_milestone_without_next_md() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        std::fs::write(ws.join("planning.md"), "the plan\n").unwrap();
        let ms = freeze_milestone(ws, "solo").unwrap();
        assert_eq!(ms.frozen, vec!["planning.md"]);
    }

    #[test]
    fn freeze_milestone_requires_a_plan() {
        let dir = tempfile::tempdir().unwrap();
        let err = freeze_milestone(dir.path(), "x").unwrap_err();
        assert!(err.to_string().contains("nothing to freeze"));
    }

    #[test]
    fn freeze_milestone_leaves_tickets_alone() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        std::fs::create_dir_all(ws.join("tickets")).unwrap();
        std::fs::write(ws.join("tickets/001.md"), "ticket\n").unwrap();
        std::fs::write(ws.join("planning.md"), "the plan\n").unwrap();

        freeze_milestone(ws, "x").unwrap();

        assert!(ws.join("tickets/001.md").exists());
    }
}
