use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::commands::worktree::format_age;
use crate::core::ticket::Status;

use super::{App, View};

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(f.area());

    match app.view {
        View::Tickets => render_tickets(f, app, chunks[0], chunks[1]),
        View::Worktrees => render_worktrees(f, app, chunks[0], chunks[1]),
    }
}

fn render_tickets(f: &mut Frame, app: &App, left: Rect, right: Rect) {
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(left);

    let search_block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", app.active_workstream));
    let search_text = format!("> {}_", app.query);
    f.render_widget(
        Paragraph::new(search_text).block(search_block),
        left_chunks[0],
    );

    let filtered = app.filtered_tickets();
    let items: Vec<ListItem> = filtered
        .iter()
        .map(|&i| {
            let t = &app.tickets[i];
            let title_style = match t.status {
                Status::Pending => Style::default(),
                Status::Complete | Status::WontImplement => Style::default().fg(Color::DarkGray),
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:03}", t.number),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw("  "),
                Span::styled(t.title.as_str(), title_style),
            ]))
        })
        .collect();

    let clamped = app.ticket_selected.min(filtered.len().saturating_sub(1));
    let mut list_state = ListState::default();
    if !filtered.is_empty() {
        list_state.select(Some(clamped));
    }

    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        left_chunks[1],
        &mut list_state,
    );

    let right_block = Block::default().borders(Borders::ALL);
    if let Some(&idx) = filtered.get(clamped) {
        let ticket = &app.tickets[idx];
        let lines = render_markdown(ticket.content.as_str());
        f.render_widget(
            Paragraph::new(lines)
                .block(right_block.title(format!(" {:03} ", ticket.number)))
                .wrap(Wrap { trim: false })
                .scroll((app.preview_scroll, 0)),
            right,
        );
    } else {
        f.render_widget(right_block, right);
    }
}

fn render_worktrees(f: &mut Frame, app: &App, left: Rect, right: Rect) {
    let items: Vec<ListItem> = app
        .worktrees
        .iter()
        .map(|wt| {
            let dirty_marker = if wt.dirty { "*" } else { "" };
            let age = format_age(wt.age_seconds);
            let tkt = format!("{}/{}", wt.tickets_pending, wt.tickets_total);
            ListItem::new(Line::from(vec![
                Span::styled(
                    wt.name.as_str(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(dirty_marker, Style::default().fg(Color::Yellow)),
                Span::raw("  "),
                Span::styled(age, Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                if wt.tickets_pending > 0 {
                    Span::styled(tkt, Style::default().fg(Color::Green))
                } else {
                    Span::styled(tkt, Style::default().fg(Color::DarkGray))
                },
            ]))
        })
        .collect();

    let clamped = app
        .worktree_selected
        .min(app.worktrees.len().saturating_sub(1));
    let mut list_state = ListState::default();
    if !app.worktrees.is_empty() {
        list_state.select(Some(clamped));
    }

    f.render_stateful_widget(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" Worktrees "))
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
        left,
        &mut list_state,
    );

    let right_block = Block::default().borders(Borders::ALL);
    if let Some(wt) = app.worktrees.get(clamped) {
        let lines: Vec<Line> = app
            .worktree_preview
            .iter()
            .map(|(num, title)| {
                Line::from(vec![
                    Span::styled(format!("{num:03}"), Style::default().fg(Color::Yellow)),
                    Span::raw("  "),
                    Span::raw(title.as_str()),
                ])
            })
            .collect();
        f.render_widget(
            Paragraph::new(lines)
                .block(right_block.title(format!(" {} ", wt.name)))
                .wrap(Wrap { trim: false }),
            right,
        );
    } else {
        f.render_widget(right_block, right);
    }
}

fn render_markdown(content: &str) -> Vec<Line<'_>> {
    content
        .lines()
        .map(|line| {
            if line.starts_with("### ") || line.starts_with("## ") || line.starts_with("# ") {
                Line::from(Span::styled(
                    line,
                    Style::default().add_modifier(Modifier::BOLD),
                ))
            } else if line.starts_with("```") {
                Line::from(Span::styled(line, Style::default().fg(Color::DarkGray)))
            } else {
                Line::from(line)
            }
        })
        .collect()
}
