# flowy

`flowy` is a Rust CLI for managing bare git repos and per-branch worktrees with
markdown ticket files.

## On-disk layout

```
project/
  .bare/                              # bare git repository
  .flowy/cache.json                   # per-worktree status cache
  .flowy/archive/<name>/              # archived workstreams, same shape
  worktrees/<safe-name>/              # per-branch worktrees (/ replaced with -)
    .local/workstream/                # workstream state, colocated with the code
      meta.toml                       # branch + worktree path
      status.md                       # append-only activity log
      planning.md                     # the current plan, if there is one
      next.md                         # rolling recommendation
      milestones/<n>-<slug>/          # a plan carried to its goal, frozen
      tickets/
        1.md                          # TOML frontmatter encodes status + priority
```

A workstream's notes live inside the worktree they describe, so `cd`-ing into a
worktree gives you its tickets too. Archived workstreams are the exception:
`wt archive` removes the worktree, so their notes move up to `.flowy/archive/`
to outlive it. `cache.json` is disposable, but `.flowy/` as a whole is not.

`.local/` is added to the repo's `.bare/info/exclude` during scaffolding.
Without it every worktree would report dirty and `git worktree remove` would
refuse; the entry is local to your clone and never committed.

Ticket metadata lives in TOML frontmatter (`+++` delimiters) with `title`,
`status`, and `priority` fields. Status values: `"pending"`, `"complete"`,
`"wont-implement"`. Priority values: `"high"`, `"medium"`, `"low"`; a ticket with
no `priority` key ranks as medium, so the field can stay absent.
Ticket IDs are plain numbers (`1`, `2`, ...); files from older versions named
`001.md` are read the same way. New numbers are claimed atomically so that
concurrent `ticket add` runs get distinct numbers rather than the last writer
overwriting the rest.

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
| `milestone <SLUG>` | Freeze `planning.md` and `next.md` under `milestones/<n>-<slug>/` |

`NAME` is either an alias (`planning`, `next`, `status`, `meta`) or a path
relative to the workstream directory (`milestones/1-parser/summary.md`).
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

### `flowy web`

Serve a browser UI for the repo's workstreams over HTTP.

```sh
flowy web                      # binds a free port and prints the URL
flowy web --port 9000 --open   # pin the port and open a browser
```

The port is chosen by the OS unless `--port` pins one, so several repos can be
served at once without colliding. The URL is printed as soon as the listener is
up.

The sidebar lists live worktrees; each one gets a filterable ticket list, a
detail page rendering the ticket's markdown, and forms for the same mutations
the CLI performs - add, done, won't-implement, reopen, priority, and journal.
`Edit body` opens a textarea over the markdown below the frontmatter; title,
status, and priority stay owned by their own forms so a save cannot corrupt the
fields every other command parses. A save carries a hash of the body it was
opened with and is refused if the file changed meanwhile, so a browser tab left
open overnight cannot overwrite what `ticket edit` or an agent wrote in the
interim.

Mutations are written through the same code the CLI uses and logged to
`status.md`, with one addition: a body edit logs `edited <id>`, which
`ticket edit` does not, because an edit made in a browser is otherwise invisible.

Archived workstreams are not served; unarchive one to see it.

The sidebar collapses from the button in the top left, and the choice is
remembered in the browser; it is applied before the first paint, so a collapsed
sidebar does not flash open on each navigation. That toggle is the only
JavaScript on the page.

The sidebar costs a `git status` per worktree, which on a large repo dominates
the time to render any page. Pages serve the last sidebar snapshot and refresh
it behind the request, so only the first page of a session waits for git; a
mutation clears the snapshot so ticket counts never lag an edit you just made.

The server binds `127.0.0.1` and serves a single repo. POSTs are accepted only
with an `Origin` matching the server's own, so a page in another tab cannot file
or close tickets through your browser. Requests are handled one at a time, which
keeps two tabs from interleaving inside a read-modify-write of a ticket file.

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
