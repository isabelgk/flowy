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
      planning.md                     # the current plan, if there is one
      next.md                         # rolling recommendation
      milestones/<nnn>-<slug>/        # a plan carried to its goal, frozen
      tickets/
        001.md                        # TOML frontmatter encodes status + priority
  notes/workstreams/archive/<name>/   # archived workstreams, same shape
```

A workstream's notes live inside the worktree they describe, so `cd`-ing into a
worktree gives you its tickets too. Archived workstreams are the exception:
`wt archive` removes the worktree, so their notes move up to
`notes/workstreams/archive/` to outlive it.

`.local/` is added to the repo's `.bare/info/exclude` during scaffolding.
Without it every worktree would report dirty and `git worktree remove` would
refuse; the entry is local to your clone and never committed.

Ticket metadata lives in TOML frontmatter (`+++` delimiters) with `title`,
`status`, and `priority` fields. Status values: `"pending"`, `"complete"`,
`"wont-implement"`. Priority values: `"high"`, `"medium"`, `"low"`; a ticket with
no `priority` key ranks as medium, so the field can stay absent.
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
| `priority` | `p` | Set a ticket's priority |
| `journal` | `j` | Append a journal entry to a ticket |
| `edit` | `e` | Open a ticket in `$EDITOR` |
| `search` | `/` | Search tickets with fuzzy matching |
| `status` | `st` | Show workstream overview |

Use `-w / --workstream <NAME>` to target a workstream other than the current branch.

Tickets list high priority first, then medium, then low, and by number within a
rank. `add` takes `-p / --priority <high|medium|low>`, `priority <ID> <LEVEL>`
changes it afterward, and `list -p <LEVEL>` narrows the listing to one rank.

```sh
flowy t add -p high "Crash on empty config"
flowy t priority 3 low
flowy t list -p high
```

`add`, `done`, and `journal` all take prose. Passing it inline (`-d`,
`--summary`, or the positional journal text) is fine for a phrase, but multi-line
markdown containing quotes or backticks has to survive the shell's quoting rules
first. `--body-file <PATH>` reads it out of band instead, and `--body-file -`
reads stdin:

```sh
flowy t add "Parser rejects empty input" --body-file - <<'EOF'
## Scope
`parse_header` in src/parse.rs.

## Acceptance
Empty input returns Err rather than panicking.
EOF
```

`--body-file` is mutually exclusive with the inline form on each subcommand.

### `flowy notes` (alias: `n`)

Read and write the non-ticket files in a workstream: `planning.md`, `next.md`,
`status.md`, `meta.toml`, and anything under `milestones/`. Workstream defaults
to the current git branch, same as `ticket`.

| Subcommand | Description |
|------------|-------------|
| `ls` | List the workstream's notes files, marking the absent ones |
| `path [NAME]` | Print the path to a note, or to the workstream directory |
| `show <NAME>` | Print a note |
| `write <NAME>` | Write a note, replacing it if it exists |
| `append <NAME>` | Append to a note, creating it if absent |
| `log <TEXT>` | Append a timestamped entry to `status.md` |
| `milestone <SLUG>` | Freeze `planning.md` and `next.md` under `milestones/<nnn>-<slug>/` |

`NAME` is either an alias (`planning`, `next`, `status`, `meta`) or a path
relative to the workstream directory (`milestones/001-parser/summary.md`).
Absolute paths and `..` are refused: the workstream directory is the write
boundary. `write` and `append` read the body from stdin unless `--body-file
<PATH>` is given, create parent directories as needed, and refuse an empty body
rather than truncating a note.

```sh
flowy notes write planning < plan.md
flowy notes append planning <<'EOF'
## Amendment
The `--fast` path turned out to be unnecessary.
EOF
flowy notes log "Planning complete. 4 tickets created."
```

`milestone` does the mechanical half of a milestone boundary: it picks the next
number, creates the directory, moves `planning.md` and `next.md` into it (moves,
not copies, so there is only one live plan), and appends a boundary line to
`status.md`. Tickets stay where they are, in one numbering sequence for the life
of the workstream. Writing `summary.md` is left to you. Pending tickets are
reported rather than treated as an error, since whether an open ticket belongs to
this milestone or the next one is a judgement call.

### `flowy migrate`

Move workstream notes from the old `notes/workstreams/<name>/` layout into
`<worktree>/.local/workstream/`. Run once per repo scaffolded before notes moved
into the worktree. The archive is left alone, and notes whose worktree no longer
exists are reported rather than moved. `--dry-run` reports without moving.

### `flowy tui`

Launch the interactive terminal UI. The ticket list ranks high priority first;
`s` cycles the selected ticket's status and `p` cycles its priority, writing
each through to the file. The list opens filtered to pending tickets; `o`
toggles completed and wont-implement tickets back in, and composes with the `i`
search filter.

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

`milestone` pairs with `flowy notes milestone`: the CLI freezes the plan, and the
skill decides whether the boundary is real and writes the summary. A workstream
can outlive several plans, and since `plan` overwrites `planning.md`, a plan that
reached its goal on a branch with more to do has to be frozen before the next one
is written.
