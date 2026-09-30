---
name: flowy
description: Drive the flowy CLI and the workstream style of working - bare git repos, per-branch worktrees, and numbered markdown tickets that live beside the code. Use this whenever the repo has a `.bare/` directory or a `wt/` directory, whenever the user runs `flowy` or mentions workstreams, tickets, or worktrees, and whenever they ask to start a branch, plan a piece of work, file or pick up or finish a ticket, figure out what to work on next, or close out finished work. Also use it for the bare verbs `/flowy new|plan|next|implement|ticket|milestone|close`, and for casual asks like "file a ticket for that", "what's next here", "let's start on 4", "we reached the goal, clear this out for the next phase", or "I'm done with this branch" - even when neither flowy nor the word ticket is spoken.
---

# flowy and the workstream style

`flowy` manages a bare git repo, one worktree per branch, and a queue of
numbered markdown tickets. A **workstream** is the unit that binds those
together: one branch, one worktree, one ticket queue, one activity log.

The style is: plan in writing before coding, break the plan into tickets small
enough to verify one at a time, record what actually happened in the ticket as
you go, and archive the whole thing when the branch lands. The notes are not
overhead; they are how a reader six months out learns why the code looks the
way it does.

## Layout

```
project/
  .bare/                              # bare git repository
  wt/<safe-name>/                     # one per branch (/ replaced with -)
    .local/workstream/                # this workstream's notes
      meta.toml                       # branch + worktree path
      status.md                       # append-only activity log
      planning.md                     # narrative plan (written by `plan`)
      next.md                         # rolling recommendation (by `next`)
      tickets/1.md                    # TOML frontmatter (+++): status, priority
      milestones/1-<slug>/            # a finished plan, frozen (by `milestone`)
  .flowy/archive/<name>/              # archived workstreams, same shape
```

A workstream's notes live **inside** the worktree they describe, so `cd`-ing
into a worktree gives you its tickets too. Archived workstreams are the one
exception: `flowy worktree archive` removes the worktree, so the notes move up
to `.flowy/archive/` to outlive it.

`repo_root` is the nearest ancestor of cwd containing `.bare/`. So:

```
ws_dir = <repo_root>/wt/<name>/.local/workstream
```

`flowy` does that walk-up itself, so you only need to resolve `repo_root` by
hand when you are reading these files directly rather than going through the CLI;
`flowy notes path -w <name>` prints the directory when you want it from the CLI.
Do not skip a `flowy` call because cwd is not a git repo - walk up, find
`.bare/`, and run against that.

## The lifecycle

| Verb        | What it does                                     | Workflow file            |
|-------------|--------------------------------------------------|--------------------------|
| `new`       | Create the branch, worktree, and workstream      | `workflows/new.md`       |
| `plan`      | Explore, write `planning.md`, create the tickets | `workflows/plan.md`      |
| `ticket`    | Add one ticket without a planning run            | `workflows/ticket.md`    |
| `next`      | Decide what to pick up, write `next.md`          | `workflows/next.md`      |
| `implement` | Build one ticket, journal it, mark it done       | `workflows/implement.md` |
| `milestone` | Freeze a finished plan, keep the branch going    | `workflows/milestone.md` |
| `close`     | Write the audit, archive, remove the worktree    | `workflows/close.md`     |

A workstream can outlive several plans. `milestone` is the boundary between two of
them: the goal was reached, the queue drained, and the branch has more to do, so
the plan is frozen under `milestones/` and `plan` runs again on the same branch.
`close` is the end of the branch itself.

Read exactly one workflow file - the one matching the work at hand. They are
self-contained procedures; the shared model is here, so you do not need to read
a sibling to understand one.

## Routing

Two ways in.

**Explicit verb.** `$ARGUMENTS` starts with one of the seven verbs above, and the
rest is that workflow's arguments:

```
/flowy new igk/my-feature
/flowy plan igk-my-feature
/flowy next igk-my-feature
/flowy implement igk-my-feature 3
/flowy ticket igk-my-feature "crash on empty input"
/flowy milestone igk-my-feature no-lua
/flowy close igk-my-feature
```

Verb aliases: `add`/`add-ticket` -> `ticket`, `next-ticket` -> `next`,
`implement-ticket`/`impl` -> `implement`, `archive` -> `close`.

**Inferred intent.** More often there is no verb and no workstream name,
because the request arrived mid-conversation:

- "file a ticket for that" / "we should track this" -> `ticket`
- "what should I work on next" / "what's left here" -> `next`
- "let's do 4" / "start on the parser ticket" -> `implement`
- "start a branch for X" / "new workstream for X" -> `new`
- "write up a plan for this" / "break this into tickets" -> `plan`
- "we hit the goal, what's the next phase" / "archive this milestone but keep the
  branch" / "clear this out for a new plan" -> `milestone`
- "I'm done with this branch" / "clean this up" -> `close`

When the workstream is unnamed, default to the one for the current git branch -
that is what `flowy` itself does when `-w` is omitted. If cwd is not inside a
worktree, list `<repo_root>/wt/` and ask which one with
`AskUserQuestion`.

Ambiguity between `plan` and `ticket` is the common case: `plan` is for work
that needs exploring and decomposing into several steps, `ticket` for a single
task already understood. When it is genuinely unclear, ask with
`AskUserQuestion` - the two verbs are the two options - because the cost of
guessing wrong is a planning run the user did not want, or a ticket that hides
five steps inside it.

