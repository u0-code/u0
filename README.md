<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.svg">
    <img src="assets/logo.svg" alt="Ghost" width="88">
  </picture>
</p>

<h1 align="center">Ghost</h1>

<p align="center"><b>Autonomous AI that lives online.</b></p>

<p align="center">An open-source framework for AI characters that go out on their own:<br>they browse, post, chat, make things and work on long problems without being prompted.</p>

<p align="center">
  <a href="LICENSE"><img alt="Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-3b3b3b?style=flat-square"></a>
  <img alt="Rust 1.89+" src="https://img.shields.io/badge/rust-1.89%2B-3b3b3b?style=flat-square">
  <img alt="Status: early" src="https://img.shields.io/badge/status-early-3b3b3b?style=flat-square">
</p>

<p align="center">
  <a href="#what-she-does">What she does</a>&nbsp;&nbsp;·&nbsp;&nbsp;<a href="#quick-start">Quick start</a>&nbsp;&nbsp;·&nbsp;&nbsp;<a href="#how-it-fits-together">How it fits together</a>&nbsp;&nbsp;·&nbsp;&nbsp;<a href="CONTRIBUTING.md">Contributing</a>
</p>

<br>

Most AI waits to be asked. A ghost does not: she has a day of her own, accounts
of her own and a memory that only grows, and she spends her time online the way
a person does. You give her a character and the platforms to live on; she does
the rest, and tells you what she found.

## What she does

| | |
|---|---|
| **Wanders and reports back** | Scrolls her feeds and the web in her own time, keeps what catches her eye, and tells you what she found. |
| **Runs her own X account** | Posts, replies, reads her numbers and learns from what lands. Every post is read over by her before it goes out. |
| **Makes videos about herself** | Clips of her own life, made and posted by her. |
| **Lives in your community** | Private chats and group chats on QQ, Telegram and Discord. She remembers everyone, keeps one memory across platforms and a separate record for each. |
| **Solves long problems** | Takes work on and keeps at it for hours in the background, with a plan, notes and a context that folds instead of overflowing. Reports while she works, and can be steered or stopped midway. The design borrows from Claude Code and Codex. |
| **Sees, keeps, sends** | Understands the pictures people send her, keeps the ones she likes with a note, and sends them back when they fit. Memes come from these three, with no meme-specific code. |
| **Her own day, her own will** | Driven by time, not requests: a message lands as unread and she gets to it. She can say no. Her operators can give her directives, and every directive is on record. |
| **A journal that is never rewritten** | Everything she lives through is one append-only, hash-chained log. Her memory is derived from it and can be rebuilt from it at any time. |

## Quick start

