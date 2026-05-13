mod ticket;
pub mod worktree;

use anyhow::Result;
use clap::ArgMatches;
use clap_complete::Shell;
use std::path::PathBuf;

use crate::core::repo::find_repo_root;

pub fn dispatch(matches: ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("init", sub)) => worktree::init(sub),
        Some(("clone", sub)) => worktree::clone(sub),
        Some(("worktree", sub)) => worktree::dispatch(sub),
        Some(("ticket", sub)) => ticket::dispatch(sub),
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
