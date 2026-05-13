use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use comfy_table::{Cell, Color, ContentArrangement, Table};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use owo_colors::OwoColorize;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::core::{
    meta::read_meta,
    names::{safe_name, workstream_dir},
    repo::{find_repo_root, notes_dir},
    ticket::{Status, Ticket, load_tickets, resolve_ticket, ticket_path},
};
use crate::output::print_json;

pub fn dispatch(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("list", sub)) => list(sub),
        Some(("show", sub)) => show(sub),
        Some(("add", sub)) => add(sub),
        Some(("done", sub)) => done(sub),
        Some(("journal", sub)) => journal(sub),
        Some(("edit", sub)) => edit(sub),
        Some(("search", sub)) => search(sub),
        Some(("status", sub)) => status(sub),
        _ => Ok(()),
    }
}

fn get_repo_root(matches: &ArgMatches) -> Result<PathBuf> {
    if let Some(path) = matches.get_one::<String>("repo-root") {
        Ok(PathBuf::from(path))
    } else {
        find_repo_root()
    }
}

// Resolve workstream name from -w flag or current git branch.
fn resolve_workstream(matches: &ArgMatches, root: &Path) -> Result<(String, PathBuf)> {
    let name = if let Some(n) = matches.get_one::<String>("workstream") {
        safe_name(n)
    } else {
        let out = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(root)
            .output()
            .context("running git branch --show-current")?;
        let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if branch.is_empty() {
            bail!("detached HEAD — cannot infer workstream; use -w NAME");
        }
        safe_name(&branch)
    };
    let dir = workstream_dir(root, &name);
    Ok((name, dir))
}

fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_hyphen = true;
    for c in title.to_lowercase().chars() {
        if c.is_alphanumeric() {
            slug.push(c);
            prev_hyphen = false;
        } else if !prev_hyphen {
            slug.push('-');
            prev_hyphen = true;
        }
    }
    if slug.ends_with('-') {
        slug.pop();
    }
    slug
}

fn filter_tickets(tickets: Vec<Ticket>, filter: &str) -> Vec<Ticket> {
    match filter {
        "all" => tickets,
        "complete" => tickets
            .into_iter()
            .filter(|t| t.status == Status::Complete)
            .collect(),
        "wont-implement" => tickets
            .into_iter()
            .filter(|t| t.status == Status::WontImplement)
            .collect(),
        _ => tickets
            .into_iter()
            .filter(|t| t.status == Status::Pending)
            .collect(),
    }
}

fn append_status_entry(ws_dir: &Path, entry: &str) -> Result<()> {
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

fn print_ticket_table(section: &str, tickets: &[Ticket]) {
    if !section.is_empty() {
        println!("\n{}", section.bold());
    }
    if tickets.is_empty() {
        println!("  (none)");
        return;
    }
    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(["#", "Title", "Status"]);
    for t in tickets {
        let (label, color) = match t.status {
            Status::Pending => ("pending", Color::Yellow),
            Status::Complete => ("complete", Color::Green),
            Status::WontImplement => ("wont-implement", Color::DarkGrey),
        };
        table.add_row([
            Cell::new(format!("{:03}", t.number)),
            Cell::new(&t.title),
            Cell::new(label).fg(color),
        ]);
    }
    println!("{table}");
}

fn list(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let filter = matches
        .get_one::<String>("status")
        .map(|s| s.as_str())
        .unwrap_or("pending");

    if matches.get_one::<String>("workstream").is_some() {
        let (ws_name, ws_dir) = resolve_workstream(matches, &root)?;
        let tickets = filter_tickets(load_tickets(&ws_dir)?, filter);
        if json {
            return print_json(&tickets);
        }
        print_ticket_table(&ws_name, &tickets);
    } else {
        // All workstreams grouped
        let notes = notes_dir(&root);
        let mut groups: Vec<(String, Vec<Ticket>)> = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&notes) {
            let mut dirs: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            dirs.sort();

            for dir in dirs {
                let name = match dir.file_name().and_then(|n| n.to_str()) {
                    Some(n) if n != "archive" => n.to_string(),
                    _ => continue,
                };
                let tickets = filter_tickets(load_tickets(&dir).unwrap_or_default(), filter);
                if !tickets.is_empty() {
                    groups.push((name, tickets));
                }
            }
        }

        if json {
            let val: Vec<serde_json::Value> = groups
                .iter()
                .map(|(name, tickets)| serde_json::json!({"workstream": name, "tickets": tickets}))
                .collect();
            return print_json(&val);
        }

        if groups.is_empty() {
            println!("No tickets found.");
        } else {
            for (name, tickets) in &groups {
                print_ticket_table(name, tickets);
            }
        }
    }

    Ok(())
}