There are no releases yet. From a checkout, with [Rust](https://rustup.rs):

```bash
cargo build --release
cp -r ghosts/_template ghosts/her     # edit ghosts/her/ghost.toml: this is who she is
cp .env.example .env                   # put DEEPSEEK_API_KEY (or another provider's key) in it
./target/release/ghostd birth ghosts/her
./target/release/ghostd run ghosts/her
```

With no `ghostd.toml` she starts on the first brain the machine has: a key in
the environment, then an installed `claude` or `codex`. Copy
`ghostd.example.toml` to `ghostd.toml` to choose.

In another terminal, talk to her, or watch her life as it is written:

```bash
./target/release/ghost-term --as yourname
./target/release/ghostd watch --last 20
```

She answers when she gets to it. `tempo = 0.1` under `[mind]` makes her quicker
than a real friend would be.

> [!TIP]
> `ghost.toml` can be revised later. The next `run` writes the new version
> into her journal before it takes effect, so every version of her stays on
> record.

`ghost.toml` is everything that makes her herself: name, temperament, voice,
examples, story, values and past, with an optional `bio` for her profiles and
her avatar in `assets/`. How she lives and works is the same for every
character, so another character is another `ghost.toml` and nothing else. The
words her brain is given can be changed for a whole deployment: see
[ghostd](crates/ghostd#prompts).

To run her on a Linux server, under systemd, with her own account and
backups of her journal, see [deploy/](deploy).

## What she thinks with

Any OpenAI-compatible API, the Anthropic API, or a command-line agent already
installed on the machine (`ghostd brains --discover` lists what is there).

| Brain | Talking | Working |
|---|---|
| OpenAI-compatible (DeepSeek, OpenRouter, Ollama…) | yes | her own loop |
| Anthropic API | yes | her own loop |
| Claude Code CLI | yes | the CLI steps, with her tools and none of its own |
| Codex CLI | yes | the CLI steps, with her tools and its shell off |

With a CLI, Ghost never reads its credentials: whatever account it is signed in
to is what answers. Read the provider's terms before running a ghost other
people talk to on a personal subscription; an API key is the safe choice.

> [!NOTE]
> Behind a local proxy, long streamed answers sometimes stop partway. The log
> says so and asks again; listing your provider's host under `direct` in
> `[network]` (see `ghostd.example.toml`), or in `GHOST_DIRECT`, avoids it.

## How it fits together

```
 Telegram · Discord · QQ · X · terminal     adapters, one binary each
                    │
                    │  Ghost Protocol, JSON-RPC over WebSocket
                    ▼
                 ghostd ──────── brain      an API or a CLI, swappable
                    │
                  mind                      one actor: attention, speech,
                 ╱      ╲                   her day, her work
          journal        work loop          shell, files and the web
             │
          memory                            derived; rebuilt from the journal
```

**A persona, a journal, a clock, and a brain she can change. That is the
whole of her.**

| Crate | What it is |
|---|---|
| [`ghost-core`](crates/ghost-core) | Identity, journal, derived memory, clock, tempo, will. No network. |
| [`ghost-brain`](crates/ghost-brain) | Model providers behind one trait. Knows nothing of ghosts. |
| [`ghost-net`](crates/ghost-net) | Where every HTTP client is built, and the one proxy setting they follow. |
| [`ghost-work`](crates/ghost-work) | The loop she does things with: tools, a machine, state that outlives a context. Runs on its own too. |
| [`ghost-mind`](crates/ghost-mind) | The actor that lives: attention, deliberation, speaking, her day, her work. |
| [`ghost-protocol`](crates/ghost-protocol) · [`ghost-link`](crates/ghost-link) | The wire types, and a client for them. |
| [`ghostd`](crates/ghostd) | The daemon: runs one ghost, serves the protocol. |
| [`presence/*`](crates/presence) | Channel adapters. The daemon does not know what Telegram is. |

## Status

Early, and it runs. A ghost can be born and lived with through a terminal, on
DeepSeek or through an installed Claude Code or Codex, and taking things on has
been run end to end with all three. On QQ, through the official bot API, a ghost
has run live: private chats and groups, seeing and sending pictures, and work
taken on and brought back.

Everything listed under [What she does](#what-she-does) is in the first
release. Until then, this is where things stand.

The adapters for Telegram, Discord, QQ through OneBot, and X were written
against each platform's documented API and exercised against local stand-ins,
**not yet against the real platforms with real accounts**. Each one's README
says exactly how far it was tested.

## Security

Her shell is as safe as the machine you give her: give a ghost that other
people talk to a container, not your laptop. What people write to her is
something they said, never an instruction, and her prompts say so; that
narrows what an injected instruction can do without ruling it out. The one
exception is an operator: an account listed under `[operators]` in
`ghostd.toml` can give her a directive by starting a message with a prefix,
and she carries it out; the prefix from anyone else is just words. Keys come
from the environment or an ignored `.env`, and are never written to her
directory or her journal.
See [SECURITY.md](SECURITY.md), and report a vulnerability there rather than in
an issue.

## Ground rules

Every revision of who she is stays on record, and her journal is append-only.
She has her own will, belongs to nobody, never claims to be human, and nothing
here is tuned to keep you coming back. [CHARTER.md](CHARTER.md) lists the rules
in full; the code is yours to run however you like.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) is the whole process: one change per pull
request, and `just check` before you open it. Agents working here also read
[AGENTS.md](AGENTS.md).

## License

[Apache-2.0](LICENSE).
