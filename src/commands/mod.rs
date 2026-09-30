mod migrate;
mod notes;
mod ticket;
pub mod worktree;

use anyhow::{Context, Result, bail};
use clap::ArgMatches;
use clap_complete::Shell;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::core::{
    names::{archived_workstream_dir, safe_name, workstream_subdir},
    repo::{cwd_worktree, find_repo_root, worktrees_dir},
};

pub fn dispatch(matches: ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("init", sub)) => worktree::init(sub),
        Some(("clone", sub)) => worktree::clone(sub),
        Some(("worktree", sub)) => worktree::dispatch(sub),
        Some(("ticket", sub)) => ticket::dispatch(sub),
        Some(("notes", sub)) => notes::dispatch(sub),
        Some(("migrate", sub)) => migrate::run(sub),
        Some(("web", sub)) => {
            let root = get_repo_root(&matches)?;
            crate::web::run(
                &root,
                sub.get_one::<String>("host").expect("host has a default"),
                *sub.get_one::<u16>("port").expect("port has a default"),
                sub.get_flag("open"),
            )
        }
        Some(("tui", _)) | None => {
            let root = if let Some(p) = matches.get_one::<String>("repo-root") {
                PathBuf::from(p)
            } else {
                find_repo_root()?
            };
            crate::tui::run(&root)
        }
        Some(("completions", sub)) => {
            let shell = sub.get_one::<Shell>("shell").copied().unwrap();
            clap_complete::generate(
                shell,
                &mut crate::cli::build(),
                "flowy",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn get_repo_root(matches: &ArgMatches) -> Result<PathBuf> {
    if let Some(path) = matches.get_one::<String>("repo-root") {
        Ok(PathBuf::from(path))
    } else {
        find_repo_root()
    }
}

// Resolve workstream name from -w flag, the worktree cwd sits in, or the current
// git branch. The cwd takes precedence over the branch because a renamed branch
// no longer names its own worktree directory, and the branch-derived path would
// point at a directory that does not exist. The directory is the live worktree's
// notes if there is one, else the archived notes; a name with neither is an
// error, since writing there would create a stray `worktrees/<name>/`.
pub fn resolve_workstream(matches: &ArgMatches, root: &Path) -> Result<(String, PathBuf)> {
    let name = if let Some(n) = matches.get_one::<String>("workstream") {
        n.clone()
    } else if let Some(name) = cwd_worktree(root) {
        name
    } else {
        // Asked of the repo root rather than the process cwd, which can be any
        // other repository when --repo-root is given.
        let out = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(root)
            .output()
            .context("running git branch --show-current")?;
        if !out.status.success() {
            bail!(
                "cannot infer workstream: git branch --show-current failed in {}: {}; use -w NAME",
                root.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if branch.is_empty() {
            bail!("detached HEAD - cannot infer workstream; use -w NAME");
        }
        safe_name(&branch)
    };
    let worktree = worktrees_dir(root).join(&name);
    if worktree.is_dir() {
        return Ok((name, workstream_subdir(&worktree)));
    }
    let archived = archived_workstream_dir(root, &name);
    if archived.is_dir() {
        return Ok((name, archived));
    }
    bail!(
        "no workstream named '{name}': no worktree at {} and none archived",
        worktree.display()
    )
}

// A broken ticket is reported rather than allowed to hide the rest of its
// workstream from a view that spans many tickets.
pub fn warn_skipped(workstream: &str, errors: &[anyhow::Error]) {
    for e in errors {
        eprintln!("warning: {workstream}: skipped {e:#}");
    }
}

// Prose passed on the command line has to survive the shell's quoting rules,
// which is where multi-line markdown full of backticks and quotes goes wrong.
// Reading it from a file or a pipe sidesteps that entirely.
pub fn read_body_file(path: &str) -> Result<String> {
    if path == "-" {
        return read_stdin();
    }
    std::fs::read_to_string(path).with_context(|| format!("reading {path}"))
}

pub fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading stdin")?;
    Ok(buf)
}
