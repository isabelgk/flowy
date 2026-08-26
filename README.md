# flowy

`flowy` is a Rust CLI for managing bare git repos and per-branch worktrees with
markdown ticket files.

## On-disk layout

```
project/
  .bare/                              # bare git repository
  .flowy/cache.json                   # per-worktree status cache
  worktrees/<safe-name>/              # per-branch worktrees (/ replaced with -)
    .local/workstream/                # workstream state, colocated with the code
      meta.toml                       # branch + worktree path
      status.md                       # append-only activity log
      tickets/
        001.md                        # TOML frontmatter encodes status
  notes/workstreams/archive/<name>/   # archived workstreams, same shape
```

A workstream's notes live inside the worktree they describe, so `cd`-ing into a
worktree gives you its tickets too. Archived workstreams are the exception:
`wt archive` removes the worktree, so their notes move up to
`notes/workstreams/archive/` to outlive it.

`.local/` is added to the repo's `.bare/info/exclude` during scaffolding.
Without it every worktree would report dirty and `git worktree remove` would
refuse; the entry is local to your clone and never committed.

Ticket metadata lives in TOML frontmatter (`+++` delimiters) with `title` and
`status` fields. Status values: `"pending"`, `"complete"`, `"wont-implement"`.
Ticket IDs are zero-padded three-digit numbers (`001`, `002`, ...), claimed
atomically so that concurrent `ticket add` runs get distinct numbers rather than
the last writer overwriting the rest.

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
| `new` | `n` | Add a new worktree and its workstream directory |
| `ls` | | List worktrees with status, ticket counts, and branch age |
| `archive` | `ar` | Archive a workstream and remove its worktree |
| `unarchive` | `un` | Restore an archived workstream |
| `rename` | `mv` | Rename a workstream: branch, worktree dir, and meta |

`rename <old-name> <new-branch>` renames the git branch, moves the worktree to
the new safe-name, and updates `meta.toml`. Notes travel with the worktree
because they live inside it. Worktrees containing submodules (which `git
worktree move` refuses) are moved manually and their admin pointers repaired.
An archived workstream must be unarchived before it can be renamed.

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

### `flowy migrate`

Move workstream notes from the old `notes/workstreams/<name>/` layout into
`<worktree>/.local/workstream/`. Run once per repo scaffolded before notes moved
into the worktree. The archive is left alone, and notes whose worktree no longer
exists are reported rather than moved. `--dry-run` reports without moving.

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

## Claude Code skill

`skills/flowy/` teaches an agent both the CLI and the workstream style of
working - planning in writing, decomposing into verifiable tickets, journaling
implementation, and archiving on completion. Install it by copying it into your
skills directory:

```sh
just skill-link      # symlink into ~/.claude/skills; edits here take effect at once
just skill-install   # copy into ~/.claude/skills, pinned against further edits
```

Both overwrite whatever is already at `~/.claude/skills/flowy`. To scope the
skill to one project instead, copy it into that project's `.claude/skills/`.

It exposes seven verbs, and also triggers on the equivalent plain-language
request:

```
/flowy new igk/my-feature                      # branch + worktree + notes
/flowy plan igk-my-feature                     # planning.md + tickets
/flowy ticket igk-my-feature "crash on EOF"    # one ad-hoc ticket
/flowy next igk-my-feature                     # what to pick up, into next.md
/flowy implement igk-my-feature 3              # build, journal, mark done
/flowy milestone igk-my-feature no-lua         # freeze a reached plan, keep going
/flowy close igk-my-feature                    # audit + archive
```

`milestone` is the one that has no CLI counterpart. A workstream can outlive
several plans, and since `plan` overwrites `planning.md`, a plan that reached its
goal on a branch with more to do has to be frozen before the next one is written;
`milestone` moves it under `milestones/<nnn>-<slug>/` with a written summary. The
tickets stay where they are, in one numbering sequence for the life of the
workstream.
