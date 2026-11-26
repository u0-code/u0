# AGENTS.md

Instructions for coding agents (Claude Code, Codex and others) working in
this repository. [CONTRIBUTING.md](CONTRIBUTING.md) applies in full; this file
adds the rules that are easy to break. Keep it current: a change that alters a
rule, path or command stated here updates this file in the same commit.

## What Ghost is

A desktop app (GPUI, macOS first) that runs other coding agents over the
Agent Client Protocol. It has no agent, model client or prompt of its own:
a feature that needs Ghost to think asks one of your agents (the writer).
Ghost may give an agent tools, as one MCP server of its own (an existing
MCP crate over `proto::Call`) attached to a session through ACP's
`mcpServers`, and the same server for agents outside Ghost.

## Definition of done

```bash
just check    # fmt, clippy -D warnings, all tests, docs, cargo-deny, typos
```

A change is reported as working only after this passes. Do not claim an
agent (Claude Code, Codex, Antigravity, ...) works with a change unless it
was run against that agent in this change, and say how far (handshake,
session opened, a prompt answered).

## Engineering standard

The codebase must stay small, with exactly one way to do each thing. Every
change is held to these rules.

1. **Delete before adding.** Find the code that already does the job, or that
   the change makes obsolete. Reuse it or remove it. Code made dead by a
   change is deleted in that change.
2. **One source of truth.** The protocol is the `agent-client-protocol` SDK's:
   its connection and its v1 schema types; never hand-built JSON-RPC or JSON
   that mirrors them. The list of agents and how each is installed is the
   ACP registry's; never a table of agents in code, but one
   (`registry::KNOWN`): what the registry does not say of an agent (the CLI
   its adapter drives and the variable that points it at yours, whether it
   is on at first, its brand's colour, the variable naming the folder its
   CLI keeps its sign-in in, which another account of it points elsewhere).
   Each other fact is defined once and referenced everywhere else.
3. **No second mechanism.** Before adding a check, gate, retry, cache,
   second pass or fallback, show that stating the rule once at its source
   does not work.
4. **No speculative generality.** No trait with one implementation, no option
   nobody sets, no parameter for a hypothetical caller.
5. **No compatibility code.** Old settings files, layouts and protocol
   versions are not supported: no migrations, fallbacks or shims. A stored
   file in an unknown format is refused with an error that says what to do,
   not converted.
6. **Growth must pay for itself.** Every commit states its net line
   change. A file holding more than one responsibility is split; length
   over about 800 lines is a prompt to check, not a rule.
7. **Nothing unfinished is merged.** No TODO/FIXME, commented-out code,
   `#[allow(dead_code)]`, or public items without a non-test caller. Every
   `#[allow]` states its reason.

## Components

