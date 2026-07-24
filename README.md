# flowy

`flowy` is a Rust CLI for managing bare git repos and per-branch worktrees with
markdown ticket files.

## On-disk layout

```
project/
  .bare/                              # bare git repository
  worktrees/<safe-name>/              # per-branch worktrees (/ replaced with -)
  notes/workstreams/<safe-name>/
    meta.toml                         # branch + worktree path
    tickets/
      001.md                          # TOML frontmatter encodes status
  notes/workstreams/archive/<name>/   # archived workstreams
```

Ticket metadata lives in TOML frontmatter (`+++` delimiters) with `title` and
`status` fields. Status values: `"pending"`, `"complete"`, `"wont-implement"`.
Ticket IDs are zero-padded three-digit numbers (`001`, `002`, ...).

## Global options

| Flag | Description |
|------|-------------|
| `--repo-root <PATH>` | Path to repo root (default: auto-detect via `.bare` walk-up) |
| `--json` | Output as JSON |

## Commands

### `flowy init <name>`

Scaffold a new bare-repo project in the current directory.

### `flowy clone <url> [name]`

Clone a bare repo and scaffold the project structure.

### `flowy worktree` (alias: `wt`)

Manage bare git worktrees.

| Subcommand | Alias | Description |
|------------|-------|-------------|
| `new` | `n` | Add a new worktree and notes directory |
| `ls` | | List worktrees with status, ticket counts, and branch age |
| `archive` | `ar` | Archive a workstream and remove its worktree |
| `unarchive` | `un` | Restore an archived workstream |
| `rename` | `mv` | Rename a workstream: branch, worktree dir, notes dir, and meta |

`rename <old-name> <new-branch>` renames the git branch, moves both the
worktree and notes directories to the new safe-name, and updates `meta.toml`.
Worktrees containing submodules (which `git worktree move` refuses) are moved
manually and their admin pointers repaired.

### `flowy ticket` (alias: `t`)

Manage markdown tickets. Workstream defaults to the current git branch.

| Subcommand | Alias | Description |
|------------|-------|-------------|
| `list` | `ls` | List tickets |
| `show` | `s` | Display a ticket |
| `add` | `a` | Create a new ticket |
| `done` | `d` | Mark a ticket as done |
| `journal` | `j` | Append a journal entry to a ticket |
| `edit` | `e` | Open a ticket in `$EDITOR` |
| `search` | `/` | Search tickets with fuzzy matching |
| `status` | `st` | Show workstream overview |

Use `-w / --workstream <NAME>` to target a workstream other than the current branch.

### `flowy tui`

Launch the interactive terminal UI.

### `flowy completions <shell>`

Generate shell completions. Supported shells: `bash`, `zsh`, `fish`,
`powershell`, `elvish`.

```sh
flowy completions zsh > "${fpath[1]}/_flowy"
flowy completions bash > /etc/bash_completion.d/flowy
flowy completions fish > ~/.config/fish/completions/flowy.fish
```
