use anyhow::{Context, Result};
use clap::ArgMatches;
use comfy_table::{Cell, Color, ContentArrangement, Table};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use owo_colors::OwoColorize;
use std::path::{Path, PathBuf};

use super::{get_repo_root, open_in_editor, read_body_file, resolve_workstream, warn_skipped};
use crate::core::{
    atomic::write_atomic,
    meta::read_meta,
    names::workstream_dir,
    notes::append_log,
    repo::{cwd_worktree, worktrees_dir},
    ticket::{
        Priority, Status, Ticket, append_journal, create_ticket, load_tickets,
        load_tickets_lenient, resolve_ticket, rewrite_status_and_append, write_priority,
    },
};
use crate::output::print_json;

pub fn dispatch(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("list", sub)) => list(sub),
        Some(("show", sub)) => show(sub),
        Some(("add", sub)) => add(sub),
        Some(("done", sub)) => done(sub),
        Some(("priority", sub)) => priority(sub),
        Some(("journal", sub)) => journal(sub),
        Some(("edit", sub)) => edit(sub),
        Some(("search", sub)) => search(sub),
        Some(("status", sub)) => status(sub),
        _ => Ok(()),
    }
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

fn filter_by_priority(tickets: Vec<Ticket>, priority: Option<Priority>) -> Vec<Ticket> {
    match priority {
        Some(p) => tickets.into_iter().filter(|t| t.priority == p).collect(),
        None => tickets,
    }
}

// Safe to unwrap the parse: clap already restricted the value to Priority::VALUES.
fn priority_arg(matches: &ArgMatches, name: &str) -> Option<Priority> {
    matches
        .get_one::<String>(name)
        .map(|s| s.parse().expect("clap restricts this to a known priority"))
}

// Prose for a ticket arrives either inline under `inline_arg` or out of band via
// --body-file, which clap already keeps mutually exclusive.
fn body_arg(matches: &ArgMatches, inline_arg: &str) -> Result<Option<String>> {
    if let Some(text) = matches.get_one::<String>(inline_arg) {
        return Ok(Some(text.clone()));
    }
    match matches.get_one::<String>("body-file") {
        Some(path) => Ok(Some(read_body_file(path)?)),
        None => Ok(None),
    }
}

fn priority_color(priority: Priority) -> Color {
    match priority {
        Priority::High => Color::Red,
        Priority::Medium => Color::Yellow,
        Priority::Low => Color::DarkGrey,
    }
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
    table.set_header(["#", "Title", "Priority", "Status"]);
    for t in tickets {
        let (label, color) = match t.status {
            Status::Pending => ("pending", Color::Yellow),
            Status::Complete => ("complete", Color::Green),
            Status::WontImplement => ("wont-implement", Color::DarkGrey),
        };
        table.add_row([
            Cell::new(format!("{:03}", t.number)),
            Cell::new(&t.title),
            Cell::new(t.priority.as_str()).fg(priority_color(t.priority)),
            Cell::new(label).fg(color),
        ]);
    }
    println!("{table}");
}

// Every live workstream, as (name, workstream dir), sorted by name. Enumerates
// worktrees rather than the notes root because a workstream's notes now live
// inside its worktree; archived ones are reachable only by explicit name.
fn live_workstreams(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(worktrees_dir(root)) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.into_iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.to_string();
            Some((name, crate::core::names::workstream_subdir(&p)))
        })
        .collect()
}

