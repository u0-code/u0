# ghost-desk

A desktop app for getting things done on this machine, and, later, for the
characters a daemon runs. The first page is about tasks and about nobody:
down the left, your sessions with the solver, laid out the way a coding
agent's app is; in the middle, the session's thread and the line you type
into, with the brain it thinks with under your hand; down the right, the
solver's environment, its context and its work.

The solver is a ghost who is nobody in particular, run by its own daemon
from `solver/` (see [solver/README.md](../../solver/README.md)): the same
runtime every character lives on, with prompts that replace her persona and
her being with a hand's. The desk looks after that deployment: it copies
its configuration on first run, makes you its operator so that it may use
the machine while you talk, gives birth to the solver if it has not been
born, and starts and restarts its daemon. Otherwise the desk is a client
like any presence: a console that asks and watches, and a presence named
`desk` whose every session is a place of its own, so the journal keeps the
threads apart. It holds no key.

## Running it

It is its own workspace, so GPUI's tree stays out of `just check`:

```bash
cd crates/ghost-desk
cargo run --release                     # or: cargo run --release -- "free some disk space"
```

A task given on the command line opens a new session with it. The desk
reaches the solver at `ws://127.0.0.1:7438`, or at `GHOSTD_URL` on a first
run; a token comes from `GHOSTD_TOKEN` in the environment or `.env`. Your
sessions are kept in `~/.config/ghost-desk/desk.json`.

To have it as an app, with the ghost in Finder, Launchpad and the Dock:

```bash
crates/ghost-desk/bundle.sh             # -> crates/ghost-desk/target/release/Ghost.app
```

## The thread

What the solver says is drawn as the Markdown it writes. What it looked up
or opened while answering is a line that opens to what it found. A piece of
work it took on is a card that says where it stands at a glance (running,
waiting on you, finished, blocked) and opens to every sitting, report and
question, with its parts, the workers it handed pieces to, underneath.
Everything is read from the solver's journal. A part is drawn when its
entries are there; today the daemon keeps parts in the work database and
not in the journal, so most work shows none yet. A session with nothing in
it says what the solver can do here, as its deployment's `[mind.work]`
allows, and offers a few things to ask for, which only fill in the line.

Nothing on the desk is made up. Down the left, a session shows a dot only
while its work runs (amber) or waits on you (purple), with the parts of
open work under it. Down the right: Environment (the machine its work runs
on, which Open shows in Finder; the daemon and its link; the brain; whether
the desk's channel is registered), Context (the journal, the tempo, your
sessions, you as its operator), Work (this session's, newest first), and
Outputs (the files its work reported or kept), which is left out until
there is one.

## Brains

The brain chip under the line lists every `[providers]` table of the
deployment, each with its mark, and whether this machine can run it (a key
set, a CLI installed). Picking one writes `[brain]` and restarts the daemon
the desk runs. Claude Code and Codex are added in one click, as the CLIs you
are signed in to: they work through tasks with the solver's tools, and talk
without them. An API is added by its name, model, base URL and the name of
the variable its key is in; the key itself is never written.

## The look

Three faces: a sans for the interface, a mono for times and names, a serif
for the one line on an empty page. The design is set in IBM Plex Sans, IBM
Plex Mono and Instrument Serif; drop their `.ttf` or `.otf` files into
`assets/fonts/` and they are used, otherwise the system's own faces stand
in. The app's icon is the ghost of the repository's own mark, on the body
macOS app icons sit on: `assets/app/icon.svg`, rendered to
`assets/app/icon.png`. The brands' marks and where they came from are in
`assets/icons/CREDITS.txt`.

Checks for this crate run inside it: `cargo fmt`, `cargo clippy
--all-targets`, `cargo test`. macOS only, as GPUI is.
