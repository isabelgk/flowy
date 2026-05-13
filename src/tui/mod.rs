use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::commands::worktree::{WorktreeInfo, list_worktrees};
use crate::core::{
    names::{safe_name, workstream_dir},
    ticket::{Status, load_tickets, ticket_path},
};

mod ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Tickets,
    Worktrees,
}

pub struct TuiTicket {
    pub number: u32,
    pub title: String,
    pub status: Status,
    pub content: String,
    pub path: PathBuf,
}

pub struct App {
    pub root: PathBuf,
    pub view: View,
    pub active_workstream: String,
    pub tickets: Vec<TuiTicket>,
    pub worktrees: Vec<WorktreeInfo>,
    pub worktree_preview: Vec<(u32, String)>,
    pub query: String,
    pub ticket_selected: usize,
    pub worktree_selected: usize,
    pub preview_scroll: u16,
    pub should_quit: bool,
}

impl App {
    pub fn filtered_tickets(&self) -> Vec<usize> {
        if self.query.is_empty() {
            return (0..self.tickets.len()).collect();
        }

        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buf = Vec::new();
        let mut results: Vec<(usize, u32)> = Vec::new();

        for (i, t) in self.tickets.iter().enumerate() {
            let haystack = format!("{}\n{}", t.title, t.content);
            let hs = Utf32Str::new(&haystack, &mut buf);
            if let Some(score) = pattern.score(hs, &mut matcher) {
                results.push((i, score));
            }
        }

        results.sort_by(|a, b| b.1.cmp(&a.1));
        results.into_iter().map(|(i, _)| i).collect()
    }

    fn load_tickets_for_workstream(&mut self, name: &str) {
        let ws_dir = workstream_dir(&self.root, name);
        self.active_workstream = name.to_string();
        self.tickets = load_tickets(&ws_dir)
            .unwrap_or_default()
            .into_iter()
            .map(|t| {
                let path = ticket_path(&ws_dir, t.number);
                TuiTicket {
                    number: t.number,
                    title: t.title,
                    status: t.status,
                    content: t.content,
                    path,
                }
            })
            .collect();
        self.ticket_selected = 0;
        self.query.clear();
        self.preview_scroll = 0;
    }

    fn update_worktree_preview(&mut self) {
        self.worktree_preview.clear();
        if let Some(wt) = self.worktrees.get(self.worktree_selected) {
            let ws_dir = workstream_dir(&self.root, &wt.name);
            if let Ok(tickets) = load_tickets(&ws_dir) {
                self.worktree_preview = tickets
                    .into_iter()
                    .filter(|t| t.status == Status::Pending)
                    .map(|t| (t.number, t.title))
                    .collect();
            }
        }
    }

    /// Returns the path to open in $EDITOR, if requested.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<PathBuf> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        match self.view {
            View::Tickets => self.handle_tickets_key(key),
            View::Worktrees => self.handle_worktrees_key(key),
        }
    }

    fn handle_tickets_key(&mut self, key: KeyEvent) -> Option<PathBuf> {
        let filtered = self.filtered_tickets();
        match key.code {
            KeyCode::Esc => {
                if self.query.is_empty() {
                    self.should_quit = true;
                } else {
                    self.query.clear();
                    self.ticket_selected = 0;
                    self.preview_scroll = 0;
                }
            }
            KeyCode::Tab => {
                self.view = View::Worktrees;
                self.preview_scroll = 0;
                self.update_worktree_preview();
            }
            KeyCode::Down => {
                if !filtered.is_empty() {
                    self.ticket_selected = (self.ticket_selected + 1).min(filtered.len() - 1);
                    self.preview_scroll = 0;
                }
            }
            KeyCode::Up => {
                if self.ticket_selected > 0 {
                    self.ticket_selected -= 1;
                    self.preview_scroll = 0;
                }
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.preview_scroll = self.preview_scroll.saturating_add(10);
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.preview_scroll = self.preview_scroll.saturating_sub(10);
            }
            KeyCode::Backspace => {
                self.query.pop();
                let new_len = self.filtered_tickets().len();
                self.ticket_selected = self.ticket_selected.min(new_len.saturating_sub(1));
                self.preview_scroll = 0;
            }
            KeyCode::Char(c)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.query.push(c);
                let new_len = self.filtered_tickets().len();
                self.ticket_selected = self.ticket_selected.min(new_len.saturating_sub(1));
                self.preview_scroll = 0;
            }
            KeyCode::Enter => {
                if let Some(&idx) = filtered.get(self.ticket_selected) {
                    return Some(self.tickets[idx].path.clone());
                }
            }
            _ => {}
        }
        None
    }

    fn handle_worktrees_key(&mut self, key: KeyEvent) -> Option<PathBuf> {
        match key.code {
            KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Tab => {
                self.view = View::Tickets;
                self.preview_scroll = 0;
            }
            KeyCode::Down => {
                if !self.worktrees.is_empty() {
                    self.worktree_selected =
                        (self.worktree_selected + 1).min(self.worktrees.len() - 1);
                    self.preview_scroll = 0;
                    self.update_worktree_preview();
                }
            }
            KeyCode::Up => {
                if self.worktree_selected > 0 {
                    self.worktree_selected -= 1;
                    self.preview_scroll = 0;
                    self.update_worktree_preview();
                }
            }
            KeyCode::Enter => {
                if let Some(wt) = self.worktrees.get(self.worktree_selected) {
                    let name = wt.name.clone();
                    self.load_tickets_for_workstream(&name);
                    self.view = View::Tickets;
                    self.preview_scroll = 0;
                }
            }
            _ => {}
        }
        None
    }
}

pub fn run(root: &Path) -> Result<()> {
    let branch_out = Command::new("git")
        .args(["branch", "--show-current"])
        .output()
        .ok();

    let (initial_view, active_workstream) = match branch_out {
        Some(out) if out.status.success() => {
            let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if branch.is_empty() {
                (View::Worktrees, String::new())
            } else {
                (View::Tickets, safe_name(&branch))
            }
        }
        _ => (View::Worktrees, String::new()),
    };

    let tickets = if initial_view == View::Tickets {
        let ws_dir = workstream_dir(root, &active_workstream);
        load_tickets(&ws_dir)
            .unwrap_or_default()
            .into_iter()
            .map(|t| {
                let path = ticket_path(&ws_dir, t.number);
                TuiTicket {
                    number: t.number,
                    title: t.title,
                    status: t.status,
                    content: t.content,
                    path,
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    let worktrees = list_worktrees(root).unwrap_or_default();

    let mut app = App {
        root: root.to_path_buf(),
        view: initial_view,
        active_workstream,
        tickets,
        worktrees,
        worktree_preview: Vec::new(),
        query: String::new(),
        ticket_selected: 0,
        worktree_selected: 0,
        preview_scroll: 0,
        should_quit: false,
    };

    if initial_view == View::Worktrees {
        app.update_worktree_preview();
    }

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    let result = run_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::render(f, app))?;

        if let Event::Key(key) = event::read()?
            && let Some(path) = app.handle_key(key)
        {
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
            disable_raw_mode()?;
            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
            let _ = Command::new(&editor).arg(&path).status();
            enable_raw_mode()?;
            execute!(terminal.backend_mut(), EnterAlternateScreen)?;
            terminal.clear()?;
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