fn render_ticket_content(content: &str) {
    let mut in_code = false;
    for line in content.lines() {
        if line.starts_with("```") {
            in_code = !in_code;
            println!("{}", line.dimmed());
        } else if in_code {
            println!("{}", line.dimmed());
        } else if let Some(h) = line.strip_prefix("### ") {
            println!("{}", h.bold());
        } else if let Some(h) = line.strip_prefix("## ") {
            println!("{}", h.bold());
        } else if let Some(h) = line.strip_prefix("# ") {
            println!("{}", h.bold());
        } else {
            println!("{line}");
        }
    }
}

fn show(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let id = matches.get_one::<String>("id").unwrap();

    let (_, ws_dir) = resolve_workstream(matches, &root)?;
    let tickets = load_tickets(&ws_dir)?;
    let ticket = resolve_ticket(&tickets, id)?;

    if json {
        return print_json(ticket);
    }

    render_ticket_content(&ticket.content);
    Ok(())
}

fn add(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let title = matches.get_one::<String>("title").unwrap();
    let description = matches.get_one::<String>("description");

    let (_, ws_dir) = resolve_workstream(matches, &root)?;

    std::fs::create_dir_all(ws_dir.join("tickets"))
        .with_context(|| format!("creating tickets dir in {}", ws_dir.display()))?;

    let existing = load_tickets(&ws_dir).unwrap_or_default();
    let number = existing.iter().map(|t| t.number).max().unwrap_or(0) + 1;
    let slug = slugify(title);
    let path = ticket_path(&ws_dir, number, &slug);

    let mut content = format!("# {title}\n");
    if let Some(desc) = description {
        content.push('\n');
        content.push_str(desc);
        content.push('\n');
    }
    std::fs::write(&path, &content).with_context(|| format!("writing {}", path.display()))?;

    append_status_entry(&ws_dir, &format!("added {number:03}-{slug} ({title})"))?;

    if json {
        return print_json(&serde_json::json!({
            "number": number,
            "slug": slug,
            "title": title,
            "path": path.to_string_lossy(),
        }));
    }
    println!("Created {number:03}-{slug}");
    Ok(())
}

fn done(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let id = matches.get_one::<String>("id").unwrap();
    let summary = matches.get_one::<String>("summary");
    let wont_impl = matches.get_flag("wont-implement");
    let reopen = matches.get_flag("reopen");

    let (_, ws_dir) = resolve_workstream(matches, &root)?;
    let tickets = load_tickets(&ws_dir)?;
    let ticket = resolve_ticket(&tickets, id)?;

    let path = ticket_path(&ws_dir, ticket.number, &ticket.slug);
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    let new_content =
        rewrite_h1_and_append(&content, reopen, wont_impl, summary.map(|s| s.as_str()));
    std::fs::write(&path, &new_content).with_context(|| format!("writing {}", path.display()))?;

    let action = if reopen {
        "reopen"
    } else if wont_impl {
        "wont-implement"
    } else {
        "done"
    };
    append_status_entry(
        &ws_dir,
        &format!("{action} {:03}-{}", ticket.number, ticket.slug),
    )?;

    if json {
        return print_json(&serde_json::json!({
            "number": ticket.number,
            "slug": ticket.slug,
            "action": action,
        }));
    }
    println!("{action} {:03}-{}", ticket.number, ticket.slug);
    Ok(())
}

