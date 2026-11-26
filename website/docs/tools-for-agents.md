---
title: Ghost's tools for agents
description: The MCP server Ghost gives each session, and how an agent outside Ghost uses it.
---

# Ghost's tools for agents

Ghost has no agent of its own, but it gives your agents tools: one MCP
server, named `ghost`, attached to each session Ghost opens. Through it an
agent can read the sessions Ghost keeps, hand work to another agent, open
a pull request, schedule a task, ask you a question, and use the
simulators and emulators of the machine it runs on.

## How a session gets them

When Ghost opens a session, it passes the server to the agent through
ACP's `mcpServers`, as a program to run: Ghost's own program with `--mcp`,
with `GHOST_HOME` and a key of the session's own (`GHOST_MCP_KEY`). That
program is a small pipe: it speaks MCP on its standard input and output
and forwards everything to the Ghost server, which listens on `127.0.0.1`
only.

The port and a key for agents outside Ghost are written to
`<home>/mcp.json`, readable by you alone (mode 0600). Both are made anew
each time the server starts.

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

### Access levels

There are two kinds of caller:

- **A session in Ghost.** It may call every tool. A tool that acts
  (`delegate_task`, `open_pr`, `schedule_task`, `use_device`,
  `send_to_session`, `interrupt_session`) first asks
  you on a permission card in that session's thread, the same card the
  agent's own questions use, with **Allow**, **Always allow** and
  **Reject**. **Always allow** holds for that tool in that session until
  the session's connection ends.
- **An agent outside Ghost** (below). It reads: `list_sessions`,
  `read_session`, `read_diff`, `wait_for_turn`, `events`, `list_devices`
  and `device_screen`. With **Settings › Agents › Agents outside Ghost may
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
`Authorization: Bearer <key>`. It is the same caller as `--mcp` with no
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
