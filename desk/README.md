# ghost-desk

A desktop app for getting things done on this machine, and for following
the characters other daemons run (see [Other daemons](#other-daemons)).
The first page is about getting things done with Ghost, the project's
mascot, on this machine: down the left, your sessions
with it, laid out the way a coding
agent's app is; in the middle, the session's thread and the line you type
into, with the brain it thinks with under your hand; down the right, the
solver's environment, its context and its work.

Ghost the mascot (the solver, in the code) is run by its own daemon
from the desk's core in `core/` (see [core/README.md](core/README.md)): the same
runtime every character lives on, with prompts that replace a character's
persona and being with its own. The desk looks after that deployment: it
copies its configuration on first run, gives birth to Ghost if
it has not been born, and starts and restarts its daemon. Otherwise the desk is a
client like any other: a console that asks and watches, and through which
you speak to her, answer and stop her work, as one of the people who run
her (`console.say`, `work.answer`, `work.stop`). Holding the daemon's
token, or being on this machine when it has none, is what makes you her
operator; nobody is listed in its configuration for it. Every session is
a place of its own on the console channel, so the journal keeps the
threads apart; what was said in a session before the desk spoke through
the console, on the `desk` channel it used to register, is in the same
thread. It holds no key.

## Running it

It is its own workspace, so GPUI's tree stays out of `just check`:

```bash
cd desk
cargo run --release                     # or: cargo run --release -- "free some disk space"
```

The window opens on what it showed last time: a copy of each daemon's
journal tail and work is kept under `~/.config/ghost-desk/cache/`, drawn
at once, and replaced by the daemon's own answer within a few seconds
of connecting. A task given on the command line opens a new session
with it. The desk reaches the solver at `ws://127.0.0.1:7438`, or at `GHOSTD_URL` on a first
run; a token comes from `GHOSTD_TOKEN` in the environment or `.env`. Your
sessions are kept in `~/.config/ghost-desk/desk.json`.

To have it as an app, with the ghost in Finder, Launchpad and the Dock:

```bash
desk/bundle.sh                          # -> desk/target/release/Ghost.app
```

The app carries the `ghostd` it was built with: `bundle.sh` builds it in
release and puts it beside the desk in `Ghost.app/Contents/MacOS`, and the
app starts its core with that one. A `cargo run` of the desk has none
beside it, and takes the checkout's `target/release/ghostd`, then
`target/debug/ghostd`, then `ghostd` on PATH. `GHOSTD_BIN` names another.

### Signing it once, for good

The core lives in the checkout, and a checkout under `~/Desktop` (or
`~/Documents`) is in a folder macOS asks about before an app may read it.
It remembers the answer for the app's signature. Signed ad hoc, as
`bundle.sh` does when it has nothing better, every build has a signature
of its own, so every build is asked again. Signed with one certificate,
every build is the same app and is asked once.

A self-signed certificate is enough, made once in Keychain Access:

1. Keychain Access → Certificate Assistant → Create a Certificate….
2. Name: `Ghost Dev`. Identity Type: Self Signed Root. Certificate Type:
   Code Signing. Create, and keep it in the login keychain.
3. `security find-identity -p codesigning` lists it, marked not trusted:
   that is expected, and signing with it works all the same.

From then on `bundle.sh` finds it and signs with it. The first time,
macOS asks whether `codesign` may use its key: Always Allow. Open the app
once, allow the Desktop folder, and builds after it keep that. To sign
with another identity, set `GHOST_SIGN_IDENTITY` to its name or hash;
`GHOST_SIGN_IDENTITY=-` signs ad hoc.

The desk itself reads nothing from the checkout as it opens: its faces
and icons are built into it, and the core is started off the main thread
once the window is up, so that a question from macOS, while it waits,
holds up the core and not the window.

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
goes to her through the console, as you, in your own place with her
(`console:<you>`): a connection holding her daemon's token makes you her
operator. Hover a connection and use `…`, or right-click it, to remove it.

A `ws://` link to another machine carries everything in the clear, so the
desk takes one only with a token named. Better is to reach the daemon over
an ssh tunnel and connect to the tunnel's end on this machine:

```bash
ssh -N -L 7537:127.0.0.1:7437 you@server   # then add ws://127.0.0.1:7537
```

## The thread

Across the top of it, the session's name, which opens the session's menu
(Rename, Copy, Delete), and at the right one switch, for the right
column; with a character open, her name and the place her thread shows.

It is drawn task-first, the way a coding agent's app draws a session:
what you say in a bubble on the right, with when it was under it and a
way to say it again or copy it; what she says as the Markdown she
writes, set in the serif, in a column about as wide as comfortable
prose, her mark in the gutter beside it, and under it when it was, how
long it took and a way to copy it; and above each answer what she did
to get to it. That is one line, "Ran 4 commands, read 2 files · 12s",
folded once she has answered and open while she is still at it, with a
row for each call: "Ran du -sh ~/Library", "Read src/main.rs",
"Searched …", "Opened example.com", or "Used" and the name of a tool of a
server she installed. Each row has how long the call took, and opens to
the head of what came back, with how much more there was when it was cut.
A call that failed is marked, calmly. The desk reads this with
`look.steps`: once for the place on screen, again whenever a look there
may have ended, and about once a second while she thinks there, so the
rows come in as she calls. A daemon from before steps were kept shows
what it looked up or opened as a line that opens to what it found.

A piece of work she took on is a card that says where it stands at a
glance (running, waiting on you, finished, blocked) and opens to what it
did step by step, from `work.steps`, every two seconds while it runs:
what the model said at a step, as a quiet line, and that step's calls as
rows, between what its journal says it reported, asked and concluded. Its
parts, the workers it handed pieces to, are cards inside it, each with
its state, which open to their own steps. A session with nothing in it
says what the solver can do here, as its deployment's `[mind.work]`
allows, and offers a few things to ask for, which only fill in the line.

When she looked and could not think (her brain refused, or is signed out),
her journal says so, and the thread says it where her answer would be, in
a quiet line, with Retry to say your last line again.

Nothing on the desk is made up. Down the left, sessions sit newest
first, and a session shows a dot only while its work runs (amber) or
waits on you (purple), with the parts of open work under it. Hover a
session for its `…`, or right-click it: Rename turns the row into a
field (Return keeps the name, Escape leaves it); Copy puts the thread on
the clipboard as text, who said what and when; Delete takes the session
off the desk's list, and no more: what was said in it stays in Ghost's
journal, which keeps everything. Down the right: Environment (the machine its work runs
on, which Open shows in Finder; the daemon and its link; the brain),
Context (what fills the context window of each brain at work here, the
way a coding agent's context panel shows its own: Role, its brain the
last time it looked at this conversation, as its daemon's record of the
look says; Worker, the model stepping this session's work, the open one
or else the newest, at its newest step. Each says how much of the window
is used, then a thin bar and a row for each part of what was given, by
its share of the characters, with its tokens and its share of the whole
window: for Role the conversation, who it is, the people here, its tools,
what it recalled, its past and the rest; for Worker its messages, tool
results, tools, system prompt and task, then the room above where the
work folds its transcript. Free space comes last. The tokens on each row
are that share of what was read, as the provider counts the whole, not
the parts; a window the daemon does not know is left out), Background
tasks (the work taken on in this session: what is still going, newest
first, each with how long it has run or that it waits on you; then what
has ended, folded into one line, `3 finished · 1 blocked`, that opens),
and Outputs (the files its work reported or kept), which is left out
until there is one. A task that waits on you shows its question there:
its options as buttons when it gave some, otherwise a box to answer in
(and Done, for something only you can do). Every task still going has
Stop, which asks once more.

## Speaking to her

The line you type into is a rounded box the width of the thread: what you
type on top, which wraps and grows to six lines, and a quiet row under it.
A line is a line: nobody picks what kind it is. At a console every line of
yours is an operator's, and her daemon treats it so without being told:
looked at at once, not held to her day or her limits on taking work on.
She tells a question from a thing to do from something to take on as
work, the way a coding agent does: asked the time, she runs `date`;
asked for a thing, she does it; given something bigger than a glance,
she takes it on as work and says so, and it shows in the thread as a
card and down the right under Background tasks, where it can be stopped.

With Ghost, the brain and model it thinks with sit under the line; its
menu names each brain with where it runs, numbers the models of the one
looked at, and takes the arrow keys, a number, Return and Esc. A brain
that cannot think here is said in one short line under the box, with the
whole of what it answered under Details. While she is thinking about
what you said in this place and nothing new is typed, send becomes Stop,
which ends her look there and what it was about to say (`console.stop`),
and the thread says Stopped. Work she took on runs on; every task has
its own Stop.

## Attention

Attention, under Search, lists what every ghost the desk follows is
waiting on you for, on every connection, newest first: work waiting on an
answer, work that ended blocked or given up, what work done for you told
her, and her reports to you. Each daemon is asked for it (`attention.list`,
one short connection per character) every minute, and whenever the list is
opened. An item says who, on which daemon, and what she needs; a question
is answered in place, as in the right column; Open goes to her page, or to
the session the work belongs to; Seen marks it. What you have seen is kept
in `desk.json` by the item's key, and forgotten once its ghost no longer
lists it; the count beside Attention is what you have not seen.

## Brains

The brain chip beside the line, and the solver's row at the foot of the
sidebar, open a small picker: every `[providers]` table of the deployment,
with its mark and its name, and the models of the one looked at, with a
check on the one in use. The models come from the brain itself, none
from a list in the desk: Claude Code's are read out of the build
installed on this machine, once per build, under the names Claude Code
gives them (Opus 4.5, Sonnet 4.5); Codex's are what its own cache lists
for your account; an API's are what its `/models` says, asked for when
the desk starts and again after a day, and kept in
`~/.config/ghost-desk/models.json` so that the picker has them at once,
with or without the network. Picking a model first asks the brain to think
once, as little as there is, the way the daemon would: Claude Code with
`claude -p` and one turn, Codex with `codex exec`, an API with its model
list or a one-token answer, each within a minute. If it cannot (a CLI
signed out answers for its version but not for this), the desk says why
in plain words and keeps the brain it had; otherwise it writes `[brain]`
and restarts the daemon the desk runs.


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
`core/ghostd.toml`, never the key.

## The look

None of the grounds is black: the sidebar sits a shade lighter than the
thread and the right column, the way a coding agent's app sets its panes
apart, and all three are easy on the eyes for a day's use. Three faces: a
sans for the interface, a mono for times and names, a serif for the one
line on an empty page: Inter, JetBrains Mono and Source Serif 4, the
open fonts in `assets/fonts/`, built into the binary (a face added there
is listed in `src/fonts.rs`, and a test says so if it is not). None of
them has Chinese, so each falls back to PingFang SC for it,
rather than to whatever the system's cascade finds first. The thread is
set from a few sizes kept in `src/thread/style.rs`: body text at 14 px on
22.5 px lines, secondary text at 12.5, commands and output in mono at 12,
small print at 11, and space in steps of four.
The app's icon is the ghost of the repository's own mark, on the body
macOS app icons sit on: `assets/app/icon.svg`, rendered to
`assets/app/icon.png`. The brands' marks and where they came from are in
`assets/icons/CREDITS.txt`.

Checks for this crate run inside it: `cargo fmt`, `cargo clippy
--all-targets`, `cargo test`. macOS only, as GPUI is.
