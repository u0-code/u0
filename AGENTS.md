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
2. **One source of truth.** The wire shapes are `agent-client-protocol-schema`'s
   v1 types; never hand-built JSON that mirrors them. The list of agents and
   how each is installed is the ACP registry's; never a table of agents in
   code. Each other fact is defined once and referenced everywhere else.
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

## Map

```
src/acp.rs        one agent process and its JSON-RPC; the ACP runtime
src/registry.rs   the ACP registry, installing an agent, starting one
src/hub.rs        the agents switched on; async work, back to the window as Msg
src/session.rs    a session: its thread built from session/update; its turns' checkpoints
src/git.rs        worktrees, checkpoints (shadow index, refs/ghost/checkpoints), diffs, commit/push/PR
src/settings.rs   ~/.ghost/settings.json and the home
src/store.rs      sessions.json and each session's record; the one writer
src/tidy.rs       what Ghost keeps here, measured and tidied
src/keys.rs       the window's actions and keys; ~/.ghost/keybindings.json
src/desk.rs       the window's state, actions and layout
src/desk/         by subject: kept, lists, projects, mention, gitview, runs,
                  palette, agents, turns, replies
src/sidebar.rs, src/thread.rs, src/aside.rs, src/pages.rs   what is drawn
src/thread/       the thread as a list (lines), folds, cards, the composer,
                  the drawer; drawing helpers: diffs, plans, command output
src/markdown.rs, src/syntax.rs, src/input.rs, src/ui.rs, src/theme.rs, src/style.rs, src/base64.rs
assets/           fonts and the desk's own icons, built into the binary
```

## Canonical helpers

| Need | Use |
|---|---|
| A request to an agent | `acp::Agent::request` with a schema type |
| Work off the window's thread | a function in `hub.rs` that returns a `Msg` |
| Atomic file write | `whole::write` |
| Where Ghost keeps its files | `settings::home()` |
| A program on the PATH | `registry::which` |
| An agent's logo | `ui::Mark::of` and `ui::mark` |
| Anything git | `git.rs`, called off the window's thread through `hub` |

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
answer recorded from a real agent is a fixture in `src/fixtures/`. Name each
test for the behaviour it proves.

## Documentation

`docs/` is git-ignored: design notes stay local. The published documents are
`README.md`, `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md` and `SECURITY.md`.

## Commits

Changes are committed straight to `main`, one change per commit, and pushed;
there are no pull requests and no CI. `just check` before each commit is
what stands in for both. The subject follows CONTRIBUTING.md and is checked
by `scripts/check-subject.sh` (`just hooks` installs it as a commit hook).
The body states what changed and why, and the net line change.
