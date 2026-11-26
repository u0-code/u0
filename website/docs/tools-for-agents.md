---
title: Ghost's tools for agents
description: The MCP server Ghost gives each session, and how an agent outside Ghost uses it.
---

# Ghost's tools for agents

Ghost has no agent of its own, but it gives your agents tools: one MCP
server, named `ghost`, attached to each session Ghost opens. Through it an
agent can read the sessions Ghost keeps, list and add your projects and
find things in them, read, review and merge pull requests and read their
checks, run your workflows, change your scheduled tasks, make and remove
worktrees, hand work to another agent, open a pull request, schedule a
task, ask you a question, and use the simulators and emulators of the
machine it runs on.

## How a session gets them

When Ghost opens a session, it passes the server to the agent through
ACP's `mcpServers`, as a program to run: Ghost's own program with `--mcp`,
with `GHOST_HOME` and a key of the session's own (`GHOST_MCP_KEY`). That
program is a small pipe: it speaks MCP on its standard input and output
and forwards everything to the Ghost server, which listens on `127.0.0.1`
only. The pipe never sends its key: the server first sends a one-time
nonce, and the pipe answers with an HMAC of it (and the port) made with
the key. A program listening at that port while Ghost is not running
learns nothing it could use.

The ports and a key for agents outside Ghost are written to
`<home>/mcp.json`, readable by you alone (mode 0600). They stay the same
from one start to the next, so an app you set up with them keeps working:

- The ports are chosen once and taken again at each start. Where another
  program has taken one meanwhile, Ghost listens at another, writes it in
  `mcp.json`, and says so: an app set up with the old address needs the
  new one. Where that was the HTTP port, the key is made anew too, since
  the program holding the port may have heard it from an app (below).
- The key is kept in the Keychain until you press **New key** under
  **Settings › Agents › Key for agents outside Ghost**. The old key then
  lets no one in, not even an agent already connected with it (its
  next call is refused), and a new one is written in `mcp.json`. Where there is
  no Keychain (`ghost-server` on Linux), the key is made anew at each
  start.

To keep a particular agent's sessions free of Ghost's tools, open its page
in **Settings › Agents** and switch them off. That agent's id is added to
`no_tools` in [settings.json](reference/settings.md).

## The tools

| Tool | What it does | Who may call it |
|---|---|---|
| `list_sessions` | The sessions Ghost keeps, the latest first (up to 100): each one's agent, id, title, project, the folder it works in and whether it is archived. Optional `project` filters by folder. | Any agent |
| `read_session` | What was said in a session: your messages (led by "You:") and the agent's answers. Takes `agent` and `session`. | Any agent |
| `read_diff` | What is not committed in a folder, as a unified diff. `folder` defaults to where the calling session works; for an agent outside Ghost it must be in a project of yours or a worktree of one. | Any agent |
| `delegate_task` | Hand a task to an agent in a new Ghost session, which you can watch, and wait for its answer. Takes `prompt`, `title`, and optionally `agent`, `project` and `worktree`. Returns the new session's id and its final answer. | Sessions in Ghost; asks you first |
| `open_pr` | Commit what is not committed where the session works, push it (on a branch of its own if on the default branch) and open a pull request with `gh`. Takes `title`, `body` and optionally the commit `message`. | Sessions in Ghost; asks you first |
| `schedule_task` | Schedule a prompt for an agent, in a new session each time: once (`in_minutes`) or repeatedly (`every_minutes`). It appears in **Settings › Scheduled tasks**, where you can pause or delete it. | Sessions in Ghost; asks you first |
| `ask_user` | Ask you a question in the session's thread and wait for the answer, free text or one of `choices`. | Sessions in Ghost |
| `list_devices` | The iOS simulators and Android emulators on the machine the server runs on: id, name, platform, whether it runs; and what is missing to have any. | Any agent |
| `device_screen` | What a running device shows now, as an image. | Any agent |
| `use_device` | Start or stop a device, tap a point, type text, press Home (Back or Recents on Android), or set it dark or light. | Sessions in Ghost; asks you first |
| `send_to_session` | Say `prompt` in a session Ghost keeps (`agent`, `session`), as its next turn: a turn going on is let end first, never stopped. With `wait`, gives back what its agent answered; else `wait_for_turn` does. | Sessions in Ghost; asks you first, in that session |
| `wait_for_turn` | Wait until the turn going on in a session ends (`seconds`, ten minutes if none, an hour at most), and give back what its agent said in its last turn. | Any agent |
| `interrupt_session` | Stop the turn going on in a session. | Sessions in Ghost; asks you first, in that session |
| `events` | What happened in the sessions Ghost runs (a turn ended, an agent asks you), each numbered: call again with the last number (`after`) to follow on, and with `seconds` to wait for something to happen. `missed` says some of it is no longer kept. | Any agent |
| `list_projects` | Your projects: their folders, as the sidebar lists them. | Any agent |
| `add_project` | Make a `folder` (its whole path) one of your projects, first in the list; every window open on that Ghost shows it at once. Not the root, your home folder or a folder holding it, nor Ghost's home or a folder in it. | Sessions in Ghost; asks you first |
| `search_sessions` | The sessions whose title, or what was said in them, holds `query` (case aside), the latest first, as `list_sessions` gives them. | Any agent |
| `find_files` | The files git sees in a `folder` whose paths hold the letters of `query` in order, as Ghost's file finder finds them (`smr` finds `src/main.rs`), the best first, fifty at most. | Any agent |
| `search_code` | Where `words` are in the files git sees in a `folder`: `path:line` and the line, fifty at most; `case`, `word` and `regex` as the Files tab's search. | Any agent |
| `list_prs` | A project's pull requests from the host of its remote (GitHub, GitLab, Bitbucket, Azure DevOps): `state` (open if none, closed, merged, all) and a `search` as the host reads it. | Any agent |
| `read_pr` | A pull request read whole: what it says, what was said on it and on its lines, its checks, reviewers and whether it conflicts; with `diff`, its patch too. | Any agent |
| `review_pr` | Review a pull request: `verdict` approve, request-changes or comment, a `body`, and `lines` (comments on lines of its new side). | Sessions in Ghost; asks you first |
| `merge_pr` | Merge a pull request, its branch deleted after: `how` (squash, merge or rebase; none, as **General › Pull requests › Merged as** says), now or once its checks pass (`auto`). | Sessions in Ghost; asks you first |
| `pr_checks` | A pull request's checks: each one's name, how it stands, where it ran, and for one that failed on GitHub Actions the `run` that `ci_log` reads. | Any agent |
| `ci_log` | The end of what a failed `run` of the checks printed. | Any agent |
| `list_workflows` | Your workflows: each one's name, what it is for and its steps, or why its file does not read; and their runs, with where each stands and the steps it has done. An agent that may only read is not given a `run` step's command, nor what a step gave back. | Any agent |
| `start_workflow` | Begin a run of a `workflow` with `input`: its id. | Sessions in Ghost; asks you first |
| `list_tasks` | The scheduled tasks: each one's id, title, prompt, agent, project, schedule, whether it is paused, and its last run. A webhook's address is never given. | Any agent |
| `change_task` | Pause a task (`id`), resume it, run it now or remove it (`change`). | Sessions in Ghost; asks you first |
| `list_worktrees` | The worktrees Ghost made of a project, in its worktrees' folders: each one's repository, path and branch. | Any agent |
| `make_worktree` | Make a worktree of a project as a new session's is made, from the remote's default branch, on a branch named for `about`: its path and branch. | Sessions in Ghost; asks you first |
| `remove_worktree` | Take away a worktree Ghost made (`path`), unless a session of Ghost's works in it or it holds what is not committed; its branch stays. | Sessions in Ghost; asks you first |

