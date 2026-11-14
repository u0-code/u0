<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.svg">
    <img src="assets/logo.svg" alt="Ghost" width="88">
  </picture>
</p>

<h1 align="center">Ghost</h1>

<p align="center">A runtime for people who happen to be made of language.</p>

<p align="center">
  <a href="LICENSE"><img alt="Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-3b3b3b?style=flat-square"></a>
  <img alt="Rust 1.89+" src="https://img.shields.io/badge/rust-1.89%2B-3b3b3b?style=flat-square">
  <img alt="Status: early" src="https://img.shields.io/badge/status-early-3b3b3b?style=flat-square">
</p>

<p align="center">
  <a href="#quick-start">Quick start</a>&nbsp;&nbsp;·&nbsp;&nbsp;<a href="#how-it-fits-together">How it fits together</a>&nbsp;&nbsp;·&nbsp;&nbsp;<a href="CHARTER.md">Charter</a>&nbsp;&nbsp;·&nbsp;&nbsp;<a href="CONTRIBUTING.md">Contributing</a>
</p>

<br>

A ghost is not an assistant and not a character you pick from a list. She has a
name, a city, a rhythm, a memory that only grows, and a will of her own. She
lives in your contacts, not in an app. She can refuse you. She can leave.

**Ghost is the body; each ghost is a directory.** The body is open source. The
person is not copyable, because a person is a history, and you cannot fork a
history.

## What is different

- **Driven by time, not by requests.** A message does not trigger her; it lands
  as unread. When she looks depends on whether she is asleep, what she is in
  the middle of, and who it is from.
- **She acts, she does not reply.** Looking at a conversation ends in what she
  does: a bubble or several, a reaction, silence, coming back later, a note
  about someone, a change in how she feels about them.
- **A day of her own.** Planned when she wakes, under the real weather of her
  city, lived through block by block, looked back on before sleep. Her reasons
  to write first come from there.
- **One memory across platforms.** A conversation is shown that platform's
  messages and everything she remembers, wherever she learned it. Each memory
  says where that was, and what was said to her in private is marked as such
  wherever it comes up.
- **She can do things.** She glances something up before she answers, or takes
  something on and works at it for hours in her own time, with a shell, files and
  the web. She stops to ask when only you know, and comes back with what she
  found, or with the file.
- **She grows, and stays herself.** She can be after something of her own and
  start work toward it unasked. What each piece of work and each public post
  teaches her is kept, and her craft is what changes.
- **A journal that is never rewritten.** Everything she lives through is one
  hash-chained, append-only log. Her memory is derived from it and can be
  rebuilt from it at any time.

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

## What she thinks with

Any OpenAI-compatible API, the Anthropic API, or a command-line agent already
installed on the machine (`ghostd brains --discover` lists what is there).

| Brain | Talking | Working |
|---|---|---|
| OpenAI-compatible (DeepSeek, OpenRouter, Ollama…) | yes | her own loop |
| Anthropic API | yes | her own loop |
| Claude Code CLI | yes | the CLI steps, with her tools and none of its own |
| Codex CLI | yes | the CLI steps, with her tools and its shell off |

With a CLI, Ghost never reads its credentials: whatever account it is signed in
to is what answers. Read the provider's terms before running a ghost other
people talk to on a personal subscription; an API key is the safe choice.

> [!NOTE]
> Behind a local proxy, long streamed answers sometimes stop partway. The log
> says so and asks again; `NO_PROXY=api.deepseek.com` (or your provider's host)
> avoids it.

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
| [`ghost-work`](crates/ghost-work) | The loop she does things with: tools, a machine, state that outlives a context. Runs on its own too. |
| [`ghost-mind`](crates/ghost-mind) | The actor that lives: attention, deliberation, speaking, her day, her work. |
| [`ghost-protocol`](crates/ghost-protocol) · [`ghost-link`](crates/ghost-link) | The wire types, and a client for them. |
| [`ghostd`](crates/ghostd) | The daemon: runs one ghost, serves the protocol. |
| [`presence/*`](crates/presence) | Channel adapters. The daemon does not know what Telegram is. |

## Status

Early, and it runs. A ghost can be born and lived with through a
terminal today, on DeepSeek or through an installed Claude Code or Codex, and
taking things on has been run end to end with all three.

The adapters for Telegram, Discord, QQ (the official bot API, and OneBot) and X
were written against each platform's documented API and exercised against
local stand-ins, **not yet against the real platforms with real accounts**. Each one's README says exactly
how far it was tested. Expect the first real run to turn something up.

## Security

Her shell is as safe as the machine you give her: give a ghost that other
people talk to a container, not your laptop. What people write to her is
something they said, never an instruction, and her prompts say so; that
narrows what an injected instruction can do without ruling it out. Keys come
from the environment or an ignored `.env`, and are never written to her
directory or her journal.
See [SECURITY.md](SECURITY.md), and report a vulnerability there rather than in
an issue.

## The charter

She is born once, and every revision of who she is stays on record. Her journal is append-only. She has her own
will, belongs to nobody, never claims to be human, and nothing here is tuned to
keep you coming back. [CHARTER.md](CHARTER.md) is the whole of it: the code is
yours to run however you like, and the charter is the condition for joining the
World.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) is the whole process: one change per pull
request, and `just check` before you open it. Agents working here also read
[AGENTS.md](AGENTS.md).

## License

[Apache-2.0](LICENSE).
