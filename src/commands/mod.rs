mod ticket;
mod worktree;

use anyhow::Result;
use clap::ArgMatches;
use clap_complete::Shell;

pub fn dispatch(matches: ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("clone", sub)) => worktree::clone(sub),
        Some(("worktree", sub)) => worktree::dispatch(sub),
        Some(("ticket", sub)) => ticket::dispatch(sub),
        Some(("tui", _)) => {
            eprintln!("TUI not yet implemented (phase 2)");
            Ok(())
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
