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

**[The user guide](https://u0-code.github.io/u0/)** ([简体中文](https://u0-code.github.io/u0/zh-Hans/))
covers installing, every page of the app, the settings, keys and
commands, and what Ghost keeps and sends.

## What it does

| | |
|---|---|
| **Projects down the left** | Each project is a folder, with its sessions under it, whichever agent each one is with. Start a session in a project with the agent you choose. |
| **Your agents beside them** | Switch on the ones you use. Each shows its logo and whether it is ready. |
| **Installs what is missing** | Click an agent that is not on this machine and install it there: its release is downloaded and kept in u0's home; one published as an npm or Python package is installed there with npm or uv. What is installed runs as it is; the registry's newest is installed when an agent is missing, or fails as an older version. |
| **One thread for every agent** | What you said, what the agent said, each tool call as a row that opens to its diff or its output, the questions it asks before it acts, its plan. The same rendering whichever agent it is: Markdown, its Mermaid diagrams drawn and, where you turn math on, its TeX set as formulas, natively. |
| **Their own harness, untouched** | Each agent runs as its own program, with its own tools, its own sign-in and its own sessions. u0 offers it no file system or terminal of its own, so it works exactly as it would in its CLI. |
| **Ghost's tools, given to them** | Each session is given one MCP server of Ghost's: the sessions kept and what was said in them, what is not committed, a task handed to an agent in a session of its own (its answer back), a pull request opened, a task scheduled, a question put to you in the thread. What acts asks you first, on the same card the agent's own questions use. |
| **Their own choices under your hand** | The agent's modes (read-only, ask first, auto), its models and how hard it thinks, as the agent itself lists them. |
| **What the session came to** | Down the right: the files it changed with lines added and removed, its plan, the commands it ran, how full its context is. |
| **A worktree of its own** | In a git repository a session can work in its own worktree, on a new branch, and leave your folder as it is. |
| **Simulators and emulators** | The column's Devices tab lists the iOS simulators (Xcode) and Android emulators (the Android SDK) of the machine the session is on: start one, power it off, watch its screen frame after frame, switch it dark or light, tap and type into it and press Home (Back and Recents on Android). An iOS simulator is touched and streamed through [AXe](https://github.com/cameroncooke/AXe) where it is installed. Agents use the same devices through Ghost's tools: `list_devices`, `device_screen`, `use_device`. |
| **A folder of repositories** | A project need not be a repository: one holding several (one level down, or two inside a folder such as `packages`) shows each in the Diff tab's repository picker, its changes, commits, commit and pull request read there. A worktree session gets a folder laid out as the project is, with a worktree of each repository on the session's branch and the rest linked; each turn is checkpointed, undone and diffed in every repository. |
| **Every turn can be undone** | Before and after each turn the files are checkpointed (in an index of u0's own, under `refs/ghost/`, never touching yours). Undo puts the files back as they were before a turn; Redo puts it back. |
| **Review, commit, pull request** | What changed in the last turn, in all of them, what is not committed, the branch against main, or one commit, as git reads it, file by file, one above the other or side by side; comment on lines for the agent; then commit, commit and push, or open a pull request (GitHub with `gh`, GitLab with `glab`), its checks watched; one behind its base is updated there (Update branch, by a merge or a rebase). |
| **Sessions kept, and yours to order** | Each agent's sessions are listed by project and read back from u0's own record, resumed with the agent when you write. Rename, pin, mark done, snooze, archive, branch one off, search what was said in all of them. |
| **What it used, and your limits** | ⌘U: what each session cost and the tokens it took, by agent and by model, as the agent said; where each limit stands and when it resets, as Claude's adapter says it. A turn a limit stopped goes on by itself when the limit resets. |
| **Keys** | ⌘K the palette, ⌘N a new session, ⌘1–⌘9 and ⌘[ ⌘] between sessions, ⌘F find, ⌘J the commands drawer, ⌘⇧M the model, ⌘U usage, ⇧⌘B the right column over the thread, Esc closes a menu. Your own in `~/.ghost/keybindings.json`: `[{"key": "cmd-k", "action": "palette"}]`. |
| **In your language** | English, Simplified Chinese, German, Spanish, French, Japanese, Korean, Brazilian Portuguese, Russian or Swedish (t3code's languages), as the system is or as Settings › Appearance › Language says; what agents say stays as they say it. |
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

Every Mac's Ghost can do both, in **Settings › Devices** (or ⌘K
"Pair a device" / "Show pairing code", and the Ghost menu):

- **Let other devices control this Mac** (off at first) makes it listen on
  `127.0.0.1:4317` only; the world reaches it through a
  [cloudflared](https://github.com/cloudflare/cloudflared) tunnel Ghost
  starts (a quick one, or a named one of yours), or an
  `ssh -L 4317:127.0.0.1:4317`. It shows a pairing link, and its QR code,
  that works once, for ten minutes, and the devices paired, each by its
  key's `SHA256:` fingerprint, with **Revoke**. Turned off, it cuts every
  device off.
- **Control other Macs** (off at first: a Mac controls only itself) puts
  each Mac paired with in this window beside this one: the sidebar has a
  row for each server, this Mac's first, with its projects and sessions
  under it, whether it is reached, and **Reach again**. A server that
  drops is shown offline, what the window had of it still readable, and
  is reached again on its own. New sessions, ⌘K and ⌘P name the server
  of each project; the settings show each server's own (its agents,
  projects, tasks, sharing), chosen at their top, saved there. It lists
  the Macs paired with, with **Forget**, and takes a pasted link;
  opening a `ghost://pair` link does the same. This Mac makes an Ed25519
  key for the other, keeps its
  private half in its Keychain and sends only the public half. From then
  on it comes in by signing a nonce the server makes for each link, so no
  secret crosses the wire or sits on the server.
- **Also let it control this Mac**, ticked as you pair, pairs both ways
  at once: the other Mac pairs back with a key of its own, by a one-time
  code this Mac makes, at `127.0.0.1` or through this Mac's own tunnel.
  Where it cannot reach this Mac, the pairing stays one way and says why.
- With no window (a Linux box, say): `ghost-server` prints a pairing link
  for `127.0.0.1`, and its QR code, as it starts.

A link that drops (a laptop shut, a network changed) is reached again, and
what was said meanwhile is told then.

## From a terminal

About → **Add the ghost command** puts `ghost` in `~/.local/bin`. As
t3code's `t3`:

```bash
ghost .                    # open this folder in Ghost, as a project
ghost serve                # Ghost's server with no window
ghost service install      # run that server from login (launchd); the window uses it
ghost service status       # whether it runs, and where it logs
ghost service uninstall    # stop it, and take it out of login
ghost status               # the agents on, the server, the tasks, the service
ghost sessions             # the sessions kept, the latest first
ghost tasks                # the scheduled tasks, and when each runs next
```

The service is a launchd agent of yours (`~/Library/LaunchAgents`), its
own for each `GHOST_HOME`. Installing it pairs this Mac's window with it,
as any desk pairs, so the window reaches this Mac's Ghost there rather
than starting a second one on the same home.

## Ghost's tools in another agent

An agent Ghost did not start (Claude Code or Codex in your terminal) can
read what Ghost keeps through the same MCP server, while Ghost runs:

```bash
claude mcp add ghost -- /Applications/Ghost.app/Contents/MacOS/ghost-desk --mcp
codex mcp add ghost -- /Applications/Ghost.app/Contents/MacOS/ghost-desk --mcp
```

`--mcp` speaks MCP on its standard input and output and passes it to the
Ghost running with the same home (`GHOST_HOME`), on `127.0.0.1`, with a
key in `~/.ghost/mcp.json` that only you can read. On a machine with no
window it is `ghost-server --mcp`; from elsewhere, `ssh host ghost-server
--mcp`. Such an agent reads (the sessions, what was said, a diff); what
acts, or asks you, is for a session in Ghost, where Ghost asks you first.

## Telemetry

Ghost tells the people who make it how it is used, as far as you let it,
in four levels like KDE's: each sends what the one before it does, and
more. As Ghost first opens it asks, the slider at Usage; Settings ›
Telemetry changes it, shows the exact JSON of the next batch, and makes
a new install id. On a server with no window, set `"telemetry": 0` to `3`
in its `settings.json`.

- **0, Off**: once, at the first launch, never again (not after an
  update): Ghost's version, commit and channel (stable or nightly), the
  system (`macos 26`, or `linux`) and the processor. No id.
- **1, Basic**: each day, that and a random id of this install, the
  agents switched on (their ACP registry ids and versions), the number of
  Macs paired, and whether others may control this one.
- **2, Usage** (the default offered): also the day's counts: sessions by
  agent, turns, worktrees made, scheduled tasks' turns and workflows' runs,
  Ghost's tools called by name, pages opened by name, crashes.
- **3, Detailed**: also the time to open, the memory Ghost holds, the
  kinds of error (an agent that did not start, a change an agent refused),
  the models chosen by id, the window's size and zoom, the look and the
  system's language.

No level sends what you or an agent wrote, a path, the name of a project,
repository, branch or session, a host name, an email, a token or the
words of an error. Nothing above level 0 goes before you confirm the
level. Batches go to a Cloudflare Worker ([telemetry/](telemetry/)),
which keeps them and the country they came from, not the address.
A build made by hand (channel `dev`) sends nothing at any level: its runs
are tests, not use.

## Managed by an organisation

An administrator can set a policy, read as Ghost starts: settings from
`settings.json`, each a default where you chose nothing or locked, shown
disabled with who manages it, and limits.

On a Mac the policy is a configuration profile for the preference domain
`local.ghost.desk` (Ghost.app's bundle identifier).
[`policy/ghost.mobileconfig`](policy/ghost.mobileconfig) is an example
that names every key: change its values, give it identifiers and UUIDs of
your own, and deploy it from your MDM (as a custom profile, or its
payload as Custom Settings for that domain), or install it by hand in
System Settings > General > Device Management. Ghost reads the managed
values with CFPreferences, and only those a profile forces:
`defaults write` sets no policy.

On Linux the policy is `/etc/ghost/policy.json`. Both have this shape,
the profile's keys and values the same in a property list:

```json
{
  "organization": "Example Corp",
  "support_url": "https://it.example.com/ghost",
  "settings": {
    "telemetry": { "value": 0, "locked": true },
    "check_updates": { "value": false, "locked": true },
    "branch_prefix": { "value": "corp/", "locked": false }
  },
  "limits": {
    "agents": ["claude-acp", "codex-acp"],
    "remote": false,
    "tunnel": false,
    "webhooks": false
  }
}
```

`limits` are what no setting can undo, enforced by the server: only the
agents named start (by their registry id), no other device links to this
Mac (Ghost does not listen), no cloudflared tunnel runs, no webhook task
runs. A limit left out limits nothing.

A policy that does not read, names something that is not a setting, or a
limit Ghost does not know, keeps the settings from loading until it is
put right.

## Security

An agent runs as you, on your machine, with whatever its own settings allow.
u0 passes on the questions it asks before it acts and answers them only
as you choose. See [SECURITY.md](SECURITY.md).

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) is the whole process; agents working here
also read [AGENTS.md](AGENTS.md).

## License

[Apache-2.0](LICENSE). Each agent's logo belongs to its company.
