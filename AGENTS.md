# AGENTS.md

Instructions for coding agents (Claude Code, Codex and others) working in
this repository. [CONTRIBUTING.md](CONTRIBUTING.md) applies in full; this file
adds the rules that are easy to break. Keep it current: a change that alters a
rule, path or command stated here updates this file in the same pull request.

## Definition of done

```bash
just check    # fmt, clippy -D warnings, all tests, docs, cargo-deny, typos
```

`desk/` is a separate workspace. For changes there, also run in `desk/`:

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

A change is reported as working only after these pass. Do not claim a
platform (QQ, Telegram, X, Linux, ...) was tested unless it was run against
the real platform in this change.

## Engineering standard

The codebase must stay small, with exactly one way to do each thing. Every
change is held to these rules.

1. **Delete before adding.** Find the code that already does the job, or that
   the change makes obsolete. Reuse it or remove it. Code made dead by a
   change is deleted in that change.
2. **One source of truth.** Each fact (a wire shape, a limit, a list of
   providers, a state machine, a policy) is defined once and referenced
   everywhere else. Never copy a type, constant or rule into another crate or
   into `desk/`: export it from its owner, or ask the owner over the protocol.
3. **No second mechanism.** Before adding a check, gate, retry, cache, second
   pass or fallback, show that stating the rule once at its source does not
   work. Where two mechanisms do the same job, merge them.
4. **No speculative generality.** No trait with one implementation, no option
   nobody sets, no parameter for a hypothetical caller, no layering with a
   single layer. Introduce an abstraction when its second real user exists.
5. **No compatibility code.** Old data, file layouts, config keys and protocol
   peers are not supported: no migrations, fallbacks, shims or branches for
   older versions. The only exception is the journal format (see Data
   invariants). A stored file in an unknown format is refused with an error
   that says what to do, not converted.
6. **Growth must pay for itself.** Every pull request states its net line
   change. Growth is justified by what it enables; a refactor should shrink
   the code. A file holding more than one responsibility is split; length
   over about 800 lines is a prompt to check, not a rule.
7. **Nothing unfinished is merged.** No TODO/FIXME, commented-out code,
   `#[allow(dead_code)]`, or public items without a non-test caller. Every
   `#[allow]` states its reason.
8. **Crate boundaries hold.** A crate depends only on what it must (see the
   map). If a change needs one crate to learn about another, the pull request
   says so and why.

## Repository map

```
crates/ghost-core      identity, journal (append-only event log), derived memory, clock, scheduling. No network, no async.
crates/ghost-brain     LLM providers (HTTP APIs and CLI agents) behind one trait. No knowledge of characters.
crates/ghost-net       the single HTTP client builder and proxy setting.
crates/ghost-work      task execution: tools, command execution, task state (work.db).
crates/ghost-mind      the runtime actor: attention, deliberation, replies, schedule, task orchestration.
crates/ghost-protocol  wire types of the Ghost Protocol.
crates/ghost-link      protocol client, and the code shared by presence adapters.
crates/ghostd          the daemon: runs one or more characters, serves the protocol, manages a core home.
crates/presence/*      channel adapters (Discord, OneBot, QQ Bot, Telegram, X, terminal), one binary each.
prompts/               all model-facing text: system/ (shared) and character/ (persona rendering).
desk/                  macOS desktop app (GPUI, separate workspace). A protocol-only client; starts a local core.
deploy/                systemd units and a backup script for servers.
```

## Canonical helpers

Use these. Do not write another.

| Need | Use |
|---|---|
| HTTP client | `ghost_net::builder()` |
| Environment for a spawned program | `ghost_net::pass_on` |
| Atomic file write | `ghost_core::whole` (`desk/src/whole.rs` in the desk) |
| Media type from a file name | `ghost_core::media::mime` |
| Protocol messages | the types in `ghost-protocol`; never hand-built JSON that mirrors them |
| Model-facing text | files in `prompts/`, read through the prompt store; never string literals in Rust |
| Core home path | `ghost_link::home()` |

## Data invariants

- **The journal is append-only and permanent.** `journal.db` is the source of
  truth. Add event variants and optional fields (`#[serde(default)]`); never
  remove, rename or change the meaning of one. A new event needs a fixture
  line in `crates/ghost-core/tests/fixtures/`, an arm in `memory/recall.rs`, a
  fold in the `memory/` feature that owns it, and an arm in
  `crates/ghostd/src/show.rs`.
- **memory.db is derived.** Everything in `crates/ghost-core/src/memory/` must
  be rebuildable from the journal alone. Changing a feature's tables or what
  its fold writes bumps that feature's `VERSION`. A fold that reads another
  feature's tables declares it in `reads` in `memory/mod.rs`.
- **work.db is state, not derived.** Changing its tables bumps
  `SCHEMA_VERSION` in `crates/ghost-work/src/undertaking/db.rs`. A file with
  another version is refused, not migrated.
- **Single writer.** Only the `Mind` actor appends to the journal. Slow work
  (model calls, network, disk) runs in a spawned job and returns an
  `Outcome`; it never touches actor state. Do not add blocking I/O to the
  actor.

## Secrets

Keys come from the environment, or from a git-ignored `.env` (repository root
or core home). Never in `ghostd.toml`, tests, examples, commit messages, the
journal or logs. Never print them.

## Local environment

- The live core home is `~/Library/Application Support/Ghost`, served on port
  7438; the desk keeps its settings in `~/.config/ghost-desk`. Tests,
  experiments and agents never write to them, never bind port 7438, and never
  run a core against `ghosts/*` in this repository. If real data is needed,
  copy it (`sqlite3 <db> ".backup <copy>"`) and work on the copy.
- Develop against a separate home on another port:
  `ghostd run --home <tmp dir> --listen 127.0.0.1:<port>`.
- Agents do not launch the desk GUI. Any tool that captures it must not
  activate its window or take keyboard focus.
- Ask the user before starting Docker.

## Prompts

English. State why, not only what: a prompt that only gives orders is
followed less reliably. Prefer contrasting examples (this, not that) to adding
rules. A prompt change is tested like code. Recorded request fixtures change
only when the wording change is intended; re-record them with
`GHOST_RECORD=1`.

## Tests

No network. Brains are scripted, channels are recorders, time is passed in.
See `crates/ghost-mind/src/mind/tests/mod.rs` for driving the actor and
`crates/ghost-work/src/worker/tests/` for the worker. Name each test for the
behaviour it proves.

## Documentation

`docs/` is git-ignored: design notes stay local. The published documents are
the READMEs, `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`, `SECURITY.md` and
`CHARTER.md`. A change to a crate's behaviour or interface updates its README
in the same pull request.

## Commits and pull requests

One change per pull request. The title follows CONTRIBUTING.md and is checked
by `scripts/check-subject.sh`. Never push to `main`. The description states
what changed and why, the net line change, and, if stored data changed, what
an existing installation does on its first start.
