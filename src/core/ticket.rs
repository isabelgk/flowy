use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::Write as _;
use std::path::{Path, PathBuf};

// Bound on the search for a free ticket number, so a directory that somehow
// cannot accept a new file fails instead of looping.
const RACE_RETRY_LIMIT: u32 = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Pending,
    Complete,
    WontImplement,
}

#[derive(Debug, Clone, Serialize)]
pub struct Ticket {
    pub number: u32,
    pub title: String,
    pub status: Status,
    pub content: String,
}

#[derive(Deserialize)]
struct Frontmatter {
    status: Option<String>,
    title: Option<String>,
}

pub fn parse_ticket(path: &Path) -> Result<Ticket> {
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid ticket path: {}", path.display()))?;

    let stem = filename.strip_suffix(".md").unwrap_or(filename);
    let number: u32 = stem
        .parse()
        .with_context(|| format!("ticket filename '{filename}' has non-numeric name '{stem}'"))?;

    let raw =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;

    let (status, title, body) = if let Some(stripped) = raw.strip_prefix("+++\n") {
        if let Some(end) = stripped.find("\n+++\n") {
            let fm_str = &stripped[..end];
            let fm: Frontmatter = toml::from_str(fm_str)
                .with_context(|| format!("parsing frontmatter of '{filename}'"))?;
            let body = stripped[end + 5..].to_string();
            let status = match fm.status.as_deref().unwrap_or("pending") {
                "complete" => Status::Complete,
                "wont-implement" => Status::WontImplement,
                _ => Status::Pending,
            };
            let title = fm.title.ok_or_else(|| {
                anyhow::anyhow!("ticket '{filename}' frontmatter missing 'title'")
            })?;
            (status, title, body)
        } else {
            bail!("ticket '{filename}' has malformed frontmatter (no closing +++)")
        }
    } else {
        bail!("ticket '{filename}' has no frontmatter")
    };

    Ok(Ticket {
        number,
        title,
        status,
        content: body,
    })
}

pub fn load_tickets(workstream_dir: &Path) -> Result<Vec<Ticket>> {
    let tickets_dir = workstream_dir.join("tickets");
    if !tickets_dir.exists() {
        return Ok(Vec::new());
    }

    let mut tickets = Vec::new();
    for entry in std::fs::read_dir(&tickets_dir)
        .with_context(|| format!("reading {}", tickets_dir.display()))?
    {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            tickets.push(parse_ticket(&path)?);
        }
    }

    tickets.sort_by_key(|t| t.number);
    Ok(tickets)
}

pub fn ticket_path(workstream_dir: &Path, number: u32) -> PathBuf {
    workstream_dir
        .join("tickets")
        .join(format!("{number:03}.md"))
}

pub fn resolve_ticket<'a>(tickets: &'a [Ticket], id: &str) -> Result<&'a Ticket> {
    let n: u32 = id
        .parse()
        .map_err(|_| anyhow::anyhow!("ticket ID must be numeric, got {id:?}"))?;
    tickets
        .iter()
        .find(|t| t.number == n)
        .ok_or_else(|| anyhow::anyhow!("no ticket with number {n}"))
}

// Highest ticket number present, read from filenames rather than by parsing the
// tickets. A malformed ticket file must not lower this number, or the next add
// would reuse a number that is already taken.
pub fn highest_number(workstream_dir: &Path) -> u32 {
    let Ok(entries) = std::fs::read_dir(workstream_dir.join("tickets")) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".md")?.parse::<u32>().ok()
        })
        .max()
        .unwrap_or(0)
}

// Claims the next free number with `create_new`, so two adds racing on the same
// workstream cannot land on the same file: the loser sees AlreadyExists and
// moves to the next slot instead of overwriting the winner.
pub fn create_ticket(
    workstream_dir: &Path,
    title: &str,
    body: Option<&str>,
) -> Result<(u32, PathBuf)> {
    let tickets_dir = workstream_dir.join("tickets");
    std::fs::create_dir_all(&tickets_dir)
        .with_context(|| format!("creating {}", tickets_dir.display()))?;

    let content = render_new_ticket(title, body)?;
    let first = highest_number(workstream_dir) + 1;
    for number in first..first + RACE_RETRY_LIMIT {
        let path = ticket_path(workstream_dir, number);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(content.as_bytes())
                    .with_context(|| format!("writing {}", path.display()))?;
                return Ok((number, path));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("creating {}", path.display())),
        }
    }
    bail!(
        "no free ticket number in {first}..{}",
        first + RACE_RETRY_LIMIT
    )
}

