# AGENTS.md

Instructions for coding agents (Claude Code, Codex and others) working in
this repository. [CONTRIBUTING.md](CONTRIBUTING.md) applies in full; this file
adds the rules that are easy to break. Keep it current: a change that alters a
rule, path or command stated here updates this file in the same commit.

## What u0 is

A desktop app (GPUI, macOS first) that runs other coding agents over the
Agent Client Protocol. It has no agent, model client or prompt of its own:
a feature that needs u0 to think asks one of your agents (the writer).
u0 may give an agent tools, as one MCP server of its own (an existing
MCP crate over `proto::Call`) attached to a session through ACP's
`mcpServers`, and the same server for agents outside u0.

## Definition of done

```bash
just check    # fmt, clippy -D warnings, all tests, docs, cargo-deny, typos
```

A change to `website/` also passes `just site` (the user guide built in both
languages; it needs Node, so it is not part of `just check`). A change is
reported as working only after these pass. Do not claim an
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

What is drawn is native GPUI, on every platform u0 runs on: no web view,
no HTML rendered from what an agent says (Markdown is the thread's format).

- UI components come from, in this order of preference: `u0ui`
  (`u0-code/gpui-u0ui`), `gpui-kit`. Use a component for what it means (a
  button is a button, a dialog a dialog), not for how it happens to look.
- Nothing is drawn by hand that `u0ui` or `gpui-kit` can draw: a hand-made
  widget in u0 is replaced by `u0ui`'s when one exists.
- Where `u0ui` lacks what is needed, or gets it wrong, the component is
  written or fixed once in `gpui-u0ui` (`u0-code/gpui-u0ui`), not inside
  u0:
  - it follows Carbon v11 and `gpui-u0ui`'s `AGENTS.md` (generated tokens,
    four themes White / G10 / G90 / G100, Material Symbols Sharp icons,
    desktop density, gallery page and tests);
  - the change goes in as a pull request (`gh pr create -R u0-code/gpui-u0ui`),
    merged at once (`gh pr merge --squash --delete-branch`), and u0 updates
    `u0ui` (`cargo update -p u0ui`).
