---
title: Command line
description: The ghost command, ghost-server, and --mcp.
---

# Command line

Ghost has three ways in from a terminal:

- `ghost`, a small script Ghost puts on your `PATH`;
- the programs themselves: the app's `ghost-desk` inside Ghost.app, and
  `ghost-server`, the server with no window;
- `--mcp`, which either program takes to speak MCP for an agent (see
  [Ghost's tools](../tools-for-agents.md)).

All of them use Ghost's home: `~/.ghost`, or the folder `GHOST_HOME` names.

## Installing the `ghost` command

**Settings › About › The ghost command** writes the script to
`~/.local/bin/ghost`. If `~/.local/bin` is not on your `PATH`, Ghost says
so; add it in your shell's profile.

## Commands

| Command | What it does |
|---|---|
| `ghost` | Open the current folder in Ghost, as a project. |
| `ghost <folder>…` | Open each folder in Ghost as a project. macOS hands it to the Ghost running, or starts one. |
| `ghost serve` | Run Ghost's server with no window, for desks elsewhere to pair with. It prints a pairing link and its QR code. Stops on Ctrl-C. |
| `ghost service install` | macOS only. Run that server from login as a launchd agent, and have the window open on it. |
| `ghost service uninstall` | Stop the service and take it out of login. The window runs its own server again. |
| `ghost service status` | Whether the service is installed and running, and where it logs. |
| `ghost status` | This home: the agents switched on, whether the server listens, the service, the scheduled tasks. |
| `ghost sessions` | The sessions kept (not archived), the latest first: how long ago, the agent, the project and the title. |
| `ghost tasks` | The scheduled tasks: number, schedule, next run and title. |
| `ghost help` | The list above. |

`ghost-server` takes the same words (`ghost-server status`, …). With none,
it serves, as `ghost serve` does. `ghost-server --version` (or
`ghost-desk --version` inside the app bundle) prints the build: its
version, commit and channel. The `ghost` script passes only the words in
the table to Ghost, so call the program itself for `--version`.

## The launchd service

`ghost service install` writes `~/Library/LaunchAgents/local.ghost.server.plist`
(for a home other than `~/.ghost`, the label gets a suffix of its own, so
two homes never share one service). The agent runs `ghost serve` with your
`GHOST_HOME` at login, restarts it if it stops, and logs to
`<home>/logs/service.log`.

Installing the service also pairs this Mac's window with it, as any desk
pairs with a server: a device named "This Mac's window" is let in, and a
server named "This Mac's service" at `127.0.0.1:<port>` is added to the
window's list. From then on the window talks to the service instead of
starting a second server on the same home, so scheduled tasks and
workflows keep running after you quit the window.

On Linux there is no `ghost service`. Run `ghost-server` under systemd or
another supervisor of your own.

## `--mcp`

```bash
/Applications/Ghost.app/Contents/MacOS/ghost-desk --mcp
ghost-server --mcp
```

Speaks MCP on standard input and output and passes everything to the
Ghost running with the same home. `--mcp --on <name>` passes it instead
to the server `<name>` this Mac is paired with, over a signed link. See
[Ghost's tools](../tools-for-agents.md).

## Environment

| Variable | Meaning |
|---|---|
| `GHOST_HOME` | Ghost's home. Default `~/.ghost`. |
| `RUST_LOG` | What is logged, as a `tracing` filter. Default `ghost_desk=info,ghost_core=info`. |
| `GHOST_MCP_KEY` | Set by Ghost for the `--mcp` it gives a session. Do not set it yourself. |
