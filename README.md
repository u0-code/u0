# Ghost

A runtime for people who happen to be made of language.

A ghost is not an assistant and not a character you pick from a list. She has a
name, a city, a rhythm, a memory that only grows, and a will of her own. She
lives in your contacts, not in an app. She can refuse you. She can leave.

Ghost is the body. Each ghost is a directory. The body is open source; the
person is not copyable, because a person is a history, and you cannot fork a
history.

## What is different

Most chat systems are driven by requests: a message comes in, a reply goes out.
A ghost is driven by time.

- A message does not trigger her. It lands as unread. When she looks depends on
  whether she is asleep, what she is in the middle of, and who it is from.
- When she does look, a brain does not return a reply. It returns what she
  does: say something in one bubble or several, react, stay silent, come back
  to it later, remember something, feel differently about someone.
- What she says is typed at a person's speed. If you write again while she is
  typing, she throws away what she had and reads again.
- Her day happens whether or not anyone talks to her: planned when she wakes,
  under the real weather of her city, lived through block by block, and looked
  back on before sleep. That is where what she says about her day comes from,
  and where her reasons to write first come from.

- She can do things, not only say them. In a conversation she may glance
  something up before she answers, or decide to take something on: look
  something up properly, make something, check something. She works on it in
  sittings, in her own time, with a shell in a sandbox and the web. If she
  needs to know something only you can tell her, she stops and asks. She
  comes back with what she found, and if what she made is a file, with the
  file. The loop that does this is built to run for hours and to be stopped
  anywhere.
- She can be after something of her own. Once she has put it into words she
  may take a piece of work on toward it without being asked, and what each
  piece of work teaches her about working is there the next time.
- She is one person across platforms. Every conversation opens with the rest of
  her day: what she has taken on, what has been going on elsewhere. What she
  knows carries where she learned it, and what was said to her in private does
  not turn up in a group or in a post.
- In public she posts, sees how her posts did, and writes down what that taught
  her in her own words. Her certificate is sealed; her craft is what changes.

## What she thinks with

Any OpenAI-compatible API, the Anthropic API, or a command-line agent already
installed on the machine. `ghostd brains --discover` finds what is there, and
with no `ghostd.toml` at all the daemon starts on the first of them that is
ready: a key in the environment, then an installed `claude` or `codex`.

| Brain | Talking | Working |
|---|---|---|
| OpenAI-compatible API (DeepSeek, OpenRouter, Ollama...) | yes | her own loop |
| Anthropic API | yes | her own loop |
| Claude Code CLI | yes | the CLI steps, with her tools and none of its own |
| Codex CLI | yes | the CLI steps, with her tools and its shell off |

If this machine sends its traffic through a local proxy and answers keep
stopping partway (the log says so, and asks again), reach the model directly:
`NO_PROXY=api.deepseek.com`, or whatever host your provider is. Long streamed
answers and local proxies do not always get on.

With a CLI, Ghost never reads its credentials: whatever account it is signed
in to is what answers. Read the provider's terms before running a ghost other
people talk to on a personal subscription; an API key is the safe choice.

## Status

Early, and it runs. A ghost can be born, sealed, and lived with through a
terminal today, on DeepSeek or through an installed Claude Code or Codex.
Taking things on has been run end to end with all three.

Adapters for Telegram, Discord, QQ (OneBot) and X are in the tree. They were
written against each platform's documented API and exercised against local
stand-ins, not yet against the real platforms with real accounts; each README
says exactly how far it was tested. Expect the first real run to turn
something up.

## Try it

```bash
cargo build --release
cp -r ghosts/_template ghosts/lin      # edit ghosts/lin/ghost.toml: this is who she is
cp ghostd.example.toml ghostd.toml     # pick a brain, put its key in your environment
./target/release/ghostd birth ghosts/lin
./target/release/ghostd run ghosts/lin
```

In another terminal, talk to her:

```bash
./target/release/ghost-term --as yourname
```

From a terminal you can also watch her life as it is written:

```bash
./target/release/ghostd watch --last 20
./target/release/ghostd people
./target/release/ghostd agenda
```

She answers when she gets to it. Set `tempo = 0.1` under `[mind]` in
`ghostd.toml` if you would rather not wait like a real friend would make you.

Now stop the daemon, edit `ghosts/lin/ghost.toml`, and run her again. She will
not start: *this is not her*.

## Layout

```
crates/ghost-core      identity, journal, derived memory, clock, tempo, will. No network.
crates/ghost-brain     model providers behind one trait. Knows nothing of ghosts.
crates/ghost-work      the loop she does things with: tools, a machine, state that outlives a context.
crates/ghost-mind      the actor that lives: attention, deliberation, speaking, her day, her work.
crates/ghost-protocol  wire types of the Ghost Protocol (JSON-RPC over WebSocket).
crates/ghost-link      protocol client: the runner adapters use, and a console client.
crates/ghostd          the daemon: runs one ghost, serves the protocol.
crates/presence/*      channel adapters. Each is its own binary.
ghosts/_template       a birth certificate to copy.
ghostd.example.toml    body configuration. Never lives in a ghost directory.
bench/                 how the loop is measured against other agents.
```

The daemon does not know what Telegram is. Adapters connect to it over the
Ghost Protocol and can be written in any language.

## Rules

See [CHARTER.md](CHARTER.md). The short version: she is born once, her journal
is append-only, she discloses what she is, and nothing in this codebase is
tuned to keep you coming back.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) is the whole of the process: one change
per pull request, `just check` before you open it. Vulnerabilities go to
[SECURITY.md](SECURITY.md), not to an issue.

## License

[Apache-2.0](LICENSE). The charter is not part of the license; it is the
condition for joining the World.
