mod actions;
mod render;

use anyhow::{Context, Result};
use maud::Markup;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Request, Response, Server};

use actions::{BodySave, Form, body_hash};
use render::{Filters, Nav, ws_url};

use crate::commands::worktree::{WorktreeInfo, list_worktrees};
use crate::core::{
    names::workstream_dir,
    notes::resolve_note,
    repo::{cwd_worktree, worktrees_dir},
    ticket::{Status, find_ticket, load_tickets},
};

// The largest form is a whole ticket body from the editor, percent-encoded at up
// to 9 bytes per character.
const MAX_BODY: usize = 16 << 20;

// How long a sidebar snapshot is served before a refresh is kicked off behind
// the next request.
const NAV_TTL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct NavState {
    snapshot: Option<Vec<WorktreeInfo>>,
    fetched_at: Option<Instant>,
    refreshing: bool,
    // Bumped by every mutation. A refresh that started before the bump lands
    // with pre-mutation ticket counts, so it drops its result instead.
    generation: u64,
}

// The sidebar costs a `git status` per worktree, which dominates the time to
// render any page and has nothing to do with what the page is showing. Requests
// serve the last snapshot and let the refresh happen behind them; only the first
// request of a session, and the first after a mutation, waits for git.
#[derive(Clone)]
struct App {
    root: PathBuf,
    nav: Arc<Mutex<NavState>>,
    // `Host` header values this server answers to.
    hosts: Arc<Vec<String>>,
    // Whether the page being served shows the sidebar; a closed one skips git.
    nav_open: bool,
}

impl App {
    fn new(root: &Path, hosts: Vec<String>) -> App {
        App {
            root: root.to_path_buf(),
            nav: Arc::new(Mutex::new(NavState::default())),
            hosts: Arc::new(hosts),
            nav_open: false,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, NavState> {
        self.nav.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn workstreams(&self) -> Vec<WorktreeInfo> {
        {
            let mut state = self.state();
            if let Some(snapshot) = state.snapshot.clone() {
                let stale = state.fetched_at.is_none_or(|at| at.elapsed() >= NAV_TTL);
                if stale && !state.refreshing {
                    state.refreshing = true;
                    let app = self.clone();
                    std::thread::spawn(move || {
                        app.refresh();
                    });
                }
                return snapshot;
            }
        }
        self.refresh()
    }

    // Reads git and writes .flowy/cache.json; it never touches a ticket file, so
    // it can run off the request thread without breaking the serialization that
    // keeps two mutations from interleaving.
    fn refresh(&self) -> Vec<WorktreeInfo> {
        let generation = self.state().generation;
        let fetched = list_worktrees(&self.root).unwrap_or_default();

        let mut state = self.state();
        state.refreshing = false;
        if state.generation == generation {
            state.snapshot = Some(fetched.clone());
            state.fetched_at = Some(Instant::now());
        }
        fetched
    }

    // Clearing the snapshot rather than just aging it out makes the next page
    // recompute synchronously, so ticket counts never lag a mutation the user
    // just made.
    fn invalidate_nav(&self) {
        let mut state = self.state();
        state.snapshot = None;
        state.fetched_at = None;
        state.generation += 1;
    }

    fn nav(&self, active: Option<&str>) -> Nav {
        Nav {
            workstreams: self.nav_open.then(|| self.workstreams()),
            active: active.map(str::to_string),
        }
    }
}

pub fn run(root: &Path, host: &str, port: u16, open_browser: bool) -> Result<()> {
    let server =
        Server::http((host, port)).map_err(|e| anyhow::anyhow!("binding {host}:{port}: {e}"))?;
    let bound = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| anyhow::anyhow!("server bound to a non-IP address"))?;
    let url = format!("http://{host}:{}", bound.port());

    println!("flowy web: {url}");
    println!("  repo: {}", root.display());
    if open_browser {
        open_url(&url);
    }

    // Requests are served one at a time on the worker. Every mutation is a
    // read-modify-write of a markdown file, and a single thread is the cheapest
    // way to keep two tabs from interleaving halfway through one.
    let app = App::new(root, allowed_hosts(host, bound.port()));
    let (tx, rx) = mpsc::channel::<(Request, Result<String, BodyError>)>();
    let worker = app.clone();
    std::thread::spawn(move || {
        for (request, body) in rx {
            if let Err(e) = serve(&worker, request, body) {
                eprintln!("flowy web: {e:#}");
            }
        }
    });

    for request in server.incoming_requests() {
        if !host_allowed(&app, &request) {
            // No page and no sidebar: the workstream names are what a rebinding
            // page would be after. Refused before its body is read at all.
            if let Err(e) =
                request.respond(Response::from_string("unknown host\n").with_status_code(403))
            {
                eprintln!("flowy web: writing response: {e}");
            }
            continue;
        }
        if request.method() != &Method::Post {
            let _ = tx.send((request, Ok(String::new())));
            continue;
        }
        // A body arrives at the client's pace, so it is read on its own thread;
        // read on the worker, one slow upload would stall every other request.
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut request = request;
            let body = read_body(&mut request);
            let _ = tx.send((request, body));
        });
    }
    Ok(())
}

