---
title: Privacy and security
description: What Ghost stores and where, what it sends, and how agents and devices are kept in bounds.
---

# Privacy and security

## Agents run as you

An agent runs as you, on your machine, with whatever its own settings
allow. Ghost does not sandbox it and does not read its credentials:
signing in is the agent's own. Ghost passes on the questions an agent
asks before it acts, and answers them only as you choose. Ghost's own
[tools](tools-for-agents.md) that act ask you first in the same way.

## What Ghost sends

- **Telemetry**, only as far as you allow: see
  [First run](first-run.md#telemetry) for exactly what each level sends.
  Nothing you or an agent wrote, no path, no name of a project,
  repository, branch or session.
- **The ACP registry** is read from `cdn.agentclientprotocol.com`, and
  agents are downloaded from where it says.
- **Updates** are looked for on the repository's GitHub releases.
- **Your hosts** (GitHub, GitLab, Bitbucket, Azure DevOps, Linear) are
  asked for your pull requests and issues, with your tokens.

What your agents send to their own services is between you and them.

## What Ghost keeps

Everything is in Ghost's home, `~/.ghost` (or `GHOST_HOME`):

| Path | What it holds |
|---|---|
| `settings.json` | Your [settings](reference/settings.md). Agent variables marked secret are named here, never stored. |
| `sessions.json` | The index of sessions seen: title, pins, archive, worktree. |
| `transcripts/<agent>/<id>.jsonl` | The record of each session: what you and the agent said, as it happened. |
| `keybindings.json` | Your [keys](reference/keybindings.md). |
| `tasks.json`, `runs.json`, `workflows/` | [Scheduled tasks, workflow runs, your workflows](automation.md). |
| `devices.json` | Devices that may control this machine (their public keys), the port, sharing settings and a named tunnel's token. |
| `servers.json` | The servers this Mac controls (the public halves of its keys). |
| `mcp.json` | The port of Ghost's tools and the key for agents outside Ghost. Made anew at each start, readable by you alone (0600). |
| `telemetry.json` | The day's counts, this install's random id, and whether the first launch was told. |
| `registry.json`, `icons/`, `agents/` | The registry last read, agents' logos, the agents installed. |
| `accounts/` | The sign-in folders of extra [accounts](first-run.md#an-agents-page) of an agent. |
| `worktrees/` | Sessions' worktrees, unless `worktrees` names another folder. |
| `themes/` | Your themes. |
| `logs/` | Ghost's log, a file a day, `log_days` kept. |

**Settings › Sessions › What Ghost keeps** shows how much each takes and
tidies what no session uses. In repositories, Ghost also keeps
[checkpoints](worktrees-and-checkpoints.md#checkpoints) under
`refs/ghost/checkpoints/`.

Files other than `mcp.json` are created with your usual permissions (your
umask), and a file Ghost rewrites keeps the permissions it had. Ghost
writes each file whole: after a crash it is either the old file or the
new one, never part of one.

## Secrets

Secrets are kept in the macOS Keychain, never in Ghost's files, under the
service `Ghost <home>` (so two homes keep apart):

| Account | Secret |
|---|---|
| `env/<agent>/<NAME>` | An agent's variable marked **Secret**. |
| `token/<host>` | A token for GitHub, GitLab, Bitbucket, Azure DevOps or Linear. |
| `server/SHA256:…` | The private half of the key this Mac made for a server it controls (PKCS#8). |

Any of these values may be a 1Password reference
(`op://vault/item/field`); Ghost reads it with 1Password's `op` CLI each
time it is used.

The Keychain is macOS's. `ghost-server` on Linux keeps no secrets: an
agent variable or token marked secret cannot be used there.

## Remote control

- Ghost listens only on `127.0.0.1`, and only when you turn listening on.
  Nothing on the network reaches it unless you run a tunnel or forward the
  port.
- A device comes in by an Ed25519 key, never a password: it signs a fresh
  nonce on each link. The server holds only public keys.
- Pairing codes are 128 random bits, used once, valid ten minutes, never
  written to disk.
- Each device may do everything or only read, until revoked or for a
  number of days, and may have to wait until you let it in. What it may
  do is checked here, for each call it makes.
- **Revoke** removes a device's key and closes its links at once; turning
  listening off closes every link and withdraws every code.
- A [policy](reference/policy.md) can forbid listening, tunnels, webhooks
  and agents.

See [Remote control](remote.md) for details, including that a named
tunnel's token is stored in `devices.json` in plain text.

## Webhooks

A webhook task's address is protected only by its random token: anyone
who has the address can run the task. Webhook bodies are marked to the
agent as untrusted. See [Automation](automation.md#webhooks).

## Reporting a vulnerability

Do not open an issue for a vulnerability. See
[SECURITY.md](https://github.com/u0-code/u0/blob/main/SECURITY.md).
