use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::commands::worktree::format_age;
use crate::core::ticket::Status;

use super::{App, InputMode, View};

fn status_color(status: &Status) -> Color {
    match status {
        Status::Pending => Color::Cyan,
        Status::Complete => Color::Green,
        Status::WontImplement => Color::Red,
    }
}

fn status_glyph(status: &Status) -> &'static str {
    match status {
        Status::Pending => "·",
        Status::Complete => "✓",
        Status::WontImplement => "✗",
    }
}

fn status_label(status: &Status) -> &'static str {
    match status {
        Status::Pending => "pending",
        Status::Complete => "complete",
        Status::WontImplement => "wont-implement",
    }
}

pub fn render(f: &mut Frame, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(f.area());

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(outer[0]);

    match app.view {
        View::Tickets => render_tickets(f, app, chunks[0], chunks[1]),
        View::Worktrees => render_worktrees(f, app, chunks[0], chunks[1]),
    }

    render_help(f, app, outer[1]);
}

fn render_tickets(f: &mut Frame, app: &App, left: Rect, right: Rect) {
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(left);

    let (search_border_style, search_text) = match app.input_mode {
        InputMode::Search => (
            Style::default().fg(Color::Yellow),
            format!("> {}_", app.query),
        ),
        InputMode::Normal => (
            Style::default().fg(Color::DarkGray),
            format!("> {}", app.query),
        ),
    };

    let search_block = Block::default()
        .borders(Borders::ALL)
        .border_style(search_border_style)
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                app.active_workstream.as_str(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));
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
                Span::styled(
                    status_glyph(&t.status),
                    Style::default().fg(status_color(&t.status)),
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
        let color = status_color(&ticket.status);
        let title = Line::from(vec![
            Span::styled(
                format!(" {:03}  ", ticket.number),
                Style::default().fg(color),
            ),
            Span::styled(status_glyph(&ticket.status), Style::default().fg(color)),
            Span::raw(" "),
            Span::styled(status_label(&ticket.status), Style::default().fg(color)),
            Span::raw(" "),
        ]);
        let lines = render_markdown(ticket.content.as_str());
        f.render_widget(
            Paragraph::new(lines)
                .block(right_block.title(title))
                .wrap(Wrap { trim: false })
                .scroll((app.preview_scroll, 0)),
            right,
        );
    } else {
        f.render_widget(right_block, right);
    }
}

fn render_worktrees(f: &mut Frame, app: &App, left: Rect, right: Rect) {
    let worktrees = app.worktree_list();
    let items: Vec<ListItem> = worktrees
        .iter()
        .map(|wt| {
            let dirty_marker = if wt.dirty { "*" } else { "" };
            let age = format_age(wt.age_seconds);
            let tkt = format!("{}/{}", wt.tickets_pending, wt.tickets_total);
            ListItem::new(Line::from(vec![
                Span::styled(
                    wt.name.as_str(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
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

    let clamped = app.worktree_selected.min(worktrees.len().saturating_sub(1));
    let mut list_state = ListState::default();
    if !worktrees.is_empty() {
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
    if let Some(wt) = worktrees.get(clamped) {
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

fn render_help(f: &mut Frame, app: &App, area: Rect) {
    let key_style = Style::default().fg(Color::Yellow);

    let spans: Vec<Span> = match (app.view, app.input_mode) {
        (View::Tickets, InputMode::Normal) => vec![
            Span::styled("j/k", key_style),
            Span::raw(" navigate  "),
            Span::styled("s", key_style),
            Span::raw(" cycle status  "),
            Span::styled("i", key_style),
            Span::raw(" search  "),
            Span::styled("enter", key_style),
            Span::raw(" edit  "),
            Span::styled("q / C-c", key_style),
            Span::raw(" quit"),
        ],
        (View::Tickets, InputMode::Search) => vec![
            Span::raw("type to filter  "),
            Span::styled("esc", key_style),
            Span::raw(" exit search  "),
            Span::styled("↑/↓", key_style),
            Span::raw(" navigate"),
        ],
        (View::Worktrees, _) => vec![
            Span::styled("j/k", key_style),
            Span::raw(" navigate  "),
            Span::styled("enter", key_style),
            Span::raw(" open  "),
            Span::styled("tab", key_style),
            Span::raw(" switch view  "),
            Span::styled("C-c", key_style),
            Span::raw(" quit"),
        ],
    };

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_markdown(content: &str) -> Vec<Line<'_>> {
    content
        .lines()
        .map(|line| {
            if line.starts_with("### ") || line.starts_with("## ") || line.starts_with("# ") {
                Line::from(Span::styled(
                    line,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
            } else if line.starts_with("```") {
                Line::from(Span::styled(line, Style::default().fg(Color::DarkGray)))
            } else {
                Line::from(line)
            }
        })
        .collect()
}