fn host_allowed(app: &App, request: &Request) -> bool {
    header(request, "Host").is_some_and(|h| app.hosts.iter().any(|a| a.eq_ignore_ascii_case(&h)))
}

// A DNS-rebinding page reaches this server under its own hostname, so it is the
// Host header, not Origin, that proves a request was addressed to loopback.
fn allowed_hosts(host: &str, port: u16) -> Vec<String> {
    let mut hosts = Vec::new();
    for name in [host, "localhost", "127.0.0.1", "::1"] {
        let name = if name.contains(':') {
            format!("[{name}]")
        } else {
            name.to_string()
        };
        hosts.push(format!("{name}:{port}"));
        // Browsers leave the default port out of Host.
        if port == 80 {
            hosts.push(name);
        }
    }
    hosts
}

fn open_url(url: &str) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener).arg(url).status();
}

struct Reply {
    status: u16,
    body: String,
    location: Option<String>,
}

impl Reply {
    fn html(markup: Markup) -> Reply {
        Reply {
            status: 200,
            body: markup.into_string(),
            location: None,
        }
    }

    fn with_status(status: u16, markup: Markup) -> Reply {
        Reply {
            status,
            ..Reply::html(markup)
        }
    }

    // 303 after every mutation so a reload re-reads the ticket instead of
    // resubmitting the form.
    fn redirect(to: String) -> Reply {
        Reply {
            status: 303,
            body: String::new(),
            location: Some(to),
        }
    }
}

fn serve(app: &App, request: Request, body: Result<String, BodyError>) -> Result<()> {
    let app = &App {
        nav_open: nav_open(&request),
        ..app.clone()
    };
    let method = request.method().as_str().to_string();
    let url = request.url().to_string();

    let reply = match body {
        Ok(body) => match route_reply(app, &method, &url, &body, &request) {
            Ok(reply) => reply,
            Err(e) => Reply::with_status(
                400,
                render::message(&app.nav(None), "Something went wrong", &format!("{e:#}")),
            ),
        },
        Err(BodyError::TooLarge) => Reply::with_status(
            413,
            render::message(
                &app.nav(None),
                "Too large",
                &format!(
                    "flowy web accepts forms up to {} MiB; nothing was saved.",
                    MAX_BODY >> 20
                ),
            ),
        ),
        Err(BodyError::Incomplete(detail)) => Reply::with_status(
            400,
            render::message(&app.nav(None), "Incomplete request", &detail),
        ),
    };

    // Any accepted POST can move a ticket count in the sidebar, so the snapshot
    // the next page renders from has to be rebuilt rather than served stale.
    if method == "POST" && reply.status == 303 {
        app.invalidate_nav();
    }

    let mut response = Response::from_string(reply.body).with_status_code(reply.status);
    response.add_header(
        Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
            .expect("static header"),
    );
    if let Some(location) = reply.location {
        response.add_header(
            Header::from_bytes(&b"Location"[..], location.as_bytes())
                .map_err(|_| anyhow::anyhow!("invalid redirect target"))?,
        );
    }
    request.respond(response).context("writing response")
}

