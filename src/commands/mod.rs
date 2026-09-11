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
    names::{safe_name, workstream_dir},
    repo::{cwd_worktree, find_repo_root},
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
// point at a directory that does not exist.
pub fn resolve_workstream(matches: &ArgMatches, root: &Path) -> Result<(String, PathBuf)> {
    let name = if let Some(n) = matches.get_one::<String>("workstream") {
        safe_name(n)
    } else if let Some(name) = cwd_worktree(root) {
        name
    } else {
        let out = Command::new("git")
            .args(["branch", "--show-current"])
            .output()
            .context("running git branch --show-current")?;
        let branch = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if branch.is_empty() {
            bail!("detached HEAD - cannot infer workstream; use -w NAME");
        }
        safe_name(&branch)
    };
    let dir = workstream_dir(root, &name);
    Ok((name, dir))
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
