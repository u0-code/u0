---
title: Worktrees and checkpoints
description: Let a session work on a branch of its own, and undo any turn, without touching your checkout.
---

# Worktrees and checkpoints

## Worktrees

In a git repository, a new session can work in a **worktree of its
own**: a separate checkout on a new branch, so your folder stays as it
is. Turn on the **Worktree** chip under the composer before the first
message. The choice is remembered for the next new session; **Settings ›
New sessions › A new session works** sets it everywhere, and a project
can hold its own choice (**Local** or **New worktree**).

### Where it starts

The **from** chip chooses the base: a local branch, or an open pull
request ("Pull requests: on its own branch"). With none chosen, Ghost
fetches `origin` and starts from its default branch; with no remote, or
if the fetch fails, from `HEAD`. Starting from a pull request checks it
out with `gh pr checkout`, forks included.

### Branch names

The branch is the prefix (`branch_prefix`, `ghost/` by default) and a
short name from your first words; a name already taken gets `-2`, `-3`, …
Once the work is under way the writer renames the branch to say what it
is, as in `ghost/retry-s3-uploads`. With **Or they begin with › The kind
of work** (`branch_kind`), the writer's `feat/`, `fix/`… replaces the
prefix. A branch already pushed is never renamed.

### Where worktrees live

In `<home>/worktrees/<repository>/<name>`, or under the folder
**Settings › New sessions › Worktree location** chooses (`worktrees`).
Those made before stay where they are.

**Submodules** of a new worktree are checked out recursively (default),
at the top level only, or skipped (for a setup script to do).

A repository's `ghost.json` can name a `sparse` file of sparse-checkout
patterns, and scripts to run in each new worktree (see
[Review](review.md#repository-commands)).

### Cleaning up

- **Archiving** a session keeps its worktree. With **An archived
  session's worktree is taken away after** set (a day, 7 days, 30 days;
  `tidy_after`), Ghost removes it once the session has been archived that
  long. A worktree with uncommitted changes stays, and the branch always
  does; opened again, the session checks the branch out again.
- **Deleting** a session drops its checkpoints and removes its worktree,
  unless another session uses it; Ghost asks first when the session is
  the only one using it.
- The Details tab's **Worktree** row has **Remove**. A session whose
  worktree is gone says so; start a new session to go on.
- **Settings › Sessions › What Ghost keeps** shows what Ghost keeps and
  has **Remove worktrees no session works in**.

Git refuses to remove a worktree with uncommitted changes, and Ghost
leaves it.

## Checkpoints

Before and after each turn in a repository, Ghost checkpoints the files:
a commit of the working tree, made with an index of Ghost's own, kept
under `refs/ghost/checkpoints/<session>/`. No branch points at these
commits, ignored files are left out, and your index and branch are never
touched. The turn's first message waits for the checkpoint; if one can't
be made, the turn says **This turn can't be undone**.

Under each turn (see [The thread](thread.md#turns-undo-restore-branch-off)):

- **Undo** / **Restore to before** first checkpoints the files as they
  are, then puts them back as they were before that turn: files added
  since are deleted. The agent still remembers the turn, so tell it what
  changed.
- **Redo** puts the turn's changes back.

In a folder of several repositories, each repository is checkpointed and
restored.

**Settings › Sessions › What Ghost keeps › Remove checkpoints of earlier
runs** removes checkpoints from before Ghost last started; undo reaches
only the turns of this run after that.
