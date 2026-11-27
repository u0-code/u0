---
title: Review, commit and pull request
description: The right column's Diff, Files, Pull request, Details and Side chat tabs.
---

# Review, commit and pull request

The right column shows what a thread came to. ⌥⌘B shows or hides it,
⇧⌘B widens it over the thread, and ⌘D opens the review.

## Tabs

The column has **Diff** and **Details** to begin with. **+** (⌘T) opens
others; with its menu open, a letter picks one:

| Letter | Tab |
|---|---|
| T | Terminal |
| F | Files |
| D | Diff |
| P | Pull request (titled `#N` once there is one) |
| S | Side chat |
| V | Devices (see [Simulators and emulators](simulators.md)) |
| I | Details |

A file opens as a tab of its own. ⌘W closes a tab (or middle-click), and
⇧⌘T reopens the last one closed.

## Diff

Choose what to compare:

- **Turn *n***: what the last turn changed;
- **All**: everything the thread changed (**Uncommitted** before there is
  a turn);
- **Branch**: from where the branch left its base (main by default; the
  **against** chip chooses another) to the files now, uncommitted
  changes included;
- a commit, chosen from **Commits**;
- **Edit**: one tool call's change.

The toolbar has a file tree, **Expand all files** / **Collapse all
files**, **Side by side** (or one above the other), **Wrap long lines**,
**Leave out changes only in spacing**, **Hide test files**, **Find in the
diff**, and **Read the changes again**. In a folder of several
repositories, a chip chooses the repository.

### Comments for the agent

Press **+** on a line to comment on it. Your comments show under their
lines (each with **Take it back**), and **Put *n* comments to *agent* in
the line** puts them in the composer as `- path:line — comment`, for you
to send.

### Commit and push

Each file has a checkbox: a file left out stays out of the next commit.
Under the diff:

- **Commit…** opens the commit form. **Write it** has the writer draft
  the message (with your instructions from **Settings › Writing**).
  **Commit** stages everything (but the files left out) and commits as
  you; with no message, the thread's title is used. **Commit & push**
  also pushes (`git push -u origin HEAD`).
- **Create PR** has the writer write the title and description, then
  commits, pushes and opens the pull request. On the default branch it
  first makes a branch of its own (`<prefix><name>`). GitHub's is opened
  with `gh pr create`, GitLab's with `glab mr create` where `glab` is
  installed; otherwise u0 opens the host's new pull request page in
  your browser.
- ***n* behind its upstream** offers **Pull** (fast-forward only).
- A repository with no remote, where `gh` is installed, offers **Publish,
  private** or **Public** (a new GitHub repository, pushed).

u0 never changes your index or branch except when you commit.

## Pull request

The **Pull request** tab shows the branch's pull request (read with `gh`):
its state, how many checks passed, failed or are running, whether it is
behind or conflicts with its base, and the review decision.

- **Open** opens it on its host; **Check again** reads it again.
- **Watch** checks it every two minutes; when its checks finish or a
  review arrives, the agent is told in the thread.
- **Merge**, when no check failed or runs and nothing conflicts, merges it
  as **General › Pull requests › Merged as** says (squash, merge commit or
  rebase).
- **Have it fix the checks** and **Have it resolve the conflicts** hand
  the work to the agent.
- **Update branch** / **Update with rebase** bring a branch behind its
  base up to date (`gh pr update-branch`).

With **Source control › Merging › Agents' credits in a squash** set to
**Taken out** (`uncredit`), a squash merge through `gh` takes agents'
`Co-authored-by` lines and "Generated with" footers out of the commit
message; people's credits stay.

## Files

The **Files** tab lists the thread's folder. **Find a file** filters it.
A file's menu has **Open**, **Open in your editor**, **Reveal in Finder**,
**Add to chat**, **Copy path** and **Copy as mention**. In an open file,
choose lines and **Comment on the lines chosen** to send them to the
agent; **Save** saves your edits; Markdown can show as **Source** or
**Drawn**. PDFs are drawn page by page.

## Details

The **Details** tab shows the thread: **Context** (tokens used of the
agent's window, the agent, working folder, worktree, mode, when it
began), **Plan**, **Background tasks**, **Servers**, **Terminals**,
**Lineage** and **Linked** (pull requests and issues linked to it).
**Choose what shows here** hides sections (`hidden_details`).

## Side chat

The **Side chat** tab asks the agent a question beside the thread: it
knows the whole thread, and nothing said there goes back into it. **Add
to chat** quotes its last answer into the thread. Its line names who
answers (**Ask Sonnet on the side…**), and holds while the agent
answers.

## Terminals

⌘J opens the drawer of terminals for the thread's folder: **New
terminal**, up to four **Side by side** or **One above another**. A
terminal's menu has Copy, Paste, Select all, Jump to latest, Clear, and
**Ask *agent* about it**; chosen text can be asked about too. An address
such as `http://localhost:3000` printed in a terminal can be opened.

## A repository's own commands {#repository-commands}

A `u0.json` at a repository's root names commands, run from the
drawer's **Scripts**:

```json
{
  "scripts": [
    { "name": "dev", "command": "npm run dev", "preview": "http://localhost:3000" },
    { "name": "setup", "command": "npm ci", "worktree": true, "wait": true }
  ],
  "sparse": ".sparse-checkout"
}
```

| Key | Meaning |
|---|---|
| `name`, `command` | Required. |
| `worktree` | Run it in each new worktree as it is made. |
| `wait` | With `worktree`: the first message waits until it has run. |
| `settle` | Run it in the thread's worktree each time the thread settles. |
| `stop` | Stop it, and close its terminal, when the thread settles. |
| `preview` | The address **Open preview** opens. |
| `sparse` | A file of sparse-checkout patterns (non-cone) for new worktrees. |

Unknown keys are an error. **Add a script to u0.json** in the drawer
writes the file for you. A script can be bound to a key as
`"action": "script:<name>"` (see [Keybindings](reference/keybindings.md#keybindingsjson)).