enum BodyError {
    TooLarge,
    Incomplete(String),
}

// A partial body still decodes as a valid form, and `base` precedes `body` in
// the editor's, so a truncated save would pass the conflict check. Anything
// short of the whole body is refused rather than acted on.
fn read_body(request: &mut Request) -> Result<String, BodyError> {
    let expected = request.body_length();
    if expected.is_some_and(|n| n > MAX_BODY) {
        return Err(BodyError::TooLarge);
    }
    let mut bytes = Vec::new();
    if let Err(e) = request
        .as_reader()
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut bytes)
    {
        return Err(BodyError::Incomplete(format!(
            "Reading the form failed ({e}); nothing was saved."
        )));
    }
    if bytes.len() > MAX_BODY {
        return Err(BodyError::TooLarge);
    }
    if expected.is_some_and(|n| bytes.len() < n) {
        return Err(BodyError::Incomplete(
            "The connection closed before the whole form arrived; nothing was saved.".into(),
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| BodyError::Incomplete("The form was not valid UTF-8.".into()))
}

fn route_reply(app: &App, method: &str, url: &str, body: &str, request: &Request) -> Result<Reply> {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let route = parse_route(method, path);

    if method == "POST" && !same_origin(request) {
        return Ok(Reply::with_status(
            403,
            render::message(
                &app.nav(None),
                "Blocked",
                "That POST did not come from this page. flowy web only accepts \
                 same-origin form submissions.",
            ),
        ));
    }

    match route {
        // Browsers ask for this alongside every page. Answering with an empty
        // 204 keeps it off the path that reads git.
        Route::Favicon => Ok(Reply {
            status: 204,
            body: String::new(),
            location: None,
        }),
        Route::Index => index(app),
        Route::Nav => {
            let active = Form::parse(query).filled("w").map(str::to_string);
            let nav = Nav {
                workstreams: Some(app.workstreams()),
                active,
            };
            Ok(Reply::html(render::sidebar(&nav)))
        }
        Route::Workstream(name) => workstream(app, &name, query),
        Route::Ticket(name, number) => ticket(app, &name, number),
        Route::EditTicket(name, number) => edit_ticket(app, &name, number, query),
        Route::Note(name, note) => show_note(app, &name, &note),
        Route::AddTicket(name) => {
            let Some(ws_dir) = writable_workstream(&app.root, &name)? else {
                return Ok(no_workstream(app, &name));
            };
            let number = actions::add_ticket(&ws_dir, &Form::parse(body))?;
            Ok(Reply::redirect(format!("{}/t/{number}", ws_url(&name))))
        }
        Route::SetStatus(name, number) => {
            let Some(ws_dir) = writable_workstream(&app.root, &name)? else {
                return Ok(no_workstream(app, &name));
            };
            actions::set_status(&ws_dir, number, &Form::parse(body))?;
            Ok(Reply::redirect(format!("{}/t/{number}", ws_url(&name))))
        }
        Route::SetPriority(name, number) => {
            let Some(ws_dir) = writable_workstream(&app.root, &name)? else {
                return Ok(no_workstream(app, &name));
            };
            actions::set_priority(&ws_dir, number, &Form::parse(body))?;
            Ok(Reply::redirect(format!("{}/t/{number}", ws_url(&name))))
        }
        Route::AddJournal(name, number) => {
            let Some(ws_dir) = writable_workstream(&app.root, &name)? else {
                return Ok(no_workstream(app, &name));
            };
            actions::add_journal(&ws_dir, number, &Form::parse(body))?;
            Ok(Reply::redirect(format!("{}/t/{number}", ws_url(&name))))
        }
        Route::SetBody(name, number) => {
            let Some(ws_dir) = writable_workstream(&app.root, &name)? else {
                return Ok(no_workstream(app, &name));
            };
            match actions::set_body(&ws_dir, number, &Form::parse(body))? {
                BodySave::Saved => Ok(Reply::redirect(format!("{}/t/{number}", ws_url(&name)))),
                BodySave::Conflict => Ok(Reply::redirect(format!(
                    "{}/t/{number}/edit?conflict=1",
                    ws_url(&name)
                ))),
            }
        }
        Route::NotFound => Ok(Reply::with_status(
            404,
            render::message(&app.nav(None), "Not found", url),
        )),
    }
}

// The sidebar starts closed; the toggle sets this cookie so the server can leave
// the git work out of pages that would hide it anyway.
fn nav_open(request: &Request) -> bool {
    header(request, "Cookie")
        .is_some_and(|cookies| cookies.split(';').any(|c| c.trim() == "flowy-nav=open"))
}

fn header(request: &Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
}

// A form posted from any other page still arrives with the browser's cookies and
// this server's authority, so a mutation has to prove it came from here. Browsers
// send Origin on every POST, and a page served from somewhere else cannot forge it.
// Host has already been checked against the allowlist, so matching it is enough.
fn same_origin(request: &Request) -> bool {
    match (header(request, "Origin"), header(request, "Host")) {
        (Some(origin), Some(host)) => {
            origin == format!("http://{host}") || origin == format!("https://{host}")
        }
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Route {
    Index,
    Nav,
    Workstream(String),
    Ticket(String, u32),
    EditTicket(String, u32),
    Note(String, String),
    Favicon,
    AddTicket(String),
    SetStatus(String, u32),
    SetPriority(String, u32),
    AddJournal(String, u32),
    SetBody(String, u32),
    NotFound,
}

fn parse_route(method: &str, path: &str) -> Route {
    let segments: Vec<String> = path
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|s| {
            percent_encoding::percent_decode_str(s)
                .decode_utf8_lossy()
                .into_owned()
        })
        .collect();
    let parts: Vec<&str> = segments.iter().map(String::as_str).collect();

    let number = |raw: &str| raw.parse::<u32>().ok();

    match (method, parts.as_slice()) {
        ("GET", []) => Route::Index,
        ("GET", ["favicon.ico"]) => Route::Favicon,
        ("GET", ["nav"]) => Route::Nav,
        ("GET", ["w", name]) => Route::Workstream(name.to_string()),
        ("GET", ["w", name, "t", id]) => match number(id) {
            Some(n) => Route::Ticket(name.to_string(), n),
            None => Route::NotFound,
        },
        ("GET", ["w", name, "t", id, "edit"]) => match number(id) {
            Some(n) => Route::EditTicket(name.to_string(), n),
            None => Route::NotFound,
        },
        ("GET", ["w", name, "notes", note]) => Route::Note(name.to_string(), note.to_string()),
        ("POST", ["w", name, "tickets"]) => Route::AddTicket(name.to_string()),
        ("POST", ["w", name, "t", id, action]) => match (number(id), *action) {
            (Some(n), "status") => Route::SetStatus(name.to_string(), n),
            (Some(n), "priority") => Route::SetPriority(name.to_string(), n),
            (Some(n), "journal") => Route::AddJournal(name.to_string(), n),
            (Some(n), "body") => Route::SetBody(name.to_string(), n),
            _ => Route::NotFound,
        },
        _ => Route::NotFound,
    }
}

// Route segments are percent-decoded, so a name can smuggle in `/`, `..`, or an
// absolute path that `Path::join` would follow out of `worktrees/`.
fn is_single_component(name: &str) -> bool {
    let mut components = Path::new(name).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(_)), None)
    ) && !name.contains('/')
}