- A bug in, or a missing piece of, a library someone else keeps
  (`gpui-kit` is longbridge's) is fixed there, by a pull request opened
  without asking first. It must stand on its own for every user of that
  library: it fixes a bug that gets in the way of using it, or it adds what
  everyone needs. A new API argues why everyone needs it. Never "an app
  needs it". The pull request carries screenshots of the component, before
  and after where it shows. Meanwhile u0 takes the fix from a fork through
  `[patch.crates-io]`, and drops the patch once upstream releases it.
- gpui is GPUI Fast (`longbridge/gpui-fast`, through gpui-kit's
  `gpui-fast` feature), a fork of Zed's gpui. It takes performance work and
  fixes of bugs, its own or upstream gpui's, and never a feature: only such
  a fix goes there by pull request, by its CONTRIBUTING.md (the logic in
  `src/fast/<topic>.rs`, upstream's files holding only hooks that name
  `fast`, `script/check-upstream` green). A feature u0 needs of gpui (a
  window's zoom, the system's spell checker) is written the same way and
  stays on the owner's fork (`mizorewww/gpui-fast`), its `u0` branch;
  u0 patches the gpui-fast crates from `u0-release`, that branch
  built as a release is (`script/prepare-release.ts`). gpui-pre, the
  snapshots of Zed's gpui, is never patched, and `zed-industries/zed` is
  never sent a pull request.
- Anything else a crate already does (a PDF view, a parser, a format) comes
  from that crate.

## Map

Two crates. `crates/core` (`u0-core`) is what touches the machine and
builds without GPUI; `crates/desk` (`u0-desk`, the app) draws it. Nothing
in core may depend on gpui: `cargo tree -p u0-core | grep gpui` finds
nothing. The desk names core's modules at its root, so `crate::git` is
core's `git` there too.

```
crates/core/src/
  acp.rs        one agent process and its SDK connection; the ACP runtime
  registry.rs   the ACP registry, installing an agent, starting one
  server.rs     the agents running, held by id; the store's one writer; each Call
                handled, answered as Events; server/scheduled: the scheduled tasks run;
                server/flows: the workflows' runs, step by step; server/tools: u0's
                tools, the one MCP server (rmcp), given to each session it opens (by
                subject in server/tools/: devices, projects, forge, flows,
                worktrees); server/happened: the turns
                going on and what happened, for the tools; server/door: how agents
                reach the tools (a pipe, HTTP, a link), who calls
  mcp.rs        how an agent reaches the tools: `--mcp`, the pipe to them; mcp.json
  proto.rs      the u0 protocol: Call (desk to server), Event (server to desk);
                Hello and Welcome, which begin a link
  link.rs       a server reached from elsewhere: a WebSocket upgraded from hyper, what
                it said numbered and kept for a desk that comes back; the desk's end
  access.rs     who may link: devices paired (devices.json), their scopes, pairing codes
  reach.rs      the ways to a server (127.0.0.1, a network, a tunnel); pairing links;
                its TLS on a network, its certificate pinned by the link
  key.rs        a desk's Ed25519 key (OpenSSH's form): signing, verifying, fingerprints
  share.rs      sharing this u0: listening, the cloudflared tunnel, pairing links,
                the link a desk gives for its server to pair back with
  servers.rs    the servers elsewhere this Mac is paired with (servers.json),
                pairing with one; whether this Mac controls others at all;
                `Host`, which of a window's servers something is on
  logs.rs       what u0 logs, and where
  bin/          u0-server: the server with no window
  cli.rs        the `u0` command, read by clap: serve, service (a launchd agent per
                home, the window paired with it), status, doctor, tasks, thread, workflow
                (what acts over a link, `link::ask`); --mcp; folders
  records.rs    the record of each session open with its agent, written as it comes
  repo.rs       what is asked of a project's folder and its git: each a Call, one Event;
                a file read for the viewer (a PDF's pages drawn by hayro)

  pty.rs        a shell in a pseudo-terminal: its process, input and size
  terminals.rs  your terminals by id, each a pty and the last 256 KiB it printed
  session.rs    a session: its thread built from session/update; its turns' checkpoints
  git.rs        worktrees, checkpoints (shadow index, refs/u0/checkpoints), diffs, commit/push/PR;
                the repositories a folder of them holds, a worktree and checkpoint of each
  github.rs     a project's pull requests and issues as gh lists, reads and changes them
                (their conversations on lines, reviews with comments on lines)
  review.rs     a pull request's files from its patch: what kind each is, a mark of its changes
  http.rs       the one HTTP client (ureq): the registry, its archives, the forges, updates
  forge.rs      the hosts of code and issues: their CLIs, your tokens for them, credits on merge
  forge/        GitLab, Bitbucket and Azure DevOps over their APIs; Linear's issues
  settings.rs   ~/.u0/settings.json and the home
  policy.rs     an organisation's policy (/etc/u0/policy.json): settings set as
                defaults or locked; limits the server enforces
  policy/       profile: on a Mac, the policy a configuration profile forces (CFPreferences)
  secrets.rs    what is kept secret: in the macOS Keychain, or a 1Password reference
  store.rs      sessions.json and each session's record; the writer thread
  tasks.rs      scheduled tasks: a prompt, its agent and project, its schedule (tasks.json)
  workflows.rs  workflows (your workflows/*.json): steps an agent, a command, a question;
                their runs (runs.json)
  whole.rs      a file written whole; a JSON file read back
  writer.rs     text an agent writes for you: what each kind asks, what is kept
  tidy.rs       what u0 keeps here, measured and tidied
  devices.rs    iOS simulators and Android emulators: listed, started, their screen
                streamed, touched
                (xcrun simctl, adb, emulator; AXe for an iOS one, where installed)
  diagnostics.rs  the processes u0 runs, as a tree with CPU and memory; its log's troubles
  path.rs       the PATH your shell sets up, for an app opened from the Dock
  scripts.rs    a repository's own commands, in its u0.json
  telemetry.rs  what u0 tells its makers, by level (0-3): the counts, the daily batch
  when.rs       times as read
  update.rs     u0's own updates: the GitHub releases the casks install, put in place
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
  macos.rs      what u0 asks of macOS: icon, badge, dictation, a notification's
                sound, the Keychain
  voice.rs      what you say, heard by gpui-kit's speech (the system's recognizer, on
                this Mac alone): your dictionary, the fillers; text read aloud
  keys.rs       the window's actions and keys; ~/.u0/keybindings.json
  desk.rs       the window's state, actions and layout
  desk/         by subject: kept, lists, projects, mention, gitview, runs,
                palette (its pages: a new session in, the look, adding a
                project), rank (what is typed, found), agents, send, turns,
                replies, notices, writing, dictation, updates, forge (the
                PRs and Issues pages), pr (a pull request reviewed: files
                viewed, ~/.u0/viewed.json; lines commented; its guide), links (the u0:// links and folders
                macOS opens u0 with), looks (themes chosen, made, removed),
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
                side chat in it (side), the Devices tab: simulators and
                emulators (sims)
  pages/        the settings dialog, gpui-kit's Settings, a page a tab (settings, prefs, projects, writing,
                usage, voice: dictation's settings, updates: on About, hosts:
                Source control, looks: Appearance's themes, diagnostics,
                devices: Devices, both ways, and the sheet that asks before pairing,
                tasks: Scheduled tasks, scope: the project a page's choices are
                set for,
                flows: Workflows, telemetry: Telemetry, and the sheet that asks
                as u0 first opens),
                the PRs and Issues pages drawn (forge, its menus forge/menus;
                a pull request reviewed, pr)
  i18n.rs       the desk's words in your language (rust-i18n, as gpui-kit): `tr`, the
                language set; locales/app.yml holds each English string in every language
  markdown.rs   what an agent says: gpui-kit's Markdown view, and where its links go
  figures.rs    what Markdown shows as a picture: Mermaid (mermaid-rs-renderer) and
                TeX (ratex), drawn natively as SVG
  ui.rs, theme.rs, style.rs   Carbon tokens, theme projection (White/G10/G90/G100), layout constants
  themes.rs     custom themes in ~/.u0/themes/: Carbon roles, a VS Code theme imported
  fonts.rs, assets.rs   IBM Plex (from u0ui), Material Symbols Sharp icons, brand marks
assets/         u0's own mark and app bundle icon, built into the app
policy/         u0.mobileconfig: the example profile, every policy key; read by a core test
website/        the user guide (Docusaurus): docs/ in English, i18n/zh-Hans/ in
                Simplified Chinese; published on GitHub Pages
telemetry/      the Cloudflare Worker the batches go to (D1), deployed with `cf deploy`;
                batch.schema.json, made from `telemetry::Batch` by a core test

```

## Canonical helpers

| Need | Use |
|---|---|
| A request to an agent | a `proto::Call` naming it by id, sent to the server it runs on with `Desk::call(host, …)`; in the server, `acp::Agent::request` with a schema type |
| Which server something is on | `Session::host`, `Slot::host`, `desk::Place` (a folder on a server); a server's settings `Desk::settings_on`, the settings dialog's `Desk::edited` |
| Work off the window's thread | what touches the machine (agents, store, git, files, the home): a `proto::Call`, answered as a `proto::Event`; the window's own (pictures, your editor, key bindings, terminals) a function in `hub.rs` that returns a `Msg` |
| Atomic file write | `whole::write`, off the window's thread: `store::queue(Job::Write)` for the desk's own files |
| Where u0 keeps its files | `settings::home()` |
| A program on the PATH | `which::which` (the `which` crate) |
| Anything over HTTP | `http::get`, `http::call`, `http::download` (ureq), off the window's thread |
| An agent's logo or a brand mark | `Desk::mark_of` and `ui::mark` (ACP registry SVG, else Simple Icons), UI icons `u0ui::Icon::carbon(CarbonIcon::…)` |
| Anything git | `git.rs`, in the server: a `proto::Call` handled with `repo.rs` |
| A tool for agents | a `#[tool]` in `server/tools.rs` or the file of its subject in `server/tools/` (its router added to `Tools::every`), over what the server already does for a `Call`; one that acts calls `Tools::allowed` first, or `Tools::allowed_on` for one that acts on a session it names |
| Text written for you (a title, a commit message, ...) | `Desk::ask_writer` with a `writer::Ask`: the writer agent, in a session of its own |
| A widget (switch, button, menu, ...) | `u0ui` (`u0ui::prelude::*`, `u0ui::components`, `u0ui::u0`, `u0ui::t3`, `u0ui::patterns`), then `gpui::component`, coloured by `u0ui::theme::set_theme` (`theme::kit`) |
| Colours, type, spacing and layout | `u0ui::theme::color(ColorRole::…)` (`theme::role`), `TypeStyled::type_style(TypeStyle::…)`, `u0ui::tokens::SPACING_*`, and `style::*` layout constants |
| A menu opened from a trigger | `Desk::menu`: gpui-kit's PopupMenu (items, ticked for a choice) in a Popover held open by `Desk::popup`; one with a line to type in, `Desk::popover`; a choice among several in the settings, gpui-kit's `SettingField::dropdown` |
| A call from inside the desk's update that may call the desk back (a window call that tells its observers, such as `set_zoom`) | `window.defer`, so it runs once the update is over: the desk updated twice at once panics |
| Something shown only on hover | `invisible`, and `visible` on `group_hover`, laid over the row; never `hidden`/`flex` on hover, which GPUI can paint without laying out, and panics |
| A setting in the settings dialog, or a row of a list there | `prefs::item` (a gpui-kit SettingItem whose control the desk draws), under `prefs::group` (a SettingGroup); a row that is more than a name, a line and a control (an agent, a task, a run), `prefs::whole`; never a card of rows drawn by hand |
| A setting a project may hold otherwise (`Settings::PROJECT`) | read with `Settings::of(project)` (`Desk::settings_in`), drawn with `Desk::own` under the page's `scope_group`, kept with `Settings::hold` |
| Words the window shows | `crate::tr("English")`, or `crate::tr_with("%{n} running", &[("n", &n)])` for a sentence with values (one key, never pieces glued), its translation in every language added to `crates/desk/locales/app.yml` (a test fails on one missing); `prefs::item`, `prefs::group` and a settings page's title translate their own. What an agent or the server says is not translated |
| What is typed, found | a file path: `repo::paths` (nucleo-matcher), in the desk and the tools alike; sessions, commands, names: `rank::rank` and `rank::terms` |
| A short name made of a title (a branch, a file, an id) | `git::slug` |

## Data

- Every file u0 writes whole (`whole::write`) is yours alone (0600):
  several hold tokens.
- `~/.u0/settings.json` and `~/.u0/sessions.json` are the files
  u0 writes that a person may care about. Their fields are
  `deny_unknown_fields`; a file that does not read is left alone and
  nothing is saved over it. A project's `prefs` hold its own settings
  (`over`, a sparse settings.json of the keys `Settings::PROJECT` names;
  any other key refuses the file) and what its new sessions of each agent
  begin with (`last`: the options and mode last chosen).
- `~/.u0/viewed.json` (the desk's) holds the files of each pull
  request you marked viewed, by its address, each with a mark of its
  changes (`review::File::mark`): a file that changes since shows unviewed.
- An administrator's policy (`policy.rs`), one shape from either source:
  on a Mac the values a configuration profile forces in the preference
  domain `local.u0.desk` (u0.app's bundle identifier; read with
  `CFPreferencesCopyAppValue`, counted only where
  `CFPreferencesAppValueIsForced`), elsewhere `/etc/u0/policy.json`.
  `policy/u0.mobileconfig` is the example profile, read back by a test.
  It sets top-level or nested (`confirm.delete`, `confirm.quit`) keys of `settings.json` as defaults or locked, applied in `Settings::load`; a
  locked control is disabled and says who manages it. One that does not
  read keeps the settings from loading, as a settings file that does not.
  Its `limits` (agents allowed, remote, tunnel, webhooks) are checked
  with `policy::allows` where the server does the thing: an agent
  launched, listening, the tunnel, a webhook run.
- `~/.u0/tasks.json` holds the scheduled tasks; the server writes it
  and runs them (`Call::SetTask`, `Event::Tasks`), each run a session kept
  in the index like any other. A task on a webhook (`Schedule::Hook`) runs
  when `POST /hook/<token>` reaches the link's listener (and its tunnel),
  what it sent put where the prompt says `{{body}}`, `{{body.<a>.<b>}}`,
  `{{header.<name>}}`, marked as data from outside. A hook is
  `signed` unless you turn it off (HMAC-SHA256 of the body, its secret `hook/<token>` in the
  Keychain; else 401), hold `only` rules (`at` is one of `is`; else 204),
  and runs each delivery its sender names, and each signed body (by its
  SHA-256), once (200 again): what was heard is kept a month in
  `~/.u0/heard.json` (`tasks::Heard`), so a restart forgets none;
  `Call::RotateHook` gives it a new token, its secret moved. Nothing holds
  a call made while no server runs. A task on an event (`Schedule::On`)
  runs as a turn ends, or an agent asks you, on a turn of yours: not
  one u0 began on its own (`Happenings::begin`, `by_u0`), so that
  no task runs on its own runs. A task with a `workflow` begins a run of it, its
  filled prompt the input: schedules, webhooks and events start
  workflows so. Workflows are your JSON files in
  `~/.u0/workflows/`, read as they are listed or started; their runs
  are the server's, in `~/.u0/runs.json`.
- A secret (an agent's variable marked so, a forge's token, a named secret
  reference's typed value under `ref/<name>`) is kept in the
  macOS Keychain under the service `u0 <home>`, never in a file (the
  server's TLS certificate and key for a network too, `share/tls`);
  `settings.json` and `~/.u0/secrets.json` (project-scoped secret
  references, `deny_unknown_fields`) name them or `op://` pointers only.
- `~/.u0/devices.json` (on the server) holds the devices paired and
  how u0 is shared (listening, on which port, on which networks
  (`bind`: on the port after it, with TLS), the tunnel, a named
  tunnel's token, `icon`).
  Each device is `{label, key, scope, until, grant_days, version, waiting, paired, seen}`:
  `key` the public half of its Ed25519 key as `authorized_keys` writes
  it (`ssh-ed25519 AAAA…`), shown by its `SHA256:` fingerprint; `scope`
  `full` or `read`; `until` when its access ends; `waiting` until you
  let it in. `~/.u0/servers.json` (on a desk)
  holds the servers paired with (`{name, host, key, pin, paired_at}`, `key` the public
  half this Mac made for it, `pin` the SHA-256 of the certificate of one
  reached on a network) and whether this Mac controls others
  (`control`: on, the window holds each of them beside this Mac's own;
  off, this Mac's alone). One at `127.0.0.1` on this home's own port is
  this Mac's service (`u0 service`), the way to this Mac's own u0
  rather than a server beside it. Each private half is a secret
  (`server/<its fingerprint>`, PKCS#8). The desk and the server (pairing back) both
  change it through `Servers::change`. Both are
  `deny_unknown_fields`, and left alone when they do not read: a file from
  before keys is refused with a word to pair again.
- A desk comes in by signature, never by a secret: the server's first
  word on each link is a nonce (`Challenge`), and the desk signs
  `u0 link <PROTOCOL>\n<nonce>` with its key for that server. A
  pairing code (128 bits, ten minutes, once) is what lets a new key in,
  as its `Grant` says: its scope, its days, and whether it waits for you
  (closed with 1013 and the server's name until then). Each `Call` on a
  link is checked against its device's scope before it is handled
  (`Access::allows`): a `read` device makes only calls that read
  (`Call::reads`, which names every call), and only of what lies, by
  its real path, in a project or a worktree. What a link is told is
  told as its scope may hear it: a `read` device is never sent a
  webhook's token (`Event::for_reader`) nor a pairing link, which goes
  only to the desk that asked for it (`Event::Offered`). Every link a device opens (a desk's,
  a port carried, the tools) lasts only while it may come in as it
  did (`link::lasts`): revoked, ended or narrowed, it is closed. Revoke
  removes the key and closes its links, as an access ended does; turning listening off
  closes every link and withdraws the codes made.
- Pairing both ways is two pairings, each by a code and a signature: the
  desk's `Pair` carries a pairing link of its own server's (`back`), and
  the server, having let the desk's key in, makes a key of its own and
  pairs with that link before its `Welcome`, which says how that went:
  only when the code it came by was made to pair both ways
  (`Grant::both`, for everything); and a server of a name this Mac is
  paired with elsewhere is refused, never put in its place.
  The way back is reached as the way there: at 127.0.0.1 for a server
  reached there, else through the desk's own tunnel; with none, the
  pairing stays one way and says why.
- A `Hello` says what its link is for (`Then`): a desk, or one
  connection to a port on the server's machine (`Forward`, a dev
  server's), its bytes carried both ways, for a device of full scope.
  The desk brings such a port here (`link::forward`) as you open an
  address another Mac's terminal printed; never through ssh.
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
- u0's tools (`server/tools`) listen on `127.0.0.1` at two ports
  `~/.u0/mcp.json` names (`port`, the pipe of `--mcp`: the server says
  a nonce first, and the pipe answers with `mcp::proof`, an HMAC of it and
  the port by its key, so the key never crosses the wire; `http`, MCP's
  streamable HTTP, the key as a bearer token, no session kept), with a
  key that file (mode 0600) and the Keychain (`mcp/key`) hold. The ports
  are taken again as the server starts (one taken by another program is
  moved from, in the file, and said with `Event::Notice`; the key is made
  anew where it was the HTTP port, whose holder may have heard it), and
  the key is kept until `Call::NewToolsKey` (**New key** on the Agents
  page) makes another, so an app set up with them keeps working. Each
  tool call checks its key still lets it in. `--mcp --on <server>` reaches a paired
  server's tools over a link (`Then::Tools`), signed in as a desk; an
  agent there reads, and acts as its device's scope lets it. A session (of an agent not in
  `settings.no_tools`, its page's switch) is given them through
  ACP's `mcpServers` as this program run with `--mcp`, with a key of its
  own (`U0_MCP_KEY`) that says which session calls; an agent outside
  u0 runs `--mcp` with none, and the file's key lets it in to read,
  and to act on a session it names only where `settings.outside_acts`
  lets it, asked of you in that session.
  What acts, or asks you, asks on the session's permission card
  (`acp::Agent::question`).
- Telemetry is the server's (`telemetry.rs`), under its own settings'
  `telemetry`, a level from 0 to 3, none until the first-run sheet is
  confirmed. Level 0's one batch (the build, the system, the processor; no
  id) goes once ever, as a home first runs; from 1 up, one batch a day,
  each level adding to the one below as README.md lists, to
  `telemetry::ENDPOINT`, the Worker in `telemetry/`. The day's counts are
  kept in `~/.u0/telemetry.json` (with the install's random id, from
  level 1); a batch that does not go is dropped. Only counts and names
  u0 or the registry gives go: never what is written, a path, a name of
  yours, a host or an error's words. The tests send nowhere.
- `~/.u0/registry.json`, `icons/` and `agents/` are rebuilt from the
  registry. `logs/` is u0's own log, a file a day, the last
  `log_days` kept.
- Sessions belong to each agent, which lists them (`session/list`, by
  project). u0 keeps an index of those it has seen (`sessions.json`:
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
- In a repository u0 writes commits no branch points at, under
  `refs/u0/checkpoints/<session>/`, and worktrees under
  `~/.u0/worktrees/`, or the folder `settings.worktrees` names
  (`Settings::worktree_folders`, sent with each call that makes, lists or
  tidies them). It never changes your index or branch except when you
  press Commit. The server takes away an archived session's worktree once
  it is left `tidy_after` days (git keeps one with changes, and its
  branch), and checks it out again on that branch when the session is
  opened again.

## Local environment

- Never point a test or experiment at `~/.u0`: set `U0_HOME` to a
  temporary directory.
- Two u0s on one machine each take a `U0_HOME` of their own, and
  a `port` of their own in its `devices.json` (4317 unless set). A test
  of the Keychain uses a service of its own and removes what it kept;
  the unit tests keep secrets in memory, never in the Keychain, and
  have no home but a temporary `U0_HOME` (`settings::home` panics
  otherwise). The desk's tests get both by building `u0-core` with its
  feature `test`.
- Agents do not launch the GUI. A check against a real agent drives
  `acp.rs` and `registry.rs` from a test run by hand with a temporary
  `U0_HOME`, and sends no prompt unless the user asked: a prompt spends
  the user's subscription.

## Tests

No network. An agent in a test is a shell script speaking JSON-RPC; an
answer recorded from a real agent is a fixture in `crates/core/src/fixtures/`. Name each
test for the behaviour it proves.

## Documentation

`docs/` is git-ignored: design notes stay local. The published documents are
`README.md`, `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`, `SECURITY.md` and
the user guide in `website/` (https://u0-code.github.io/u0/).

- The guide is written from the code: it describes what u0 does now,
  never what it is meant to do. Work not yet on `main` is at most a short
  "Coming" note.
- A change to what a person sees or does (a setting, a key, a label, a
  command, a file u0 keeps, what is sent) updates the guide's page for
  it, in English and in `website/i18n/zh-Hans/`, in the same commit. The
  reference pages (`reference/settings.md`, `keybindings.md`, `cli.md`,
  `policy.md`) list every key, action and word; they stay complete.
- A UI label in the guide is the app's own words; the Chinese pages use
  its zh-CN string from `crates/desk/locales/app.yml`.
- No screenshots: the guide is text and keyboard paths.

## Commits

Changes are committed straight to `main`, one change per commit, and pushed;
there are no pull requests, and no CI checks a change. `just check` before
each commit is what stands in for both. CI only builds: each push to `main`
has `.github/workflows/app.yml` build u0.app with `bundle.sh` and replace
the `nightly` prerelease with it, which `Casks/u0@nightly.rb` installs;
then it builds `u0-server` on Linux and puts it beside the app.
A push to `main` that changes `website/` has `.github/workflows/docs.yml`
build the guide and deploy it to GitHub Pages.
A tag `v<version>` (the version in Cargo.toml) is released the same way, and
the workflow commits `Casks/u0.rb` at that version to `main`. The subject follows CONTRIBUTING.md and is checked
by `scripts/check-subject.sh` (`just hooks` installs it as a commit hook).
The body states what changed and why, and the net line change.