What is drawn is native GPUI, on every platform Ghost runs on: no web view,
no HTML rendered from what an agent says (Markdown is the thread's format).

- UI components come from, in this order of preference: `gpui-kit`,
  `gpui-kit-material`, `ely-gpui-component`. Use a component for what it
  means (a button is a button, a dialog a dialog), not for how it happens to
  look.
- Nothing is drawn by hand that one of them can draw: a hand-made widget in
  Ghost is replaced by the library's when one exists.
- Where none of them has what is needed, or one gets it wrong, the component
  is written once in `gpui-kit-material` (the owner's repository,
  `mizorewww/gpui-kit-material`), not inside Ghost:
  - it follows Material 3; what Ghost needs beyond M3 is added as a variant
    of the M3 component, not as a component of its own;
  - the change goes in as a pull request (`gh pr create`), merged at once
    (`gh pr merge`); Ghost uses it from git until a release is cut;
  - releases are published to crates.io by that repository's GitHub
    Actions, when its components have settled; Ghost then moves back to
    the crates.io version.
  - A component that is not Material at all goes in a crate and repository
    of its own, the same way.
- A bug in, or a missing piece of, a library someone else keeps
  (`gpui-kit` is longbridge's) is fixed there, by a pull request opened
  without asking first. It must stand on its own for every user of that
  library: it fixes a bug that gets in the way of using it, or it adds what
  everyone needs. A new API argues why everyone needs it. Never "an app
  needs it". The pull request carries screenshots of the component, before
  and after where it shows. Meanwhile Ghost takes the fix from a fork through
  `[patch.crates-io]`, and drops the patch once upstream releases it.
- Anything else a crate already does (a PDF view, a parser, a format) comes
  from that crate.

## Map

Two crates. `crates/core` (`ghost-core`) is what touches the machine and
builds without GPUI; `crates/desk` (`ghost-desk`, the app) draws it. Nothing
in core may depend on gpui: `cargo tree -p ghost-core | grep gpui` finds
nothing. The desk names core's modules at its root, so `crate::git` is
core's `git` there too.

```
crates/core/src/
  acp.rs        one agent process and its SDK connection; the ACP runtime
  registry.rs   the ACP registry, installing an agent, starting one
  server.rs     the agents running, held by id; the store's one writer; each Call
                handled, answered as Events; server/scheduled: the scheduled tasks run;
                server/flows: the workflows' runs, step by step; server/tools: Ghost's
                tools, the one MCP server (rmcp), given to each session it opens
  mcp.rs        how an agent reaches the tools: `--mcp`, the pipe to them; mcp.json
  proto.rs      the Ghost protocol: Call (desk to server), Event (server to desk);
                Hello and Welcome, which begin a link
  link.rs       a server reached from elsewhere: a WebSocket on 127.0.0.1, what
                it said numbered and kept for a desk that comes back; the desk's end
  access.rs     who may link: devices paired (devices.json), pairing codes
  key.rs        a desk's Ed25519 key (OpenSSH's form): signing, verifying, fingerprints
  share.rs      sharing this Ghost: listening, the cloudflared tunnel, pairing links,
                the link a desk gives for its server to pair back with
  servers.rs    the servers elsewhere this Mac is paired with (servers.json),
                pairing with one; whether this Mac controls others at all;
                `Host`, which of a window's servers something is on
  logs.rs       what Ghost logs, and where
  bin/          ghost-server: the server with no window
  cli.rs        the `ghost` command's words: serve, service (a launchd agent per
                home, the window paired with it), status, sessions, tasks
  records.rs    the record of each session open with its agent, written as it comes
  repo.rs       what is asked of a project's folder and its git: each a Call, one Event;
                a file read for the viewer (a PDF's pages drawn by hayro)

  pty.rs        a shell in a pseudo-terminal: its process, input and size
  terminals.rs  your terminals by id, each a pty and the last 256 KiB it printed
  session.rs    a session: its thread built from session/update; its turns' checkpoints
  git.rs        worktrees, checkpoints (shadow index, refs/ghost/checkpoints), diffs, commit/push/PR
  github.rs     a project's pull requests and issues as gh lists, reads and changes them
  http.rs       the one HTTP client (ureq): the registry, its archives, the forges, updates
  forge.rs      the hosts of code and issues: their CLIs, your tokens for them, credits on merge
  forge/        GitLab, Bitbucket and Azure DevOps over their APIs; Linear's issues
  settings.rs   ~/.ghost/settings.json and the home
  secrets.rs    what is kept secret: in the macOS Keychain, or a 1Password reference
  store.rs      sessions.json and each session's record; the writer thread
  tasks.rs      scheduled tasks: a prompt, its agent and project, its schedule (tasks.json)
  workflows.rs  workflows (your workflows/*.json): steps an agent, a command, a question;
                their runs (runs.json)
  whole.rs      a file written whole; a JSON file read back
  writer.rs     text an agent writes for you: what each kind asks, what is kept
  tidy.rs       what Ghost keeps here, measured and tidied
  diagnostics.rs  the processes Ghost runs, as a tree with CPU and memory; its log's troubles
  path.rs       the PATH your shell sets up, for an app opened from the Dock
  scripts.rs    a repository's own commands, in its ghost.json
  when.rs       times as read
  update.rs     Ghost's own updates: the GitHub releases the casks install, put in place
  usage.rs      what agents say they used, read from the records: cost, tokens by
                model, an account's limits and when a refused turn may go on
  fixtures/     answers recorded from real agents
crates/desk/src/
  main.rs       the app: the window opened, the menus, the keys bound
  terminal.rs   a terminal of yours: its screen (its shell is the server's)
  hub.rs        the servers the window holds (Machine: this Mac's, in this
                process or its service, and each paired with, over a link),
                what each says back as Msg tagged by its Host; the window's
                own work, back as Msg
  hub/          agent (each agent's slot, as the desk sees it)
  sessions.rs   the sessions on the desk, by key and by the agent's id; what agents say of them
  macos.rs      what Ghost asks of macOS: icon, badge, dictation, a notification's
                sound, the Keychain
  voice.rs      what you say, heard by gpui-kit's speech (the system's recognizer, on
                this Mac alone): your dictionary, the fillers; text read aloud
  keys.rs       the window's actions and keys; ~/.ghost/keybindings.json
  desk.rs       the window's state, actions and layout
  desk/         by subject: kept, lists, projects, mention, gitview, runs,
                palette (its pages: a new session in, the look, adding a
                project), rank (what is typed, found), agents, send, turns,
                replies, notices, writing, dictation, updates, forge (the
                PRs and Issues pages), links (the ghost:// links and folders
                macOS opens Ghost with), looks (themes chosen, made, removed),
                tasks (the scheduled tasks, the one written, a task's turn),
                flows (the workflows and their runs)
  sidebar.rs, thread.rs, aside.rs, pages.rs   what is drawn
  sidebar/      its list (gpui-kit's Tree): pinned, projects, the shelves
  thread/       the thread as a list (lines), folds, cards, the composer and what
                waits beside it (later), what an agent asks for (form), the model
                picker (picker) and the mode menu (modes), the start,
                the drawer, the top bar; calls read (kinds);
                drawing helpers: plans (drawn), command output
  aside/        the column's tabs (tabs), the Diff tab as gpui-kit's diff view
                reads the patches (diff), the Files tab and a file read
                (files), git's scopes, commits and pull request (repo), a
                side chat in it (side)
  pages/        the settings dialog, gpui-kit's Settings, a page a tab (settings, prefs, projects, writing,
                usage, voice: dictation's settings, updates: on About, hosts:
                Source control, looks: Appearance's themes, diagnostics,
                devices: Devices, both ways, and the sheet that asks before pairing,
                tasks: Scheduled tasks, scope: the project a page's choices are
                set for,
                flows: Workflows),
                the PRs and Issues pages drawn (forge)
  markdown.rs   what an agent says: gpui-kit's Markdown view, and where its links go
  ui.rs, theme.rs, style.rs
  themes.rs     themes in gpui-kit's format: Ghost's, yours, a VS Code theme imported
  fonts.rs, assets.rs   faces, icons
assets/         fonts, the desk's own icons and the themes it brings, built into the app

```

## Canonical helpers

| Need | Use |
|---|---|
| A request to an agent | a `proto::Call` naming it by id, sent to the server it runs on with `Desk::call(host, …)`; in the server, `acp::Agent::request` with a schema type |
| Which server something is on | `Session::host`, `Slot::host`, `desk::Place` (a folder on a server); a server's settings `Desk::settings_on`, the settings dialog's `Desk::edited` |
| Work off the window's thread | what touches the machine (agents, store, git, files, the home): a `proto::Call`, answered as a `proto::Event`; the window's own (pictures, your editor, key bindings, terminals) a function in `hub.rs` that returns a `Msg` |
| Atomic file write | `whole::write`, off the window's thread: `store::queue(Job::Write)` for the desk's own files |
| Where Ghost keeps its files | `settings::home()` |
| A program on the PATH | `which::which` (the `which` crate) |
| Anything over HTTP | `http::get`, `http::call`, `http::download` (ureq), off the window's thread |
| An agent's logo | `Desk::mark_of` and `ui::mark` |
| Anything git | `git.rs`, in the server: a `proto::Call` handled with `repo.rs` |
| A tool for agents | a `#[tool]` in `server/tools.rs`, over what the server already does for a `Call`; one that acts calls `Tools::allowed` first |
| Text written for you (a title, a commit message, ...) | `Desk::ask_writer` with a `writer::Ask`: the writer agent, in a session of its own |
| A widget (switch, button, menu, ...) | `gpui::component` (gpui-kit, which the desk names `gpui`), coloured by `theme::kit` |
| A menu opened from a trigger | `Desk::menu`: gpui-kit's PopupMenu (items, ticked for a choice) in a Popover held open by `Desk::popup`; one with a line to type in, `Desk::popover`; a choice among several in the settings, gpui-kit's `SettingField::dropdown` |
| A call from inside the desk's update that may call the desk back (a window call that tells its observers, such as `set_zoom`) | `window.defer`, so it runs once the update is over: the desk updated twice at once panics |
| Something shown only on hover | `invisible`, and `visible` on `group_hover`, laid over the row; never `hidden`/`flex` on hover, which GPUI can paint without laying out, and panics |
| A setting in the settings dialog, or a row of a list there | `prefs::item` (a gpui-kit SettingItem whose control the desk draws), under `prefs::group` (a SettingGroup); a row that is more than a name, a line and a control (an agent, a task, a run), `prefs::whole`; never a card of rows drawn by hand |
| A setting a project may hold otherwise (`Settings::PROJECT`) | read with `Settings::of(project)` (`Desk::settings_in`), drawn with `Desk::own` under the page's `scope_group`, kept with `Settings::hold` |
| What is typed, found | a file path: `rank::paths` (nucleo-matcher); sessions, commands, names: `rank::rank` and `rank::terms` |

## Data

- `~/.ghost/settings.json` and `~/.ghost/sessions.json` are the files
  Ghost writes that a person may care about. Their fields are
  `deny_unknown_fields`; a file that does not read is left alone and
  nothing is saved over it. A project's `prefs` hold its own settings
  (`over`, a sparse settings.json of the keys `Settings::PROJECT` names;
  any other key refuses the file) and what its new sessions of each agent
  begin with (`last`: the options and mode last chosen).
- `~/.ghost/tasks.json` holds the scheduled tasks; the server writes it
  and runs them (`Call::SetTask`, `Event::Tasks`), each run a session kept
  in the index like any other. A task on a webhook (`Schedule::Hook`) runs
  when `POST /hook/<token>` reaches the link's listener (and its tunnel),
  its body put where the prompt says `{{body}}`; nothing holds a call made
  while no server runs. Workflows are your JSON files in
  `~/.ghost/workflows/`, read as they are listed or started; their runs
  are the server's, in `~/.ghost/runs.json`.
- A secret (an agent's variable marked so, a forge's token) is kept in the
  macOS Keychain under the service `Ghost <home>`, never in a file;
  `settings.json` names it only.
- `~/.ghost/devices.json` (on the server) holds the devices paired and
  how Ghost is shared (listening, on which port, the tunnel, a named
  tunnel's token).
  Each device is `{label, key, paired, seen}`: `key` the public half of
  its Ed25519 key as `authorized_keys` writes it (`ssh-ed25519 AAAA…`),
  shown by its `SHA256:` fingerprint. `~/.ghost/servers.json` (on a desk)
  holds the servers paired with (`{name, host, key}`, `key` the public
  half this Mac made for it) and whether this Mac controls others
  (`control`: on, the window holds each of them beside this Mac's own;
  off, this Mac's alone). One at `127.0.0.1` on this home's own port is
  this Mac's service (`ghost service`), the way to this Mac's own Ghost
  rather than a server beside it. Each private half is a secret
  (`server/<its fingerprint>`, PKCS#8). The desk and the server (pairing back) both
  change it through `Servers::change`. Both are
  `deny_unknown_fields`, and left alone when they do not read: a file from
  before keys is refused with a word to pair again.
- A desk comes in by signature, never by a secret: the server's first
  word on each link is a nonce (`Challenge`), and the desk signs
  `ghost link <PROTOCOL>\n<nonce>` with its key for that server. A
  pairing code (128 bits, ten minutes, once) is what lets a new key in;
  Revoke removes the key and closes its links; turning listening off
  closes every link and withdraws the codes made.
- Pairing both ways is two pairings, each by a code and a signature: the
  desk's `Pair` carries a pairing link of its own server's (`back`), and
  the server, having let the desk's key in, makes a key of its own and
  pairs with that link before its `Welcome`, which says how that went.
  The way back is reached as the way there: at 127.0.0.1 for a server
  reached there, else through the desk's own tunnel; with none, the
  pairing stays one way and says why.
- Every server a window holds is reached at once, each its own link
  (resumed from what it last heard), each reached again on its own,
  waiting longer each time, while it is out of reach; what the desk
  had of it stays to be read. Everything of a server is that server's:
  its sessions, agents, projects, terminals and files, asked of it alone
  (`Desk::call`), what it says taken as its (`Msg::Event(Host, …)`).
  Its settings are its `settings.json` (in its `Welcome`), saved there
  (`Call::SaveSettings`); the settings dialog picks the server its pages
  of a server's own show, while the window's own (its look, its keys)
  are this Mac's. What an agent needs before it starts is the server's
  to say (`Event::Here`).
- Ghost's tools (`server/tools`) listen on `127.0.0.1` at a port
  `~/.ghost/mcp.json` names, with a key only that file holds (mode 0600),
  both made anew as the server starts. A session (of an agent not in
  `settings.no_tools`, its page's switch) is given them through
  ACP's `mcpServers` as this program run with `--mcp`, with a key of its
  own (`GHOST_MCP_KEY`) that says which session calls; an agent outside
  Ghost runs `--mcp` with none, and the file's key lets it in to read only.
  What acts, or asks you, asks on the session's permission card
  (`acp::Agent::question`).
- `~/.ghost/registry.json`, `icons/` and `agents/` are rebuilt from the
  registry. `logs/` is Ghost's own log, a file a day, the last
  `log_days` kept.
- Sessions belong to each agent, which lists them (`session/list`, by
  project). Ghost keeps an index of those it has seen (`sessions.json`:
  title, pins, archive, worktree, the pull requests and issues linked,
  tags) and a record of each
  (`transcripts/<agent>/<id>.jsonl`), so the list and a thread show at
  once. A session with a record is resumed (`session/resume`); one
  without is loaded (`session/load`), and recorded as it comes.
- The server writes `sessions.json` and the records, and nothing else
  does: the desk reads the index once (`Call::Watch`), sends each meta
  that changed (`Call::Keep`), and asks for a record back (`Call::Read`).
  Each line of a record is made once, where it happens: what the agent
  says, on the server as it arrives (sent to the desk as
  `Event::Appended`); what you said, a turn's end and a note, on the desk
  (sent with `Call::Append` before the agent is asked anything more). The
  desk's thread is built from those lines by the reducer the server's is
  (`Session::replay`), so a record read back is the thread you saw.
- In a repository Ghost writes commits no branch points at, under
  `refs/ghost/checkpoints/<session>/`, and worktrees under
  `~/.ghost/worktrees/`, or the folder `settings.worktrees` names
  (`Settings::worktree_folders`, sent with each call that makes, lists or
  tidies them). It never changes your index or branch except when you
  press Commit. The server takes away an archived session's worktree once
  it is left `tidy_after` days (git keeps one with changes, and its
  branch), and checks it out again on that branch when the session is
  opened again.

## Local environment

- Never point a test or experiment at `~/.ghost`: set `GHOST_HOME` to a
  temporary directory.
- Two Ghosts on one machine each take a `GHOST_HOME` of their own, and
  a `port` of their own in its `devices.json` (4317 unless set). A test
  of the Keychain uses a service of its own and removes what it kept;
  the unit tests keep secrets in memory, never in the Keychain.
- Agents do not launch the GUI. A check against a real agent drives
  `acp.rs` and `registry.rs` from a test run by hand with a temporary
  `GHOST_HOME`, and sends no prompt unless the user asked: a prompt spends
  the user's subscription.

## Tests

No network. An agent in a test is a shell script speaking JSON-RPC; an
answer recorded from a real agent is a fixture in `crates/core/src/fixtures/`. Name each
test for the behaviour it proves.

## Documentation

`docs/` is git-ignored: design notes stay local. The published documents are
`README.md`, `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md` and `SECURITY.md`.

## Commits

Changes are committed straight to `main`, one change per commit, and pushed;
there are no pull requests, and no CI checks a change. `just check` before
each commit is what stands in for both. CI only builds: each push to `main`
has `.github/workflows/app.yml` build Ghost.app with `bundle.sh` and replace
the `nightly` prerelease with it, which `Casks/ghost@nightly.rb` installs;
then it builds `ghost-server` on Linux and puts it beside the app.
A tag `v<version>` (the version in Cargo.toml) is released the same way, and
the workflow commits `Casks/ghost.rb` at that version to `main`. The subject follows CONTRIBUTING.md and is checked
by `scripts/check-subject.sh` (`just hooks` installs it as a commit hook).
The body states what changed and why, and the net line change.
