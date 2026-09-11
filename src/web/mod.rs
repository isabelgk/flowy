mod actions;
mod render;

use anyhow::{Context, Result};
use maud::Markup;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use tiny_http::{Header, Request, Response, Server};

use actions::{BodySave, Form, body_hash};
use render::{Filters, Nav};

use crate::commands::worktree::list_worktrees;
use crate::core::{
    names::workstream_dir,
    notes::resolve_note,
    repo::{cwd_worktree, worktrees_dir},
    ticket::{Status, load_tickets, parse_ticket, ticket_path},
};

// Bodies are small forms; anything larger is a client that has lost its way.
const MAX_BODY: usize = 1 << 20;

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

    // Requests are served one at a time. Every mutation is a read-modify-write of
    // a markdown file, and a single thread is the cheapest way to keep two tabs
    // from interleaving halfway through one.
    for request in server.incoming_requests() {
        if let Err(e) = serve(root, request) {
            eprintln!("flowy web: {e:#}");
        }
    }
    Ok(())
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

fn serve(root: &Path, mut request: Request) -> Result<()> {
    let method = request.method().as_str().to_string();
    let url = request.url().to_string();

    let mut body = String::new();
    if method == "POST" {
        request
            .as_reader()
            .take(MAX_BODY as u64)
            .read_to_string(&mut body)
            .context("reading request body")?;
    }

    let reply = match route_reply(root, &method, &url, &body, &request) {
        Ok(reply) => reply,
        Err(e) => Reply::with_status(
            400,
            render::message(
                &nav_for(root, None),
                "Something went wrong",
                &format!("{e:#}"),
            ),
        ),
    };

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

fn route_reply(
    root: &Path,
    method: &str,
    url: &str,
    body: &str,
    request: &Request,
) -> Result<Reply> {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let route = parse_route(method, path);

    if method == "POST" && !same_origin(request) {
        return Ok(Reply::with_status(
            403,
            render::message(
                &nav_for(root, None),
                "Blocked",
                "That POST did not come from this page. flowy web only accepts \
                 same-origin form submissions.",
            ),
        ));
    }

    match route {
        Route::Index => index(root),
        Route::Workstream(name) => workstream(root, &name, query),
        Route::Ticket(name, number) => ticket(root, &name, number),
        Route::EditTicket(name, number) => edit_ticket(root, &name, number, query),
        Route::Note(name, note) => show_note(root, &name, &note),
        Route::AddTicket(name) => {
            let Some(ws_dir) = writable_workstream(root, &name)? else {
                return Ok(no_workstream(root, &name));
            };
            let number = actions::add_ticket(&ws_dir, &Form::parse(body))?;
            Ok(Reply::redirect(format!("/w/{name}/t/{number}")))
        }
        Route::SetStatus(name, number) => {
            let Some(ws_dir) = writable_workstream(root, &name)? else {
                return Ok(no_workstream(root, &name));
            };
            actions::set_status(&ws_dir, number, &Form::parse(body))?;
            Ok(Reply::redirect(format!("/w/{name}/t/{number}")))
        }
        Route::SetPriority(name, number) => {
            let Some(ws_dir) = writable_workstream(root, &name)? else {
                return Ok(no_workstream(root, &name));
            };
            actions::set_priority(&ws_dir, number, &Form::parse(body))?;
            Ok(Reply::redirect(format!("/w/{name}/t/{number}")))
        }
        Route::AddJournal(name, number) => {
            let Some(ws_dir) = writable_workstream(root, &name)? else {
                return Ok(no_workstream(root, &name));
            };
            actions::add_journal(&ws_dir, number, &Form::parse(body))?;
            Ok(Reply::redirect(format!("/w/{name}/t/{number}")))
        }
        Route::SetBody(name, number) => {
            let Some(ws_dir) = writable_workstream(root, &name)? else {
                return Ok(no_workstream(root, &name));
            };
            match actions::set_body(&ws_dir, number, &Form::parse(body))? {
                BodySave::Saved => Ok(Reply::redirect(format!("/w/{name}/t/{number}"))),
                BodySave::Conflict => Ok(Reply::redirect(format!(
                    "/w/{name}/t/{number}/edit?conflict=1"
                ))),
            }
        }
        Route::NotFound => Ok(Reply::with_status(
            404,
            render::message(&nav_for(root, None), "Not found", url),
        )),
    }
}

// A form posted from any other page still arrives with the browser's cookies and
// this server's authority, so a mutation has to prove it came from here. Browsers
// send Origin on every POST, and a page served from somewhere else cannot forge it.
fn same_origin(request: &Request) -> bool {
    let header = |name: &'static str| {
        request
            .headers()
            .iter()
            .find(|h| h.field.equiv(name))
            .map(|h| h.value.as_str().to_string())
    };
    match (header("Origin"), header("Host")) {
        (Some(origin), Some(host)) => {
            origin == format!("http://{host}") || origin == format!("https://{host}")
        }
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Route {
    Index,
    Workstream(String),
    Ticket(String, u32),
    EditTicket(String, u32),
    Note(String, String),
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

fn nav_for(root: &Path, active: Option<&str>) -> Nav {
    // The sidebar is decoration: a workstream that git cannot enumerate should
    // not take the page down with it.
    Nav {
        workstreams: list_worktrees(root).unwrap_or_default(),
        active: active.map(str::to_string),
    }
}

fn live_workstream(root: &Path, name: &str) -> Option<PathBuf> {
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

fn no_workstream(root: &Path, name: &str) -> Reply {
    Reply::with_status(
        404,
        render::message(
            &nav_for(root, None),
            "No such workstream",
            &format!("This repo has no worktree named {name}."),
        ),
    )
}

fn no_ticket(root: &Path, name: &str, number: u32) -> Reply {
    Reply::with_status(
        404,
        render::message(
            &nav_for(root, Some(name)),
            "No such ticket",
            &format!("{name} has no ticket {number:03}."),
        ),
    )
}

fn index(root: &Path) -> Result<Reply> {
    if let Some(name) = cwd_worktree(root) {
        return Ok(Reply::redirect(format!("/w/{name}")));
    }
    let nav = nav_for(root, None);
    match nav.workstreams.first() {
        Some(ws) => Ok(Reply::redirect(format!("/w/{}", ws.name))),
        None => Ok(Reply::html(render::message(
            &nav,
            "No worktrees",
            "This repo has no worktrees yet. Create one with `flowy wt new -b <branch>`.",
        ))),
    }
}

fn workstream(root: &Path, name: &str, query: &str) -> Result<Reply> {
    let Some(ws_dir) = live_workstream(root, name) else {
        return Ok(no_workstream(root, name));
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

    let nav = nav_for(root, Some(name));
    let filters = Filters {
        status: &status,
        priority: &priority,
        query: params.get("q"),
    };
    Ok(Reply::html(render::ticket_list(
        &nav, name, &tickets, &filters,
    )))
}

fn ticket(root: &Path, name: &str, number: u32) -> Result<Reply> {
    let Some(ws_dir) = live_workstream(root, name) else {
        return Ok(no_workstream(root, name));
    };
    let path = ticket_path(&ws_dir, number);
    if !path.exists() {
        return Ok(no_ticket(root, name, number));
    }
    let ticket = parse_ticket(&path)?;
    let nav = nav_for(root, Some(name));
    Ok(Reply::html(render::ticket_detail(&nav, name, &ticket)))
}

fn edit_ticket(root: &Path, name: &str, number: u32, query: &str) -> Result<Reply> {
    let Some(ws_dir) = live_workstream(root, name) else {
        return Ok(no_workstream(root, name));
    };
    let path = ticket_path(&ws_dir, number);
    if !path.exists() {
        return Ok(no_ticket(root, name, number));
    }
    let ticket = parse_ticket(&path)?;
    let conflict = Form::parse(query).get("conflict") == "1";
    let nav = nav_for(root, Some(name));
    Ok(Reply::html(render::ticket_edit(
        &nav,
        name,
        &ticket,
        &body_hash(&ticket.content),
        conflict,
    )))
}

fn show_note(root: &Path, name: &str, note: &str) -> Result<Reply> {
    let Some(ws_dir) = live_workstream(root, name) else {
        return Ok(no_workstream(root, name));
    };
    let path = resolve_note(&ws_dir, note)?;
    let nav = nav_for(root, Some(name));
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
        assert_eq!(parse_route("GET", "/../etc/passwd"), Route::NotFound);
    }

    #[test]
    fn a_get_route_is_never_a_mutation() {
        for path in ["/w/a/tickets", "/w/a/t/1/status", "/w/a/t/1/body"] {
            assert_eq!(parse_route("GET", path), Route::NotFound);
        }
    }
}
