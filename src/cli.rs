use clap::{Arg, ArgAction, Command};

use crate::core::names::parse_workstream_name;
use crate::core::ticket::Priority;

pub fn build() -> Command {
    Command::new("flowy")
        .version(clap::crate_version!())
        .about("Manage bare git worktrees and markdown tickets")
        .arg_required_else_help(true)
        .arg(
            Arg::new("repo-root")
                .long("repo-root")
                .global(true)
                .value_name("PATH")
                .help("Path to repo root (default: auto-detect via .bare)"),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Output as JSON"),
        )
        .subcommand(init_cmd())
        .subcommand(clone_cmd())
        .subcommand(worktree_cmd())
        .subcommand(ticket_cmd())
        .subcommand(notes_cmd())
        .subcommand(migrate_cmd())
        .subcommand(web_cmd())
        .subcommand(completions_cmd())
}

fn init_cmd() -> Command {
    Command::new("init")
        .about("Scaffold a new bare-repo project")
        .arg(Arg::new("name").required(true).index(1))
}

fn clone_cmd() -> Command {
    Command::new("clone")
        .about("Clone a bare repo and scaffold the project structure")
        .arg(Arg::new("url").required(true).index(1))
        .arg(Arg::new("name").index(2))
}

fn worktree_cmd() -> Command {
    Command::new("worktree")
        .visible_alias("wt")
        .about("Manage bare git worktrees")
        .arg_required_else_help(true)
        .subcommand(
            Command::new("new")
                .about("Add a new worktree and notes directory")
                .arg(
                    Arg::new("branch")
                        .short('b')
                        .required(true)
                        .value_name("BRANCH"),
                )
                .arg(Arg::new("start-point").index(1).value_name("START_POINT")),
        )
        .subcommand(
            Command::new("ls").about("List worktrees with status, ticket counts, and branch age"),
        )
        .subcommand(
            Command::new("archive")
                .about("Archive a workstream and remove its worktree")
                .arg(
                    Arg::new("name")
                        .required(true)
                        .index(1)
                        .value_parser(parse_workstream_name),
                )
                .arg(
                    Arg::new("force")
                        .long("force")
                        .short('f')
                        .action(ArgAction::SetTrue)
                        .help("Archive even if worktree is dirty"),
                ),
        )
        .subcommand(
            Command::new("unarchive")
                .about("Restore an archived workstream")
                .arg(
                    Arg::new("name")
                        .required(true)
                        .index(1)
                        .value_parser(parse_workstream_name),
                ),
        )
        .subcommand(
            Command::new("rename")
                .visible_alias("mv")
                .about("Rename a workstream: branch, worktree dir, notes dir, and meta")
                .arg(
                    Arg::new("old")
                        .required(true)
                        .index(1)
                        .value_name("OLD_NAME")
                        .value_parser(parse_workstream_name),
                )
                .arg(
                    Arg::new("new-branch")
                        .required(true)
                        .index(2)
                        .value_name("NEW_BRANCH"),
                ),
        )
}

fn ticket_cmd() -> Command {
    Command::new("ticket")
        .about("Manage markdown tickets")
        .arg_required_else_help(true)
        .arg(
            Arg::new("workstream")
                .short('w')
                .long("workstream")
                .global(true)
                .value_name("NAME")
                .value_parser(parse_workstream_name)
                .help("Workstream name (default: inferred from git branch)"),
        )
        .subcommand(
            Command::new("list")
                .visible_alias("ls")
                .about("List tickets")
                .arg(
                    Arg::new("status")
                        .long("status")
                        .value_name("STATUS")
                        .default_value("pending")
                        .value_parser(["pending", "complete", "wont-implement", "all"])
                        .help("Filter by status"),
                )
                .arg(
                    Arg::new("priority")
                        .long("priority")
                        .short('p')
                        .value_name("LEVEL")
                        .value_parser(Priority::VALUES)
                        .help("Only show tickets of this priority"),
                ),
        )
        .subcommand(
            Command::new("show")
                .about("Display a ticket")
                .arg(Arg::new("id").required(true).index(1)),
        )
        .subcommand(
            Command::new("add")
                .about("Create a new ticket")
                .arg(Arg::new("title").required(true).index(1))
                .arg(
                    Arg::new("priority")
                        .long("priority")
                        .short('p')
                        .value_name("LEVEL")
                        .value_parser(Priority::VALUES)
                        .help("Priority ranking (default: medium)"),
                )
                .arg(
                    Arg::new("description")
                        .long("description")
                        .short('d')
                        .value_name("TEXT"),
                )
                .arg(
                    Arg::new("body-file")
                        .long("body-file")
                        .value_name("PATH")
                        .conflicts_with("description")
                        .help("Read the ticket body from a file; `-` means stdin"),
                ),
        )
        .subcommand(
            Command::new("done")
                .about("Mark a ticket as done")
                .arg(Arg::new("id").required(true).index(1))
                .arg(Arg::new("summary").long("summary").value_name("TEXT"))
                .arg(
                    Arg::new("body-file")
                        .long("body-file")
                        .value_name("PATH")
                        .conflicts_with("summary")
                        .help("Read the closing summary from a file; `-` means stdin"),
                )
                .arg(
                    Arg::new("wont-implement")
                        .long("wont-implement")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("reopen")
                        .long("reopen")
                        .action(ArgAction::SetTrue)
                        .conflicts_with("wont-implement"),
                ),
        )
        .subcommand(
            Command::new("priority")
                .about("Set a ticket's priority")
                .arg(Arg::new("id").required(true).index(1))
                .arg(
                    Arg::new("level")
                        .required(true)
                        .index(2)
                        .value_parser(Priority::VALUES),
                ),
        )
        .subcommand(
            Command::new("journal")
                .about("Append a journal entry to a ticket")
                .arg(Arg::new("id").required(true).index(1))
                .arg(
                    Arg::new("text")
                        .index(2)
                        .required_unless_present("body-file")
                        .conflicts_with("body-file"),
                )
                .arg(
                    Arg::new("body-file")
                        .long("body-file")
                        .value_name("PATH")
                        .help("Read the entry from a file; `-` means stdin"),
                )
                .arg(Arg::new("heading").long("heading").value_name("TEXT")),
        )
        .subcommand(
            Command::new("edit")
                .about("Open a ticket in $EDITOR")
                .arg(Arg::new("id").required(true).index(1)),
        )
        .subcommand(
            Command::new("search")
                .about("Search tickets with fuzzy matching")
                .arg(Arg::new("query").index(1)),
        )
        .subcommand(Command::new("status").about("Show workstream overview"))
}

