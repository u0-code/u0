---
title: Projects and threads
description: Projects are folders; threads are each agent's own, listed by project.
---

# Projects and threads

## Projects

A project is a folder. It does not have to be a git repository.

### Adding a project

- Drop folders from Finder onto the sidebar.
- **Add project** (in the sidebar when there are none, in the **Projects**
  popover, or in the palette, ⌘K) offers:
  - **Choose a folder…**;
  - **Clone a repository**, into the folder `projects_in` names
    (`~/Documents/u0` by default; with `host_folders`, laid out as
    `<host>/<owner>/<repo>`);
  - **New project**: a new folder there, and, where `gh` is installed, a
    private GitHub repository for it;
  - up to six folders your agents already worked in.
- Type a path beginning with `/` or `~` to browse: Tab goes into the
  folder, ⌘Return adds what you typed, ⌫ on an empty line goes back.
- From a terminal: `u0 <folder>` (see [CLI](reference/cli.md)).
- A `file://` link to a folder opened with u0 adds it and begins a
  thread.

### A folder of repositories

A project folder that is not itself a repository may hold several: each
repository directly inside it, or one level further down (such as
`packages/web`), up to 128, leaving out hidden folders and submodules.
Each shows in the Diff tab's repository picker with its own changes,
commits, commit form and pull request. A worktree thread gets a folder
laid out as the project is, with a worktree of each repository on the
thread's branch and the rest linked; each turn is checkpointed, undone
and diffed in every repository.

### Project settings

**Settings › Projects** (or **Project settings** on a project's menu)
holds, for each project: its name and icon, whether it is pinned, the
agent its new threads begin with, whether they work in a worktree,
whether its default branch is pulled as u0 starts and on each fetch,
the Linear team its issues come from, and a few settings it holds
otherwise than everywhere else (how pull requests merge, agents' credits
in a squash, submodules, branch names by kind of work, going on when a
limit resets, and removing archived threads' worktrees). See `prefs` in
[settings.json](reference/settings.md#projects).

Removing a project leaves its folder, worktrees and files alone.

## Threads

Threads belong to each agent, which lists them by project. u0 keeps an
index of those it has seen (title, pins, archive, worktree) and a record
of each, so the list and a thread show at once. A thread with a record
is resumed with the agent when you write; one without (begun elsewhere)
is loaded from the agent and recorded from then on.

A thread begun in a terminal shows in its project (within a minute while
u0 runs, as each agent is asked again) and can be taken over: open it, and its history
shows as the agent loads it; once loaded, write in it as in any other.
Where the agent's CLI says which threads it runs (Claude Code does), a
thread still open in another process, such as a terminal, shows a
terminal mark in the sidebar, and above the composer **Claude Code has this
thread open elsewhere too**: what you send here goes on in u0, and that
process does not see it. Write in one place at a time, or **Branch off a
copy** to go on apart.

### The sidebar

- **Pinned** threads from every project come first, in the order you
  dragged them.
- Each **project** lists its threads that are not done. The order is
  `sort` in settings: by your last message (default), by any activity, by
  when it began, or by name.
- **Snoozed** and **Settled** shelves sit folded at the foot.
- A new thread is not a row until you send it. Each project keeps one
  draft: + on the project, ⌘N, ⇧⌘N, ⌥⌘N (for Scratch) and the
  palette's **New thread in…** bring it back as you left it (what you
  typed, attached and chose), and begin one where there is none. Begun
  for another agent, it keeps all that but the model and options. A
  thread begun about an issue, a pull request, a failed workflow step
  or a link's prompt asks before it takes the place of a draft that
  holds something. With no thread open, the start page's composer is
  the project's draft, with what it holds; there is never a second one
  beside it. What you were typing in a thread you archive or delete
  goes into that draft, after what it holds.

Each row ends with the thread's state: Working, Approval, Sign in,
Failed, Limited, Plan, Done, or Woke.

The sidebar has no search of its own; the palette (⌘K, or the top bar's
**Search threads, projects, commands and settings** box) is where threads
are found. It matches titles, #tags, pull request numbers, branches and
projects, and from three characters on it also looks through what was
said in every thread, on every server the window holds, a moment after
you stop typing. Those come under **What was said**, after the strong
matches by name, and leave out threads already listed above: each row
is the line it was said in, over its thread, and opens the thread at
that message.

Keys in the list: ↑ ↓ → ← move and fold; Return or Space opens; F2
renames; ⇧F10 or a right-click opens the menu. ⌘-click and ⇧-click choose
several rows. Hold ⌘ to see the numbers ⌘1–⌘9 jump to. Drag a row onto a
pinned row to pin it, onto Settled to settle it, onto a project to make
it active again.

### A thread's menu

Right-click a thread (or ⇧F10):

| Item | What it does |
|---|---|
| Rename, Regenerate title | Rename it yourself, or have the writer title it again. |
| Pin / Unpin | |
| Settle / Un-settle | Mark it done, or not. |
| Snooze / Wake | Hide it until a time: the choices from `snoozes`, or **Custom…** ("in 3h", "in 2 days", "at 5pm"). Writing in it wakes it. |
| Mark unread | |
| Find in thread | |
| Filter by project, Project settings | |
| New thread on *branch* | Begin a new thread on this thread's branch. |
| Branch off a copy | A copy of the thread to take another way (where the agent can fork). |
| Continue with | Hand the thread to another agent. |
| Copy | The conversation, the path, the branch, a link, or the thread's id. |
| Never settle on its own / Let it settle on its own | |
| Archive, Delete | Not while the thread is busy. |

Unpin, Settle, Snooze, Archive and Delete show a notice with **Undo** for
five seconds; ⌘Z takes them back too. Delete asks first (see `confirm`);
if the thread alone uses its worktree, u0 asks whether to delete the
worktree too.

Archived threads and projects are in **Settings › Archive**, with
**Restore** and **Delete**.

### Done for now

A thread settles (is marked done) on its own when:

- it has been left for `settle_days` (7 by default);
- its pull request merged or closed (`settle_merged`).

A pinned thread, one set to never settle, and (with `keep_with_pr`) one
with an open pull request do not.

### Links

u0 opens `u0://` links:

| Link | What it does |
|---|---|
| `u0://new?project=…&agent=…&prompt=…` | A new thread in a project you have (with `pr=` or `issue=`, about that pull request or issue). A link never adds a project. |
| `u0://session/<agent>/<id>?prompt=…` | Open a thread, with a draft. |
| `u0://pair?host=…#…` | Pair with a machine (see [Remote control](remote.md)). |

Invisible characters in a link's prompt are taken out, and you are told.
A link that would replace a draft asks first.
