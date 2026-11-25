# AGENTS.md

Instructions for coding agents (Claude Code, Codex and others) working in
this repository. [CONTRIBUTING.md](CONTRIBUTING.md) applies in full; this file
adds the rules that are easy to break. Keep it current: a change that alters a
rule, path or command stated here updates this file in the same commit.

## What Ghost is

A desktop app (GPUI, macOS first) that runs other coding agents over the
Agent Client Protocol. It has no agent, model client or prompt of its own.
A feature that needs Ghost to think, or to give an agent tools, is out of
scope: the agent brings those.

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
   is on at first, its brand's colour). Each other fact is defined once and
   referenced everywhere else.
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
                handled, answered as Events
  proto.rs      the Ghost protocol: Call (desk to server), Event (server to desk)
  records.rs    the record of each session open with its agent, written as it comes
  repo.rs       what is asked of a project's folder and its git: each a Call, one Event;
                a file read for the viewer (a PDF's pages drawn by hayro)
  remote.rs     projects on another machine; their agents over SSH
  pty.rs        a shell in a pseudo-terminal: its process, input and size
  terminals.rs  your terminals by id, each a pty and the last 256 KiB it printed
  session.rs    a session: its thread built from session/update; its turns' checkpoints
  git.rs        worktrees, checkpoints (shadow index, refs/ghost/checkpoints), diffs, commit/push/PR
  github.rs     a project's pull requests and issues as gh lists, reads and changes them
  settings.rs   ~/.ghost/settings.json and the home
  store.rs      sessions.json and each session's record; the writer thread
  whole.rs      a file written whole; a JSON file read back
  writer.rs     text an agent writes for you: what each kind asks, what is kept
  tidy.rs       what Ghost keeps here, measured and tidied
  path.rs       the PATH your shell sets up, for an app opened from the Dock
  scripts.rs    a repository's own commands, in its ghost.json
  when.rs       times as read
  fixtures/     answers recorded from real agents
crates/desk/src/
  main.rs       the app: the window opened, the menus, the keys bound
  terminal.rs   a terminal of yours: its screen (its shell is the server's)
  hub.rs        the Transport to the server; the window's own work, back as Msg
  hub/          agent (each agent's slot, as the desk sees it)
  sessions.rs   the sessions on the desk, by key and by the agent's id; what agents say of them
  macos.rs      what Ghost asks of macOS: icon, badge, dictation, a notification's sound
  voice.rs      what you say, heard by Apple's speech recognition on this Mac alone;
                text read aloud
  keys.rs       the window's actions and keys; ~/.ghost/keybindings.json
  desk.rs       the window's state, actions and layout
  desk/         by subject: kept, lists, projects, mention, gitview, runs,
                palette (its pages: a new session in, the look, adding a
                project), rank (what is typed, found), agents, send, turns,
                replies, notices, writing, dictation, forge (the PRs and
                Issues pages), links (the ghost:// links and folders
                macOS opens Ghost with)
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
  pages/        the settings dialog and its tabs (settings, prefs, projects, writing,
                usage, voice: dictation's settings),
                the PRs and Issues pages drawn (forge)
  markdown.rs   what an agent says: gpui-kit's Markdown view, and where its links go
  ui.rs, theme.rs, style.rs
  fonts.rs, assets.rs   faces, icons
assets/         fonts and the desk's own icons, built into the app
```

## Canonical helpers

| Need | Use |
|---|---|
| A request to an agent | a `proto::Call` naming it by id, sent with `hub::Transport::send`; in the server, `acp::Agent::request` with a schema type |
| Work off the window's thread | what touches the machine (agents, store, git, files, the home): a `proto::Call`, answered as a `proto::Event`; the window's own (pictures, your editor, key bindings, terminals) a function in `hub.rs` that returns a `Msg` |
| Atomic file write | `whole::write`, off the window's thread: `store::queue(Job::Write)` for the desk's own files |
| Where Ghost keeps its files | `settings::home()` |
| A program on the PATH | `registry::which` |
| An agent's logo | `Desk::mark_of` and `ui::mark` |
| Anything git | `git.rs`, in the server: a `proto::Call` handled with `repo.rs` |
| Text written for you (a title, a commit message, ...) | `Desk::ask_writer` with a `writer::Ask`: the writer agent, in a session of its own |
| A widget (switch, button, menu, ...) | `gpui::component` (gpui-kit, which the desk names `gpui`), coloured by `theme::kit` |
| A menu opened from a trigger | `Desk::menu`: gpui-kit's Popover, held open by `Desk::popup` |

## Data

- `~/.ghost/settings.json` and `~/.ghost/sessions.json` are the files
  Ghost writes that a person may care about. Their fields are
  `deny_unknown_fields`; a file that does not read is left alone and
  nothing is saved over it.
- `~/.ghost/registry.json`, `icons/` and `agents/` are rebuilt from the
  registry.
- Sessions belong to each agent, which lists them (`session/list`, by
  project). Ghost keeps an index of those it has seen (`sessions.json`:
  title, pins, archive, worktree) and a record of each
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
  `~/.ghost/worktrees/`. It never changes your index or branch except when
  you press Commit.

## Local environment

- Never point a test or experiment at `~/.ghost`: set `GHOST_HOME` to a
  temporary directory.
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
the `nightly` prerelease with it, which `Casks/ghost@nightly.rb` installs.
A tag `v<version>` (the version in Cargo.toml) is released the same way, and
the workflow commits `Casks/ghost.rb` at that version to `main`. The subject follows CONTRIBUTING.md and is checked
by `scripts/check-subject.sh` (`just hooks` installs it as a commit hook).
The body states what changed and why, and the net line change.
