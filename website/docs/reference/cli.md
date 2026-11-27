---
title: Command line
description: The u0 command, u0-server, and --mcp.
---

# Command line

u0 has three ways in from a terminal:

- `u0`, a small script u0 puts on your `PATH`;
- the programs themselves: the app's `u0-desk` inside u0.app, and
  `u0-server`, the server with no window;
- `--mcp`, which either program takes to speak MCP for an agent (see
  [u0's tools](../tools-for-agents.md)).

All of them use u0's home: `~/.u0`, or the folder `U0_HOME` names.

## Installing the `u0` command

**Settings › About › The u0 command** writes the script to
`~/.local/bin/u0`. If `~/.local/bin` is not on your `PATH`, u0 says
so; add it in your shell's profile.

## Commands

| Command | What it does |
|---|---|
| `u0` | Open the current folder in u0, as a project. |
| `u0 <folder>…` | Open each folder in u0 as a project. macOS hands it to the u0 running, or starts one. |
| `u0 serve` | Run u0's server with no window, for desks elsewhere to pair with. In a terminal it prints a pairing link and its QR code (never to a log). Stops on Ctrl-C. |
| `u0 service install` | macOS only. Run that server from login as a launchd agent, and have the window open on it. |
| `u0 service uninstall` | Stop the service and take it out of login. The window runs its own server again. |
| `u0 service status` | Whether the service is installed and running, and where it logs. |
| `u0 status` | This home: the agents switched on, whether the server listens, the service, the scheduled tasks. |
| `u0 tasks` | The scheduled tasks: number, schedule, next run and title. |
| `u0 doctor` | What of this home is not well (settings, the files u0 keeps, the agents on, the service), and what to do; exits non-zero if anything is not. |
| `u0 thread list` | The threads kept, the latest first, each with its agent and id. |
| `u0 thread show <agent> <id>` | What was said in a thread. |
| `u0 thread start <project> "<prompt>"` | Begin a thread in a project (`.`: this folder). `--agent <id>` names the agent (none: the first on), `--wait` waits for its answer and prints it. |
| `u0 thread send <agent> <id> "<prompt>"` | Say something in a thread, as its next turn (a turn going on is let end first). `--wait` as above. |
| `u0 workflow list` | The workflows, and their runs. |
| `u0 workflow start <name> [input]` | Begin a run of a workflow. |
| `u0 help`, `u0 --help` | The list above; `u0 <command> --help` says more of one. |

`u0-server` takes the same words (`u0-server status`, …). With none,
it serves, as `u0 serve` does. `u0 --version` (or
`u0-server --version`) prints the build: its version, commit and
channel. A word u0 does not know is refused, with what it takes.

What acts (`thread start`, `thread send`, `workflow start`) is asked of
this Mac's service, or, with `--on <name>`, of a Mac this one is paired
with, over a link signed with this Mac's key as the window's is. A Mac
that may only read there is refused. With no service and no `--on`, they
say to install the service.

## The launchd service

`u0 service install` writes `~/Library/LaunchAgents/local.u0.server.plist`
(for a home other than `~/.u0`, the label gets a suffix of its own, so
two homes never share one service). The agent runs `u0 serve` with your
`U0_HOME` at login, restarts it if it stops, and logs to
`<home>/logs/service.log`.

Installing the service also pairs this Mac's window with it, as any desk
pairs with a machine: a device named "This Mac's window" is let in, and a
machine named "This Mac's service" at `127.0.0.1:<port>` is added to the
window's list. From then on the window talks to the service instead of
starting a second server on the same home, so scheduled tasks and
workflows keep running after you quit the window.

On Linux there is no `u0 service`. Run `u0-server` under systemd or
another supervisor of your own.

## `--mcp`

```bash
/Applications/u0.app/Contents/MacOS/u0-desk --mcp
u0-server --mcp
```

Speaks MCP on standard input and output and passes everything to the
u0 running with the same home. `--mcp --on <name>` passes it instead
to the machine `<name>` this Mac is paired with, over a signed link. See
[u0's tools](../tools-for-agents.md).

## Environment

| Variable | Meaning |
|---|---|
| `U0_HOME` | u0's home. Default `~/.u0`. |
| `RUST_LOG` | What is logged, as a `tracing` filter. Default `u0_desk=info,u0_core=info`. |
| `U0_MCP_KEY` | Set by u0 for the `--mcp` it gives a thread. Do not set it yourself. |
