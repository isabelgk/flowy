use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::commands::open_in_editor;
use crate::commands::worktree::{WorktreeInfo, list_worktrees};
use crate::core::{
    atomic::write_atomic,
    names::{workstream_dir, workstream_subdir},
    notes::append_log,
    repo::cwd_worktree,
    ticket::{Priority, Status, load_tickets_lenient, rewrite_status_and_append, write_priority},
};

mod ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Tickets,
    Worktrees,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
}

pub struct TuiTicket {
    pub number: u32,
    pub title: String,
    pub status: Status,
    pub priority: Priority,
    pub content: String,
    pub path: PathBuf,
}

pub struct App {
    pub root: PathBuf,
    pub view: View,
    pub input_mode: InputMode,
    pub active_workstream: String,
    // Where `tickets` was loaded from, so they can be re-read after an edit.
    pub active_ws_dir: PathBuf,
    pub tickets: Vec<TuiTicket>,
    // None = not yet fetched; Some = loaded (may be empty)
    pub worktrees: Option<Vec<WorktreeInfo>>,
    pub worktree_preview: Vec<(u32, String)>,
    pub query: String,
    pub pending_only: bool,
    pub ticket_selected: usize,
    pub worktree_selected: usize,
    // Index of the first visible row in each list. Persisted across frames so
    // the viewport stays put instead of snapping back to the top.
    pub ticket_offset: usize,
    pub worktree_offset: usize,
    // Inner height of the list pane, recorded each render so key handling can
    // size a half-page scroll.
    pub list_height: u16,
    // Shown in place of the key help until the next key press.
    pub error: Option<String>,
    pub should_quit: bool,
}

impl App {
    pub fn worktree_list(&self) -> &[WorktreeInfo] {
        self.worktrees.as_deref().unwrap_or(&[])
    }

    // Fetched on every visit to the view, so counts track edits made since.
    fn refresh_worktrees(&mut self) {
        let worktrees = list_worktrees(&self.root).unwrap_or_default();
        self.worktree_selected = self
            .worktree_selected
            .min(worktrees.len().saturating_sub(1));
        self.worktrees = Some(worktrees);
    }

    fn clamp_ticket_selection(&mut self) {
        let len = self.filtered_tickets().len();
        self.ticket_selected = self.ticket_selected.min(len.saturating_sub(1));
    }

    // Re-reads the active workstream's tickets, keeping the selection on the same
    // ticket while it is still listed.
    fn reload_tickets(&mut self) {
        let selected = self
            .filtered_tickets()
            .get(self.ticket_selected)
            .map(|&i| self.tickets[i].number);
        self.tickets = load_tui_tickets(&self.active_ws_dir);
        let filtered = self.filtered_tickets();
        if let Some(pos) =
            selected.and_then(|n| filtered.iter().position(|&i| self.tickets[i].number == n))
        {
            self.ticket_selected = pos;
        }
        self.clamp_ticket_selection();
    }

    pub fn filtered_tickets(&self) -> Vec<usize> {
        let candidates = (0..self.tickets.len())
            .filter(|&i| !self.pending_only || self.tickets[i].status == Status::Pending);

        if self.query.is_empty() {
            return candidates.collect();
        }

        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buf = Vec::new();
        let mut results: Vec<(usize, u32)> = Vec::new();

        for i in candidates {
            let t = &self.tickets[i];
            let haystack = format!("{}\n{}", t.title, t.content);
            let hs = Utf32Str::new(&haystack, &mut buf);
            if let Some(score) = pattern.score(hs, &mut matcher) {
                results.push((i, score));
            }
        }

        results.sort_by(|a, b| b.1.cmp(&a.1));
        results.into_iter().map(|(i, _)| i).collect()
    }

    fn load_tickets_for_workstream(&mut self, name: &str, ws_dir: &Path) {
        self.active_workstream = name.to_string();
        self.active_ws_dir = ws_dir.to_path_buf();
        self.tickets = load_tui_tickets(ws_dir);
        self.ticket_selected = 0;
        self.query.clear();
        self.pending_only = true;
        self.ticket_offset = 0;
        self.input_mode = InputMode::Normal;
    }