fn list(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let filter = matches
        .get_one::<String>("status")
        .map(|s| s.as_str())
        .unwrap_or("pending");
    let priority = priority_arg(matches, "priority");

    let single_ws: Option<(String, PathBuf)> = if matches.get_one::<String>("workstream").is_some()
    {
        Some(resolve_workstream(matches, &root)?)
    } else {
        cwd_worktree(&root).map(|name| (name.clone(), workstream_dir(&root, &name)))
    };

    if let Some((ws_name, ws_dir)) = single_ws {
        let tickets = filter_by_priority(filter_tickets(load_tickets(&ws_dir)?, filter), priority);
        if json {
            let val: Vec<serde_json::Value> = tickets
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "number": t.number,
                        "title": t.title,
                        "status": t.status,
                        "priority": t.priority,
                    })
                })
                .collect();
            return print_json(&val);
        }
        print_ticket_table(&ws_name, &tickets);
    } else {
        // All workstreams grouped
        let mut groups: Vec<(String, Vec<Ticket>)> = Vec::new();

        for (name, dir) in live_workstreams(&root) {
            let (tickets, errors) = load_tickets_lenient(&dir);
            warn_skipped(&name, &errors);
            let tickets = filter_by_priority(filter_tickets(tickets, filter), priority);
            if !tickets.is_empty() {
                groups.push((name, tickets));
            }
        }

        if json {
            let val: Vec<serde_json::Value> = groups
                .iter()
                .flat_map(|(ws, tickets)| {
                    tickets.iter().map(move |t| {
                        serde_json::json!({
                            "workstream": ws,
                            "number": t.number,
                            "title": t.title,
                            "status": t.status,
                            "priority": t.priority,
                        })
                    })
                })
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

    println!("{}", ticket.title.bold());
    println!(
        "{}",
        format!("priority: {}", ticket.priority.as_str()).dimmed()
    );
    render_ticket_content(&ticket.content);
    Ok(())
}

fn add(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let title = matches.get_one::<String>("title").unwrap();
    let body = body_arg(matches, "description")?;
    let priority = priority_arg(matches, "priority");

    let (_, ws_dir) = resolve_workstream(matches, &root)?;
    let (number, path) = create_ticket(&ws_dir, title, priority, body.as_deref())?;

    append_log(&ws_dir, &format!("added {number:03} ({title})"))?;

    if json {
        return print_json(&serde_json::json!({
            "number": number,
            "path": path.to_string_lossy(),
        }));
    }
    println!("Created {number:03}");
    Ok(())
}

fn done(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let id = matches.get_one::<String>("id").unwrap();
    let summary = body_arg(matches, "summary")?;
    let status = if matches.get_flag("reopen") {
        Status::Pending
    } else if matches.get_flag("wont-implement") {
        Status::WontImplement
    } else {
        Status::Complete
    };

    let (_, ws_dir) = resolve_workstream(matches, &root)?;
    let tickets = load_tickets(&ws_dir)?;
    let ticket = resolve_ticket(&tickets, id)?;

    let path = &ticket.path;
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;

    let new_content = rewrite_status_and_append(&content, status, summary.as_deref());
    write_atomic(path, &new_content).with_context(|| format!("writing {}", path.display()))?;

    let action = status.verb();
    append_log(&ws_dir, &format!("{action} {:03}", ticket.number))?;

    if json {
        return print_json(&serde_json::json!({
            "number": ticket.number,
            "status": status,
        }));
    }
    println!("{action} {:03}", ticket.number);
    Ok(())
}

fn priority(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let id = matches.get_one::<String>("id").unwrap();
    let level = priority_arg(matches, "level").expect("clap requires the level argument");

    let (_, ws_dir) = resolve_workstream(matches, &root)?;
    let tickets = load_tickets(&ws_dir)?;
    let ticket = resolve_ticket(&tickets, id)?;

    write_priority(&ticket.path, level)?;
    append_log(
        &ws_dir,
        &format!("priority {:03} {}", ticket.number, level.as_str()),
    )?;

    if json {
        return print_json(&serde_json::json!({
            "number": ticket.number,
            "priority": level,
        }));
    }
    println!("priority {:03} {}", ticket.number, level.as_str());
    Ok(())
}

fn journal(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let id = matches.get_one::<String>("id").unwrap();
    let text = body_arg(matches, "text")?
        .ok_or_else(|| anyhow::anyhow!("no journal text: pass it inline or via --body-file"))?;
    let heading = matches.get_one::<String>("heading");

    let (_, ws_dir) = resolve_workstream(matches, &root)?;
    let tickets = load_tickets(&ws_dir)?;
    let ticket = resolve_ticket(&tickets, id)?;

    let path = &ticket.path;
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;

    let (content, ts) = append_journal(&content, &text, heading.map(String::as_str));
    write_atomic(path, &content).with_context(|| format!("writing {}", path.display()))?;

    if json {
        return print_json(&serde_json::json!({
            "number": ticket.number,
            "appended_at": ts,
        }));
    }
    println!("Journal entry added to {:03}", ticket.number);
    Ok(())
}

fn edit(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let id = matches.get_one::<String>("id").unwrap();

    let (_, ws_dir) = resolve_workstream(matches, &root)?;
    let tickets = load_tickets(&ws_dir)?;
    let ticket = resolve_ticket(&tickets, id)?;

    let path = &ticket.path;
    let editor = std::env::var("EDITOR").context("$EDITOR is not set")?;
    open_in_editor(&editor, path)
}

struct SearchTicket {
    workstream: String,
    number: u32,
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
                title: t.title,
                content: t.content,
            })
            .collect());
    }

    let mut result = Vec::new();
    for (name, dir) in live_workstreams(root) {
        let (tickets, errors) = load_tickets_lenient(&dir);
        warn_skipped(&name, &errors);
        for t in tickets {
            result.push(SearchTicket {
                workstream: name.clone(),
                number: t.number,
                title: t.title,
                content: t.content,
            });
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

fn interactive_search(tickets: &[SearchTicket]) -> Result<Option<(String, u32, String)>> {
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
                    return Ok(results
                        .get(selected)
                        .map(|(t, _)| (t.workstream.clone(), t.number, t.content.clone())));
                }
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
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
                .map(|(t, score)| {
                    serde_json::json!({
                        "workstream": t.workstream,
                        "number": t.number,
                        "title": t.title,
                        "score": score,
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
                .map(|(t, score)| {
                    serde_json::json!({
                        "workstream": t.workstream,
                        "number": t.number,
                        "title": t.title,
                        "score": score,
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
    if let Some((ws, num, content)) = interactive_search(&tickets)? {
        println!("{} {:03}", ws, num);
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

    let (tickets, errors) = load_tickets_lenient(&ws_dir);
    warn_skipped(&ws_name, &errors);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ticket::{Priority, Status, Ticket};

    fn make_ticket(number: u32, status: Status) -> Ticket {
        make_ranked_ticket(number, status, Priority::Medium)
    }

    fn make_ranked_ticket(number: u32, status: Status, priority: Priority) -> Ticket {
        Ticket {
            number,
            title: format!("Ticket {number}"),
            status,
            priority,
            content: String::new(),
            path: PathBuf::new(),
        }
    }

    #[test]
    fn filter_tickets_all() {
        let tickets = vec![
            make_ticket(1, Status::Pending),
            make_ticket(2, Status::Complete),
            make_ticket(3, Status::WontImplement),
        ];
        assert_eq!(filter_tickets(tickets, "all").len(), 3);
    }

    #[test]
    fn filter_tickets_by_status() {
        let tickets = vec![
            make_ticket(1, Status::Pending),
            make_ticket(2, Status::Complete),
            make_ticket(3, Status::WontImplement),
        ];
        assert_eq!(filter_tickets(tickets.clone(), "complete").len(), 1);
        assert_eq!(filter_tickets(tickets.clone(), "wont-implement").len(), 1);
        assert_eq!(filter_tickets(tickets.clone(), "pending").len(), 1);
        // default arm returns pending
        assert_eq!(filter_tickets(tickets, "").len(), 1);
    }

    #[test]
    fn filter_tickets_empty_input() {
        assert_eq!(filter_tickets(vec![], "all").len(), 0);
        assert_eq!(filter_tickets(vec![], "pending").len(), 0);
    }

    #[test]
    fn filter_by_priority_keeps_only_the_requested_level() {
        let tickets = vec![
            make_ranked_ticket(1, Status::Pending, Priority::High),
            make_ranked_ticket(2, Status::Pending, Priority::Medium),
            make_ranked_ticket(3, Status::Pending, Priority::High),
        ];
        let high = filter_by_priority(tickets.clone(), Some(Priority::High));
        assert_eq!(
            high.iter().map(|t| t.number).collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(filter_by_priority(tickets, None).len(), 3);
    }
}