fn rewrite_h1_and_append(
    content: &str,
    reopen: bool,
    wont_impl: bool,
    summary: Option<&str>,
) -> String {
    let mut result = String::new();
    let mut h1_done = false;

    for line in content.lines() {
        if !h1_done && line.starts_with("# ") {
            let raw = &line[2..];
            // Strip any existing status prefix to get the bare title
            let title = raw
                .strip_prefix("completed: ")
                .or_else(|| raw.strip_prefix("wont-implement: "))
                .unwrap_or(raw);
            let prefix = if reopen {
                ""
            } else if wont_impl {
                "wont-implement: "
            } else {
                "completed: "
            };
            result.push_str(&format!("# {prefix}{title}\n"));
            h1_done = true;
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    if let Some(s) = summary {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(&format!("\n## Summary\n\n{s}\n"));
    }

    result
}

fn journal(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let id = matches.get_one::<String>("id").unwrap();
    let text = matches.get_one::<String>("text").unwrap();
    let heading = matches.get_one::<String>("heading");

    let (_, ws_dir) = resolve_workstream(matches, &root)?;
    let tickets = load_tickets(&ws_dir)?;
    let ticket = resolve_ticket(&tickets, id)?;

    let path = ticket_path(&ws_dir, ticket.number, &ticket.slug);
    let mut content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;

    // Create ## Journal section if absent
    if !content.contains("\n## Journal\n") && !content.starts_with("## Journal\n") {
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str("\n## Journal\n");
    }

    let ts = chrono::Utc::now().to_rfc3339();
    let header = match heading {
        Some(h) => format!("### [{ts}] {h}"),
        None => format!("### [{ts}]"),
    };
    content.push_str(&format!("\n{header}\n\n{text}\n"));

    std::fs::write(&path, &content).with_context(|| format!("writing {}", path.display()))?;

    println!(
        "Journal entry added to {:03}-{}",
        ticket.number, ticket.slug
    );
    Ok(())
}

fn edit(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let id = matches.get_one::<String>("id").unwrap();

    let (_, ws_dir) = resolve_workstream(matches, &root)?;
    let tickets = load_tickets(&ws_dir)?;
    let ticket = resolve_ticket(&tickets, id)?;

    let path = ticket_path(&ws_dir, ticket.number, &ticket.slug);
    let editor = std::env::var("EDITOR").context("$EDITOR is not set")?;

    let status = Command::new(&editor)
        .arg(&path)
        .status()
        .with_context(|| format!("running {editor}"))?;
    if !status.success() {
        bail!("editor exited with non-zero status");
    }

    Ok(())
}

struct SearchTicket {
    workstream: String,
    number: u32,
    slug: String,
    title: String,
    content: String,
}

fn collect_search_tickets(matches: &ArgMatches, root: &Path) -> Result<Vec<SearchTicket>> {
    if matches.get_one::<String>("workstream").is_some() {
        let (ws_name, ws_dir) = resolve_workstream(matches, root)?;
        return Ok(load_tickets(&ws_dir)?
            .into_iter()
            .map(|t| SearchTicket {
                workstream: ws_name.clone(),
                number: t.number,
                slug: t.slug,
                title: t.title,
                content: t.content,
            })
            .collect());
    }

    let notes = notes_dir(root);
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&notes) {
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for dir in dirs {
            let name = match dir.file_name().and_then(|n| n.to_str()) {
                Some(n) if n != "archive" => n.to_string(),
                _ => continue,
            };
            for t in load_tickets(&dir).unwrap_or_default() {
                result.push(SearchTicket {
                    workstream: name.clone(),
                    number: t.number,
                    slug: t.slug,
                    title: t.title,
                    content: t.content,
                });
            }
        }
    }
    Ok(result)
}

fn score_tickets<'a>(tickets: &'a [SearchTicket], query: &str) -> Vec<(&'a SearchTicket, u32)> {
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    let mut results: Vec<(&SearchTicket, u32)> = Vec::new();

    for t in tickets {
        // Score against title + full content so body matches rank too
        let haystack = format!("{}\n{}", t.title, t.content);
        let hs = Utf32Str::new(&haystack, &mut buf);
        if let Some(score) = pattern.score(hs, &mut matcher) {
            results.push((t, score));
        }
    }

    results.sort_by(|a, b| b.1.cmp(&a.1));
    results
}

fn highlight_title(title: &str, indices: &[u32]) -> String {
    let mut out = String::with_capacity(title.len() * 2);
    for (i, c) in title.chars().enumerate() {
        if indices.binary_search(&(i as u32)).is_ok() {
            out.push_str(&format!("{}", c.yellow().bold()));
        } else {
            out.push(c);
        }
    }
    out
}

fn show_scored_results(results: &[(&SearchTicket, u32)], query: &str) {
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buf = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for (t, _) in results {
        indices.clear();
        // Compute highlight indices against title only
        let title_hs = Utf32Str::new(&t.title, &mut buf);
        pattern.indices(title_hs, &mut matcher, &mut indices);
        indices.sort_unstable();
        indices.dedup();

        let highlighted = highlight_title(&t.title, &indices);
        println!("{:03} {} {}", t.number, highlighted, t.workstream.dimmed());
    }
}

