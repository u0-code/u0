---
title: Troubleshooting
description: Diagnostics, logs, and what to do when something does not start or does not read.
---

# Troubleshooting

## Where to look

- **Settings › Maintenance & diagnostics** shows **Processes**: every process u0
  runs (agents, terminals, tunnels) as a tree with its CPU and memory. An
  agent can be let go (**Let it go**), and another process stopped
  (**Stop**). Below it, **Warnings and errors in the log** lists what
  went wrong recently.
- **Settings › About** shows **Where u0 keeps its files** and
  **u0's log**, each with **Show**.
- The log is `<home>/logs/u0.<date>.log`, a file a day, the last 14
  days kept (`log_days`). The launchd service also logs to
  `<home>/logs/service.log`. Set `RUST_LOG` (such as
  `RUST_LOG=u0_core=debug,u0_desk=debug`) before starting u0
  from a terminal to log more.
- `u0 status` says what the home holds: the agents switched on,
  whether the server listens, the service, the tasks, and whether
  `settings.json` reads.
- Every error u0 shows can be copied: select its words, or press
  **Copy** on it, which copies the whole of it, past what **Show all**
  unfolds. A notice that offers a way on (such as **Retry**) has **Copy
  details** under its words.

u0 opened from the Dock reads the `PATH` your login shell sets up, so
the programs it looks for (`claude`, `codex`, `npm`, `uv`, `gh`, `glab`,
`cloudflared`, `op`, `adb`) are found where your terminal finds them.
It takes your login shell's proxy too (`https_proxy`, `http_proxy`,
`all_proxy`, `no_proxy`, in either case) where it was started with none,
so the agents go out the way `claude` or `codex` do in your terminal.

## A file does not read

u0 refuses to guess at a file it cannot read, and never writes over
it. Each says what is wrong and what to do:

| File | What happens until it is fixed or moved away |
|---|---|
| `settings.json` | u0 runs on the defaults and saves nothing. Fix it, or move it away to start afresh. |
| A [policy](reference/policy.md) | Same: the settings do not load until the policy reads. |
| `sessions.json` | The thread index is not saved over. |
| `tasks.json` | No task runs or is saved. |
| `runs.json` | No workflow runs or is saved. |
| A workflow's file | It is listed with its error and cannot start. |
| `devices.json` | No device can come in. Move it away and pair each device again. |
| `servers.json` | No machine is reached. Move it away and pair with each machine again. |
| `keybindings.json` | Ignored, and logged; Settings › Keybindings does not write over it. |

u0 does not convert files from older versions: a file in a format it
no longer reads is refused in the same way.

## An agent does not start

- **Install**: the agent is not on this machine. Press it to install it
  from the registry. An agent published as an npm package needs `npm`;
  one published for `uvx` needs `uv`.
- **No build for this Mac**: the registry has no release of it for this
  platform.
- **Sign in**: the agent needs you to sign in (see
  [First run](first-run.md#signing-in)). After signing in in Terminal,
  press **I've signed in**.
- **Stopped** or **Failed**: open the agent's page. If you start it with
  a command of your own, **Check that it speaks ACP** tries it. u0
  tries a stopped agent again every five minutes, updating it first when
  it is behind the registry.
- An organisation's policy may allow only some agents; the agent then
  says the policy does not allow it.

## A turn cannot be undone

Checkpoints need the folder to be (or hold) a git repository. If a
checkpoint fails, the turn says **This turn can't be undone** with git's
reason.

## macOS asks for folder access again and again

macOS asks before an app reads a folder under `~/Desktop` or
`~/Documents` and remembers the answer for the app's signature. A build
signed ad hoc (as one built from source) is asked again after each
rebuild; see [Install](install.md#from-source).

## u0's tools

`--mcp` says **u0 is not running with** *home* when no u0 runs on
that home, or its `mcp.json` is missing. Start u0 (or `u0 serve`)
with the same `U0_HOME`.

## Remote control

| Message | What to do |
|---|---|
| The pairing link is used up or out of date: make another | Links work once, for ten minutes. Make a **New link**. |
| This device is not paired with this u0 | It was revoked, or its `devices.json` was moved away. Pair again. |
| This u0 speaks protocol N and yours M: update the older | Update one of the two so both run the same version. |
| cloudflared is not installed | `brew install cloudflared`. |
| A quick tunnel's address changed | Quick tunnels get a new address each time they start: pair again, or use a named tunnel. |
| Paired one way only | The other machine could not reach this one: start this Mac's tunnel (or reach it at `127.0.0.1`) and pair again. |
| *Organisation*'s policy does not allow … | A [policy](reference/policy.md) forbids it. |
