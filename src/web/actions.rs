use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::Path;

use crate::core::atomic::write_atomic;
use crate::core::notes::append_log;
use crate::core::ticket::{
    Priority, Ticket, append_journal, create_ticket, parse_ticket, rewrite_body,
    rewrite_status_and_append, ticket_path, write_priority,
};

pub struct Form(HashMap<String, String>);

impl Form {
    pub fn parse(body: &str) -> Self {
        Form(
            form_urlencoded::parse(body.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
        )
    }

    pub fn get(&self, key: &str) -> &str {
        self.0.get(key).map(String::as_str).unwrap_or_default()
    }

    // Empty and whitespace-only fields read as absent: a browser submits every
    // field in the form, so an untouched optional textarea arrives as "".
    pub fn filled(&self, key: &str) -> Option<&str> {
        Some(self.get(key).trim()).filter(|v| !v.is_empty())
    }
}

// FNV-1a over the body as it was loaded into the textarea. Not a security
// boundary; it exists so a save can tell that the file moved underneath it.
pub fn body_hash(body: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in body.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn read_ticket(ws_dir: &Path, number: u32) -> Result<(std::path::PathBuf, Ticket)> {
    let path = ticket_path(ws_dir, number);
    if !path.exists() {
        bail!("no ticket {number:03} in this workstream");
    }
    let ticket = parse_ticket(&path)?;
    Ok((path, ticket))
}

fn priority_field(form: &Form) -> Result<Option<Priority>> {
    match form.filled("priority") {
        Some(value) => Ok(Some(value.parse()?)),
        None => Ok(None),
    }
}

pub fn add_ticket(ws_dir: &Path, form: &Form) -> Result<u32> {
    let title = form
        .filled("title")
        .ok_or_else(|| anyhow::anyhow!("a ticket needs a title"))?;
    let (number, _) = create_ticket(
        ws_dir,
        title,
        priority_field(form)?,
        form.filled("description"),
    )?;
    append_log(ws_dir, &format!("added {number:03} ({title})"))?;
    Ok(number)
}

pub fn set_status(ws_dir: &Path, number: u32, form: &Form) -> Result<()> {
    let action = form.get("action");
    let (reopen, wont_impl) = match action {
        "done" => (false, false),
        "wont-implement" => (false, true),
        "reopen" => (true, false),
        other => bail!("unknown status action {other:?}"),
    };

    let (path, _) = read_ticket(ws_dir, number)?;
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let updated = rewrite_status_and_append(&content, reopen, wont_impl, form.filled("summary"));
    write_atomic(&path, updated).with_context(|| format!("writing {}", path.display()))?;

    append_log(ws_dir, &format!("{action} {number:03}"))
}

pub fn set_priority(ws_dir: &Path, number: u32, form: &Form) -> Result<()> {
    let priority = priority_field(form)?.ok_or_else(|| anyhow::anyhow!("no priority given"))?;
    let (path, _) = read_ticket(ws_dir, number)?;
    write_priority(&path, priority)?;
    append_log(
        ws_dir,
        &format!("priority {number:03} {}", priority.as_str()),
    )
}

pub fn add_journal(ws_dir: &Path, number: u32, form: &Form) -> Result<()> {
    let text = form
        .filled("text")
        .ok_or_else(|| anyhow::anyhow!("a journal entry needs text"))?;
    let (path, _) = read_ticket(ws_dir, number)?;
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let (updated, _) = append_journal(&content, text, form.filled("heading"));
    write_atomic(&path, updated).with_context(|| format!("writing {}", path.display()))
}

pub enum BodySave {
    Saved,
    // The file changed between the editor loading it and the save arriving, so
    // the submitted text would silently drop whatever `ticket edit`, an agent,
    // or a second tab wrote in between.
    Conflict,
}

pub fn set_body(ws_dir: &Path, number: u32, form: &Form) -> Result<BodySave> {
    let (path, ticket) = read_ticket(ws_dir, number)?;
    if form.get("base") != body_hash(&ticket.content) {
        return Ok(BodySave::Conflict);
    }

    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let updated = rewrite_body(&content, form.get("body"))?;
    write_atomic(&path, updated).with_context(|| format!("writing {}", path.display()))?;

    append_log(ws_dir, &format!("edited {number:03}"))?;
    Ok(BodySave::Saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_decodes_percent_and_plus() {
        let form = Form::parse("title=parser+bug&description=a%20%26%20b");
        assert_eq!(form.get("title"), "parser bug");
        assert_eq!(form.get("description"), "a & b");
    }

    #[test]
    fn form_filled_treats_blank_as_absent() {
        let form = Form::parse("a=&b=%20%20&c=x");
        assert_eq!(form.filled("a"), None);
        assert_eq!(form.filled("b"), None);
        assert_eq!(form.filled("c"), Some("x"));
        assert_eq!(form.filled("missing"), None);
    }

    #[test]
    fn body_hash_tracks_content() {
        assert_eq!(body_hash("same"), body_hash("same"));
        assert_ne!(body_hash("one"), body_hash("two"));
    }

    #[test]
    fn set_body_refuses_a_stale_base() {
        let dir = tempfile::tempdir().unwrap();
        let tickets = dir.path().join("tickets");
        std::fs::create_dir_all(&tickets).unwrap();
        let path = tickets.join("001.md");
        let original = "+++\nstatus = \"pending\"\ntitle = \"T\"\n+++\n\nOriginal.\n";
        std::fs::write(&path, original).unwrap();

        let stale = Form::parse("base=0000000000000000&body=Mine");
        assert!(matches!(
            set_body(dir.path(), 1, &stale).unwrap(),
            BodySave::Conflict
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

        let current = parse_ticket(&path).unwrap();
        let fresh = Form::parse(&format!("base={}&body=Mine", body_hash(&current.content)));
        assert!(matches!(
            set_body(dir.path(), 1, &fresh).unwrap(),
            BodySave::Saved
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "+++\nstatus = \"pending\"\ntitle = \"T\"\n+++\n\nMine\n"
        );
    }

    #[test]
    fn mutations_reject_a_missing_ticket() {
        let dir = tempfile::tempdir().unwrap();
        let form = Form::parse("action=done");
        assert!(set_status(dir.path(), 7, &form).is_err());
    }
}
