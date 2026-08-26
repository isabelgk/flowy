mod migrate;
mod ticket;
pub mod worktree;

use anyhow::{Context, Result};
use clap::ArgMatches;
use clap_complete::Shell;
use std::io::Read as _;
use std::path::PathBuf;

use crate::core::repo::find_repo_root;

pub fn dispatch(matches: ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("init", sub)) => worktree::init(sub),
        Some(("clone", sub)) => worktree::clone(sub),
        Some(("worktree", sub)) => worktree::dispatch(sub),
        Some(("ticket", sub)) => ticket::dispatch(sub),
        Some(("migrate", sub)) => migrate::run(sub),
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