fn live_workstream(root: &Path, name: &str) -> Option<PathBuf> {
    if !is_single_component(name) {
        return None;
    }
    worktrees_dir(root)
        .join(name)
        .is_dir()
        .then(|| workstream_dir(root, name))
}

// `wt new` scaffolds the notes directory, but a worktree added with plain git has
// none until something writes to it.
fn writable_workstream(root: &Path, name: &str) -> Result<Option<PathBuf>> {
    let Some(dir) = live_workstream(root, name) else {
        return Ok(None);
    };
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(Some(dir))
}

fn no_workstream(app: &App, name: &str) -> Reply {
    Reply::with_status(
        404,
        render::message(
            &app.nav(None),
            "No such workstream",
            &format!("This repo has no worktree named {name}."),
        ),
    )
}

fn no_ticket(app: &App, name: &str, number: u32) -> Reply {
    Reply::with_status(
        404,
        render::message(
            &app.nav(Some(name)),
            "No such ticket",
            &format!("{name} has no ticket {number:03}."),
        ),
    )
}

fn index(app: &App) -> Result<Reply> {
    if let Some(name) = cwd_worktree(&app.root) {
        return Ok(Reply::redirect(ws_url(&name)));
    }
    // Directory names, not list_worktrees, so opening the UI never waits on git.
    let mut names: Vec<String> = std::fs::read_dir(worktrees_dir(&app.root))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| live_workstream(&app.root, name).is_some())
        .collect();
    names.sort();
    match names.first() {
        Some(name) => Ok(Reply::redirect(ws_url(name))),
        None => Ok(Reply::html(render::message(
            &app.nav(None),
            "No worktrees",
            "This repo has no worktrees yet. Create one with `flowy wt new -b <branch>`.",
        ))),
    }
}

