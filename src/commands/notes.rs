use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use owo_colors::OwoColorize;
use std::path::Path;

use super::{get_repo_root, read_body_file, read_stdin, resolve_workstream};
use crate::core::{
    notes::{
        WELL_KNOWN, append_log, append_note, freeze_milestone, milestones_dir, resolve_note,
        write_note,
    },
    ticket::{Status, load_tickets},
};
use crate::output::print_json;

pub fn dispatch(matches: &ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("ls", sub)) => ls(sub),
        Some(("path", sub)) => path(sub),
        Some(("show", sub)) => show(sub),
        Some(("write", sub)) => write(sub),
        Some(("append", sub)) => append(sub),
        Some(("log", sub)) => log(sub),
        Some(("milestone", sub)) => milestone(sub),
        _ => Ok(()),
    }
}

fn ls(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let all = matches.get_flag("all");
    let (name, ws_dir) = resolve_workstream(matches, &root)?;

    let mut entries: Vec<serde_json::Value> = WELL_KNOWN
        .iter()
        .map(|file| {
            let path = ws_dir.join(file);
            serde_json::json!({
                "name": file,
                "exists": path.is_file(),
                "bytes": std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            })
        })
        .collect();

    if all {
        let mut extra: Vec<String> = std::fs::read_dir(&ws_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "tickets" && !WELL_KNOWN.contains(&n.as_str()))
            .collect();
        extra.sort();
        for n in extra {
            let path = ws_dir.join(&n);
            entries.push(serde_json::json!({
                "name": n,
                "exists": true,
                "dir": path.is_dir(),
                "bytes": std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            }));
        }
    }

    if json {
        return print_json(&serde_json::json!({
            "workstream": name,
            "dir": ws_dir.to_string_lossy(),
            "notes": entries,
        }));
    }

    println!("{}", ws_dir.display().to_string().dimmed());
    for e in &entries {
        let name = e["name"].as_str().unwrap_or("");
        if e["exists"].as_bool().unwrap_or(false) {
            let suffix = if e["dir"].as_bool().unwrap_or(false) {
                "/".to_string()
            } else {
                format!(" ({} bytes)", e["bytes"])
            };
            println!("  {name}{suffix}");
        } else {
            println!("  {}", format!("{name} (absent)").dimmed());
        }
    }
    Ok(())
}

fn path(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let (_, ws_dir) = resolve_workstream(matches, &root)?;

    let target = match matches.get_one::<String>("name") {
        Some(name) => resolve_note(&ws_dir, name)?,
        None => ws_dir,
    };

    if json {
        return print_json(&serde_json::json!({
            "path": target.to_string_lossy(),
            "exists": target.exists(),
        }));
    }
    println!("{}", target.display());
    Ok(())
}

fn show(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let name = matches.get_one::<String>("name").unwrap();
    let (_, ws_dir) = resolve_workstream(matches, &root)?;
    let target = resolve_note(&ws_dir, name)?;

    let content = std::fs::read_to_string(&target)
        .with_context(|| format!("reading {}", target.display()))?;

    if json {
        return print_json(&serde_json::json!({
            "path": target.to_string_lossy(),
            "content": content,
        }));
    }
    print!("{content}");
    Ok(())
}

fn write(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let name = matches.get_one::<String>("name").unwrap();
    let (_, ws_dir) = resolve_workstream(matches, &root)?;
    let target = resolve_note(&ws_dir, name)?;

    let existed = target.is_file();
    write_note(&target, &body(matches)?)?;
    report_write(json, &target, if existed { "Replaced" } else { "Wrote" })
}

fn append(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let name = matches.get_one::<String>("name").unwrap();
    let (_, ws_dir) = resolve_workstream(matches, &root)?;
    let target = resolve_note(&ws_dir, name)?;

    append_note(&target, &body(matches)?)?;
    report_write(json, &target, "Appended to")
}

fn log(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let text = matches.get_one::<String>("text").unwrap();
    let (_, ws_dir) = resolve_workstream(matches, &root)?;

    if !ws_dir.is_dir() {
        bail!("no workstream at {}", ws_dir.display());
    }
    append_log(&ws_dir, text.trim())?;

    let path = ws_dir.join("status.md");
    if json {
        return print_json(&serde_json::json!({"path": path.to_string_lossy()}));
    }
    println!("Logged to {}", path.display());
    Ok(())
}

fn milestone(matches: &ArgMatches) -> Result<()> {
    let root = get_repo_root(matches)?;
    let json = matches.get_flag("json");
    let slug = matches.get_one::<String>("slug").unwrap();
    let (_, ws_dir) = resolve_workstream(matches, &root)?;

    // Reported rather than refused: whether an open ticket belongs to this
    // milestone or the next one is a judgement the caller has to make, but a
    // milestone frozen over a live queue is worth noticing.
    let pending = load_tickets(&ws_dir)
        .unwrap_or_default()
        .iter()
        .filter(|t| t.status == Status::Pending)
        .count();

    let ms = freeze_milestone(&ws_dir, slug)?;

    if json {
        return print_json(&serde_json::json!({
            "number": ms.number,
            "slug": ms.slug,
            "dir": ms.dir.to_string_lossy(),
            "frozen": ms.frozen,
            "summary": ms.dir.join("summary.md").to_string_lossy(),
            "pending_tickets": pending,
            "index": milestones_dir(&ws_dir).join("README.md").to_string_lossy(),
        }));
    }

    println!("Froze {} into {}", ms.frozen.join(", "), ms.dir.display());
    println!(
        "Write the summary at {}",
        ms.dir.join("summary.md").display()
    );
    if pending > 0 {
        println!(
            "{}",
            format!("{pending} ticket(s) still pending in this workstream").yellow()
        );
    }
    Ok(())
}

// stdin is the default source so a quoted heredoc can carry markdown that would
// need escaping as an argv string.
fn body(matches: &ArgMatches) -> Result<String> {
    match matches.get_one::<String>("body-file") {
        Some(path) => read_body_file(path),
        None => read_stdin(),
    }
}

fn report_write(json: bool, target: &Path, verb: &str) -> Result<()> {
    let bytes = std::fs::metadata(target).map(|m| m.len()).unwrap_or(0);
    if json {
        return print_json(&serde_json::json!({
            "path": target.to_string_lossy(),
            "bytes": bytes,
        }));
    }
    println!("{verb} {} ({bytes} bytes)", target.display());
    Ok(())
}
