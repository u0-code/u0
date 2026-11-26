---
title: First run
description: Agents from the ACP registry, signing in, accounts, and telemetry.
---

# First run

## Telemetry

As Ghost first opens, it asks **Help make Ghost better?**: how much it may
tell the people who make it. Move the slider and choose **Share**, or
choose **Don't share** (Escape too), which is level 0. **Settings ›
Telemetry** changes it later, and **Show what is sent** shows the exact
JSON of the next day's batch at any level. **Reset install id** sends
future batches under a new random id, unlinked from the old one.

There are four levels, like KDE's. Each sends everything the one before
it does, and more:

| Level | What is sent |
|---|---|
| **0, Off** | Once, at the very first launch, never again (not even after an update): Ghost's version, commit and channel (stable or nightly), the system (`macos 26` or `linux`) and the processor (`aarch64` or `x86_64`). No id. |
| **1, Basic** | Once a day: the above, a random id of this install, the agents switched on (their registry ids and versions), the number of Macs this one is paired with, and whether others may control this one. |
| **2, Usage** | Also the day's counts: sessions by agent, turns, worktrees made, scheduled tasks' turns and workflow runs, Ghost's tools called (by tool name), the window's pages opened (by name), crashes. |
| **3, Detailed** | Also: the time from launch to the first frame, the memory Ghost and what it runs hold, the kinds of error (by kind, such as an agent that did not start), the models chosen (by id), the window's size and zoom, the look (light, dark, system) and the system's language. |

No level ever sends what you or an agent wrote, a path, the name of a
project, repository, branch or session, a host name, an address, an
email, a token, or the words of an error. Names that are sent (an agent's
id, a tool's, a page's, a model's) must be short tokens; anything else is
dropped.

Nothing above level 0 is sent before you choose a level. The day's counts
are kept in `<home>/telemetry.json` so a restart does not lose them; a
batch that fails to send is dropped. A build made by hand (channel `dev`)
never sends anything.

Batches go to a Cloudflare Worker (its source is in the repository's
`telemetry/` folder), which checks each against a schema, refuses
anything else, and keeps the batch with the country it came from, not the
address.

On a server with no window, set `"telemetry": 0` to `3` in its
`settings.json`. An organisation can set or lock the level with a
[policy](reference/policy.md).

## Your agents

Ghost starts with three agents switched on: **Claude Code**
(`claude-acp`), **Codex** (`codex-acp`) and **Antigravity**
(`antigravity-acp`). Every other agent the
[ACP registry](https://agentclientprotocol.com) lists (Gemini CLI,
GitHub Copilot, OpenCode and more) can be switched on in
**Settings › Agents**.

Each agent shows at the foot of the sidebar with its logo and whether it
is ready: **Sign in**, **Install**, **Installing** or **Stopped** when it
needs something.

### Installing

Ghost installs an agent the way the registry says:

- a **binary** release is downloaded and unpacked in `<home>/agents/`;
- an **npm** package is installed with `npm` (an agent run with `npx`),
  and a **Python** package with `uv` (an agent run with `uvx`), into
  Ghost's home. You need `npm` or `uv` on your `PATH` for those.

What is installed runs as it is. Ghost installs the registry's newest
version when an agent is missing, or when an older version fails to
start. It reads the registry again every hour (`check_agents`), and every
five minutes it retries agents that stopped.

**Settings › Agents › Read the registry again** reads it now. An agent's
page has **Uninstall**.

### Signing in

Signing in is the agent's own; Ghost never sees a credential.

- **Claude Code** and **Codex** run the `claude` and `codex` on your
  `PATH` where you have them (through `CLAUDE_CODE_EXECUTABLE` and
  `CODEX_PATH`), so they use the version, login and settings of your
  terminal. Without one, the adapter runs its own.
- An agent that signs in over ACP (Antigravity, with your Google account)
  shows its ways to sign in; the session opens once you are in.
- Otherwise **Sign in** runs the agent's own program in Terminal (on
  another Mac you control, in one of Ghost's terminals there), and
  **I've signed in** checks again.

**Log out** is on the agent's page where the agent supports it.

### An agent's page

Open an agent in **Settings › Agents** to set:

- **Use this command**: a command line of yours that starts it instead of
  the registry's way (your own build, a version manager's). It must speak
  ACP on standard input and output; **Check that it speaks ACP** tries it.
- **Variables**: set in the agent's environment as it starts. Mark one
  **Secret** to keep it in the macOS Keychain, never in Ghost's files. A
  value may be a 1Password reference (`op://vault/item/field`), read with
  `op` as the agent starts.
- **Your own** models: model ids offered in the composer beside those the
  agent lists. The agent may refuse one it does not know.
- **Ghost's tools**: whether its new sessions are given
  [Ghost's tools](tools-for-agents.md).
- **Accounts**: **Add an account** runs the agent again with its own
  sign-in and settings, side by side with the first. Claude Code and Codex
  support this: the new account's `CLAUDE_CONFIG_DIR` or `CODEX_HOME`
  points at a folder of its own under `<home>/accounts/`. A session begun
  in an account stays with it. Taking an account away switches it off and
  forgets its variables; its folder, with its sign-in, stays.

## Your first session

1. Add a project: drop a folder from Finder onto the sidebar, or press
   **Add project** (see [Projects and sessions](projects-and-sessions.md)).
2. Press ⌘N. Choose the agent and its model on the composer's model chip
   (or ⇧⌘M), write a task, and press Return.
3. Follow it in the thread. When the agent asks before it acts, answer on
   its card.
