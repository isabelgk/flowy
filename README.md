# flowy

`flowy` is a Rust CLI for managing bare git repos + per-branch worktrees with
markdown ticket files. It is currently in Phase 1 (core CLI); Phase 2 adds a
`ratatui` TUI.

The project manages this on-disk layout

```
project/
  .bare/                              # bare git repository
  worktrees/<safe-name>/              # per-branch worktrees (/ replaced with -)
  notes/workstreams/<safe-name>/
    meta.toml                         # branch + worktree path
    tickets/
      001-slug.md                     # h1 prefix encodes status
  notes/workstreams/archive/<name>/   # archived workstreams
```

Ticket status lives in the h1 heading: no prefix = pending, `completed: ` =
done, `wont-implement: ` = skipped. Ticket IDs are zero-padded three-digit
numbers (`001`, `002`, ...).
