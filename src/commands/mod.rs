mod ticket;
mod worktree;

use anyhow::Result;
use clap::ArgMatches;

pub fn dispatch(matches: ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("clone", sub)) => worktree::clone(sub),
        Some(("worktree", sub)) => worktree::dispatch(sub),
        Some(("ticket", sub)) => ticket::dispatch(sub),
        Some(("tui", _)) => {
            eprintln!("TUI not yet implemented (phase 2)");
            Ok(())
        }
        _ => Ok(()),
    }
}
