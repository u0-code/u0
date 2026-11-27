---
title: Privacy and security
description: What u0 stores and where, what it sends, and how agents and devices are kept in bounds.
---

# Privacy and security

## Agents run as you

An agent runs as you, on your machine, with whatever its own settings
allow. u0 does not sandbox it and does not read its credentials:
signing in is the agent's own. u0 passes on the questions an agent
asks before it acts, and answers them only as you choose. u0's own
[tools](tools-for-agents.md) that act ask you first in the same way.

## What u0 sends

- **Telemetry**, only as far as you allow: see
  [First run](first-run.md#telemetry) for exactly what each level sends.
  Nothing you or an agent wrote, no path, no name of a project,
  repository, branch or thread.
- **The ACP registry** is read from `cdn.agentclientprotocol.com`, and
  agents are downloaded from where it says.
- **Updates** are looked for on the repository's GitHub releases.
- **Your hosts** (GitHub, GitLab, Bitbucket, Azure DevOps, Linear) are
  asked for your pull requests and issues, with your tokens.

What your agents send to their own services is between you and them.

## What u0 keeps

Everything is in u0's home, `~/.u0` (or `U0_HOME`):

| Path | What it holds |
|---|---|
| `settings.json` | Your [settings](reference/settings.md). Agent variables marked secret are named here, never stored. |
| `sessions.json` | The index of threads seen: title, pins, archive, worktree. |
| `transcripts/<agent>/<id>.jsonl` | The record of each thread: what you and the agent said, as it happened. |
| `keybindings.json` | Your [keys](reference/keybindings.md). |
| `tasks.json`, `runs.json`, `workflows/` | [Scheduled tasks, workflow runs, your workflows](automation.md). |
| `heard.json` | The webhook calls heard in the last month (a delivery's name, a signed body's SHA-256), so none runs twice. |
| `devices.json` | Devices that may control this machine (their public keys), the port, sharing settings and a named tunnel's token. |
| `servers.json` | The machines this Mac controls (the public halves of its keys). |
| `mcp.json` | The ports of u0's tools and the key for agents outside u0 (kept too in the Keychain, until you make a new one). Readable by you alone (0600). |
| `telemetry.json` | The day's counts, this install's random id, and whether the first launch was told. |
| `registry.json`, `icons/`, `agents/` | The registry last read, agents' logos, the agents installed. |
| `accounts/` | The sign-in folders of an agent's extra [instances](first-run.md#an-agents-page). |
| `worktrees/` | Threads' worktrees, unless `worktrees` names another folder. |
| `themes/` | Your themes. |
| `logs/` | u0's log, a file a day, `log_days` kept. |

**Settings › Archive › What u0 keeps** shows how much each takes and
tidies what no thread uses. In repositories, u0 also keeps
[checkpoints](worktrees-and-checkpoints.md#checkpoints) under
`refs/u0/checkpoints/`.

u0's home and its `logs/` are readable by you alone (0700). Each file
u0 writes whole (its settings, the index, the tasks, the
devices and machines, `mcp.json`) is readable and writable by you alone
(0600), whatever it was before. u0 writes each file whole: after a crash it is either the old file or the
new one, never part of one.

## Secrets

Secrets are kept in the macOS Keychain, never in u0's files, under the
service `u0 <home>` (so two homes keep apart):

| Account | Secret |
|---|---|
| `env/<agent>/<NAME>` | An agent's variable marked **Secret**. |
| `token/<host>` | A token for GitHub, GitLab, Bitbucket, Azure DevOps or Linear. |
| `server/SHA256:…` | The private half of the key this Mac made for a machine it controls (PKCS#8). |
| `share/tls` | The certificate and key u0 speaks TLS with on a network. |
| `mcp/key` | The key agents outside u0 reach its tools by (also in `mcp.json`), until you make a new one. |

Any of these values may be a 1Password reference
(`op://vault/item/field`); u0 reads it with 1Password's `op` CLI each
time it is used.

The Keychain is macOS's. `u0-server` on Linux keeps no secrets: an
agent variable or token marked secret cannot be used there.

## Remote control

- u0 listens only on `127.0.0.1`, and only when you turn listening on.
  Nothing on the network reaches it unless you run a tunnel, forward the
  port, or choose a network under **Listen on**, where it speaks TLS with
  a certificate the pairing link pins.
- A device comes in by an Ed25519 key, never a password: it signs a fresh
  nonce on each link. The machine controlled holds only public keys.
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

A webhook task is signed unless you turn it off: only a call signed with
its secret runs it. Unsigned, its address is protected only by its random
token: anyone who has the address can run the task. A device that may
only read is never told a webhook's address, nor a pairing link, nor the
value of an agent's variable or a command line that starts an agent. Webhook bodies are marked to the
agent as untrusted. See [Automation](automation.md#webhooks).

## Reporting a vulnerability

Do not open an issue for a vulnerability. See
[SECURITY.md](https://github.com/u0-code/u0/blob/main/SECURITY.md).
