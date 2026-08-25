# close - archive a workstream and remove its worktree

## Arguments

Everything after the verb: `<workstream-name>`

If missing, default to the current git branch's workstream; if cwd is not inside
a worktree, list `<repo_root>/worktrees/` and ask.

## Steps

1. **Read `<ws_dir>/meta.toml`** to confirm the worktree path and branch name.

2. **Ensure the branch is clean.** Run `git status` from inside the worktree. If
   there are uncommitted changes, commit them before archiving - archiving with
   uncommitted changes loses work permanently. Only proceed once the branch is
   clean, or the user explicitly confirms they want to discard.

3. **Write `<ws_dir>/audit.md`** before archiving. It covers the whole branch,
   across every milestone it went through - a `milestones/` directory means the
   plan was retired and replaced at least once, and those summaries say what each
   one reached. From the worktree:

   ```
   git log --oneline --reverse
   ```

   Write:

   ```markdown
   # Audit: <safe-name>
   Closed: <current date and time>

   ## Commits
   - <hash> <subject>
   - <hash> <subject>
   ...
   ```

4. **Append a closing entry** to `<ws_dir>/status.md`:
   `- <current date and time>: Workstream closed.`

   Both this and `audit.md` have to be written *before* the archive step -
   afterward the worktree is gone and `ws_dir` no longer exists.

5. **Archive.** From the repo root (the directory containing `.bare/`):

   ```
   flowy worktree archive <name>
   ```

   If git refuses due to uncommitted changes, use
   `flowy worktree archive <name> --force` only if the user has explicitly
   confirmed they want to discard them.

   This moves the notes out of the worktree to
   `<repo_root>/notes/workstreams/archive/<name>/` before removing the worktree,
   so they outlive it, `milestones/` and all. `flowy worktree unarchive <name>`
   reverses both steps.

6. **Report** what was removed and where the notes were archived.
