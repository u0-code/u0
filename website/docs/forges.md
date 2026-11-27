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

Each page lists one project's items, or **All projects** together. A
project that is a folder of repositories is listed repository by
repository (`project / alpha`). A project that is on no forge is left out
of **All projects**, and a notice names it and says why.
Filter by state (open, closed, merged, all) and by your part: **All**,
**Mine**, **Reviewing**, **Assigned**, **Mentions**; search, and **Save
this view** to keep the filter (`views` in settings).

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