fn workstream(app: &App, name: &str, query: &str) -> Result<Reply> {
    let Some(ws_dir) = live_workstream(&app.root, name) else {
        return Ok(no_workstream(app, name));
    };
    let params = Form::parse(query);
    let status = match params.filled("status") {
        Some(value) => value.to_string(),
        None => "open".to_string(),
    };
    let priority = params.filled("priority").unwrap_or_default().to_string();
    let search = params.filled("q").unwrap_or_default().to_lowercase();

    let tickets = load_tickets(&ws_dir)?
        .into_iter()
        .filter(|t| match status.as_str() {
            "open" => t.status == Status::Pending,
            "complete" => t.status == Status::Complete,
            "wont-implement" => t.status == Status::WontImplement,
            _ => true,
        })
        .filter(|t| priority.is_empty() || t.priority.as_str() == priority)
        .filter(|t| {
            search.is_empty()
                || t.title.to_lowercase().contains(&search)
                || t.content.to_lowercase().contains(&search)
        })
        .collect::<Vec<_>>();

    let nav = app.nav(Some(name));
    let filters = Filters {
        status: &status,
        priority: &priority,
        query: params.get("q"),
    };
    Ok(Reply::html(render::ticket_list(
        &nav, name, &tickets, &filters,
    )))
}

fn ticket(app: &App, name: &str, number: u32) -> Result<Reply> {
    let Some(ws_dir) = live_workstream(&app.root, name) else {
        return Ok(no_workstream(app, name));
    };
    let Some(ticket) = find_ticket(&ws_dir, number)? else {
        return Ok(no_ticket(app, name, number));
    };
    let nav = app.nav(Some(name));
    Ok(Reply::html(render::ticket_detail(&nav, name, &ticket)))
}

fn edit_ticket(app: &App, name: &str, number: u32, query: &str) -> Result<Reply> {
    let Some(ws_dir) = live_workstream(&app.root, name) else {
        return Ok(no_workstream(app, name));
    };
    let Some(ticket) = find_ticket(&ws_dir, number)? else {
        return Ok(no_ticket(app, name, number));
    };
    let conflict = Form::parse(query).get("conflict") == "1";
    let nav = app.nav(Some(name));
    Ok(Reply::html(render::ticket_edit(
        &nav,
        name,
        &ticket,
        &body_hash(&ticket.content),
        conflict,
    )))
}

