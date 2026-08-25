# milestone - freeze a finished plan and start a new one on the same branch

A **milestone** is one plan carried to its goal. When the queue drains and the
goal is reached but the branch has more to do, the plan and its rolling
recommendation are frozen under `milestones/<nnn>-<slug>/` beside a written
summary, and the workstream starts a fresh plan on the same branch and worktree.

This exists because `plan` overwrites `planning.md`. Without a milestone step,
the only way to start a second plan on a branch is to destroy the first one, and
the reasoning behind everything already merged goes with it.

## What this is not

- **Not `close`.** The branch, the worktree, and the workstream all survive. Only
  the plan is retired.
- **Not a re-plan.** A re-plan is `plan` overwriting `planning.md`, and it is right
  when the goal itself turned out to be wrong and little or nothing was reached.
  A milestone is for a goal that *was* reached.
- **Not a tidying pass.** A milestone boundary is a fact about the work, not a
  housekeeping preference. If the goal is not reached, there is no boundary to
  record however cluttered the directory looks.

## Arguments

Everything after the verb: `<workstream-name> [slug]`

- `<workstream-name>`: if missing, default to the current git branch's workstream;
  if cwd is not inside a worktree, list `<repo_root>/worktrees/` and ask.
- `[slug]`: a short kebab-case name for the milestone. If omitted, derive one from
  the plan's goal and say what you chose.

## Steps

1. **Confirm the boundary is real** before writing anything.

   ```
   flowy ticket status -w <workstream-name>
   ```

   Pending tickets mean this is not a boundary yet. Say which ones are open and
   ask whether they belong to this milestone (implement them first) or to the next
   one (they carry over, and the summary says so). Then read `<ws_dir>/planning.md`
   and check its stated goal against what actually landed. If the goal is not
   reached, stop and say so - a summary claiming a goal was reached is worse than
   no summary.

2. **Pick the number and slug.** `milestones/<nnn>-<slug>/`, zero-padded, one past
   the highest directory already there. The slug names the goal, not the date.

3. **Read the material the summary is made of.** `planning.md` in full, including
   every amendment - the amendments are where the plan corrected itself, and a
   correction is exactly what a later reader needs. `next.md` for where the board
   stood. Then the closing summaries of the milestone's tickets:

   ```
   flowy --json ticket show -w <workstream-name> <n>
   ```

   Read the ones that recorded a decision rather than all of them. A ticket's
   summary is where a decision was actually settled, and a decision settled there
   and nowhere else is the thing most likely to be lost.

4. **Write `<ws_dir>/milestones/<nnn>-<slug>/summary.md`.** This is the artifact
   that matters; the frozen plan beside it is working detail. It has to stand on
   its own, so do not write it as a diff against the plan or point at another file
   for something a reader needs:

   ```markdown
   # Milestone <n>: <goal, in a phrase>

   Opened <date>, closed <date>. Branch `<branch>`, ending at `<hash>`.
   Tickets <first>-<last>.

   ## The goal, and whether it was reached
   The goal as stated, then the answer, then what makes the answer checkable.

   ## How it was done
   The shape of the work - the strands, phases, or stages it broke into, and the
   order that mattered. Not a ticket-by-ticket walk.

   ## Decisions and rules that outlive the milestone
   The ones still binding on work that has not happened yet. Each with its reason,
   because a rule whose reason is lost gets repealed by the next person who finds
   it inconvenient.

   ## Carried forward
   Open questions, parked items, tickets that carried over, and anything being
   watched rather than filed.

   ## Where the detail is
   One short section. The frozen plan and recommendation beside this file, and the
   fact that the tickets stayed in the workstream's `tickets/`.
   ```

   Omit a section that would be empty rather than writing "none".

5. **Freeze the plan.** Move `planning.md` and `next.md` into the milestone
   directory - move, not copy. Two live copies of a finished plan is the confusion
   this whole step exists to prevent, and the next `plan` run would overwrite the
   top-level one anyway.

   The top level is now left with no `planning.md`, which is what makes the next
   `plan` run write a fresh one instead of appending to a finished one.

6. **Leave `tickets/` where it is.** Every milestone's tickets stay in the one
   directory under one numbering sequence: the plans and the journals cite those
   numbers constantly, and `flowy ticket add` numbers from the highest file it
   finds, so moving them out restarts numbering at `001` and collides with
   everything already written.

   `notes/`, `tools/`, `archive/` and anything else useful past one plan stay too.

7. **Write or update `<ws_dir>/milestones/README.md`.** A short statement of the
   convention plus one entry per milestone: number, slug, dates, ticket range, the
   goal in a phrase, and whether it was reached. This is the index somebody lands
   on, so it says why `tickets/` and `status.md` did not move.

8. **Append a boundary line** to `<ws_dir>/status.md`:
   `- <current date and time>: Milestone <nnn>-<slug> closed. <goal> reached.`

   One chronology spans the whole workstream, so the boundary belongs in it rather
   than in a per-milestone log.

9. **Report** the milestone path, and say that `plan` is the next step - the
   workstream now has no plan, which is deliberate and not a missing file.
