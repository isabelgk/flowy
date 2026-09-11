use maud::{DOCTYPE, Markup, PreEscaped, html};
use pulldown_cmark::{Event, Options, Parser};

use crate::commands::worktree::{WorktreeInfo, format_age};
use crate::core::ticket::{Priority, Status, Ticket};

const CSS: &str = include_str!("style.css");
const SCRIPT: &str = include_str!("script.js");

// Runs in <head> rather than with the rest of the script, so a collapsed sidebar
// is collapsed in the first paint instead of flashing open on every navigation.
const RESTORE_NAV: &str = "try{if(localStorage.getItem('flowy-nav')==='closed')\
     document.documentElement.classList.add('nav-collapsed')}catch(e){}";

pub struct Nav {
    pub workstreams: Vec<WorktreeInfo>,
    pub active: Option<String>,
}

// Ticket bodies are local files, but they are also written by agents and pasted
// from elsewhere; passing their raw HTML through would let a <script> in a
// ticket run with access to the mutating endpoints on this origin.
pub fn markdown(src: &str) -> Markup {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let events = Parser::new_ext(src, options).map(|event| match event {
        Event::Html(raw) => Event::Text(raw),
        Event::InlineHtml(raw) => Event::Text(raw),
        other => other,
    });

    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, events);
    PreEscaped(out)
}

pub fn page(title: &str, nav: &Nav, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " - flowy" }
                style { (PreEscaped(CSS)) }
                script { (PreEscaped(RESTORE_NAV)) }
            }
            body {
                header.topbar {
                    button.nav-toggle type="button" aria-expanded="true"
                        aria-controls="workstreams" title="Hide workstreams" {
                        span.bars {}
                        span.sr-only { "Toggle workstreams" }
                    }
                    a.brand href="/" { "flowy" }
                    span.title { (title) }
                }
                .shell {
                    nav.sidebar #workstreams {
                        h2 { "Workstreams" }
                        @if nav.workstreams.is_empty() {
                            p.empty { "No worktrees yet." }
                        }
                        ul {
                            @for ws in &nav.workstreams {
                                li {
                                    a href={ "/w/" (ws.name) }
                                      class=@if nav.active.as_deref() == Some(ws.name.as_str()) { "active" } @else { "" } {
                                        span.ws-name { (ws.name) }
                                        span.ws-meta {
                                            (ws.tickets_pending) "/" (ws.tickets_total) " open"
                                            @if ws.dirty { span.dot title="uncommitted changes" { "*" } }
                                            " - " (format_age(ws.age_seconds))
                                        }
                                    }
                                }
                            }
                        }
                    }
                    main.content { (content) }
                }
                script { (PreEscaped(SCRIPT)) }
            }
        }
    }
}

pub fn message(nav: &Nav, title: &str, body: &str) -> Markup {
    page(title, nav, html! { .card { h1 { (title) } p { (body) } } })
}

fn status_label(status: &Status) -> &'static str {
    match status {
        Status::Pending => "open",
        Status::Complete => "done",
        Status::WontImplement => "won't",
    }
}

fn status_class(status: &Status) -> &'static str {
    match status {
        Status::Pending => "badge status-pending",
        Status::Complete => "badge status-complete",
        Status::WontImplement => "badge status-wont",
    }
}

fn priority_class(priority: Priority) -> String {
    format!("badge prio-{}", priority.as_str())
}

pub struct Filters<'a> {
    pub status: &'a str,
    pub priority: &'a str,
    pub query: &'a str,
}

