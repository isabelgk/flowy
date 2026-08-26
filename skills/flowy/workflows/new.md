# new - scaffold a workstream and its worktree

Create a branch, a linked git worktree, and the workstream notes inside it.
Delegates to `flowy worktree new`, which must be on PATH.

## Arguments

Everything after the verb: `<name> [--branch BRANCH]`

- `<name>`: the branch name for the workstream (e.g. `igk/my-feature`).
  Required - ask if missing. The worktree and workstream directory are named
  after the filesystem-safe form (`/` replaced with `-`, so `igk-my-feature`).
- `--branch BRANCH`: branch name override; defaults to `<name>`.

`--skip` (notes without a worktree) is gone. Notes live inside the worktree, so
skipping the worktree leaves nowhere to put them. If the user passes it, say so
and stop rather than scaffolding something the tooling cannot read.

## Steps

1. **Parse.** Derive `branch` (from `--branch` or `<name>`) and
   `safe` (`branch` with `/` replaced by `-`).

2. **Find repo root.** Walk up from cwd for a directory containing `.bare/`.
   Do not skip worktree creation just because cwd is not itself a git repo -
   resolve the root and run against it.

3. **Check for collision.** If `<repo_root>/worktrees/<safe>/` exists, stop with
   an error rather than scaffolding over it.

4. **Create the worktree.** From `repo_root`:

   ```
   flowy worktree new -b <branch>
   ```

   This creates `<repo_root>/worktrees/<safe>` and writes
   `<ws_dir>/meta.toml` inside it.

5. **Open the log:**

   ```
   flowy notes log -w <safe> "Workstream created."
   ```

6. **Report** the worktree path, the workstream path, and the branch. Suggest
   `/flowy plan <safe>` as the next step.
