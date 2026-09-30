mod common;

use common::TestRepo;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
    port: u16,
}

impl Server {
    fn start(repo: &TestRepo) -> Server {
        let bin = assert_cmd::cargo::cargo_bin("flowy");
        let mut child = Command::new(bin)
            .envs(common::GIT_ENV)
            .arg("--repo-root")
            .arg(&repo.root)
            .args(["web", "--port", "0"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn flowy web");

        // The banner is printed after the listener is bound, so reading it is
        // also how the test waits for the port to accept connections.
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().expect("piped stdout"))
            .read_line(&mut line)
            .expect("read banner");
        let port = line
            .rsplit(':')
            .next()
            .expect("banner has a port")
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("unexpected banner: {line:?}"));

        Server { child, port }
    }

    fn get(&self, path: &str) -> (u16, String) {
        self.send("GET", path, None, "")
    }

    fn post(&self, path: &str, body: &str) -> (u16, String) {
        let origin = format!("http://127.0.0.1:{}", self.port);
        self.send("POST", path, Some(&origin), body)
    }

    fn send(&self, method: &str, path: &str, origin: Option<&str>, body: &str) -> (u16, String) {
        let host = format!("127.0.0.1:{}", self.port);
        self.send_to(&host, method, path, origin, body)
    }

    fn send_to(
        &self,
        host: &str,
        method: &str,
        path: &str,
        origin: Option<&str>,
        body: &str,
    ) -> (u16, String) {
        let mut request =
            format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
        if let Some(origin) = origin {
            request.push_str(&format!("Origin: {origin}\r\n"));
        }
        if method == "POST" {
            request.push_str("Content-Type: application/x-www-form-urlencoded\r\n");
            request.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        request.push_str("\r\n");
        request.push_str(body);

        let mut stream =
            TcpStream::connect(("127.0.0.1", self.port)).expect("connect to flowy web");
        stream.write_all(request.as_bytes()).expect("send request");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read response");

        let status = response
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or_else(|| panic!("no status line in {response:?}"));
        (status, response)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn repo_with_worktree() -> TestRepo {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "feat/web"])
        .assert()
        .success();
    repo
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| {
            format!(
                "{k}={}",
                form_urlencoded::byte_serialize(v.as_bytes()).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn base_hash(page: &str) -> String {
    let marker = "name=\"base\" value=\"";
    let start = page.find(marker).expect("edit page has a base hash") + marker.len();
    let rest = &page[start..];
    rest[..rest.find('"').expect("closing quote")].to_string()
}

#[test]
fn serves_the_ticket_list_and_detail() {
    let repo = repo_with_worktree();
    repo.cmd()
        .args([
            "t",
            "add",
            "-w",
            "feat-web",
            "-p",
            "high",
            "Crash on empty config",
        ])
        .assert()
        .success();
    let server = Server::start(&repo);

    let (status, list) = server.get("/w/feat-web");
    assert_eq!(status, 200);
    assert!(list.contains("Crash on empty config"), "{list}");
    assert!(list.contains("prio-high"), "{list}");

    let (status, detail) = server.get("/w/feat-web/t/1");
    assert_eq!(status, 200);
    assert!(detail.contains("Crash on empty config"), "{detail}");

    assert_eq!(server.get("/w/feat-web/t/99").0, 404);
    assert_eq!(server.get("/w/no-such-workstream").0, 404);
}

#[test]
fn posting_a_ticket_writes_it_to_disk() {
    let repo = repo_with_worktree();
    let server = Server::start(&repo);

    let (status, response) = server.post(
        "/w/feat-web/tickets",
        &form(&[
            ("title", "Filed from the browser"),
            ("priority", "low"),
            ("description", "Body with **markdown**."),
        ]),
    );
    assert_eq!(status, 303);
    assert!(response.contains("Location: /w/feat-web/t/1"), "{response}");

    let written = std::fs::read_to_string(repo.ticket("feat-web", 1)).expect("ticket written");
    assert!(
        written.contains("title = \"Filed from the browser\""),
        "{written}"
    );
    assert!(written.contains("priority = \"low\""), "{written}");
    assert!(written.contains("Body with **markdown**."), "{written}");

    let log = std::fs::read_to_string(repo.ws_dir("feat-web").join("status.md")).expect("log");
    assert!(log.contains("added 001 (Filed from the browser)"), "{log}");
}

#[test]
fn a_cross_origin_post_changes_nothing() {
    let repo = repo_with_worktree();
    let server = Server::start(&repo);

    let body = form(&[("title", "Forged")]);
    let (status, _) = server.send(
        "POST",
        "/w/feat-web/tickets",
        Some("http://evil.example"),
        &body,
    );
    assert_eq!(status, 403);

    // A form posted with no Origin at all is refused on the same grounds.
    let (status, _) = server.send("POST", "/w/feat-web/tickets", None, &body);
    assert_eq!(status, 403);

    assert!(!repo.ticket("feat-web", 1).exists());
}

#[test]
fn a_foreign_host_is_refused_even_with_a_matching_origin() {
    let repo = repo_with_worktree();
    let server = Server::start(&repo);
    let evil = format!("evil.example:{}", server.port);

    let (status, page) = server.send_to(&evil, "GET", "/w/feat-web", None, "");
    assert_eq!(status, 403);
    assert!(!page.contains("feat-web"), "{page}");

    let origin = format!("http://{evil}");
    let body = form(&[("title", "Rebound")]);
    let (status, _) = server.send_to(&evil, "POST", "/w/feat-web/tickets", Some(&origin), &body);
    assert_eq!(status, 403);
    assert!(!repo.ticket("feat-web", 1).exists());

    let localhost = format!("localhost:{}", server.port);
    assert_eq!(
        server.send_to(&localhost, "GET", "/w/feat-web", None, "").0,
        200
    );
}

#[test]
fn a_workstream_name_cannot_escape_the_worktrees_dir() {
    let repo = repo_with_worktree();
    let outside = tempfile::tempdir().expect("tempdir");
    let server = Server::start(&repo);

    let absolute = form_urlencoded::byte_serialize(outside.path().to_str().unwrap().as_bytes())
        .collect::<String>()
        .replace('+', "%20");
    let target = outside.path().join(".local/workstream/tickets/001.md");
    for name in [absolute.as_str(), "..%2Fworktrees%2Ffeat-web"] {
        let body = form(&[("title", "Escaped")]);
        let (status, _) = server.post(&format!("/w/{name}/tickets"), &body);
        assert_eq!(status, 404, "{name}");
        assert_eq!(server.get(&format!("/w/{name}")).0, 404, "{name}");
    }
    assert!(!target.exists());
    assert!(!repo.ticket("feat-web", 1).exists());
}

fn send_head(server: &Server, path: &str, content_length: usize, body: &str) -> String {
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\n\
         Content-Type: application/x-www-form-urlencoded\r\n\
         Content-Length: {content_length}\r\nConnection: close\r\n\r\n{body}",
        port = server.port
    );
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).expect("connect");
    stream.write_all(request.as_bytes()).expect("send request");
    stream
        .shutdown(std::net::Shutdown::Write)
        .expect("close write half");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    response
}

#[test]
fn an_oversized_or_cut_off_body_is_refused() {
    let repo = repo_with_worktree();
    let server = Server::start(&repo);

    // Past the size tiny_http reads eagerly, so the body reaches flowy's reader.
    let body = form(&[("title", "Partial"), ("description", &"x".repeat(4096))]);
    let response = send_head(&server, "/w/feat-web/tickets", 64 << 20, &body);
    assert!(response.starts_with("HTTP/1.1 413"), "{response}");

    let response = send_head(&server, "/w/feat-web/tickets", body.len() + 100, &body);
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");

    assert!(!repo.ticket("feat-web", 1).exists());
}

#[test]
fn names_outside_ascii_and_with_url_syntax_are_encoded() {
    let repo = TestRepo::new();
    repo.cmd()
        .args(["wt", "new", "-b", "caf\u{e9}"])
        .assert()
        .success();
    repo.cmd()
        .args(["wt", "new", "-b", "fix#1"])
        .assert()
        .success();
    let server = Server::start(&repo);

    let (status, index) = server.get("/");
    assert_eq!(status, 303, "{index}");
    assert!(index.contains("Location: /w/caf%C3%A9"), "{index}");

    let (status, response) = server.post("/w/caf%C3%A9/tickets", &form(&[("title", "Accented")]));
    assert_eq!(status, 303, "{response}");
    assert!(
        response.contains("Location: /w/caf%C3%A9/t/1"),
        "{response}"
    );

    let (status, page) = server.get("/w/caf%C3%A9");
    assert_eq!(status, 200);
    assert!(page.contains("href=\"/w/caf%C3%A9/t/1\""), "{page}");
    assert!(page.contains("href=\"/w/fix%231\""), "{page}");
    assert_eq!(server.get("/w/fix%231").0, 200);
}

#[test]
fn a_slow_upload_does_not_stall_other_requests() {
    let repo = repo_with_worktree();
    let server = Server::start(&repo);

    // Headers promise more body than is ever sent, past the size tiny_http
    // would read eagerly, and the connection stays open.
    let mut slow = TcpStream::connect(("127.0.0.1", server.port)).expect("connect");
    let head = format!(
        "POST /w/feat-web/tickets HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
         Origin: http://127.0.0.1:{port}\r\nContent-Length: 100000\r\n\r\ntitle=Slow",
        port = server.port
    );
    slow.write_all(head.as_bytes()).expect("send head");

    let mut fast = TcpStream::connect(("127.0.0.1", server.port)).expect("connect");
    fast.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let request = format!(
        "GET /w/feat-web HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
        server.port
    );
    fast.write_all(request.as_bytes()).expect("send request");
    let mut response = String::new();
    fast.read_to_string(&mut response)
        .expect("a page while the upload is still open");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    drop(slow);
}

#[test]
fn the_editor_saves_a_body_and_refuses_a_stale_one() {
    let repo = repo_with_worktree();
    repo.cmd()
        .args(["t", "add", "-w", "feat-web", "Editable"])
        .assert()
        .success();
    let server = Server::start(&repo);

    let (status, page) = server.get("/w/feat-web/t/1/edit");
    assert_eq!(status, 200);
    let hash = base_hash(&page);

    let (status, response) = server.post(
        "/w/feat-web/t/1/body",
        &form(&[("base", &hash), ("body", "Rewritten in the textarea.\r\n")]),
    );
    assert_eq!(status, 303);
    assert!(response.contains("Location: /w/feat-web/t/1"), "{response}");

    let written = std::fs::read_to_string(repo.ticket("feat-web", 1)).expect("ticket readable");
    assert_eq!(
        written,
        "+++\nstatus = \"pending\"\ntitle = \"Editable\"\n+++\n\nRewritten in the textarea.\n"
    );

    // The same form submitted twice: the second save is working from a body that
    // no longer matches the file, so it is bounced to the conflict page.
    let (status, response) = server.post(
        "/w/feat-web/t/1/body",
        &form(&[("base", &hash), ("body", "Clobbered.")]),
    );
    assert_eq!(status, 303);
    assert!(
        response.contains("Location: /w/feat-web/t/1/edit?conflict=1"),
        "{response}"
    );
    assert_eq!(
        std::fs::read_to_string(repo.ticket("feat-web", 1)).unwrap(),
        written
    );
}

#[test]
fn status_priority_and_journal_match_the_cli_format() {
    let repo = repo_with_worktree();
    repo.cmd()
        .args(["t", "add", "-w", "feat-web", "Workable"])
        .assert()
        .success();
    let server = Server::start(&repo);

    assert_eq!(
        server
            .post("/w/feat-web/t/1/priority", &form(&[("priority", "high")]))
            .0,
        303
    );
    assert_eq!(
        server
            .post(
                "/w/feat-web/t/1/journal",
                &form(&[("heading", "first look"), ("text", "Reproduced.")])
            )
            .0,
        303
    );
    assert_eq!(
        server
            .post(
                "/w/feat-web/t/1/status",
                &form(&[("action", "done"), ("summary", "Shipped.")])
            )
            .0,
        303
    );

    let written = std::fs::read_to_string(repo.ticket("feat-web", 1)).expect("ticket readable");
    assert!(written.contains("status = \"complete\""), "{written}");
    assert!(written.contains("priority = \"high\""), "{written}");
    assert!(written.contains("## Journal"), "{written}");
    assert!(written.contains("] first look"), "{written}");
    assert!(written.contains("## Summary\n\nShipped."), "{written}");

    // The CLI has to keep reading what the web UI wrote.
    repo.cmd()
        .args(["t", "show", "1", "-w", "feat-web"])
        .assert()
        .success();
}

#[test]
fn the_sidebar_counts_follow_a_mutation() {
    let repo = repo_with_worktree();
    let server = Server::start(&repo);

    // Prime the sidebar snapshot, then change the counts behind it.
    let (_, before) = server.get("/w/feat-web");
    assert!(before.contains("0/0 open"), "{before}");

    assert_eq!(
        server
            .post("/w/feat-web/tickets", &form(&[("title", "Counted")]))
            .0,
        303
    );

    let (_, after) = server.get("/w/feat-web");
    assert!(after.contains("1/1 open"), "{after}");

    assert_eq!(
        server
            .post("/w/feat-web/t/1/status", &form(&[("action", "done")]))
            .0,
        303
    );

    let (_, closed) = server.get("/w/feat-web");
    assert!(closed.contains("0/1 open"), "{closed}");
}

#[test]
fn favicon_is_answered_without_a_page() {
    let repo = repo_with_worktree();
    let server = Server::start(&repo);

    let (status, response) = server.get("/favicon.ico");
    assert_eq!(status, 204);
    assert!(!response.contains("<html"), "{response}");
}

#[test]
fn every_page_carries_the_sidebar_toggle() {
    let repo = repo_with_worktree();
    let server = Server::start(&repo);

    let (status, page) = server.get("/w/feat-web");
    assert_eq!(status, 200);
    assert!(page.contains("class=\"nav-toggle\""), "{page}");
    assert!(page.contains("aria-controls=\"workstreams\""), "{page}");
    assert!(page.contains("id=\"workstreams\""), "{page}");
    // Applied before first paint, so a collapsed sidebar does not flash open.
    // The stylesheet and the toggle script mention the class and key too, so
    // look for the restore itself, inside <head>.
    let head = page.split("</head>").next().unwrap();
    assert!(head.contains("getItem('flowy-nav')"), "{page}");
}
