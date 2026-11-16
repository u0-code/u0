# For an agent working in this repository

Read [CONTRIBUTING.md](CONTRIBUTING.md) first: everything in it applies to
you. What follows is what a person would pick up by being here.

## Before you say a change is done

```bash
just check
```

That is formatting, clippy with warnings as errors, every test, the docs,
the dependency checks and spelling: what CI runs. Do not report a change as
working on the strength of having written it.

## Where things are

```
crates/ghost-core      identity, journal, derived memory, clock, tempo, will. No network.
crates/ghost-brain     model providers behind one trait. Knows nothing of ghosts.
crates/ghost-net       where every HTTP client is built; the one proxy setting.
crates/ghost-work      the loop she does things with: tools, a machine, state on disk.
crates/ghost-mind      the actor that lives: attention, deliberation, speaking, her day, her work.
crates/ghost-protocol  wire types of the Ghost Protocol.
crates/ghost-link      protocol client.
crates/ghostd          the daemon: runs one or many characters, serves the protocol.
crates/presence/*      channel adapters, each its own binary.
desk/                  the desktop app (GPUI, its own workspace); runs a ghost core of its own.
bench/                 how the work loop is measured against other agents.
deploy/                systemd units, a backup script, running on a server.
```

## Rules that are easy to break without noticing

- **The journal is a forever format.** Add event variants and optional
  fields (`#[serde(default)]`). Never remove, rename or repurpose one. A new
  event needs a line in `crates/ghost-core/tests/fixtures/`, an arm in
  `memory/recall.rs` (and a fold in whichever `memory/` file keeps it), and
  one in `ghostd`'s `show.rs`.
- **Memory is derived.** Anything in `memory/` must be rebuildable from the
  journal alone. If you add a table or a column, bump `SCHEMA_VERSION`.
- **One writer.** Only the `Mind` actor writes her journal. Anything slow is
  spawned and comes back as an `Outcome`; it does not touch her state.
- **Every HTTP client comes from `ghost_net::builder()`**, and every program
  she starts gets `ghost_net::pass_on`, so one proxy setting holds for all.
- **Keys come from the environment**, or from the ignored `.env` file every
  binary loads at start. Never from ghostd.toml, never in a test, an example,
  a commit message, her journal or a log.
- **Do not claim a platform was tested** unless it was, against the real
  platform, in this change.
- **Design documents are not committed.** `docs/` is ignored on purpose.
  READMEs, this file, `CONTRIBUTING.md`, `SECURITY.md` and `CHARTER.md` are
  the documents this repository publishes.
- **Prompts are in English, and say why.** A prompt that only gives orders
  gets worse behaviour than one that explains.
- **Fewer mechanisms.** Before adding a check, a gate or a second pass, ask
  whether saying the rule once, up front, does the job.

## Tests

They run with no network. Brains are scripted, bodies are recorders, and time
is passed in: look at `crates/ghost-mind/src/mind/tests.rs` for how a mind is
driven, and `crates/ghost-work/src/worker.rs` for a worker. Name a test for
what it shows to be true.

## Commits and pull requests

One change per pull request. The title follows the rule in CONTRIBUTING.md
and is checked by `scripts/check-subject.sh`. Never push to `main`.
