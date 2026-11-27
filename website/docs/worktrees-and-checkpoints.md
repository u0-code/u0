---
title: Worktrees and checkpoints
description: Let a thread work on a branch of its own, and undo any turn, without touching your checkout.
---

# Worktrees and checkpoints

## Worktrees

In a git repository, a new thread can work in a **worktree of its
own**: a separate checkout on a new branch, so your folder stays as it
is. Turn on the **Worktree** chip under the composer before the first
message. The choice is remembered for the next new thread; **Settings ›
New threads › A new thread works** sets it everywhere, and a project
can hold its own choice (**Local** or **New worktree**).

### Where it starts

The **from** chip chooses the base: a local branch, or an open pull
request ("Pull requests: on its own branch"). With none chosen, u0
fetches `origin` and starts from its default branch; with no remote, or
if the fetch fails, from `HEAD`. Starting from a pull request checks it
out with `gh pr checkout`, forks included.

### Branch names

The branch is the prefix (`branch_prefix`, `u0/` by default) and a
short name from your first words; a name already taken gets `-2`, `-3`, …
Once the work is under way the writer renames the branch to say what it
is, as in `u0/retry-s3-uploads`. With **Or they begin with › The kind
of work** (`branch_kind`), the writer's `feat/`, `fix/`… replaces the
prefix. A branch already pushed is never renamed.

### Where worktrees live

In `<home>/worktrees/<repository>/<name>`, or under the folder
**Settings › New threads › Worktree location** chooses (`worktrees`).
Those made before stay where they are.

**Submodules** of a new worktree are checked out recursively (default),
at the top level only, or skipped (for a setup script to do).

A repository's `u0.json` can name a `sparse` file of sparse-checkout
patterns, and scripts to run in each new worktree (see
[Review](review.md#repository-commands)).

### Cleaning up

- **Archiving** a thread keeps its worktree. With **An archived
  thread's worktree is taken away after** set (a day, 7 days, 30 days;
  `tidy_after`), u0 removes it once the thread has been archived that
  long. A worktree with uncommitted changes stays, and the branch always
  does; opened again, the thread checks the branch out again.
- **Deleting** a thread drops its checkpoints and removes its worktree,
  unless another thread uses it; u0 asks first when the thread is
  the only one using it.
- The Details tab's **Worktree** row has **Remove**. A thread whose
  worktree is gone says so; start a new thread to go on.
- **Settings › Archive › What u0 keeps** shows what u0 keeps and
  has **Remove worktrees no thread works in**.

Git refuses to remove a worktree with uncommitted changes, and u0
leaves it.

## Checkpoints

Before and after each turn in a repository, u0 checkpoints the files:
a commit of the working tree, made with an index of u0's own, kept
under `refs/u0/checkpoints/<thread>/`. No branch points at these
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

**Settings › Archive › What u0 keeps › Remove checkpoints of earlier
runs** removes checkpoints from before u0 last started; undo reaches
only the turns of this run after that.
