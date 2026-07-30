# next - decide what to work on next

Look at the pending tickets, the status history, and any related planning
context, and write a rolling recommendation to `<ws_dir>/next.md`.

## Arguments

Everything after the verb: `<workstream-name>`

Required. If missing, default to the current git branch's workstream; if cwd is
not inside a worktree, list `<repo_root>/worktrees/` and ask.

## Steps

1. **Read workstream context** directly (these are small):

   - `<ws_dir>/meta.toml` - worktree path and branch.
   - `<ws_dir>/status.md` - read the tail (last ~40 entries) to see what
     recently closed, what was added, and what was reopened. The most recent
     `done` entry's ticket number is what "through ticket <N>" means in step 5 -
     not the highest ticket number that exists, and not the highest pending
     number.
   - `<ws_dir>/planning.md`, if present - goals and constraints.
   - `<ws_dir>/next.md`, if present - the previous recommendation. It exists only
     to be replaced, not preserved as history, but check whether its top
     recommendation already closed; that is a signal for what just got
     unblocked.
   - Any other `*.md` in the workstream root (a dated review doc, an `ideas.md`,
     a `fuzzing.md`). These often carry exactly the kind of detail ordering
     depends on - known divergences, parked decisions, findings from a branch
     review - that is in no single ticket. List the root directory to find them;
     do not assume the four names above are the only ones. Read the ones that
     look recent or substantial; skip clearly stale reference material unrelated
     to sequencing.

2. **List pending tickets:**
   ```
   flowy ticket list -w <workstream-name> --status pending --json
   ```
   If the pending count is 0, report that nothing is left to schedule and stop -
   do not write an empty `next.md` over a real one.

3. **Read every pending ticket and reason about ordering.** For more than ~15
   pending tickets, delegate to a `general-purpose` subagent rather than reading
   all bodies inline. Give it:
   - the workstream directory path (so it can read `tickets/*.md` directly, plus
     `status.md`, `planning.md`, and any other root-level `*.md` from step 1),
   - the pending ticket numbers from step 2,
   - the previous `next.md` content, if one existed.

   Ask it to work out, per ticket - from each ticket's `Related tickets`
   section, prose references to other ticket numbers, and any note of the form
   "don't do X before Y" - what blocks what, what has just become unblocked by
   recently-done tickets, what is small and independently verifiable, and what
   is real but deliberately parked rather than genuinely next.

   Require the recommendation to come back anchored to specifics: ticket
   numbers, file paths, counts. Require each standing constraint to name which
   ticket or doc it came from. A recommendation that reduces to adjectives
   ("risky", "important") without a concrete anchor is not acceptable; ask the
   subagent to redo it if that is what comes back.

   Ask for structure, not prose keyed on vibes:
   - one immediate next ticket (or a small paired set, only if two tickets share
     a file or verification path and doing them together is real economy) with
     concrete reasoning - not just "it's next" but why it beats the
     alternatives: risk, blast radius, what it unblocks, how it gets verified.
   - an ordered list of what comes after, each with a one-line reason.
   - standing constraints: anything currently blocking a class of work (e.g.
     "don't wire X into CI until Y lands") that is not itself a ticket to do next
     but shapes the ordering.
   - a "not on the critical path" list: pending tickets that are real but
     deliberately deprioritized, with why.

   For ~15 or fewer pending tickets, do this reasoning yourself by reading the
   ticket files directly.

4. **Get the current branch position**, if a worktree is linked (non-blank path
   in `meta.toml`): `git -C <worktree-path> log -1 --format=%h`. Skip if there
   is no worktree.

5. **Write `<ws_dir>/next.md`**, overwriting it entirely. Follow this shape, but
   omit a section if there is nothing real to put in it - do not pad:

   ```markdown
   # Suggested next ticket

   Rolling recommendation for what to pick up next. Rewritten whenever a
   ticket closes or the board shifts; there is no history here, `status.md`
   has that.

   **As of <current date>, branch at `<short hash>`** (through ticket <N>).

   ## Next: <ticket number(s) and short title>

   <the concrete reasoning from step 3>

   ## Then, in order

   1. **<ticket>** -- <one-line reason>
   2. ...

   ## Standing constraints

   - <constraint, and what it's waiting on>

   ## Not on the critical path

   <pending tickets that are real but parked, and why>
   ```

   The `## Next:` heading is the contract `implement` reads when invoked without
   a ticket number, so it needs to name the ticket number(s) plainly.

6. **Report** the `next.md` path and read back the top recommendation in one or
   two sentences.
