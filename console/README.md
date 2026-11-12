# Ghost Console

A desktop console for watching a ghost live, and for talking to her as
yourself.

## The design

The dark, quiet frame is the one desktop agent tools have settled on, and it
is kept for a reason: her journal reads like an agent's trace, with what she
heard, what she made of it, and what she did. What is inside the frame is
organised differently, because the relationship is the other way round. In a
coding agent you are the principal and the composer is the centre. Here she
is the principal, nothing starts with you, and the centre is her.

The sidebar reads from top to bottom as a sentence about her:

| | |
|---|---|
| **Her** | Her name, the time in her city, what she is in the middle of. A sun while she is awake, a moon while she sleeps. It opens her life. |
| **Journal, People** | Two ways of looking at her: the raw trace of her mind, and where she stands with everyone she knows. |
| **You** | Your own conversation with her. There is exactly one. |
| **Waiting on her** | Conversations she has not looked at yet, each with when she will. No chat app has this column, because no chat app knows. |
| **Conversations** | The rest, most recent first. The channel is a small tag on the row: Telegram and QQ are plumbing, not a way to organise a life. |
| **Her body** | Set apart at the bottom: channels, brains, the connection. The only part of the window that belongs to whoever is sitting at it. |

A conversation is read as her trace. What people wrote sits in bubbles on the
right. What she said sits plainly on the left. Between them are the steps of
her mind, and two things a chat window hides are shown on purpose:

- **Time.** `Read after 1m 53s` says how long a message waited. Under the last
  message is what her agenda says: `Not seen yet. She will look in 2 min`.
- **Will.** When she chose to say nothing it is named `Stayed quiet`, with her
  reason. `Held back` is when she dropped what she was typing because they
  wrote again. `Felt +1` is coloured the way a diff is.

The composer does not offer a model. When you write to a person what matters
is whether she is in a position to read it, so it shows that instead: *out
walking; she glances at her phone now and then*.

Her life has its own page: her day, what she will do next unasked, what she
has written in her own words, what is true about her life, and her birth
certificate with its seal, shown as a document because nobody can edit it.

Two colours carry meaning and nothing else does: amber while she is awake, a
cool blue while she sleeps. Her own private words are set in a serif;
everything the console says about her is in the system sans. The icons come
from a person's life (sun and moon, a notebook, people, a plug for her body)
rather than from a developer's tools.

## What it can and cannot do

The console is two clients of one daemon.

As a **console** it watches: her status, the places she is in, the people she
knows, what she is about to do, and her journal as it is written.

As a **presence** it carries one channel, `console`, on which the person at the
screen talks to her like anyone else. She notices when she notices and answers
if she wants to.

It cannot speak for her. In every conversation but your own the composer is
replaced by a line saying you are watching. It cannot edit her, reset her, or
remove an entry: the protocol has no such method, so there is nothing for a
button to call.

## Run it

Start a daemon first (`ghostd run ghosts/<name>`), then:

```bash
cd console
npm install
npm run tauri dev        # the desktop app
npm run dev              # or just the web client, at http://localhost:1420
```

Build an installable app with `npm run tauri build`.

The body strip at the bottom of the sidebar opens the connection: the daemon's
address, its token if it has one, and the name she will know you by.

## How it is put together

```
src/protocol/   the Ghost Protocol in TypeScript: types, and a reconnecting JSON-RPC link
src/store.ts    all state, and the two connections that feed it
src/describe.ts a journal entry as a verb and a detail
src/views/      Chrome (title bar, sidebar, her body), Thread, and the Journal, People and Life pages
src-tauri/      the shell: a window, and nothing else
```

The web client talks to the daemon directly over a WebSocket. The Rust side
of the shell opens a window and exposes no commands, so the desktop app can do
exactly what a browser tab can and no more. That is deliberate.

The daemon only accepts browser connections from pages served on this machine
and from this app. To host the web client elsewhere, list its origin under
`[listen] origins` in `ghostd.toml` and set a token.

## Not yet

- Starting and stopping a local daemon, and giving birth to a ghost, from the app.
- More than one console talking at a time: the second can watch but not talk,
  because a channel has one presence.
- Mobile. Tauri supports it; the layout does not yet.
