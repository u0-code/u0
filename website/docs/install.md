---
title: Install
description: Install u0 on a Mac with Homebrew, or u0-server on Linux.
---

# Install

u0.app runs on Macs with Apple silicon, macOS 13 (Ventura) or later.
`u0-server`, the same server without a window, also runs on Linux
(x86_64). Builds are published on the repository's
[GitHub releases](https://github.com/u0-code/u0/releases).

## On a Mac, with Homebrew

The casks live in u0's own repository, so add it as a tap first:

```bash
brew tap u0-code/u0 https://github.com/u0-code/u0
```

Then install one of the two channels. They conflict with each other, so
install only one.

| Cask | What it installs |
|---|---|
| `u0` | The latest release. `brew upgrade` keeps it current. |
| `u0@nightly` | The latest build of `main`. It is replaced on every push. |

```bash
# The latest release
brew install --cask u0-code/u0/u0

# Or the nightly build
brew install --cask u0-code/u0/u0@nightly
brew reinstall --cask u0-code/u0/u0@nightly   # fetch the newest build
```

u0 is signed ad hoc and is not notarized, so Gatekeeper would refuse to
open it. When the cask installs u0.app it removes the quarantine
attribute (`xattr -dr com.apple.quarantine`). Installing the cask is your
choice to open it.

`brew uninstall --zap` also removes u0's home, `~/.u0`.

## Updates

u0 checks for a newer build when it starts and every four hours, unless
you turn that off (`check_updates` in [settings](reference/settings.md)).
**Settings › About** shows the version you run and lets you choose the
channel u0 updates to: **Stable** (the releases the `u0` cask
installs) or **Nightly** (what `u0@nightly` installs), and has
**Check now**. When a newer build is found, u0 says so; nothing is
installed until you press **Install**. u0 then downloads it, checks its
code signature, puts it in place of u0.app (putting the old one back
if that fails), and offers **Restart**.

## On Linux: u0-server

Each release, and the nightly build, carries
`u0-server-linux-x86_64.tar.gz` beside the Mac app. It holds one
program, `u0-server`: u0's server with no window. It runs agents,
keeps threads and serves them to a u0 window on a Mac that pairs with
it.

```bash
curl -LO https://github.com/u0-code/u0/releases/download/nightly/u0-server-linux-x86_64.tar.gz
tar -xzf u0-server-linux-x86_64.tar.gz
./u0-server
```

As it starts, `u0-server` prints where it listens and a pairing link
(with its QR code) that works once, for ten minutes. See
[Remote control](remote.md) for how a Mac pairs with it, and
[CLI](reference/cli.md) for its commands. Run it under your own supervisor
(systemd or another); `u0 service` is for macOS's launchd only.

## From source

With [Rust](https://rustup.rs) installed, from a checkout of
[u0-code/u0](https://github.com/u0-code/u0):

```bash
cargo run --release        # run the app
./bundle.sh                # build target/release/u0.app
```

A build made by hand reports its channel as `dev`, and a `dev` build never
sends [telemetry](first-run.md#telemetry), whatever the level.

macOS asks before an app may read a folder under `~/Desktop` or
`~/Documents`, and remembers the answer for the app's signature. A build
signed ad hoc is asked again after every rebuild. To stop that, create a
self-signed code-signing certificate named `u0 Dev` in Keychain Access
(Certificate Assistant › Create a Certificate…, Self Signed Root, Code
Signing). `bundle.sh` finds it and signs with it; `U0_SIGN_IDENTITY`
names another identity.

## Where u0 keeps its files

Everything u0 keeps is in its home, `~/.u0`, or the folder the
environment variable `U0_HOME` names. Two copies of u0 on one machine each
need their own `U0_HOME`. See [Privacy and security](privacy-security.md)
for what each file holds.
