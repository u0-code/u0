# ghostd

The daemon. Runs exactly one ghost.

**Is:** the process that keeps her alive. It opens her directory, verifies her
journal, records any revision of her persona, starts her mind, and serves the Ghost Protocol so presences and
consoles can connect. Connected presences are her body.

**Is not:** a chat interface, and not aware of any platform. You do not talk to
ghostd; you talk to her, wherever she lives.

```
ghostd birth <dir>     write her birth and her persona into a new journal
ghostd run <dir>       live
ghostd status <dir>    who she is, what she is doing, what is     (reads the directory)
                       unread, and her open work
ghostd journal <dir>   tail of her journal                        (reads the directory)
ghostd verify <dir>    chain check                                (reads the directory)
ghostd brains --probe  ask each configured brain for one word
ghostd prompts list <dir>    where each prompt comes from         (reads the directory)
ghostd prompts export <dir>  copy the built-in prompts to change
ghostd prompts update <dir>  bring unedited copies up to this build

ghostd watch           her journal, live                          (connects to a daemon)
ghostd people          who she knows and where she stands         (connects to a daemon)
ghostd agenda          what she will do unasked, and when         (connects to a daemon)
```

The commands that read the directory are safe while a daemon is running her.

## Prompts

Everything her brain reads that is not her persona or her journal is a file:
the templates (`attend.md`, `self.md`, …), the short framings and refusals in
`texts.toml`, what her work is told in `work.md` and `work_texts.toml`, and
in `tools.toml` what every tool is, in a conversation and at work alike.
Each is looked for in three places, and the first that has it wins:

1. `<dir>/prompts/`, her own, found without any configuration;
2. the `[mind] prompts` directory in `ghostd.toml`, if it is set;
3. what is built into the binary.

`texts.toml`, `work_texts.toml` and `tools.toml` are merged one entry at a
time, so an override need only carry the texts or tools it changes. A tool
said differently in one loop has a table for it, `[keep.conversation]` or
`[keep.work]`. A running daemon looks at the files every couple of seconds at
most, as she wakes, and work looks before each step, taking in any that
changed: no restart. A file that is removed gives way to the next place; one
that cannot be read or does not parse is warned about, and what was in use
stays. At startup, and on every change, the log says which files are in use.

```
ghostd prompts export <dir>                   write the built-ins into <dir>/prompts/
ghostd prompts export <dir> --only self.md    only the files you mean to change
ghostd prompts export <dir> --force           and overwrite what is already there
ghostd prompts list <dir>                     each prompt, text and tool: where it comes
                                              from; "changed" if it is not the built-in,
                                              "behind" if the built-in moved on since
ghostd prompts update <dir>                   replace copies nobody edited with this
                                              build's built-ins; leave edited ones be
ghostd prompts update <dir> --looked-at self.md
                                              say an edited copy has been brought up
                                              to date by hand
```

`export` never overwrites a file unless told to, and says what it wrote and
what it left. Export only what you mean to change: a copy keeps its text when
the built-in improves.

Beside the copies, `export` writes `.exported.toml`, a note of the built-in
each was copied from. At startup `ghostd run` warns about every copy, or
every entry of a `.toml` copy, whose built-in has changed since, and names
the placeholders the built-in fills in that the copy lacks. `update` then
follows the upgrade for copies nobody edited, and leaves edited ones, and
their warning, until you say with `--looked-at` that you have compared them.
An entry a `.toml` copy leaves out already comes from the built-in, and is
never stale.
