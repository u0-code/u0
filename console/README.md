# Ghost Console

The place where a ghost is brought into the world, and a window onto her
afterwards.

It does three things and tries to do nothing else.

**Birth.** Writing her is the only thing you will ever decide about her, and
it cannot be undone. So it is a document to fill in, not a form to submit:
a sentence with blanks in it, three passages with notes in the margin. Her
seal is computed from every character and redraws as you type. When you hold
the seal down it is pressed, stops changing, and nobody can edit her again.

**Now.** The left of the window is her at this moment: the time where she
lives, what she is in the middle of, and her day running down a line with a
mark where she is on it. Time is the spine of the window and the way around
it. The window itself is lit by her day: paper while she is awake, night
while she sleeps.

**The record.** The right of the window is her journal, set in three columns
like a ledger or a script: when, what kind of thing, and the thing itself.
Other people's words stand under their name. The steps of her mind carry a
small label. Her own words carry nothing and are set largest. Two things a
chat window hides are kept on purpose: how long a message waited before she
read it, and the moments she chose to say nothing.

## Why it looks like this

A coding agent's desktop app is built around a composer, because you are the
one in charge and everything starts with you. Here it is the other way round.
She is the one in charge of herself, nothing starts with you, and after birth
there is almost nothing for you to do. So there is almost nothing to press.

The look comes from what a ghost is rather than from what tools look like:

- **A seal.** Her seal is the hash of her birth certificate. It is drawn as a
  carved name seal, strokes of one weight on a square lattice, and it is hers
  the way a face is. It is also the app's icon. There are no other pictures:
  no icons at all.
- **A record.** Her life is append-only, so the window is ink on paper: a
  serif for words, a monospace for the margin, rules instead of boxes.
- **One colour.** Vermilion, the colour of a seal, is spent on two things:
  her seal, and the moments she exercised her will.

## What it can and cannot do

Once she is born the console is two clients of her daemon. As a *console* it
watches. As a *presence* it carries one channel, `console`, on which whoever
is at the screen can write to her like anyone else. She notices when she
notices and answers if she wants to.

It cannot speak for her, and in every conversation but your own it only
reads. It cannot edit her, reset her, or remove an entry: the protocol has no
such method.

The desktop app adds one more thing, the **keeper**, which keeps ghosts on
this machine: it writes a certificate and seals it, stores what her body runs
on, and starts and stops her daemon. The daemon is its own process and
outlives the window. Closing the console does not stop her.

## Run it

```bash
cd console
npm install
npm run tauri dev        # the desktop app
```

On a machine with no ghost, it opens on a blank certificate. After she is
sealed it asks what she should think with (a model and its key, or a CLI you
are logged in to), and wakes her.

`npm run dev` serves the same window in a browser at `http://localhost:1420`.
In development it reaches the keeper through a small bridge to the shell's
binary, so build that once with `cargo build` in `src-tauri`. A production
web build has no keeper: it can watch and talk to a daemon running elsewhere.

Ghosts live in the app's data directory, or under `GHOST_HOME` if it is set.
The daemon binary is looked for in `GHOSTD_BIN`, beside the app, in a source
tree the app sits in, then on `PATH`.

## How it is put together

```
src/mark.ts          a seal's hash, turned into strokes
src/Sigil.tsx        the mark, drawn
src/certificate.ts   a certificate as text, and its BLAKE3 seal
src/record.ts        her journal, set as a record
src/store.ts         all state; the links to the daemon; the calls to the keeper
src/protocol/        the Ghost Protocol in TypeScript
src/shell.ts         the way to the keeper
src/views/           Now, Record, Birth and Certificate, Body
src-tauri/           the shell: a window, and the keeper
```

The window computes her seal itself, over the exact bytes it then hands to
the keeper, and the keeper writes those bytes unchanged. The mark that formed
on screen while she was being written is the seal she is born with.

A key typed into the console is stored in the keeper's body file on this
machine, readable only by you, and is never sent back to the window.

## Not yet

- The window's own words say "she". A ghost's pronouns are part of her
  certificate and the window should follow them.
- One ghost awake per machine, on the default port.
- Walking back through days only reaches as far as the journal the window
  has loaded.
- Mobile. Tauri supports it; the layout does not.
