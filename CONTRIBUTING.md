# Contributing

Ghost is a small codebase with a few rules it holds to. This page is the
whole of the process.

## Setting up

You need Rust (the version in `rust-toolchain.toml` is installed for you by
`rustup`) and, for the full set of checks, [`just`], [`cargo-deny`] and
[`typos`].

```bash
just check     # format, lints, tests, docs, dependencies, spelling
just fix       # format, and apply the lint fixes that are mechanical
just hooks     # optional: have git check your commit subjects
```

No test needs a network, an account or a running agent. If yours does, it
is not a test: see below.

## How a change gets in

1. Make one change. A commit that does two things is two commits.
2. `just check`.
3. Commit to `main` with a subject that follows the rules below, and push.
   There are no pull requests, and no CI checks a change: `just check` is
   the gate. CI only builds the app (see AGENTS.md).

## Commit subjects

[Conventional Commits], with a short list of types and scopes:

```
type(scope): what changed, in plain words
```

- **type**, exactly one: `feat`, `fix`, `perf`, `refactor`, `docs`, `style`,
  `test`, `bench`, `build`, `ci`, `chore`, `revert`.
- **scope**, optional: `acp`, `registry`, `thread`, `sidebar`, `aside`,
  `settings`, `ui`, or `repo`, `deps`, `readme`, `assets`. A change across
  two is `feat(acp, thread): …`.
- 72 characters at most, no full stop at the end.
- Say what is now true, not what you did: "a stalled answer no longer holds
  a run past its time", not "fix timeout bug".
- `!` after the scope, and a `BREAKING CHANGE:` footer, for a change someone
  using Ghost has to act on.

The body says why. What the code does is in the diff; what made you write it
is not. If something was measured, give the number.

`scripts/check-subject.sh` checks all of this; `just hooks` has git run it
on every commit.

## What will not be merged

- **An agent of Ghost's own.** Ghost runs other agents; it has no model
  client, no prompts and no tools to give them.
- **A second copy of what the protocol or the registry says.** Wire shapes
  are the ACP schema crate's; the list of agents is the registry's.
- **A secret in a tracked file**, or Ghost reading an agent's credentials.
- **Tests that only show the code runs.** A test here says something that
  would be false if the behaviour were wrong, in words a person can read:
  look at the names of the ones that are there.
- **Claims about an agent nobody ran it with.** Say which agents a change
  was run against, and how far.

## Code

- `cargo fmt`, and clippy's pedantic lints with the exceptions listed in
  `Cargo.toml`. An `#[allow]` carries a reason.
- Comments say why, or what would surprise. They are sentences.
- Everything that waits (a process, a download, an answer) runs off the
  window's thread and comes back as a `hub::Msg`.

## Written by a model

Much of this repository was. That is welcome, and it changes nothing about
who answers for a change: whoever commits it has read it, run it, and can
say why each line is there. [AGENTS.md](AGENTS.md) is what an
agent working here is told.

## Licence

Ghost is under [Apache-2.0](LICENSE). What you contribute is offered under
the same licence, as its section 5 says, with nothing to sign.

## Security

Do not open an issue for a vulnerability. See [SECURITY.md](SECURITY.md).

[`just`]: https://github.com/casey/just
[`cargo-deny`]: https://github.com/EmbarkStudios/cargo-deny
[`typos`]: https://github.com/crate-ci/typos
[Conventional Commits]: https://www.conventionalcommits.org/en/v1.0.0/
