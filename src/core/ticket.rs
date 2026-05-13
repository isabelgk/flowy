use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::path::{Path, PathBuf};

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
    pub slug: String,
    pub title: String,
    pub status: Status,
    pub content: String,
}

pub fn parse_ticket(path: &Path) -> Result<Ticket> {
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid ticket path: {}", path.display()))?;

    let stem = filename.strip_suffix(".md").unwrap_or(filename);
    let (num_str, slug) = stem
        .split_once('-')
        .ok_or_else(|| anyhow::anyhow!("ticket filename '{filename}' missing number prefix"))?;
    let number: u32 = num_str.parse().with_context(|| {
        format!("ticket filename '{filename}' has non-numeric prefix '{num_str}'")
    })?;
    let slug = slug.to_string();

    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;

    let heading = content
        .lines()
        .find(|l| l.starts_with("# "))
        .ok_or_else(|| anyhow::anyhow!("ticket '{filename}' has no h1 heading"))?
        .trim_start_matches("# ");

    let (status, title) = if let Some(t) = heading.strip_prefix("completed: ") {
        (Status::Complete, t.to_string())
    } else if let Some(t) = heading.strip_prefix("wont-implement: ") {
        (Status::WontImplement, t.to_string())
    } else {
        (Status::Pending, heading.to_string())
    };

    Ok(Ticket {
        number,
        slug,
        title,
        status,
        content,
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

pub fn ticket_path(workstream_dir: &Path, number: u32, slug: &str) -> PathBuf {
    workstream_dir
        .join("tickets")
        .join(format!("{number:03}-{slug}.md"))
}

pub fn resolve_ticket<'a>(tickets: &'a [Ticket], id: &str) -> Result<&'a Ticket> {
    // Numeric lookup: "1", "01", "001" all resolve to number 1
    if let Ok(n) = id.parse::<u32>() {
        return tickets
            .iter()
            .find(|t| t.number == n)
            .ok_or_else(|| anyhow::anyhow!("no ticket with number {n}"));
    }

    // Slug prefix match
    let matches: Vec<&Ticket> = tickets.iter().filter(|t| t.slug.starts_with(id)).collect();
    match matches.len() {
        0 => bail!("no ticket matching {id:?}"),
        1 => Ok(matches[0]),
        _ => bail!(
            "ambiguous id {id:?} matches {} tickets: {}",
            matches.len(),
            matches
                .iter()
                .map(|t| t.slug.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
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
        let path = write_ticket(dir.path(), "001-my-ticket.md", "# My ticket\n\nBody.\n");
        let t = parse_ticket(&path).unwrap();
        assert_eq!(t.number, 1);
        assert_eq!(t.slug, "my-ticket");
        assert_eq!(t.title, "My ticket");
        assert_eq!(t.status, Status::Pending);
    }

    #[test]
    fn parse_ticket_complete() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(dir.path(), "002-task.md", "# completed: Task\n");
        let t = parse_ticket(&path).unwrap();
        assert_eq!(t.status, Status::Complete);
        assert_eq!(t.title, "Task");
    }

    #[test]
    fn parse_ticket_wont_implement() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(dir.path(), "003-task.md", "# wont-implement: Task\n");
        let t = parse_ticket(&path).unwrap();
        assert_eq!(t.status, Status::WontImplement);
        assert_eq!(t.title, "Task");
    }

    #[test]
    fn parse_ticket_no_h1_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(dir.path(), "001-task.md", "No heading here.\n");
        let err = parse_ticket(&path).unwrap_err();
        assert!(err.to_string().contains("no h1"));
    }

    #[test]
    fn parse_ticket_no_separator_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(dir.path(), "badname.md", "# Title\n");
        let err = parse_ticket(&path).unwrap_err();
        assert!(err.to_string().contains("missing number prefix"));
    }

    #[test]
    fn parse_ticket_non_numeric_prefix_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_ticket(dir.path(), "abc-slug.md", "# Title\n");
        let err = parse_ticket(&path).unwrap_err();
        assert!(err.to_string().contains("non-numeric prefix"));
    }

    fn make_ticket(number: u32, slug: &str) -> Ticket {
        Ticket {
            number,
            slug: slug.to_string(),
            title: slug.to_string(),
            status: Status::Pending,
            content: String::new(),
        }
    }

    #[test]
    fn resolve_ticket_by_number_variants() {
        let tickets = vec![make_ticket(1, "my-task"), make_ticket(2, "other-task")];
        assert_eq!(resolve_ticket(&tickets, "1").unwrap().number, 1);
        assert_eq!(resolve_ticket(&tickets, "01").unwrap().number, 1);
        assert_eq!(resolve_ticket(&tickets, "001").unwrap().number, 1);
        assert_eq!(resolve_ticket(&tickets, "2").unwrap().number, 2);
    }

    #[test]
    fn resolve_ticket_by_slug_prefix() {
        let tickets = vec![make_ticket(1, "my-task"), make_ticket(2, "other-task")];
        assert_eq!(resolve_ticket(&tickets, "my").unwrap().number, 1);
        assert_eq!(resolve_ticket(&tickets, "my-task").unwrap().number, 1);
        assert_eq!(resolve_ticket(&tickets, "other").unwrap().number, 2);
    }

    #[test]
    fn resolve_ticket_not_found_errors() {
        let tickets = vec![make_ticket(1, "my-task")];
        assert!(resolve_ticket(&tickets, "zzz").is_err());
        assert!(resolve_ticket(&tickets, "99").is_err());
    }

    #[test]
    fn resolve_ticket_ambiguous_errors() {
        let tickets = vec![make_ticket(1, "my-task"), make_ticket(2, "my-other")];
        let err = resolve_ticket(&tickets, "my").unwrap_err();
        assert!(err.to_string().contains("ambiguous"));
    }
}
