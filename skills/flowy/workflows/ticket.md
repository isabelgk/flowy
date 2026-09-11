# ticket - add one ticket without a planning run

Write a single ticket to the workstream's `tickets/` for a bug, an
investigation, or any task where the work is already understood.

## Arguments

Everything after the verb: `<workstream-name> [brief]`

- `<workstream-name>`: first token. If missing, default to the current git
  branch's workstream; if cwd is not inside a worktree, list
  `<repo_root>/worktrees/` and ask which one with `AskUserQuestion`.
- `[brief]`: the rest - a natural-language description of what needs doing. If
  absent, ask before proceeding. When the request arrived mid-conversation
  ("file a ticket for that"), the brief is the preceding discussion; use it.

Do not expect a structured title or formatted description. Both are synthesized
from context below.

## Steps

1. **Read workstream context.** `<ws_dir>/meta.toml`, `<ws_dir>/planning.md` if
   present, and:
   ```
   flowy ticket list -w <workstream-name>
   ```
   Use the existing titles to calibrate style - tense, verb choice,
   granularity - for the new one.

2. **Decide whether to explore.** Delegate to an `Explore` subagent only if the
   brief is genuinely ambiguous about *what* to change (e.g. "tighten up the
   validation logic" with no file hints). Skip it when the brief names specific
   files, is a small self-contained fix, or is obvious from the workstream
   context.

3. **Ask clarifying questions - only if needed.** Gate strictly: ask only when
   the answer would *materially change* the ticket and cannot be inferred.
   Common triggers:

   - Motivation is absent and the "why" matters for scoping (a bug with unknown
     root cause, a refactor with unclear goal).
   - Acceptance criteria are ambiguous, with multiple plausible definitions of
     "done."
   - The brief implies a choice between two meaningfully different approaches.

   If any fire, ask **all of them in one `AskUserQuestion` call** - never
   drip-feed follow-ups. Cap at 2-3 questions. Only an answer with no bounded
   option set (a name, a paragraph of missing context) belongs in prose
   instead.

   Skip this step entirely when the brief is already specific. Do not ask for
   what you can derive.

4. **Synthesize title and description.**

   **Title** - an imperative verb phrase, matching the verb choice and
   granularity of sibling tickets.

   **Description** - the shared ticket description format: `## Scope`,
   `## Affected files`, `## Acceptance criteria`, and optionally
   `## Related tickets`, per the rules already in SKILL.md.

5. **Create the ticket.** `--body-file -` reads the body from stdin, so a quoted
   heredoc carries newlines and backticks through verbatim:

   ```bash
   flowy ticket add -w <workstream-name> "<title>" --body-file - <<'EOF'
   ## Scope
   ...

   ## Affected files
   - ...

   ## Acceptance criteria
   - ...
   EOF
   ```

   Add `-p high` or `-p low` only when the user said something that ranks this
   work against the rest of the queue. Omitting the flag leaves the ticket at
   medium, which is the right default for a ticket filed without that signal.

6. **Report** the filename printed by `flowy ticket add`.