    fn update_worktree_preview(&mut self) {
        self.worktree_preview.clear();
        if let Some(wt) = self.worktree_list().get(self.worktree_selected) {
            let ws_dir = workstream_subdir(&wt.path);
            self.worktree_preview = load_tickets_lenient(&ws_dir)
                .0
                .into_iter()
                .filter(|t| t.status == Status::Pending)
                .map(|t| (t.number, t.title))
                .collect();
        }
    }

    /// Never zero, so scrolling still works on a short terminal or before the
    /// first render has recorded a height.
    fn half_page(&self) -> usize {
        usize::from(self.list_height / 2).max(1)
    }

    /// Returns the path to open in $EDITOR, if requested.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<PathBuf> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        self.error = None;
        // In search, `q` is a character of the query.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
            || key.code == KeyCode::Char('q') && self.input_mode == InputMode::Normal
        {
            self.should_quit = true;
            return None;
        }
        match self.view {
            View::Tickets => self.handle_tickets_key(key),
            View::Worktrees => self.handle_worktrees_key(key),
        }
    }

    fn handle_tickets_key(&mut self, key: KeyEvent) -> Option<PathBuf> {
        let filtered = self.filtered_tickets();
        match self.input_mode {
            InputMode::Normal => match key.code {
                KeyCode::Tab => {
                    self.view = View::Worktrees;
                    self.refresh_worktrees();
                    self.update_worktree_preview();
                }
                KeyCode::Down | KeyCode::Char('j')
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    if !filtered.is_empty() {
                        self.ticket_selected = (self.ticket_selected + 1).min(filtered.len() - 1);
                    }
                }
                KeyCode::Up | KeyCode::Char('k')
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.ticket_selected = self.ticket_selected.saturating_sub(1);
                }
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if !filtered.is_empty() {
                        self.ticket_selected = self
                            .ticket_selected
                            .saturating_add(self.half_page())
                            .min(filtered.len() - 1);
                    }
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.ticket_selected = self.ticket_selected.saturating_sub(self.half_page());
                }
                KeyCode::Char('i') => {
                    self.input_mode = InputMode::Search;
                }
                KeyCode::Char('o') => {
                    self.pending_only = !self.pending_only;
                    let new_len = self.filtered_tickets().len();
                    self.ticket_selected = self.ticket_selected.min(new_len.saturating_sub(1));
                    self.ticket_offset = 0;
                }
                // The list is priority-sorted at load, but a cycled ticket keeps its
                // row until the next load: resorting under the cursor would move the
                // selection out from under the key that just pressed it.
                KeyCode::Char('p') => {
                    if let Some(&idx) = filtered.get(self.ticket_selected) {
                        let new_priority = next_priority(self.tickets[idx].priority);
                        if write_ticket_priority(&self.tickets[idx], new_priority).is_ok() {
                            self.tickets[idx].priority = new_priority;
                        }
                    }
                }
                KeyCode::Char('s') => {
                    if let Some(&idx) = filtered.get(self.ticket_selected) {
                        let new_status = next_status(&self.tickets[idx].status);
                        if write_ticket_status(&self.tickets[idx], &new_status).is_ok() {
                            self.tickets[idx].status = new_status;
                            // The pending filter may have just dropped this row.
                            self.clamp_ticket_selection();
                        }
                    }
                }
                KeyCode::Enter => {
                    if let Some(&idx) = filtered.get(self.ticket_selected) {
                        return Some(self.tickets[idx].path.clone());
                    }
                }
                _ => {}
            },
            InputMode::Search => match key.code {
                KeyCode::Esc => {
                    self.input_mode = InputMode::Normal;
                }
                KeyCode::Down => {
                    if !filtered.is_empty() {
                        self.ticket_selected = (self.ticket_selected + 1).min(filtered.len() - 1);
                    }
                }
                KeyCode::Up => {
                    self.ticket_selected = self.ticket_selected.saturating_sub(1);
                }
                KeyCode::Backspace => {
                    self.query.pop();
                    let new_len = self.filtered_tickets().len();
                    self.ticket_selected = self.ticket_selected.min(new_len.saturating_sub(1));
                    self.ticket_offset = 0;
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT) =>
                {
                    self.query.push(c);
                    let new_len = self.filtered_tickets().len();
                    self.ticket_selected = self.ticket_selected.min(new_len.saturating_sub(1));
                    self.ticket_offset = 0;
                }
                _ => {}
            },
        }
        None
    }

    fn handle_worktrees_key(&mut self, key: KeyEvent) -> Option<PathBuf> {
        match key.code {
            KeyCode::Tab => {
                self.view = View::Tickets;
            }
            KeyCode::Down | KeyCode::Char('j')
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                if !self.worktree_list().is_empty() {
                    self.worktree_selected =
                        (self.worktree_selected + 1).min(self.worktree_list().len() - 1);
                    self.update_worktree_preview();
                }
            }
            KeyCode::Up | KeyCode::Char('k')
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.worktree_selected = self.worktree_selected.saturating_sub(1);
                self.update_worktree_preview();
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if !self.worktree_list().is_empty() {
                    self.worktree_selected = self
                        .worktree_selected
                        .saturating_add(self.half_page())
                        .min(self.worktree_list().len() - 1);
                    self.update_worktree_preview();
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.worktree_selected = self.worktree_selected.saturating_sub(self.half_page());
                self.update_worktree_preview();
            }
            KeyCode::Enter => {
                if let Some(wt) = self.worktree_list().get(self.worktree_selected) {
                    let name = wt.name.clone();
                    let ws_dir = workstream_subdir(&wt.path);
                    self.load_tickets_for_workstream(&name, &ws_dir);
                    self.view = View::Tickets;
                }
            }
            _ => {}
        }
        None
    }
}

