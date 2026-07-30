# plan - write planning.md and create the tickets

Explore the codebase, write a narrative `planning.md`, and create one numbered
ticket per implementation step.

`planning.md` is narrative - goals, context, constraints, approach. Tickets are
the authoritative structured steps. They are separate artifacts, not a
line-by-line derivation of `planning.md`.

## Arguments

Everything after the verb: `<workstream-name>`

Required. If missing, default to the current git branch's workstream; if cwd is
not inside a worktree, list `<repo_root>/worktrees/` and ask.

## Steps

1. **Read workstream context.** `<ws_dir>/meta.toml` and `<ws_dir>/status.md`.

2. **Explore the codebase.** Delegate to an `Explore` subagent
   (subagent_type: Explore). Hand it the worktree path from `meta.toml` and ask
   it to identify the files and components relevant to the goal, note key
   interfaces and dependencies, and flag anything architecturally significant.
   Use its findings as the research input - do not re-explore yourself.

3. **Write `<ws_dir>/planning.md`** covering:
   - Goal and motivation.
   - Relevant context and constraints.
   - Approach overview and key decisions.
   - Known risks or open questions.

   Not a step-by-step list. Overwrite if it already exists.

4. **Measure the central claim of each ticket before writing it.** Reading the
   source tells you what a thing appears to do; it does not tell you what it
   does. A plan written only from reading routinely misjudges scope, and scope
   discovered during implementation becomes a NEW ticket instead of part of this
   one - which is how a queue grows faster than it drains.

   So for each intended ticket, run the cheapest thing that would falsify it:
   the probe program, the fixture, the IR dump, the two implementations diffed
   against each other. Then record in the ticket:
   - what was measured and what it showed, distinguished from what was inferred;
   - `git rev-parse --short HEAD` - the commit the anchors were verified at, so a
     later reader can check it is still an ancestor;
   - anything the measurement contradicted about the original framing.

   Where a claim genuinely cannot be measured yet, say so in the ticket in those
   words rather than presenting a reading as a finding.

   Watch for generators while measuring - a duplicated implementation, or a
   harness that covers one of two paths. A generator belongs in the plan as its
   own step rather than being discovered later.

5. **Create tickets.** Once per discrete implementation step:

   ```
   flowy ticket add -w <workstream-name> "<title>" -d "<scope, affected files, and acceptance criteria>"
   ```

   `flowy ticket add` assigns numbers sequentially and appends individual
   entries to `status.md`; do not write ticket files by hand. When re-planning,
   it numbers new tickets after the current highest - do not renumber existing
   ones.

   The description uses the shared ticket description format from SKILL.md,
   plus the measurements from step 4.

6. **Update status.** Append to `<ws_dir>/status.md`:
   `[<current date and time>] Planning complete. <N> tickets created.`

7. **Report** the plan path, the tickets written, and a brief summary. Say what
   was measured versus inferred, and name anything the measurements
   contradicted.
