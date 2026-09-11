# implement - build one ticket

Load a ticket, implement it in the linked worktree, and record all work in the
ticket file as you go.

## Arguments

Everything after the verb: `<workstream-name> [ticket-number]`

- `<workstream-name>`: required. If missing, default to the current git branch's
  workstream; if cwd is not inside a worktree, list `<repo_root>/worktrees/` and
  ask which one with `AskUserQuestion`.
- `[ticket-number]`: the ticket to implement (`1`, `01`, `003` all match the
  leading number in the filename). If omitted, derive it from `next.md` per
  step 1.

## Steps

1. **Resolve the ticket number** if it was not given. Look for
   `<ws_dir>/next.md`:

   - If it exists, read the `## Next: <ticket number(s) and short title>`
     section. A single ticket: use it. A small paired set: treat the whole set as
     this run's ticket-number list and implement them together as described
     below. Tell the user which ticket(s) `next.md` recommended before
     proceeding.
   - If it does not exist, list the pending tickets
     (`flowy ticket list -w <workstream-name> --status pending --json`) and ask
     with `AskUserQuestion` which to implement: the likeliest few by priority as
     options, plus one for running `/flowy next <workstream-name>` first to work
     the order out properly. Stop until they answer.

2. **Read workstream context.** `<ws_dir>/meta.toml`, `<ws_dir>/status.md`, and
   `<ws_dir>/planning.md`.

3. **Read the ticket:**
   ```
   flowy --json ticket show -w <workstream-name> <ticket-number>
   ```
   Use the returned `title` and `content`. If there is no match, run
   `flowy ticket list -w <workstream-name>` to show what exists and stop.

4. **Give a summary of the ticket to the user**.

5. **Re-derive the ticket's citations before trusting any of them.** A ticket's
   file/line anchors were written at some past commit and go stale silently. If
   the ticket records the commit it was verified at, first check that commit is
   still reachable:

   ```
   git merge-base --is-ancestor <hash> HEAD
   ```

   A hash that is **not** an ancestor means the plan was verified against a
   history line that has since been rebased away, so treat every anchor and
   every empirical claim in it as unverified. Re-find each cited symbol by name
   (grep, not line number) and reconcile before writing code. Report any citation
   that moved, and any acceptance criterion the current code shows to be wrong,
   rather than silently working around it.

6. **Implement.** Work in the linked worktree from `meta.toml`, following the
   ticket's description. After each meaningful unit of work, commit with a
   concise subject line (under 60 chars).

   Prefer a separate commit for any change whose blast radius is wide enough that
   a regression should be attributable to it alone (a shared helper, a guard
   every caller reaches). Land it first, verify, then build on it.

7. **Keep the ticket's Journal up to date** throughout. After each commit:

   ```bash
   flowy ticket journal -w <workstream-name> <ticket-number> \
     --heading "<brief description>" --body-file - <<'EOF'
   <what was done, past tense>
   EOF
   ```

   The heredoc matters once the entry names a symbol or quotes output;
   `--body-file -` is what keeps backticks and quotes from needing escapes.

8. **Dispose of incidental findings - default to fixing, not filing.**
   Implementation work surfaces adjacent defects constantly, and a new ticket per
   finding grows the queue faster than it is drained. Take the cheapest
   disposition that does not lose the finding:

   - **Fix it now, in this ticket.** The default when the fix is small and lives
     in a file this ticket already touches. Give it its own commit so it stays
     reviewable, and say so in the journal.
   - **Write it down, not up.** For a finding nothing can currently reach (dead
     until another ticket lands, or true only of code no fixture exercises), a
     note under `<repo_root>/notes/knowledge/` costs less than a queue entry and
     is not lost. Create the directory if it does not exist. Name the note in
     the journal so it is findable.
   - **File a ticket** only when one of these holds: it needs a decision that is
     the user's (especially anything that changes shipping behaviour rather than
     the work at hand); it is genuinely a separate chunk of work; or it is a
     *generator* - a duplicated implementation or a coverage hole that will keep
     producing findings until fixed. A generator is worth filing even when large;
     prefer filing the generator over filing its instances one at a time.

   Ask before filing, not after, with `AskUserQuestion` - fix it here, write it
   down, or file it - and lead with the one you think it is. Why fixing it in
   place is or is not the better call goes in the option descriptions.

9. **Report implementation complete, then hand over the decisions.** The
   ticket's journal already holds the detail; this step exists only to get the
   user to the decisions that are theirs. Report in prose:

   ~~~
   Implemented <ticket>: <one or two sentences on what was built>.

   Commits:
   - <hash> <subject>
   ~~~

   Do not restate the ticket description, walk through each file changed, or
   narrate incidental findings you already fixed.

   Then put everything still open into one `AskUserQuestion` call:

   - One question per decision that genuinely needs the user's answer and could
     not be settled during implementation: a choice that changes shipping
     behaviour, an acceptance criterion the current code contradicts, something
     left unimplemented. Anything filed as a ticket was already agreed with the
     user when the finding was disposed of, and is not a question here.
   - Every question gets a recommendation, as the first option marked
     `(Recommended)` with the reason as its description. Then wait - do not
     resolve a question that is the user's to answer, and do not act on your own
     recommendation before they answer.
   - Last question in the call: whether it works, given they still have to go
     test it. Options are that it is correct and ready to mark done, or that it
     needs changes. Four questions is the cap, so when the decisions fill the
     call, ask them first and this one after.
   - Manufacture nothing to fill the call. With no decisions open, the test
     question is the whole call.

   Do **not** run `flowy ticket done` before they answer that last question with
   the work being correct.

10. **On user confirmation**, mark it done:

   ```bash
   flowy ticket done -w <workstream-name> <ticket-number> --body-file - <<'EOF'
   <summary>
   EOF
   ```

   The summary should cover:
   - What was built or changed.
   - Key decisions made during implementation.
   - Any incidental findings and their disposition (fixed in place, written to
     `notes/knowledge/`, or filed - with the ticket number).
   - Any deferred items or open questions, and how each was resolved.
   - A **Commits** subsection listing each commit hash and subject, one per line
     (from `git log --oneline` in the worktree):

     ```
     ### Commits
     - <hash> first commit subject
     - <hash> second commit subject
     ```
