---
title: Pull requests and issues
description: The PRs and Issues pages, on GitHub, GitLab, Bitbucket, Azure DevOps and Linear.
---

# Pull requests and issues

**Pull requests** and **Issues**, at the foot of the sidebar, list your
projects' pull requests and issues from where their code is hosted.

## Hosts

**Settings › Source control › Hosts** shows each host and how u0
reaches it. A token you give there is used instead of the CLI's own
sign-in, and is kept in the macOS Keychain, never in a file. It may be a
1Password reference (`op://vault/item/field`), read with `op` when used.

| Host | How u0 reaches it | Token |
|---|---|---|
| GitHub | the `gh` CLI | yours from `gh auth login`, or one given here |
| GitLab (any host whose name contains `gitlab`) | its REST API; `glab` to open merge requests | given here (also passed to `glab`) |
| Bitbucket (`bitbucket.org`) | its REST API | given here (a token, or `user:app-password`) |
| Azure DevOps (`dev.azure.com`, `*.visualstudio.com`) | its REST API | given here (a personal access token) |
| Linear | its API | given here (an API key) |

A project's host is read from its `origin`. Without a token, the APIs
are asked without one, which reaches only what is public.

For issues, a project can be bound to a **Linear team** instead (by its
key, such as `ENG`) in its
project settings; Bitbucket and Azure DevOps projects need that for
issues.

## The pages

Each page opens on the project of the thread on screen (in a project of
several repositories, the one the thread works in), and lists that
project's items, or **All projects** together. A project that is a folder
of repositories is listed repository by repository (`project / alpha`).
A project that is on no forge is left out of **All projects**, and a
notice names it and says why. The top bar says which repositories are
listed and when they were last read.

Above the list are a search field and its filters:

- **State**: open, closed, merged (pull requests), all;
- **Repository**: **All projects**, or one;
- **Author** (pull requests): **Anyone** or **You**; **Assignee**
  (issues): **Anyone** or **Me**;
- **Label** (pull requests): one of the labels of those listed;
- **Tracker** (issues): the project's host, or **A Linear team…**, which
  asks for the team's key and binds the project to it.

The menu beside them orders the list (**Sort: Recently updated**,
**Sort: Oldest first**, and **Sort: Most changed** for pull requests or
**Sort: Priority** for issues), shows a view you saved, and keeps the
filters shown with **Save this view** (`views` in settings); **Remove**
takes a saved view away. **Read again** lists them again.

Pull requests are listed in sections: **Needs your review** (open, your
review asked for), **Yours** (opened by you, as the account `gh` is
signed in to says) and **Watching** (the rest). Each row shows its state,
title, number and labels; its repository, author, branches and when it
was last updated; and how its review stands, its checks, its comments
and the lines it adds and removes. Issues are listed by project, each row
with its priority (Linear's), key, state, title, labels, the threads
about it and how the latest stands. A pull request opens on its own
page; an issue opens beside the list.

An item's menu:

- for a pull request: **Squash and merge**, **Merge**, **Rebase and
  merge**, **Merge when its checks pass** (each deletes the branch);
  **Ready for review** / **Convert to draft**; **A thread on it, in a
  worktree** or **checked out here**; **Copy the checkout command**;
  **Load new commits**;
- for an issue: **A thread about it**, and **Triage it** (a thread that
  triages it);
- for both: **Close** / **Reopen**, **Edit title and description**,
  **Open on its host**, **Copy its link**.

In an item's view you can add labels, reviewers and assignees, re-run
failed checks (**Run again**) and read their **Log**, and open a **New
issue** from the repository's issue templates (GitHub).

The pictures in a description or a comment (a Markdown image, or an
`<img>` tag, read for its address alone) show where they are written. The
server the project is on fetches each, over HTTPS only, from the public
internet only: the first 50 an item shows, each up to 8 MiB and
8192 pixels a side, an animated one up to 300 frames. A picture asked
for again within a minute is shown from what was fetched. A file attached on GitHub
(`github.com/user-attachments/…`) is asked with your GitHub token, so a
private repository's shows too; no other address is sent it.

## Reviewing a pull request

A pull request opens on its own page, with **Summary**, **Timeline**,
**Code**, **Conversations** and **Guide**:

- **Code** sorts the files into code, tests, mocks, generated files and
  docs. Tick **Viewed** on a file as you go; it shows unviewed again if it
  changes.
- **Comment on this line** queues a comment for your next review. Finish
  with **Comment**, **Approve** or **Request changes**.
- **Conversations** has **Reply** and **Resolve** / **Reopen**; resolved
  conversations and bots' are folded away.
- **Guide** has the writer write a review guide to read the change by. It
  says so when commits came after it was written.

## What each host supports

| | GitHub | GitLab | Bitbucket | Azure DevOps | Linear |
|---|---|---|---|---|---|
| Pull requests | all of the above | merge, close, reopen, draft/ready, comment, approve, edit, reply, resolve, labels | merge, decline, comment, approve, request changes, edit, reply, resolve | merge, abandon, reopen, draft/ready, comment, edit, reply, resolve | — |
| Issues | yes | yes | through Linear | through Linear | list, read, comment, edit, close, reopen, create |
| Reviewers, assignees, check logs, re-runs | yes | on GitLab's own page | — | — | — |