fn show_note(app: &App, name: &str, note: &str) -> Result<Reply> {
    let Some(ws_dir) = live_workstream(&app.root, name) else {
        return Ok(no_workstream(app, name));
    };
    let path = resolve_note(&ws_dir, note)?;
    let nav = app.nav(Some(name));
    let label = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| note.to_string());

    match std::fs::read_to_string(&path) {
        Ok(body) => {
            let as_markdown = path.extension().is_some_and(|e| e == "md");
            Ok(Reply::html(render::note(
                &nav,
                name,
                &label,
                &body,
                as_markdown,
            )))
        }
        Err(_) => Ok(Reply::with_status(
            404,
            render::message(&nav, &label, &format!("{name} has no {label} yet.")),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favicon_has_its_own_route() {
        assert_eq!(parse_route("GET", "/favicon.ico"), Route::Favicon);
    }

    #[test]
    fn routes_cover_the_get_pages() {
        assert_eq!(parse_route("GET", "/"), Route::Index);
        assert_eq!(
            parse_route("GET", "/w/feat-x"),
            Route::Workstream("feat-x".into())
        );
        assert_eq!(
            parse_route("GET", "/w/feat-x/t/7"),
            Route::Ticket("feat-x".into(), 7)
        );
        assert_eq!(
            parse_route("GET", "/w/feat-x/t/7/edit"),
            Route::EditTicket("feat-x".into(), 7)
        );
        assert_eq!(
            parse_route("GET", "/w/feat-x/notes/planning"),
            Route::Note("feat-x".into(), "planning".into())
        );
    }

    #[test]
    fn routes_cover_the_post_actions() {
        assert_eq!(
            parse_route("POST", "/w/a/tickets"),
            Route::AddTicket("a".into())
        );
        assert_eq!(
            parse_route("POST", "/w/a/t/2/status"),
            Route::SetStatus("a".into(), 2)
        );
        assert_eq!(
            parse_route("POST", "/w/a/t/2/priority"),
            Route::SetPriority("a".into(), 2)
        );
        assert_eq!(
            parse_route("POST", "/w/a/t/2/journal"),
            Route::AddJournal("a".into(), 2)
        );
        assert_eq!(
            parse_route("POST", "/w/a/t/2/body"),
            Route::SetBody("a".into(), 2)
        );
    }

    #[test]
    fn route_decodes_percent_escapes_in_names() {
        assert_eq!(
            parse_route("GET", "/w/feat%20x"),
            Route::Workstream("feat x".into())
        );
    }

    #[test]
    fn unknown_shapes_and_methods_are_not_found() {
        assert_eq!(parse_route("GET", "/w/a/t/abc"), Route::NotFound);
        assert_eq!(parse_route("GET", "/w/a/t/1/frobnicate"), Route::NotFound);
        assert_eq!(parse_route("POST", "/w/a"), Route::NotFound);
        assert_eq!(parse_route("DELETE", "/w/a/t/1"), Route::NotFound);
    }

    #[test]
    fn allowed_hosts_cover_the_loopback_names() {
        let hosts = allowed_hosts("0.0.0.0", 8123);
        for host in [
            "0.0.0.0:8123",
            "localhost:8123",
            "127.0.0.1:8123",
            "[::1]:8123",
        ] {
            assert!(hosts.contains(&host.to_string()), "{hosts:?}");
        }
        assert!(!hosts.contains(&"localhost".to_string()));
        assert!(allowed_hosts("::1", 80).contains(&"[::1]".to_string()));
    }

    #[test]
    fn workstream_names_cannot_leave_worktrees() {
        for name in ["/tmp/x", "../../x", "..", ".", "a/b", "a/", ""] {
            assert!(!is_single_component(name), "{name}");
        }
        assert!(is_single_component("feat-x"));
    }

    #[test]
    fn a_get_route_is_never_a_mutation() {
        for path in ["/w/a/tickets", "/w/a/t/1/status", "/w/a/t/1/body"] {
            assert_eq!(parse_route("GET", path), Route::NotFound);
        }
    }
}
