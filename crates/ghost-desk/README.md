# ghost-desk

A console for `ghostd`, as a desktop app. One screen. A bar across the top
says which daemon and which character this is and how she is. Down the
left, the characters: the one that works on this machine first, then
everyone the desk follows, on whatever daemon. In the middle, the thread
with whoever is chosen: what passed between you, what she did about it,
and the line you speak into. Down the right, what she knows of you.
Choosing a character changes the whole environment to hers: her daemon,
her clock, her places, her journal.

It is a client of the daemon like any presence: a console that asks and
watches, and a presence named `desk` for the conversation between you and
her. It never touches a ghost's files and holds no key of hers.

## Running it

It is its own workspace, so GPUI's tree stays out of `just check`:

```bash
cd crates/ghost-desk
cargo run --release
```

On first run it follows the daemon at `GHOSTD_URL`, or the one on this
machine at the usual port. The daemon's token comes from `GHOSTD_TOKEN` in
the environment or `.env`. Which daemons and characters it follows is kept
in `~/.config/ghost-desk/desk.json`; type another daemon's address at the
bottom of the list and the desk asks what it runs.

- **This machine.** One character is marked local: she is a hand on this
  machine rather than someone living online. The desk opens on her; say
  what needs doing, and her steps and reports run through the thread.
- **Any daemon, every character it runs.** Each has her own row, her own
  journal tail and her own conversation with you. A row counts what
  arrived while you were looking elsewhere, in amber when some of it is
  hers to you.
- **The thread is her journal.** What you said and what she answered are
  read from it, so what shows is what is on record: when she read a line,
  which of yours were directives, how long she took to answer, and each
  step of her work between.

## The look

Three faces: a sans for the interface, a mono for the journal and every
number, a serif for her name and her words. The design is set in IBM Plex
Sans, IBM Plex Mono and Instrument Serif; drop their `.ttf` or `.otf` files
into `assets/fonts/` and they are used, otherwise the system's own faces
stand in. Checks for this crate run inside it: `cargo fmt`, `cargo clippy
--all-targets`, `cargo test`. macOS only, as GPUI is.