fn next_priority(priority: Priority) -> Priority {
    match priority {
        Priority::High => Priority::Medium,
        Priority::Medium => Priority::Low,
        Priority::Low => Priority::High,
    }
}

fn next_status(status: &Status) -> Status {
    match status {
        Status::Pending => Status::Complete,
        Status::Complete => Status::WontImplement,
        Status::WontImplement => Status::Pending,
    }
}

// Logs to status.md alongside the write, as the CLI and web UI do.
fn write_ticket_status(ticket: &TuiTicket, new_status: &Status) -> Result<()> {
    let (reopen, wont_impl, action) = match new_status {
        Status::Pending => (true, false, "reopen"),
        Status::Complete => (false, false, "done"),
        Status::WontImplement => (false, true, "wont-implement"),
    };
    let path = &ticket.path;
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let updated = rewrite_status_and_append(&content, reopen, wont_impl, None);
    write_atomic(path, updated).with_context(|| format!("writing {}", path.display()))?;
    append_log(
        ticket_ws_dir(ticket)?,
        &format!("{action} {:03}", ticket.number),
    )
}

fn write_ticket_priority(ticket: &TuiTicket, priority: Priority) -> Result<()> {
    write_priority(&ticket.path, priority)?;
    append_log(
        ticket_ws_dir(ticket)?,
        &format!("priority {:03} {}", ticket.number, priority.as_str()),
    )
}

// Ticket paths are `<ws_dir>/tickets/<n>.md`.
fn ticket_ws_dir(ticket: &TuiTicket) -> Result<&Path> {
    ticket
        .path
        .parent()
        .and_then(Path::parent)
        .with_context(|| format!("no workstream above {}", ticket.path.display()))
}

fn load_tui_tickets(ws_dir: &Path) -> Vec<TuiTicket> {
    load_tickets_lenient(ws_dir)
        .0
        .into_iter()
        .map(|t| TuiTicket {
            number: t.number,
            title: t.title,
            status: t.status,
            priority: t.priority,
            content: t.content,
            path: t.path,
        })
        .collect()
}