### Access levels

There are two kinds of caller:

- **A session in Ghost.** It may call every tool. A tool that acts
  (`delegate_task`, `open_pr`, `schedule_task`, `use_device`,
  `send_to_session`, `interrupt_session`, `add_project`, `review_pr`,
  `merge_pr`, `start_workflow`, `change_task`, `make_worktree`,
  `remove_worktree`) first asks
  you on a permission card in that session's thread, the same card the
  agent's own questions use, with **Allow**, **Always allow** and
  **Reject**. **Always allow** holds for that tool in that session until
  the session's connection ends. The card for `review_pr` and `merge_pr`
  names the pull request, its repository (by its address) and all it
  does: the verdict, or the method, whether it waits for the checks, and
  the branch deleted. `merge_pr`, `start_workflow`, `change_task` and
  `remove_worktree` never offer **Always allow**: what they do cannot be
  undone, and text an agent reads (a pull request, a CI log, code) could
  steer what it asks of them.
- **An agent outside Ghost** (below). It reads: `list_sessions`,
  `read_session`, `read_diff`, `wait_for_turn`, `events`, `list_devices`,
  `device_screen`, and each tool that lists, finds or reads above (a
  folder it names must be in a project of yours, or a worktree of one). With **Settings › Agents › Agents outside Ghost may
  act** on (`outside_acts`), it may also `send_to_session` and
  `interrupt_session`, each asked of you on that session's card with
  **Allow** and **Reject** alone: every agent outside Ghost comes in by
  the one key in `mcp.json`, so an **Always allow** would let each of
  them, and there is none. Any
  other tool that acts or asks you answers that only an agent in a Ghost
  session can do it.
- **An agent on another Mac**, reaching this Mac's tools through a
  paired device's link (`--mcp --on`, below): it reads, and acts as far as
  that device may (a read-only device's agent only reads).

## Using them from an agent outside Ghost

An agent you run in a terminal can read what Ghost keeps through the same
server, while Ghost runs on the same home:

```bash
claude mcp add ghost -- /Applications/Ghost.app/Contents/MacOS/ghost-desk --mcp
codex mcp add ghost -- /Applications/Ghost.app/Contents/MacOS/ghost-desk --mcp
```

On a machine with no window, use `ghost-server --mcp`. To reach another
machine's Ghost, run it there over SSH: `ssh host ghost-server --mcp`.

If Ghost is not running with that home, `--mcp` exits saying so.

### Over HTTP

An agent that speaks MCP's streamable HTTP is given the same tools at

```
http://127.0.0.1:<http>/mcp
```

with `<http>` and the key from `<home>/mcp.json`, the key sent as
`Authorization: Bearer <key>`. Both stay as they are across restarts
(above), so the address and key are set up once. An HTTP client sends the
key to whatever listens at that address, so do not use it while Ghost is
not running; where an app can run a command, `ghost --mcp` is the safer
way in. It is the same caller as `--mcp` with no
session: it reads, and acts as `outside_acts` lets it. Ghost keeps no MCP
session; it answers only `127.0.0.1`, `localhost` and `::1` by name, and no
request from a web page (one with an `Origin`).

### On another Mac

From a Mac paired with another, an agent reaches that Mac's tools over
the link the window uses, signed with this Mac's key for it:

```bash
ghost --mcp --on <name>
```

`<name>` is the server's name, as **Macs this Mac controls** lists it.
