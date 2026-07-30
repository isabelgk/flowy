# ticket - add one ticket without a planning run

Write a single ticket to the workstream's `tickets/` for a bug, an
investigation, or any task where the work is already understood.

## Arguments

Everything after the verb: `<workstream-name> [brief]`

- `<workstream-name>`: first token. If missing, default to the current git
  branch's workstream; if cwd is not inside a worktree, list
  `<repo_root>/worktrees/` and ask.
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

   If any fire, ask **all questions in a single round** - never drip-feed
   follow-ups. Cap at 2-3. For bounded choices (priority, scope variant A vs. B)
   use `AskUserQuestion`; for open-ended ones, plain prose.

   Skip this step entirely when the brief is already specific. Do not ask for
   what you can derive.

4. **Synthesize title and description.**

   **Title** - an imperative verb phrase, matching the verb choice and
   granularity of sibling tickets.

   **Description** - markdown with section headers and bullet lists. Keep each
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
   - 003: dependency or follow-on (only if it falls out naturally; omit if none)
   ```

   - **Scope** is always present and always includes a one-line motivation.
   - **Affected files** is a bullet list; omit the section entirely if unknown.
   - **Acceptance criteria** are outcomes that can be verified, not vague
     intentions. `next` and `implement` both read these, so a criterion that
     cannot be checked leaves the ticket impossible to close honestly.
   - **Related tickets** only if a dependency or follow-on is obvious from
     context. Never manufacture links - `next` derives ordering from this
     section, so a fabricated one distorts the queue.

5. **Create the ticket** with a heredoc so newlines survive:

   ```bash
   flowy ticket add -w <workstream-name> "<title>" -d "$(cat <<'EOF'
   ## Scope
   ...

   ## Affected files
   - ...

   ## Acceptance criteria
   - ...
   EOF
   )"
   ```

6. **Report** the filename printed by `flowy ticket add`.
