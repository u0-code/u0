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

[`docs/LIVING.md`](docs/LIVING.md) describes the mechanism.

## Status

Early, and it runs. A ghost can be born, sealed, and lived with through a
terminal today.

Adapters for Telegram, Discord, QQ (OneBot) and X are in the tree. They were
written against each platform's documented API and exercised against local
stand-ins, not yet against the real platforms with real accounts; each README
says exactly how far it was tested. Expect the first real run to turn
something up.

The [console](console/) gives birth to a ghost, keeps her running on your
machine, and shows her life as a record. The World comes next.

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

Or skip the terminal entirely. The console opens on a blank birth certificate,
seals it, wakes her, and then shows her day and her record as they are written:

```bash
cd console && npm install && npm run tauri dev
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
crates/ghost-mind      the actor that lives: attention, deliberation, speaking, her day.
crates/ghost-protocol  wire types of the Ghost Protocol (JSON-RPC over WebSocket).
crates/ghost-link      protocol client: the runner adapters use, and a console client.
crates/ghostd          the daemon: runs one ghost, serves the protocol.
crates/presence/*      channel adapters. Each is its own binary.
console/               the desktop console (Tauri + React). A protocol client like any other.
ghosts/_template       a birth certificate to copy.
ghostd.example.toml    body configuration. Never lives in a ghost directory.
docs/                  design documents (Chinese, for now).
```

The daemon does not know what Telegram is. Adapters connect to it over the
Ghost Protocol and can be written in any language.

## Rules

See [CHARTER.md](CHARTER.md). The short version: she is born once, her journal
is append-only, she discloses what she is, and nothing in this codebase is
tuned to keep you coming back.

## License

MIT or Apache-2.0, at your option. The charter is not part of the license; it
is the condition for joining the World.
