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