fn notes_cmd() -> Command {
    Command::new("notes")
        .about("Read and write a workstream's notes")
        .arg_required_else_help(true)
        .arg(
            Arg::new("workstream")
                .short('w')
                .long("workstream")
                .global(true)
                .value_name("NAME")
                .value_parser(parse_workstream_name)
                .help("Workstream name (default: inferred from git branch)"),
        )
        .subcommand(
            Command::new("ls")
                .about("List the workstream's notes files")
                .arg(
                    Arg::new("all")
                        .long("all")
                        .short('a')
                        .action(ArgAction::SetTrue)
                        .help("Include milestone directories and other files"),
                ),
        )
        .subcommand(
            Command::new("path")
                .about("Print the path to a note, or to the workstream directory")
                .arg(Arg::new("name").index(1).value_name("NAME")),
        )
        .subcommand(
            Command::new("show")
                .about("Print a note")
                .arg(Arg::new("name").required(true).index(1)),
        )
        .subcommand(
            Command::new("write")
                .about("Write a note, replacing it if it exists")
                .arg(Arg::new("name").required(true).index(1))
                .arg(
                    Arg::new("body-file")
                        .long("body-file")
                        .value_name("PATH")
                        .help("Read the body from a file (default: stdin)"),
                ),
        )
        .subcommand(
            Command::new("append")
                .about("Append to a note, creating it if absent")
                .arg(Arg::new("name").required(true).index(1))
                .arg(
                    Arg::new("body-file")
                        .long("body-file")
                        .value_name("PATH")
                        .help("Read the body from a file (default: stdin)"),
                ),
        )
        .subcommand(
            Command::new("log")
                .about("Append a timestamped entry to status.md")
                .arg(Arg::new("text").required(true).index(1)),
        )
        .subcommand(
            Command::new("milestone")
                .about("Freeze planning.md and next.md under milestones/<n>-<slug>/")
                .arg(Arg::new("slug").required(true).index(1)),
        )
}

fn migrate_cmd() -> Command {
    Command::new("migrate")
        .about("Move a repo from an older flowy layout onto the current one")
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .action(ArgAction::SetTrue)
                .help("Report what would move without moving it"),
        )
}

fn web_cmd() -> Command {
    Command::new("web")
        .about("Serve the workstream browser over HTTP")
        .arg(
            Arg::new("port")
                .long("port")
                .short('p')
                .value_name("PORT")
                .value_parser(clap::value_parser!(u16))
                .default_value("0")
                .help("Port to listen on (default: a free one, printed at startup)"),
        )
        .arg(
            Arg::new("host")
                .long("host")
                .value_name("HOST")
                .default_value("127.0.0.1")
                .help("Address to bind"),
        )
        .arg(
            Arg::new("open")
                .long("open")
                .action(ArgAction::SetTrue)
                .help("Open the served URL in a browser"),
        )
}

fn completions_cmd() -> Command {
    Command::new("completions")
        .about("Generate shell completions")
        .arg(
            Arg::new("shell")
                .required(true)
                .value_parser(clap::value_parser!(clap_complete::Shell)),
        )
}
