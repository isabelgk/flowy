# flowy

`flowy` is a Rust CLI for managing bare git repos + per-branch worktrees with
markdown ticket files.

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

## Shell completions

Generate completions with `flowy completions <shell>`. Supported shells: `bash`,
`zsh`, `fish`, `powershell`, `elvish`.

```sh
flowy completions zsh > "${fpath[1]}/_flowy"
flowy completions bash > /etc/bash_completion.d/flowy
flowy completions fish > ~/.config/fish/completions/flowy.fish
```