pub fn ticket_list(nav: &Nav, workstream: &str, tickets: &[Ticket], filters: &Filters) -> Markup {
    let content = html! {
        form.filters method="get" action={ "/w/" (workstream) } {
            label { "Status"
                select name="status" {
                    @for (value, label) in [("open", "Open"), ("all", "All"), ("complete", "Done"), ("wont-implement", "Won't implement")] {
                        option value=(value) selected[filters.status == value] { (label) }
                    }
                }
            }
            label { "Priority"
                select name="priority" {
                    option value="" selected[filters.priority.is_empty()] { "Any" }
                    @for value in Priority::VALUES {
                        option value=(value) selected[filters.priority == value] { (value) }
                    }
                }
            }
            label { "Search"
                input type="search" name="q" value=(filters.query) placeholder="title or body";
            }
            button type="submit" { "Apply" }
        }

        @if tickets.is_empty() {
            p.empty { "No tickets match." }
        } @else {
            table.tickets {
                thead { tr { th { "#" } th { "Title" } th { "Priority" } th { "Status" } } }
                tbody {
                    @for t in tickets {
                        tr {
                            td.num { a href={ "/w/" (workstream) "/t/" (t.number) } { (format!("{:03}", t.number)) } }
                            td { a href={ "/w/" (workstream) "/t/" (t.number) } { (t.title) } }
                            td { span class=(priority_class(t.priority)) { (t.priority.as_str()) } }
                            td { span class=(status_class(&t.status)) { (status_label(&t.status)) } }
                        }
                    }
                }
            }
        }

        details.card {
            summary { "New ticket" }
            form method="post" action={ "/w/" (workstream) "/tickets" } {
                label { "Title"
                    input type="text" name="title" required;
                }
                label { "Priority"
                    select name="priority" {
                        @for value in Priority::VALUES {
                            option value=(value) selected[value == "medium"] { (value) }
                        }
                    }
                }
                label { "Description"
                    textarea name="description" rows="8" placeholder="markdown" {}
                }
                button type="submit" { "Create" }
            }
        }

        p.notes {
            "Notes: "
            a href={ "/w/" (workstream) "/notes/planning" } { "planning" } " - "
            a href={ "/w/" (workstream) "/notes/next" } { "next" } " - "
            a href={ "/w/" (workstream) "/notes/status" } { "status" }
        }
    };
    page(workstream, nav, content)
}

pub fn ticket_detail(nav: &Nav, workstream: &str, ticket: &Ticket) -> Markup {
    let base = format!("/w/{workstream}/t/{}", ticket.number);
    let content = html! {
        .card {
            .ticket-head {
                h1 { (format!("{:03}", ticket.number)) " " (ticket.title) }
                span class=(status_class(&ticket.status)) { (status_label(&ticket.status)) }
                span class=(priority_class(ticket.priority)) { (ticket.priority.as_str()) }
            }
            .markdown { (markdown(&ticket.content)) }
            p { a.button href={ (base) "/edit" } { "Edit body" } }
        }

        .card {
            h2 { "Status" }
            form method="post" action={ (base) "/status" } {
                label { "Summary (optional, added once)"
                    textarea name="summary" rows="3" {}
                }
                .row {
                    button type="submit" name="action" value="done" { "Mark done" }
                    button type="submit" name="action" value="wont-implement" { "Won't implement" }
                    button type="submit" name="action" value="reopen" { "Reopen" }
                }
            }
        }

        .card {
            h2 { "Priority" }
            form.row method="post" action={ (base) "/priority" } {
                select name="priority" {
                    @for value in Priority::VALUES {
                        option value=(value) selected[value == ticket.priority.as_str()] { (value) }
                    }
                }
                button type="submit" { "Set" }
            }
        }

        .card {
            h2 { "Journal" }
            form method="post" action={ (base) "/journal" } {
                label { "Heading (optional)"
                    input type="text" name="heading";
                }
                label { "Entry"
                    textarea name="text" rows="5" required {}
                }
                button type="submit" { "Append" }
            }
        }

        p { a href={ "/w/" (workstream) } { "Back to " (workstream) } }
    };
    page(
        &format!("{:03} {}", ticket.number, ticket.title),
        nav,
        content,
    )
}

pub fn ticket_edit(
    nav: &Nav,
    workstream: &str,
    ticket: &Ticket,
    base_hash: &str,
    conflict: bool,
) -> Markup {
    let action = format!("/w/{workstream}/t/{}/body", ticket.number);
    let content = html! {
        .card {
            h1 { "Edit " (format!("{:03}", ticket.number)) " " (ticket.title) }
            @if conflict {
                p.conflict {
                    "This ticket changed on disk since the editor was opened, so nothing was saved. \
                     Below is the current file; reapply your changes."
                }
            }
            p.hint { "Frontmatter (title, status, priority) is not editable here; use the forms on the ticket page." }
            form method="post" action=(action) {
                input type="hidden" name="base" value=(base_hash);
                textarea.editor name="body" rows="28" { (ticket.content) }
                .row {
                    button type="submit" { "Save" }
                    a.button href={ "/w/" (workstream) "/t/" (ticket.number) } { "Cancel" }
                }
            }
        }
    };
    page("Edit ticket", nav, content)
}

pub fn note(nav: &Nav, workstream: &str, name: &str, body: &str, as_markdown: bool) -> Markup {
    let content = html! {
        .card {
            h1 { (name) }
            @if as_markdown {
                .markdown { (markdown(body)) }
            } @else {
                pre { (body) }
            }
            p { a href={ "/w/" (workstream) } { "Back to " (workstream) } }
        }
    };
    page(name, nav, content)
}
