use clap::{Arg, ArgAction, Command};

pub fn build() -> Command {
    Command::new("flowy")
        .version(clap::crate_version!())
        .about("Manage bare git worktrees and markdown tickets")
        .arg_required_else_help(false)
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
        .subcommand(tui_cmd())
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
                .visible_alias("n")
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
                .visible_alias("ar")
                .about("Archive a workstream and remove its worktree")
                .arg(Arg::new("name").required(true).index(1))
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
                .visible_alias("un")
                .about("Restore an archived workstream")
                .arg(Arg::new("name").required(true).index(1)),
        )
        .subcommand(
            Command::new("rename")
                .visible_alias("mv")
                .about("Rename a workstream: branch, worktree dir, notes dir, and meta")
                .arg(
                    Arg::new("old")
                        .required(true)
                        .index(1)
                        .value_name("OLD_NAME"),
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
        .visible_alias("t")
        .about("Manage markdown tickets")
        .arg_required_else_help(true)
        .arg(
            Arg::new("workstream")
                .short('w')
                .long("workstream")
                .global(true)
                .value_name("NAME")
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
                        .help("Filter by status: pending, complete, wont-implement, all"),
                ),
        )
        .subcommand(
            Command::new("show")
                .visible_alias("s")
                .about("Display a ticket")
                .arg(Arg::new("id").required(true).index(1)),
        )
        .subcommand(
            Command::new("add")
                .visible_alias("a")
                .about("Create a new ticket")
                .arg(Arg::new("title").required(true).index(1))
                .arg(
                    Arg::new("description")
                        .long("description")
                        .short('d')
                        .value_name("TEXT"),
                ),
        )
        .subcommand(
            Command::new("done")
                .visible_alias("d")
                .about("Mark a ticket as done")
                .arg(Arg::new("id").required(true).index(1))
                .arg(Arg::new("summary").long("summary").value_name("TEXT"))
                .arg(
                    Arg::new("wont-implement")
                        .long("wont-implement")
                        .action(ArgAction::SetTrue),
                )
                .arg(Arg::new("reopen").long("reopen").action(ArgAction::SetTrue)),
        )
        .subcommand(
            Command::new("journal")
                .visible_alias("j")
                .about("Append a journal entry to a ticket")
                .arg(Arg::new("id").required(true).index(1))
                .arg(Arg::new("text").required(true).index(2))
                .arg(Arg::new("heading").long("heading").value_name("TEXT")),
        )
        .subcommand(
            Command::new("edit")
                .visible_alias("e")
                .about("Open a ticket in $EDITOR")
                .arg(Arg::new("id").required(true).index(1)),
        )
        .subcommand(
            Command::new("search")
                .visible_alias("/")
                .about("Search tickets with fuzzy matching")
                .arg(Arg::new("query").index(1)),
        )
        .subcommand(
            Command::new("status")
                .visible_alias("st")
                .about("Show workstream overview"),
        )
}

fn tui_cmd() -> Command {
    Command::new("tui").about("Launch the interactive terminal UI")
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
