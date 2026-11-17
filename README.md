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
  <img alt="Status: pre-release" src="https://img.shields.io/badge/status-pre--release-3b3b3b?style=flat-square">
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
| **Lives in your community** | Private chats and group chats on QQ, Telegram and Discord. She remembers everyone, keeps one memory across platforms and a separate record for each. In a group she speaks when spoken to, and otherwise only when she has something to add. |
| **Solves long problems** | Takes work on and keeps at it for hours in the background, with a plan, notes and a context that folds instead of overflowing. Reports while she works, and can be steered or stopped midway. The design borrows from Claude Code and Codex. |
| **Picks up new abilities** | When something calls for what she cannot do yet, she finds a tool on the MCP Registry or a skill on skills.sh, installs it and uses it from then on: a real browser, a site's API, a maker of pictures or video. |
| **Sees, keeps, sends** | Understands the pictures people send her, keeps the ones she likes with a note, and sends them back when they fit. Memes come from these three, with no meme-specific code. |
| **Her own day, her own will** | Driven by time, not requests: a message lands as unread and she gets to it. She can say no. Whoever runs her talks to her at a console, where she answers at once, and everything she does is on record. |
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
`ghostd.example.toml` to `ghostd.toml` to choose; every setting it can hold
is in [ghostd's Configuration](crates/ghostd/README.md#configuration).

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

## Characters

A character is one folder: `ghost.toml` (name, temperament, voice, story,
values, past) and `assets/` (avatar, bio). How she lives, works and stays
safe is shared by every character, so another character is another
`ghost.toml` and nothing else. One `ghostd` runs as many as you give it,
each with her own journal, memory and accounts, none able to touch another.

## What she thinks with

| Brain | Talking | Working |
|---|---|---|
| OpenAI-compatible (DeepSeek, OpenRouter, Ollama…) | yes | her own loop |
| Anthropic API | yes | her own loop |
| Claude Code CLI | yes | the CLI steps, with her tools and none of its own |
| Codex CLI | yes | the CLI steps, with her tools and its shell off |

`ghostd brains --discover` lists what the machine has. With a CLI, Ghost never
reads its credentials; read the provider's terms before running a ghost other
people talk to on a personal subscription.

## How it fits together

```
 QQ · Telegram · Discord · X · terminal       adapters, one binary each
                    │
                    │  Ghost Protocol, JSON-RPC over WebSocket
                    ▼
                 ghostd ──────── brain        an API or a CLI, swappable
                    │
                  mind                        one per character: attention,
                 ╱      ╲                     speech, her day, her work
          journal        work loop            the web, a machine, what she added
             │
          memory                              derived; rebuilt from the journal
```

| Crate | What it is |
|---|---|
| [`ghost-core`](crates/ghost-core) | Identity, journal, derived memory, clock, tempo, will. No network. |
| [`ghost-brain`](crates/ghost-brain) | Model providers behind one trait. Knows nothing of ghosts. |
| [`ghost-net`](crates/ghost-net) | Where every HTTP client is built, and the one proxy setting they follow. |
| [`ghost-work`](crates/ghost-work) | The loop she does things with: tools, servers and skills she adds, state that outlives a context. |
| [`ghost-mind`](crates/ghost-mind) | The actor that lives: attention, deliberation, speaking, her day, her work. |
| [`ghost-protocol`](crates/ghost-protocol) · [`ghost-link`](crates/ghost-link) | The wire types, and a client for them. |
| [`ghostd`](crates/ghostd) | The daemon: runs her, or several characters, and serves the protocol. [deploy/](deploy) runs it on a server. |
| [`presence/*`](crates/presence) | Platform adapters. The daemon does not know what Telegram is. |
| [`prompts/`](prompts) | Every word a brain is given: `system/`, the same for every character, and `character/`, how her own `ghost.toml` is put into prose. Built in; a deployment overrides any file. |
| [`desk/`](desk) | A desktop app for tasks on this machine: Ghost, the project's mascot, living on this machine and run by the desk's own core ([`desk/core/`](desk/core)) on the same runtime with its own prompts. macOS, its own workspace. |

## Security

Anything a page, a file, a tool or a skill brings back reaches her marked as
outside words, and only the people she talks with and whoever runs her
count as asks. What she installs runs with none of her keys, and
everything she does is in her journal. Give a ghost other people talk to its
own account or a container, not your laptop. See [SECURITY.md](SECURITY.md),
and report a vulnerability there rather than in an issue.

> Ghost is pre-release; this page describes the first release. Each
> adapter's README says how far its platform has been tested.

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