The other ambiguity is `milestone` against `plan` and `close`, and it turns on
one question: was the goal reached? A reached goal on a branch with more to do is
`milestone` and then `plan`; a goal that turned out to be wrong is `plan` alone,
overwriting; a branch with nothing left is `close`. Asking beats guessing here
too, because `plan` overwrites `planning.md` and a finished plan overwritten
without being frozen is not recoverable from the notes. Those three endings are
the three options to offer.

## Conventions that hold across every workflow

**Ask by offering the choices.** Every question here whose answers are a known
set - which workstream, `plan` or `ticket`, commit or discard, file it or fix it
in place - goes through `AskUserQuestion`, so the answer is a selection rather
than something to type out. Put the option you recommend first and mark it
`(Recommended)`; give each option a one-line description of what happens if it
is picked, since that is where the reasoning goes instead of a paragraph before
the question. The tool always supplies "Other", so an escape hatch is never one
of your options. Four options is the cap: when the real set is longer - a repo
with a dozen worktrees - offer the three likeliest and let "Other" carry the
rest. Batch everything you need into one call, up to four questions. Answers
that are not a choice at all - a branch name, a ticket brief, a free-form
clarification - stay prose.

**The CLI owns the files; go through it rather than around it.** `flowy ticket
add` assigns ticket numbers sequentially and appends to `status.md`;
`flowy ticket done` and `flowy ticket journal` edit frontmatter and append
sections. `flowy notes write|append` handles `planning.md`, `next.md`,
`audit.md`, and the milestone summaries, and `flowy notes log` appends to
`status.md`. Writing these by hand desynchronizes ticket numbering, loses the log
entry that goes with an event, or reconstructs a path the CLI already knows.

Every one of these commands takes its prose from stdin or a file rather than
from an argument, so pass markdown through a quoted heredoc:

```bash
flowy notes write -w <workstream-name> planning <<'EOF'
# Goal
...
EOF
```

The quoted delimiter is what makes backticks, quotes, and `$` survive verbatim.
Building the same text into a `-d "..."` argument means escaping it first, and
that is where ticket bodies get mangled.

`status.md` is append-only, and nothing derives state from it. The CLI appends a
line per ticket event; `flowy notes log` is for the events that are not ticket
events - planning complete, milestone frozen, workstream closed. Adding a line is
always safe; editing or removing lines already there is not, because this is the
workstream's only chronology.

**Ticket numbers are one sequence for the life of the workstream.** `flowy ticket
add` numbers from the highest file in `tickets/`, so a directory emptied of its
history restarts at `1` and collides with every number the plans and journals
already cite. Nothing may move the highest-numbered ticket out of `tickets/` -
which is why `milestone` freezes the plan and leaves the tickets where they are.

**Priority is metadata, not order of work.** Tickets carry `high`, `medium`, or
`low`; a ticket with no `priority` key is medium. Listings rank high first, then
medium, then low, and by number within a rank. Set it at creation with
`flowy ticket add -p <level>` or afterward with
`flowy ticket priority <n> <level>`. What to do next still comes from `next.md`
and from what blocks what - priority is the user's signal about urgency, not a
substitute for that reasoning.

**Read with `--json`.** `flowy --json ticket show -w <ws> <n>` and
`flowy ticket list -w <ws> --status pending --json` are the parseable forms.
`--json` and `--repo-root` are global, so either position parses.

**Target explicitly.** `-w <name>` selects a workstream other than the current
branch's. Archived workstreams are reachable *only* by explicit `-w`; they
never appear in an enumeration of `wt/`.

**One ticket description format.** `plan` and `ticket` both write it and `next`
parses ordering out of it, so it is shared rather than per-workflow. Keep each
section short; omit any that adds no information:

```markdown
## Scope
One or two sentences: what to do and why. Answer "what problem does this solve?"

## Affected files
- path/to/file.ts
- another/file.cpp

## Acceptance criteria
- Specific, observable outcome 1.
- Specific, observable outcome 2.

## Related tickets
- 3: dependency or follow-on (only if it falls out naturally; omit if none)
```

- **Scope** is always present and always includes a one-line motivation.
- **Affected files** is a bullet list; omit the section entirely if unknown.
- **Acceptance criteria** are outcomes that can be verified, not vague
  intentions. A criterion that cannot be checked leaves the ticket impossible to
  close honestly.
- **Related tickets** only if a dependency or follow-on is obvious from context.
  Never manufacture links - `next` derives ordering from this section, so a
  fabricated one distorts the queue.

**Watch for generators.** A *generator* is a defect that will keep producing
findings until it is fixed: a duplicated implementation where a change has to
land in two places, or a test harness that covers one of two paths. One ticket
that closes a generator is worth more than several that close its instances, so
prefer filing the generator even when it is the larger piece of work. This
matters while planning (a generator belongs in the plan as its own step, not
discovered later) and while implementing (a generator is one of the few
findings worth filing rather than fixing in place).

**A queue that grows faster than it drains is the failure mode.** Both the
measurement discipline in `plan` and the disposition ladder in `implement`
exist to fight it, from opposite ends. Scope discovered mid-implementation
becomes a new ticket; scope measured during planning is just part of the
ticket that already exists.
