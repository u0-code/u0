# ghost-desk

A desktop app for getting things done on this machine, and for following
the characters other daemons run (see [Other daemons](#other-daemons)).
The first page is about tasks and about nobody:
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
cd desk
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

## Other daemons

The desk runs one ghost core of its own, the first group down the left,
"This machine". It can also follow daemons it does not run: another
server, or a character's own `ghostd` somewhere else. **Add connection**,
under the groups, takes a name, a URL and the name of the environment
variable that holds that daemon's token, if it asks for one. The token is
read from the environment or `.env` when the desk connects, and never
written to `desk.json`, which keeps only:

```json
"connections": [
  {"name": "studio", "url": "wss://studio.example/ghost", "token_env": "STUDIO_GHOSTD_TOKEN"}
]
```

Each connection is its own group: its name, a dot for its link, and the
characters it runs as `ghost.list` tells them, each with a dot for whether
her day has her awake. The desk only connects to these; it never starts or
stops them. Opening a character shows her thread in the middle, one of
her places at a time (the top bar picks which, newest first), and her
daemon, her clock, her brain and her work down the right. What you type
goes to her on the desk channel, as you: her deployment treats you as an
operator only if its `[operators]` lists you on `desk`. Hover a connection
and use `…`, or right-click it, to remove it.

A `ws://` link to another machine carries everything in the clear, so the
desk takes one only with a token named. Better is to reach the daemon over
an ssh tunnel and connect to the tunnel's end on this machine:

```bash
ssh -N -L 7537:127.0.0.1:7437 you@server   # then add ws://127.0.0.1:7537
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

The brain chip under the line, and the solver's row at the foot of the
sidebar, open a small picker: every `[providers]` table of the deployment,
with its mark, its name and model, and a check on the one in use. An amber
dot means this machine cannot run it, and hovering the dot says why: its
key's variable is set neither in the environment nor in the checkout's
`.env`, its CLI is not installed, or nothing answers at a local API's
address. Picking one writes `[brain]` and restarts the daemon the desk runs.

**Add brain…** opens a sheet with a tile for each provider the desk knows:
DeepSeek, OpenAI, Anthropic, Gemini, OpenRouter, Kimi, Qwen, GLM, Grok,
Mistral, Groq, Ollama on this machine, the Claude Code and Codex CLIs you
are signed in to, and any other OpenAI-compatible API. A tile fills in the
provider's base URL, the variable its key is conventionally in, and a model
to start from; the model field offers the provider's own list once the key
is there. Test makes one real request (the model list, or a one-token
answer; a CLI's version), through the same proxy setting as the daemon.
The pencil on a row of the picker opens that brain in the sheet, where it
can also be removed. Only the name of the variable is written to
`solver/ghostd.toml`, never the key.

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
