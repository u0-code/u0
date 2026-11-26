---
title: Install
description: Install Ghost on a Mac with Homebrew, or ghost-server on Linux.
---

# Install

Ghost.app runs on Macs with Apple silicon, macOS 13 (Ventura) or later.
`ghost-server`, the same server without a window, also runs on Linux
(x86_64). Builds are published on the repository's
[GitHub releases](https://github.com/u0-code/u0/releases).

## On a Mac, with Homebrew

The casks live in Ghost's own repository, so add it as a tap first:

```bash
brew tap u0-code/ghost https://github.com/u0-code/u0
```

Then install one of the two channels. They conflict with each other, so
install only one.

| Cask | What it installs |
|---|---|
| `ghost` | The latest release. `brew upgrade` keeps it current. |
| `ghost@nightly` | The latest build of `main`. It is replaced on every push. |

```bash
# The latest release
brew install --cask u0-code/ghost/ghost

# Or the nightly build
brew install --cask u0-code/ghost/ghost@nightly
brew reinstall --cask u0-code/ghost/ghost@nightly   # fetch the newest build
```

Ghost is signed ad hoc and is not notarized, so Gatekeeper would refuse to
open it. When the cask installs Ghost.app it removes the quarantine
attribute (`xattr -dr com.apple.quarantine`). Installing the cask is your
choice to open it.

`brew uninstall --zap` also removes Ghost's home, `~/.ghost`.

## Updates

Ghost checks for a newer build when it starts and every four hours, unless
you turn that off (`check_updates` in [settings](reference/settings.md)).
**Settings › About** shows the version you run and lets you choose the
channel Ghost updates to: **Stable** (the releases the `ghost` cask
installs) or **Nightly** (what `ghost@nightly` installs), and has
**Check now**. When a newer build is found, Ghost says so; nothing is
installed until you press **Install**. Ghost then downloads it, checks its
code signature, puts it in place of Ghost.app (putting the old one back
if that fails), and offers **Restart**.

## On Linux: ghost-server

Each release, and the nightly build, carries
`ghost-server-linux-x86_64.tar.gz` beside the Mac app. It holds one
program, `ghost-server`: Ghost's server with no window. It runs agents,
keeps sessions and serves them to a Ghost window on a Mac that pairs with
it.

```bash
curl -LO https://github.com/u0-code/u0/releases/download/nightly/ghost-server-linux-x86_64.tar.gz
tar -xzf ghost-server-linux-x86_64.tar.gz
./ghost-server
```

As it starts, `ghost-server` prints where it listens and a pairing link
(with its QR code) that works once, for ten minutes. See
[Remote control](remote.md) for how a Mac pairs with it, and
[CLI](reference/cli.md) for its commands. Run it under your own supervisor
(systemd or another); `ghost service` is for macOS's launchd only.

## From source

With [Rust](https://rustup.rs) installed, from a checkout of
[u0-code/u0](https://github.com/u0-code/u0):

```bash
cargo run --release        # run the app
./bundle.sh                # build target/release/Ghost.app
```

A build made by hand reports its channel as `dev`, and a `dev` build never
sends [telemetry](first-run.md#telemetry), whatever the level.

macOS asks before an app may read a folder under `~/Desktop` or
`~/Documents`, and remembers the answer for the app's signature. A build
signed ad hoc is asked again after every rebuild. To stop that, create a
self-signed code-signing certificate named `Ghost Dev` in Keychain Access
(Certificate Assistant › Create a Certificate…, Self Signed Root, Code
Signing). `bundle.sh` finds it and signs with it; `GHOST_SIGN_IDENTITY`
names another identity.

## Where Ghost keeps its files

Everything Ghost keeps is in its home, `~/.ghost`, or the folder the
environment variable `GHOST_HOME` names. Two Ghosts on one machine each
need their own `GHOST_HOME`. See [Privacy and security](privacy-security.md)
for what each file holds.