fn interactive_search(tickets: &[SearchTicket]) -> Result<Option<(String, u32, String, String)>> {
    use crossterm::{
        cursor::{Hide, MoveTo},
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
        execute,
        terminal::{self, Clear, ClearType},
    };
    use std::io::{Write, stdout};

    // Restores raw mode and cursor on drop (handles early returns via `?`)
    struct TermGuard;
    impl Drop for TermGuard {
        fn drop(&mut self) {
            let _ = crossterm::terminal::disable_raw_mode();
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::cursor::Show,
                crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
                crossterm::cursor::MoveTo(0, 0)
            );
        }
    }

    let mut out = stdout();
    terminal::enable_raw_mode()?;
    let _guard = TermGuard;
    execute!(out, Hide)?;

    let mut query = String::new();
    let mut selected: usize = 0;

    loop {
        let results: Vec<(&SearchTicket, u32)> = if query.is_empty() {
            tickets.iter().map(|t| (t, 0)).collect()
        } else {
            score_tickets(tickets, &query)
        };

        if !results.is_empty() && selected >= results.len() {
            selected = results.len() - 1;
        }

        let (_, rows) = terminal::size().unwrap_or((80, 24));
        let max_show = (rows as usize).saturating_sub(2);

        execute!(out, MoveTo(0, 0), Clear(ClearType::All))?;
        write!(out, "Search: {}_\r\n", query)?;

        for (i, (t, _)) in results.iter().take(max_show).enumerate() {
            if i == selected {
                write!(out, "> {:03} {} ({})\r\n", t.number, t.title, t.workstream)?;
            } else {
                write!(out, "  {:03} {} ({})\r\n", t.number, t.title, t.workstream)?;
            }
        }
        out.flush()?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                KeyCode::Enter => {
                    return Ok(results.get(selected).map(|(t, _)| {
                        (
                            t.workstream.clone(),
                            t.number,
                            t.slug.clone(),
                            t.content.clone(),
                        )
                    }));
                }
                KeyCode::Up => {
                    if selected > 0 {
                        selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if selected < results.len().saturating_sub(1) {
                        selected += 1;
                    }
                }
                KeyCode::Backspace => {
                    query.pop();
                    selected = 0;
                }
                KeyCode::Char(c) => {
                    query.push(c);
                    selected = 0;
                }
                _ => {}
            }
        }
    }
}

fn search(matches: &ArgMatches) -> Result<()> {
    use std::io::IsTerminal;

    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let query = matches.get_one::<String>("query").map(String::as_str);
    let is_tty = std::io::stdout().is_terminal();

    let tickets = collect_search_tickets(matches, &root)?;

    if !is_tty {
        let q = query.ok_or_else(|| anyhow::anyhow!("query required in pipe mode"))?;
        let results = score_tickets(&tickets, q);
        if json {
            let val: Vec<serde_json::Value> = results
                .iter()
                .map(|(t, _)| {
                    serde_json::json!({
                        "workstream": t.workstream,
                        "number": t.number,
                        "slug": t.slug,
                        "title": t.title,
                    })
                })
                .collect();
            return print_json(&val);
        }
        for (t, _) in &results {
            println!("{:03} {} ({})", t.number, t.title, t.workstream);
        }
        return Ok(());
    }

    if let Some(q) = query {
        let results = score_tickets(&tickets, q);
        if json {
            let val: Vec<serde_json::Value> = results
                .iter()
                .map(|(t, _)| {
                    serde_json::json!({
                        "workstream": t.workstream,
                        "number": t.number,
                        "slug": t.slug,
                        "title": t.title,
                    })
                })
                .collect();
            return print_json(&val);
        }
        if results.is_empty() {
            println!("No matches.");
        } else {
            show_scored_results(&results, q);
        }
        return Ok(());
    }

    // TTY with no query: live interactive filter
    if let Some((ws, num, slug, content)) = interactive_search(&tickets)? {
        println!("{} {:03}-{}", ws, num, slug);
        render_ticket_content(&content);
    }

    Ok(())
}

fn status(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");

    let (ws_name, ws_dir) = resolve_workstream(matches, &root)?;
    let branch = read_meta(&ws_dir)
        .ok()
        .map(|m| m.worktree.branch)
        .unwrap_or_else(|| ws_name.clone());

    let tickets = load_tickets(&ws_dir).unwrap_or_default();
    let pending = tickets
        .iter()
        .filter(|t| t.status == Status::Pending)
        .count();
    let total = tickets.len();

    if json {
        return print_json(&serde_json::json!({
            "workstream": ws_name,
            "branch": branch,
            "pending": pending,
            "total": total,
        }));
    }

    println!("Workstream: {ws_name}");
    println!("Branch:     {branch}");
    println!("Tickets:    {pending} pending / {total} total");
    Ok(())
}
