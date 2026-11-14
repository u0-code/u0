# Contributing

Ghost is a small codebase with a few rules it holds to. This page is the
whole of the process.

## Setting up

You need Rust (the version in `rust-toolchain.toml` is installed for you by
`rustup`) and, for the full set of checks, [`just`], [`cargo-deny`] and
[`typos`].

```bash
just check     # what CI runs: format, lints, tests, docs, dependencies, spelling
just fix       # format, and apply the lint fixes that are mechanical
just hooks     # optional: have git check your commit subjects
```

No test needs a network, an API key or a container. If yours does, it is not
a test: see below.

## How a change gets in

1. Branch from `main`: `feat/…`, `fix/…`, `docs/…`, `chore/…`.
2. Make one change. A pull request that does two things is two pull requests.
3. `just check`.
4. Open a pull request. Its title is the subject of the commit it will
   become, so it follows the rules below; its description says what changed,
   why, how you know it works, and what could go wrong.
5. CI has to pass. Pull requests are squashed into one commit on `main`.

`main` takes no direct pushes.

## Commit subjects and pull request titles

[Conventional Commits], with a short list of types and scopes:

```
type(scope): what changed, in plain words
```

- **type**, exactly one: `feat`, `fix`, `perf`, `refactor`, `docs`, `style`,
  `test`, `bench`, `build`, `ci`, `chore`, `revert`.
- **scope**, optional: the crate without its `ghost-` prefix (`core`,
  `brain`, `work`, `mind`, `protocol`, `link`, `ghostd`, `term`, `telegram`,
  `discord`, `onebot`, `x`), or `presence`, `bench`, `repo`, `deps`,
  `readme`, `assets`. A change across two is `feat(work, mind): …`.
- 72 characters at most, no full stop at the end.
- Say what is now true, not what you did: "a stalled answer no longer holds
  a run past its time", not "fix timeout bug".
- `!` after the scope, and a `BREAKING CHANGE:` footer, for a change someone
  running a ghost has to act on.

The body says why. What the code does is in the diff; what made you write it
is not. If something was measured, give the number.

`scripts/check-subject.sh` checks all of this, and CI runs it on every pull
request title.

## What will not be merged

- **A change to what is already written in a journal.** The journal is a
  forever format. Events gain variants and optional fields; they never lose
  one, rename one or change what one means. `ghost-core` has a test that
  reads journals as every earlier version wrote them; a new event adds a
  line to its fixtures.
- **Anything that rewrites her journal**, keeps someone coming back, or lets
  her pass as human. See [CHARTER.md](CHARTER.md).
- **A secret in a tracked file.** Keys come from the environment or the
  ignored `.env`, in code, in tests and in examples.
- **Tests that only show the code runs.** A test here says something that
  would be false if the behaviour were wrong, in words a person can read:
  look at the names of the ones that are there.
- **Claims about a platform nobody ran it on.** An adapter's README says
  exactly how far it was tested. Keep it true.

## Code

- `cargo fmt`, and clippy's pedantic lints with the exceptions listed in
  `Cargo.toml`. An `#[allow]` carries a reason.
- Comments say why, or what would surprise. They are sentences.
- A crate knows as little as it can: `ghost-brain` does not know what a ghost
  is, `ghostd` does not know what Telegram is. If a change needs one to learn
  about another, say so in the pull request.
- Prompts are code. They live in `prompts/` beside the crate that uses them,
  and a change to one is tested like any other.

## Written by a model

Much of this repository was. That is welcome, and it changes nothing about
who answers for a change: whoever opens the pull request has read it, run
it, and can say why each line is there. [AGENTS.md](AGENTS.md) is what an
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
