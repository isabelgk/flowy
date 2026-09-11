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

// Ordered high-to-low: deriving Ord over the declaration order is what makes
// `sort_by_key` put the most urgent tickets first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Priority {
    High,
    // A ticket that never opted into a priority ranks here, so the field can stay
    // absent from frontmatter that predates it.
    #[default]
    Medium,
    Low,
}

impl Priority {
    pub const VALUES: [&'static str; 3] = ["high", "medium", "low"];

    pub fn as_str(&self) -> &'static str {
        match self {
            Priority::High => "high",
            Priority::Medium => "medium",
            Priority::Low => "low",
        }
    }
}

impl std::str::FromStr for Priority {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "high" => Ok(Priority::High),
            "medium" => Ok(Priority::Medium),
            "low" => Ok(Priority::Low),
            other => bail!("unknown priority {other:?}, expected high, medium, or low"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Ticket {
    pub number: u32,
    pub title: String,
    pub status: Status,
    pub priority: Priority,
    pub content: String,
}

#[derive(Deserialize)]
struct Frontmatter {
    status: Option<String>,
    title: Option<String>,
    priority: Option<String>,
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

    let (status, title, priority, body) = if let Some(stripped) = raw.strip_prefix("+++\n") {
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
            let priority = fm
                .priority
                .as_deref()
                .and_then(|p| p.parse().ok())
                .unwrap_or_default();
            (status, title, priority, body)
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
        priority,
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

    tickets.sort_by_key(|t| (t.priority, t.number));
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
    priority: Option<Priority>,
    body: Option<&str>,
) -> Result<(u32, PathBuf)> {
    let tickets_dir = workstream_dir.join("tickets");
    std::fs::create_dir_all(&tickets_dir)
        .with_context(|| format!("creating {}", tickets_dir.display()))?;

    let content = render_new_ticket(title, priority, body)?;
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

// `priority` of None leaves the field out entirely, so a ticket that does not
// care about ranking keeps the frontmatter it has always had.
fn render_new_ticket(
    title: &str,
    priority: Option<Priority>,
    body: Option<&str>,
) -> Result<String> {
    #[derive(Serialize)]
    struct NewFrontmatter<'a> {
        status: &'a str,
        title: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        priority: Option<&'a str>,
    }
    let fm = toml::to_string(&NewFrontmatter {
        status: "pending",
        title,
        priority: priority.map(|p| p.as_str()),
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

pub fn write_priority(path: &Path, priority: Priority) -> Result<()> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let updated = rewrite_priority(&content, priority)?;
    std::fs::write(path, updated).with_context(|| format!("writing {}", path.display()))
}

// Rewrites an existing `priority` key in place and otherwise appends one, so a
// ticket written before priorities existed keeps its other frontmatter and its
// body byte for byte.
pub fn rewrite_priority(content: &str, priority: Priority) -> Result<String> {
    let stripped = content
        .strip_prefix("+++\n")
        .ok_or_else(|| anyhow::anyhow!("ticket has no frontmatter"))?;
    let end = stripped
        .find("\n+++\n")
        .ok_or_else(|| anyhow::anyhow!("ticket has malformed frontmatter (no closing +++)"))?;

    let body = &stripped[end + 5..];
    let line = format!("priority = \"{}\"", priority.as_str());

    let mut replaced = false;
    let mut fm: Vec<String> = stripped[..end]
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("priority") && l.contains('=') {
                replaced = true;
                line.clone()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !replaced {
        fm.push(line);
    }

    Ok(format!("+++\n{}\n+++\n{body}", fm.join("\n")))
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
            priority: Priority::Medium,
            content: String::new(),
        }
    }

    #[test]
    fn parse_ticket_reads_priority() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(
            dir.path(),
            "001.md",
            "+++\nstatus = \"pending\"\ntitle = \"T\"\npriority = \"high\"\n+++\n",
        );
        assert_eq!(parse_ticket(&path).unwrap().priority, Priority::High);
    }

    #[test]
    fn parse_ticket_without_priority_is_medium() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(
            dir.path(),
            "001.md",
            "+++\nstatus = \"pending\"\ntitle = \"T\"\n+++\n",
        );
        assert_eq!(parse_ticket(&path).unwrap().priority, Priority::Medium);
    }

    // Same leniency as an unrecognized `status`: a hand-edited typo must not make
    // the ticket unreadable.
    #[test]
    fn parse_ticket_unknown_priority_is_medium() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(
            dir.path(),
            "001.md",
            "+++\nstatus = \"pending\"\ntitle = \"T\"\npriority = \"urgent\"\n+++\n",
        );
        assert_eq!(parse_ticket(&path).unwrap().priority, Priority::Medium);
    }

    #[test]
    fn load_tickets_sorts_by_priority_then_number() {
        let dir = tempfile::tempdir().unwrap();
        let tickets = dir.path().join("tickets");
        fs::create_dir_all(&tickets).unwrap();
        for (name, priority) in [
            ("001.md", "low"),
            ("002.md", "high"),
            ("003.md", "medium"),
            ("004.md", "high"),
        ] {
            write_ticket(
                &tickets,
                name,
                &format!(
                    "+++\nstatus = \"pending\"\ntitle = \"T\"\npriority = \"{priority}\"\n+++\n"
                ),
            );
        }

        let numbers: Vec<u32> = load_tickets(dir.path())
            .unwrap()
            .iter()
            .map(|t| t.number)
            .collect();
        assert_eq!(numbers, vec![2, 4, 3, 1]);
    }

    #[test]
    fn rewrite_priority_replaces_an_existing_key() {
        let input = "+++\nstatus = \"pending\"\npriority = \"low\"\ntitle = \"T\"\n+++\n\nBody.\n";
        let out = rewrite_priority(input, Priority::High).unwrap();
        assert_eq!(
            out,
            "+++\nstatus = \"pending\"\npriority = \"high\"\ntitle = \"T\"\n+++\n\nBody.\n"
        );
    }

    #[test]
    fn rewrite_priority_appends_when_absent() {
        let input = "+++\nstatus = \"pending\"\ntitle = \"T\"\n+++\n\nBody.\n";
        let out = rewrite_priority(input, Priority::Low).unwrap();
        assert_eq!(
            out,
            "+++\nstatus = \"pending\"\ntitle = \"T\"\npriority = \"low\"\n+++\n\nBody.\n"
        );
    }

    #[test]
    fn rewrite_priority_rejects_missing_frontmatter() {
        assert!(rewrite_priority("no frontmatter\n", Priority::High).is_err());
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
        let (n1, p1) = create_ticket(dir.path(), "First", None, None).unwrap();
        let (n2, _) = create_ticket(dir.path(), "Second", None, Some("Body.")).unwrap();
        assert_eq!((n1, n2), (1, 2));
        assert!(p1.ends_with("001.md"));
    }

    #[test]
    fn create_ticket_writes_frontmatter_and_body() {
        let dir = tempfile::tempdir().unwrap();
        let (_, path) = create_ticket(dir.path(), "My ticket", None, Some("Details.\n\n")).unwrap();
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

        let (number, _) = create_ticket(dir.path(), "New", None, None).unwrap();

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

        let (number, _) = create_ticket(dir.path(), "New", None, None).unwrap();

        assert_eq!(number, 4);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("tickets/003.md")).unwrap(),
            "squatter\n"
        );
    }

    #[test]
    fn create_ticket_writes_priority_only_when_given() {
        let dir = tempfile::tempdir().unwrap();
        let (_, plain) = create_ticket(dir.path(), "Plain", None, None).unwrap();
        let (_, ranked) = create_ticket(dir.path(), "Ranked", Some(Priority::High), None).unwrap();

        assert_eq!(
            std::fs::read_to_string(&plain).unwrap(),
            "+++\nstatus = \"pending\"\ntitle = \"Plain\"\n+++\n"
        );
        assert_eq!(
            std::fs::read_to_string(&ranked).unwrap(),
            "+++\nstatus = \"pending\"\ntitle = \"Ranked\"\npriority = \"high\"\n+++\n"
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