pub fn run(root: &Path) -> Result<()> {
    // Open on the workstream whose worktree we are standing in. Keyed on the
    // directory rather than the checked-out branch: `git branch -m` leaves the
    // two disagreeing, and a branch-derived path finds no tickets.
    let (initial_view, active_workstream) = match cwd_worktree(root) {
        Some(name) => (View::Tickets, name),
        None => (View::Worktrees, String::new()),
    };

    let active_ws_dir = if initial_view == View::Tickets {
        workstream_dir(root, &active_workstream)
    } else {
        PathBuf::new()
    };
    let tickets = if initial_view == View::Tickets {
        load_tui_tickets(&active_ws_dir)
    } else {
        Vec::new()
    };

    let mut app = App {
        root: root.to_path_buf(),
        view: initial_view,
        input_mode: InputMode::Normal,
        active_workstream,
        active_ws_dir,
        tickets,
        worktrees: None,
        worktree_preview: Vec::new(),
        query: String::new(),
        pending_only: true,
        ticket_selected: 0,
        worktree_selected: 0,
        ticket_offset: 0,
        worktree_offset: 0,
        list_height: 0,
        error: None,
        should_quit: false,
    };

    if initial_view == View::Worktrees {
        app.refresh_worktrees();
        app.update_worktree_preview();
    }

    // Installed before raw mode is entered, so there is no window in which a
    // panic leaves the terminal raw.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    enable_raw_mode()?;
    let result = (|| -> Result<()> {
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        run_loop(&mut terminal, &mut app)
    })();

    // Restored whether or not setup or the loop failed.
    let restored = disable_raw_mode().and(execute!(std::io::stdout(), LeaveAlternateScreen));
    result?;
    Ok(restored?)
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
            let opened = open_in_editor(&editor, &path);
            enable_raw_mode()?;
            execute!(terminal.backend_mut(), EnterAlternateScreen)?;
            terminal.clear()?;
            app.error = opened.err().map(|e| format!("{e:#}"));
            // The editor may have changed status or priority behind the list.
            app.reload_tickets();
        }

        if app.should_quit {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ticket::ticket_path;
    use ratatui::backend::TestBackend;

    fn ticket_on_disk(frontmatter: &str) -> (tempfile::TempDir, TuiTicket) {
        let dir = tempfile::tempdir().unwrap();
        let path = ticket_path(dir.path(), 1);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("+++\n{frontmatter}\n+++\n\nBody.\n")).unwrap();
        let ticket = TuiTicket {
            number: 1,
            title: "T".to_string(),
            status: Status::Pending,
            priority: Priority::Medium,
            content: String::new(),
            path,
        };
        (dir, ticket)
    }

    #[test]
    fn status_write_leaves_similar_keys_alone_and_logs() {
        let (dir, ticket) =
            ticket_on_disk("status = \"pending\"\nstatus_note = \"x\"\ntitle = \"T\"");
        write_ticket_status(&ticket, &Status::Complete).unwrap();

        assert_eq!(
            std::fs::read_to_string(&ticket.path).unwrap(),
            "+++\nstatus = \"complete\"\nstatus_note = \"x\"\ntitle = \"T\"\n+++\n\nBody.\n"
        );
        let log = std::fs::read_to_string(dir.path().join("status.md")).unwrap();
        assert!(log.contains("done 001"), "{log}");
    }

    #[test]
    fn status_write_adds_a_missing_key() {
        let (_dir, ticket) = ticket_on_disk("title = \"T\"");
        write_ticket_status(&ticket, &Status::WontImplement).unwrap();
        let written = std::fs::read_to_string(&ticket.path).unwrap();
        assert!(written.contains("status = \"wont-implement\""), "{written}");
    }

    #[test]
    fn priority_write_logs() {
        let (dir, ticket) = ticket_on_disk("status = \"pending\"\ntitle = \"T\"");
        write_ticket_priority(&ticket, Priority::High).unwrap();
        let log = std::fs::read_to_string(dir.path().join("status.md")).unwrap();
        assert!(log.contains("priority 001 high"), "{log}");
    }

    // 80x22 puts 16 rows inside the ticket list block, so a half page is 8.
    const TERM_WIDTH: u16 = 80;
    const TERM_HEIGHT: u16 = 22;

    fn app_with_tickets(count: u32) -> App {
        App {
            root: PathBuf::new(),
            view: View::Tickets,
            input_mode: InputMode::Normal,
            active_workstream: "test".to_string(),
            active_ws_dir: PathBuf::new(),
            tickets: (1..=count)
                .map(|n| TuiTicket {
                    number: n,
                    title: format!("ticket {n}"),
                    status: Status::Pending,
                    priority: Priority::Medium,
                    content: String::new(),
                    path: PathBuf::new(),
                })
                .collect(),
            worktrees: Some(Vec::new()),
            worktree_preview: Vec::new(),
            query: String::new(),
            pending_only: true,
            ticket_selected: 0,
            worktree_selected: 0,
            ticket_offset: 0,
            worktree_offset: 0,
            list_height: 0,
            error: None,
            should_quit: false,
        }
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    /// A worktree whose notes are reachable only through its own path: the branch
    /// disagrees with the directory name, and the directory sits outside
    /// worktrees/, so no name the TUI holds can rebuild the path. Both `git
    /// branch -m` and `git worktree add <anywhere>` produce this shape.
    fn app_with_divergent_worktree() -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        let worktree = dir.path().join("checkouts").join("feat-foo");
        let tickets_dir = workstream_subdir(&worktree).join("tickets");
        std::fs::create_dir_all(&tickets_dir).unwrap();
        std::fs::write(
            tickets_dir.join("001.md"),
            "+++\nstatus = \"pending\"\ntitle = \"only ticket\"\n+++\n",
        )
        .unwrap();

        let mut app = app_with_tickets(0);
        app.root = dir.path().to_path_buf();
        app.view = View::Worktrees;
        app.active_workstream = String::new();
        app.worktrees = Some(vec![WorktreeInfo {
            name: "feat-foo".to_string(),
            path: worktree,
            branch: "feat/bar".to_string(),
            dirty: false,
            age_seconds: 0,
            tickets_pending: 1,
            tickets_total: 1,
        }]);
        (dir, app)
    }

    #[test]
    fn worktree_preview_loads_when_the_branch_diverges_from_the_dir() {
        let (_dir, mut app) = app_with_divergent_worktree();
        assert!(
            !workstream_dir(&app.root, "feat-foo").exists(),
            "fixture must be unreachable by name for this test to mean anything"
        );

        app.update_worktree_preview();
        assert_eq!(app.worktree_preview, vec![(1, "only ticket".to_string())]);
    }

    #[test]
    fn enter_loads_tickets_when_the_branch_diverges_from_the_dir() {
        let (_dir, mut app) = app_with_divergent_worktree();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.view, View::Tickets);
        assert_eq!(app.active_workstream, "feat-foo");
        assert_eq!(app.tickets.len(), 1);
        assert_eq!(app.tickets[0].title, "only ticket");
    }

    #[test]
    fn p_cycles_priority_and_writes_it_through() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("001.md");
        std::fs::write(
            &path,
            "+++\nstatus = \"pending\"\ntitle = \"t\"\n+++\n\nBody.\n",
        )
        .unwrap();

        let mut app = app_with_tickets(1);
        app.tickets[0].path = path.clone();

        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(app.tickets[0].priority, Priority::Low);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("priority = \"low\"")
        );

        app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(app.tickets[0].priority, Priority::High);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("priority = \"high\""));
        assert!(content.ends_with("+++\n\nBody.\n"));
    }

    #[test]
    fn o_filters_to_pending_tickets_and_clamps_the_selection() {
        let mut app = app_with_tickets(3);
        app.tickets[1].status = Status::Complete;
        app.tickets[2].status = Status::WontImplement;
        assert!(app.pending_only, "the filter starts on");
        assert_eq!(app.filtered_tickets(), vec![0]);

        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        assert!(!app.pending_only);
        assert_eq!(app.filtered_tickets(), vec![0, 1, 2]);

        app.ticket_selected = 2;
        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::NONE));
        assert!(app.pending_only);
        assert_eq!(app.filtered_tickets(), vec![0]);
        assert_eq!(app.ticket_selected, 0);
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn app_on_disk(count: u32) -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("tickets")).unwrap();
        for n in 1..=count {
            std::fs::write(
                ticket_path(dir.path(), n),
                format!("+++\nstatus = \"pending\"\ntitle = \"ticket {n}\"\n+++\n"),
            )
            .unwrap();
        }
        let mut app = app_with_tickets(0);
        app.active_ws_dir = dir.path().to_path_buf();
        app.reload_tickets();
        (dir, app)
    }

    #[test]
    fn q_in_search_is_part_of_the_query() {
        let mut app = app_with_tickets(1);
        app.handle_key(key('i'));
        app.handle_key(key('q'));
        assert!(!app.should_quit);
        assert_eq!(app.query, "q");

        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_key(key('q'));
        assert!(app.should_quit);
    }

    #[test]
    fn s_on_the_last_pending_row_keeps_a_selection() {
        let (_dir, mut app) = app_on_disk(2);
        app.ticket_selected = 1;

        app.handle_key(key('s'));
        assert_eq!(app.filtered_tickets().len(), 1);
        assert_eq!(app.ticket_selected, 0);

        // The next `s` lands on the remaining row instead of doing nothing.
        app.handle_key(key('s'));
        let first = std::fs::read_to_string(ticket_path(&app.active_ws_dir, 1)).unwrap();
        assert!(first.contains("status = \"complete\""), "{first}");
    }

    #[test]
    fn reload_picks_up_an_edit_and_keeps_the_selection() {
        let (_dir, mut app) = app_on_disk(3);
        app.ticket_selected = 2;
        std::fs::write(
            ticket_path(&app.active_ws_dir, 1),
            "+++\nstatus = \"complete\"\ntitle = \"ticket 1\"\n+++\n",
        )
        .unwrap();

        app.reload_tickets();
        let filtered = app.filtered_tickets();
        assert_eq!(filtered.len(), 2);
        assert_eq!(app.tickets[filtered[app.ticket_selected]].number, 3);
    }

    #[test]
    fn ctrl_d_and_ctrl_u_move_selection_by_half_the_list_height() {
        let mut app = app_with_tickets(30);
        let mut terminal = Terminal::new(TestBackend::new(TERM_WIDTH, TERM_HEIGHT)).unwrap();
        terminal.draw(|f| ui::render(f, &mut app)).unwrap();
        assert_eq!(app.list_height, 16);

        app.handle_key(ctrl('d'));
        assert_eq!(app.ticket_selected, 8);

        app.handle_key(ctrl('u'));
        assert_eq!(app.ticket_selected, 0);
    }

    #[test]
    fn the_list_row_shows_a_priority_glyph() {
        let mut app = app_with_tickets(1);
        app.tickets[0].priority = Priority::High;
        let mut terminal = Terminal::new(TestBackend::new(TERM_WIDTH, TERM_HEIGHT)).unwrap();
        terminal.draw(|f| ui::render(f, &mut app)).unwrap();

        let row: String = terminal.backend().buffer().content()[TERM_WIDTH as usize * 4..]
            .iter()
            .take(TERM_WIDTH as usize)
            .map(|c| c.symbol())
            .collect();
        assert!(row.contains("· ^  ticket 1"), "unexpected row: {row:?}");
    }

    #[test]
    fn ctrl_d_stops_at_the_last_ticket() {
        let mut app = app_with_tickets(10);
        let mut terminal = Terminal::new(TestBackend::new(TERM_WIDTH, TERM_HEIGHT)).unwrap();
        terminal.draw(|f| ui::render(f, &mut app)).unwrap();

        app.handle_key(ctrl('d'));
        app.handle_key(ctrl('d'));
        assert_eq!(app.ticket_selected, 9);
    }

    #[test]
    fn list_viewport_stays_put_when_scrolling_back_up() {
        let mut app = app_with_tickets(30);
        let mut terminal = Terminal::new(TestBackend::new(TERM_WIDTH, TERM_HEIGHT)).unwrap();
        let mut draw = |app: &mut App| {
            terminal.draw(|f| ui::render(f, app)).unwrap();
        };
        draw(&mut app);

        // Push the selection past the bottom of the viewport so the top row is
        // no longer the first ticket.
        for _ in 0..3 {
            app.handle_key(ctrl('d'));
            draw(&mut app);
        }
        let scrolled_offset = app.ticket_offset;
        assert!(scrolled_offset > 0);

        app.handle_key(ctrl('u'));
        draw(&mut app);
        assert_eq!(app.ticket_offset, scrolled_offset);

        app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        draw(&mut app);
        assert_eq!(app.ticket_offset, scrolled_offset);
    }
}