fn render_new_ticket(title: &str, body: Option<&str>) -> Result<String> {
    #[derive(Serialize)]
    struct NewFrontmatter<'a> {
        status: &'a str,
        title: &'a str,
    }
    let fm = toml::to_string(&NewFrontmatter {
        status: "pending",
        title,
    })
    .context("serializing ticket frontmatter")?;
    let mut content = format!("+++\n{}\n+++\n", fm.trim_end_matches('\n'));
    if let Some(body) = body.map(str::trim_end).filter(|b| !b.is_empty()) {
        content.push('\n');
        content.push_str(body);
        content.push('\n');
    }
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_ticket(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn parse_ticket_pending() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(
            dir.path(),
            "001.md",
            "+++\nstatus = \"pending\"\ntitle = \"My ticket\"\n+++\n\nBody.\n",
        );
        let t = parse_ticket(&path).unwrap();
        assert_eq!(t.number, 1);
        assert_eq!(t.title, "My ticket");
        assert_eq!(t.status, Status::Pending);
    }

    #[test]
    fn parse_ticket_complete() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(
            dir.path(),
            "002.md",
            "+++\nstatus = \"complete\"\ntitle = \"Task\"\n+++\n",
        );
        let t = parse_ticket(&path).unwrap();
        assert_eq!(t.status, Status::Complete);
        assert_eq!(t.title, "Task");
    }

    #[test]
    fn parse_ticket_wont_implement() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(
            dir.path(),
            "003.md",
            "+++\nstatus = \"wont-implement\"\ntitle = \"Task\"\n+++\n",
        );
        let t = parse_ticket(&path).unwrap();
        assert_eq!(t.status, Status::WontImplement);
        assert_eq!(t.title, "Task");
    }

    #[test]
    fn parse_ticket_no_title_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(dir.path(), "001.md", "+++\nstatus = \"pending\"\n+++\n");
        let err = parse_ticket(&path).unwrap_err();
        assert!(err.to_string().contains("missing 'title'"));
    }

    #[test]
    fn parse_ticket_no_frontmatter_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(dir.path(), "001.md", "No frontmatter here.\n");
        let err = parse_ticket(&path).unwrap_err();
        assert!(err.to_string().contains("no frontmatter"));
    }

    #[test]
    fn parse_ticket_non_numeric_name_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(dir.path(), "abc.md", "# Title\n");
        let err = parse_ticket(&path).unwrap_err();
        assert!(err.to_string().contains("non-numeric name"));
    }

    fn make_ticket(number: u32) -> Ticket {
        Ticket {
            number,
            title: format!("Ticket {number}"),
            status: Status::Pending,
            content: String::new(),
        }
    }

    #[test]
    fn resolve_ticket_by_number_variants() {
        let tickets = vec![make_ticket(1), make_ticket(2)];
        assert_eq!(resolve_ticket(&tickets, "1").unwrap().number, 1);
        assert_eq!(resolve_ticket(&tickets, "01").unwrap().number, 1);
        assert_eq!(resolve_ticket(&tickets, "001").unwrap().number, 1);
        assert_eq!(resolve_ticket(&tickets, "2").unwrap().number, 2);
    }

    #[test]
    fn resolve_ticket_not_found_errors() {
        let tickets = vec![make_ticket(1)];
        assert!(resolve_ticket(&tickets, "99").is_err());
    }

    #[test]
    fn resolve_ticket_non_numeric_errors() {
        let tickets = vec![make_ticket(1)];
        let err = resolve_ticket(&tickets, "abc").unwrap_err();
        assert!(err.to_string().contains("must be numeric"));
    }
}

#[cfg(test)]
mod create_tests {
    use super::*;

    #[test]
    fn create_ticket_numbers_sequentially() {
        let dir = tempfile::tempdir().unwrap();
        let (n1, p1) = create_ticket(dir.path(), "First", None).unwrap();
        let (n2, _) = create_ticket(dir.path(), "Second", Some("Body.")).unwrap();
        assert_eq!((n1, n2), (1, 2));
        assert!(p1.ends_with("001.md"));
    }

    #[test]
    fn create_ticket_writes_frontmatter_and_body() {
        let dir = tempfile::tempdir().unwrap();
        let (_, path) = create_ticket(dir.path(), "My ticket", Some("Details.\n\n")).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            content,
            "+++\nstatus = \"pending\"\ntitle = \"My ticket\"\n+++\n\nDetails.\n"
        );
    }

    // The old allocator wrote unconditionally, so a ticket whose frontmatter did
    // not parse made `add` believe the queue was empty and overwrite 001.
    #[test]
    fn create_ticket_does_not_overwrite_an_unparseable_ticket() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("tickets")).unwrap();
        std::fs::write(dir.path().join("tickets/001.md"), "no frontmatter\n").unwrap();

        let (number, _) = create_ticket(dir.path(), "New", None).unwrap();

        assert_eq!(number, 2);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("tickets/001.md")).unwrap(),
            "no frontmatter\n"
        );
    }

    // Stands in for a racing writer: the slot the allocator is about to claim is
    // already taken, so it has to move past it rather than clobber it.
    #[test]
    fn create_ticket_steps_over_a_taken_slot() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("tickets")).unwrap();
        std::fs::write(dir.path().join("tickets/003.md"), "squatter\n").unwrap();
        std::fs::write(dir.path().join("tickets/x.md"), "not a ticket\n").unwrap();

        let (number, _) = create_ticket(dir.path(), "New", None).unwrap();

        assert_eq!(number, 4);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("tickets/003.md")).unwrap(),
            "squatter\n"
        );
    }

    #[test]
    fn highest_number_ignores_non_numeric_files() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(highest_number(dir.path()), 0);
        std::fs::create_dir_all(dir.path().join("tickets")).unwrap();
        std::fs::write(dir.path().join("tickets/007.md"), "").unwrap();
        std::fs::write(dir.path().join("tickets/notes.md"), "").unwrap();
        std::fs::write(dir.path().join("tickets/012.txt"), "").unwrap();
        assert_eq!(highest_number(dir.path()), 7);
    }
}
