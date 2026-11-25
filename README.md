<p align="center">
  <img src="assets/logo.png" alt="u0" width="144">
</p>

<h1 align="center">u0</h1>

<p align="center"><b>One desktop app for the coding agents you already use.</b></p>

<p align="center">Claude Code, Codex, Antigravity and every other agent that speaks the<br>Agent Client Protocol, side by side, in one window, drawn the same way.</p>

<p align="center">
  <a href="LICENSE"><img alt="Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-3b3b3b?style=flat-square"></a>
  <img alt="Rust 1.98+" src="https://img.shields.io/badge/rust-1.98%2B-3b3b3b?style=flat-square">
  <img alt="Status: pre-release" src="https://img.shields.io/badge/status-pre--release-3b3b3b?style=flat-square">
</p>

<br>

A programmer today keeps several agents: Claude Code for one thing, Codex for
another, Antigravity to try. Each has its own terminal, its own app, its own
way of showing what it did. u0 is not another agent. It has none of its
own. It is the window you run them all from: pick an agent, give it a
folder and a task, and follow it the way you would in its own app.

## What it does

| | |
|---|---|
| **Projects down the left** | Each project is a folder, with its sessions under it, whichever agent each one is with. Start a session in a project with the agent you choose. |
| **Your agents beside them** | Switch on the ones you use. Each shows its logo and whether it is ready. |
| **Installs what is missing** | Click an agent that is not on this machine and install it there: its release is downloaded and kept in u0's home; one published as an npm or Python package is installed there with npm or uv. What is installed runs as it is; the registry's newest is installed when an agent is missing, or fails as an older version. |
| **One thread for every agent** | What you said, what the agent said, each tool call as a row that opens to its diff or its output, the questions it asks before it acts, its plan. The same rendering whichever agent it is. |
| **Their own harness, untouched** | Each agent runs as its own program, with its own tools, its own sign-in and its own sessions. u0 offers it no file system or terminal of its own, so it works exactly as it would in its CLI. |
| **Their own choices under your hand** | The agent's modes (read-only, ask first, auto), its models and how hard it thinks, as the agent itself lists them. |
| **What the session came to** | Down the right: the files it changed with lines added and removed, its plan, the commands it ran, how full its context is. |
| **A worktree of its own** | In a git repository a session can work in its own worktree, on a new branch, and leave your folder as it is. |
| **Every turn can be undone** | Before and after each turn the files are checkpointed (in an index of u0's own, under `refs/ghost/`, never touching yours). Undo puts the files back as they were before a turn; Redo puts it back. |
| **Review, commit, pull request** | What changed in the last turn, in all of them, what is not committed, the branch against main, or one commit, as git reads it, file by file, one above the other or side by side; comment on lines for the agent; then commit, commit and push, or open a pull request (GitHub with `gh`, GitLab with `glab`), its checks watched. |
| **Sessions kept, and yours to order** | Each agent's sessions are listed by project and read back from u0's own record, resumed with the agent when you write. Rename, pin, mark done, snooze, archive, branch one off, search what was said in all of them. |
| **Keys** | ⌘K the palette, ⌘N a new session, ⌘1–⌘9 and ⌘[ ⌘] between sessions, ⌘F find, ⌘J the commands drawer, ⌘⇧M the model, ⌘U usage, ⇧⌘B the right column over the thread, Esc closes a menu. Your own in `~/.ghost/keybindings.json`: `[{"key": "cmd-k", "action": "palette"}]`. |
| **A repository's own commands** | `ghost.json` at its root names commands (`{"scripts": [{"name": "dev", "command": "npm run dev"}]}`) run from the drawer; one marked `"worktree": true` runs in each new worktree. |

## How it works

Every agent is spoken to over the [Agent Client Protocol](https://agentclientprotocol.com)
(ACP): JSON-RPC, one message a line, on the standard input and output of a
program u0 starts. The [ACP registry](https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json)
is the one list of agents: their names, versions, logos, and how each is
distributed. u0 reads it, keeps a copy, and installs and starts agents
the way it says.

| Agent | How it runs |
|---|---|
| Claude Code | `claude-agent-acp`, the release archive for this platform, running your `claude` |
| Codex | `@agentclientprotocol/codex-acp`, installed with npm, running your `codex` |
| Antigravity | `agy_acp_server`, the release archive for this platform |
| Gemini CLI, Copilot, Cursor, OpenCode, … | as the registry lists them |

Signing in is the agent's own. Codex and Claude Code run the `codex` and
`claude` on your PATH where you have them, so they are the version, the
login and the settings of your terminal; without one, the adapter's own. An agent that signs in over ACP (Antigravity, with your Google account)
shows its ways to sign in in the thread, and the session opens once you are
in. u0 never sees a credential.

```
crates/core/   everything that touches the machine, built without a window
  acp.rs        one agent: its process and its ACP connection
  registry.rs   the ACP registry; installing an agent from it
  session.rs    a session's thread, built from the agent's updates
  git.rs        worktrees, checkpoints, diffs, commit / push / pull request
  store.rs      what Ghost keeps of each session
crates/desk/   the app, which draws it
  hub.rs        the agents switched on, and their work off the window's thread
  desk.rs       the window: its state and what happens when you or an agent act
  sidebar.rs, thread.rs, aside.rs, pages.rs    the three columns and the pages
```

## Installing it

Ghost.app for Apple silicon, macOS 13 or later, with Homebrew, from the
casks in this repository. The latest release, which `brew upgrade` keeps
current:

```bash
brew tap u0-code/ghost https://github.com/u0-code/u0
brew install --cask u0-code/ghost/ghost
```

Or the latest build of `main`, replaced on each push, in its place:

```bash
brew install --cask u0-code/ghost/ghost@nightly
brew reinstall --cask u0-code/ghost/ghost@nightly   # the newest build
```

The app is signed ad hoc, not notarized, so macOS would refuse to open it;
the cask removes its quarantine (`xattr -dr com.apple.quarantine`) when it
installs it.

## Running it

macOS, from a checkout, with [Rust](https://rustup.rs):

```bash
cargo run --release
```

u0 keeps its settings, the registry it last read, the agents it installed
and their logos in `~/.ghost` (or `GHOST_HOME`). Claude Code, Codex and
Antigravity are switched on the first time; **Agents & settings**, at the
foot of the left column, switches any other on.

To have it as an app in Finder, Launchpad and the Dock:

```bash
./bundle.sh                  # -> target/release/Ghost.app
```

Agents work in the folders you choose, and macOS asks before an app may read
one under `~/Desktop` or `~/Documents`. It remembers the answer for the
app's signature, so a build signed ad hoc is asked again each time. A
self-signed certificate stops that: in Keychain Access, Certificate Assistant
→ Create a Certificate…, name it `Ghost Dev`, Self Signed Root, Code Signing.
`bundle.sh` finds it and signs with it; `GHOST_SIGN_IDENTITY` names another.

## From another machine

The Ghost on one machine can be used from a Mac elsewhere: its agents,
sessions, files, git and terminals, as if you sat at it.

- On the machine with the agents, **Settings › Sharing & access** lets
  other machines reach it. It listens on `127.0.0.1:4317` only; the world
  reaches it through a [cloudflared](https://github.com/cloudflare/cloudflared)
  tunnel Ghost starts (a quick one, or a named one of yours), or an
  `ssh -L 4317:127.0.0.1:4317`. **Create link** makes a pairing link that
  works once, for ten minutes.
- On the other Mac, **Settings › Connections** takes the link and pairs. The
  server keeps only a hash of the token it gave; the Mac keeps the token in
  its Keychain. **Open** restarts the window on that server; **Revoke** on
  the server cuts a device off.
- With no window (a Linux box, say): `ghost-server`, or
  `ghost-desk --serve`, prints a pairing link for `127.0.0.1` as it starts.

A link that drops (a laptop shut, a network changed) is reached again, and
what was said meanwhile is told then.

## Security

An agent runs as you, on your machine, with whatever its own settings allow.
u0 passes on the questions it asks before it acts and answers them only
as you choose. See [SECURITY.md](SECURITY.md).

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) is the whole process; agents working here
also read [AGENTS.md](AGENTS.md).

## License

[Apache-2.0](LICENSE). Each agent's logo belongs to its company.
