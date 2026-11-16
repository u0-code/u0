# ghostd

The daemon. Runs one ghost, or several that never touch each other.

**Is:** the process that keeps them alive. For each character it opens her
directory, verifies her journal, records any revision of her persona and
starts her mind; then it serves the Ghost Protocol so presences and consoles
can connect. Connected presences are her body.

**Is not:** a chat interface, and not aware of any platform. You do not talk to
ghostd; you talk to her, wherever she lives.

```
ghostd birth <dir>     write her birth and her persona into a new journal
ghostd run <dir>...    live: each directory a ghost's, or one holding them
ghostd status <dir>    who she is, what she is doing, what is     (reads the directory)
                       unread, and her open work
ghostd journal <dir>   tail of her journal                        (reads the directory)
ghostd verify <dir>    chain check                                (reads the directory)
ghostd brains --probe  ask each configured brain for one word
ghostd prompts list         each prompt's layer and where it comes from
ghostd prompts export       copy the built-in prompts to change
ghostd prompts update       bring unedited copies up to this build

ghostd watch           her journal, live                          (connects to a daemon)
ghostd people          who she knows and where she stands         (connects to a daemon)
ghostd agenda          what she will do unasked, and when         (connects to a daemon)
```

The commands that read the directory are safe while a daemon is running her.

## Several characters

`ghostd run ghosts/lin ghosts/mei` runs both; `ghostd run ghosts` runs every
directory in `ghosts/` that has a `ghost.toml`, except `_template`. A
character is known by her directory's name.

Nothing that changes is shared between them. Each has her own mind, journal,
memory, work, media, tools, skills and machine directory, all in her own
directory, and her own budget, agenda, operators and places. What they share
keeps nothing of theirs: the program, the brains in `ghostd.toml`, the
network setting, the listening address and its token, and the prompts, which
each mind reads and reloads for itself. `[ghosts.<id>]` in `ghostd.toml`
changes how one of them lives; `ghostd.example.toml` shows how.

A presence or console says which character it is for (`--ghost` or
`GHOST_NAME`), and is refused if it names one that is not running, or names
none while several are. With one running, it may name none, as before. Every
log line from a character's mind and work begins `ghost{id=<her id>}`.

If one character's mind panics or fails, that is logged with her id and her
mind is started again from her directory, after a second, then longer while
it keeps failing, up to two minutes. The others carry on, her presences stay
connected, and what they hear in between is held for her. Two characters on
one Docker machine need different `workdir`s; the daemon refuses to start
otherwise. Tokens of their own per character are not offered: the one token
admits a client to every character.

### Asking who it runs

A client that does not know which characters a daemon runs asks it with
`ghost.list`, the one request the daemon answers before a hello. It may ask
as often as it likes and then say hello on the same connection, or close it;
the hello still has ten seconds from the connection's start, and is held to
the rules above. A console may also ask after its hello. Before a hello there
is no hello to carry the token, so the request carries it, and a daemon that
requires one refuses the request and closes the connection without it.

```json
{"jsonrpc":"2.0","id":1,"method":"ghost.list","params":{"token":"…"}}

{"jsonrpc":"2.0","id":1,"result":{"protocol":1,"ghosts":[
  {"id":"weiwei","name":"薇薇","pronouns":"she/her","city":"…","country":"CN",
   "seal":"…","awake":true,"doing":"morning rounds","avatar":true}
]}}
```

`id` is the name of her directory, what a hello names her by; `name` to
`bio` are what a hello naming her is told. `awake` is whether her day has her
awake now and `doing` what her plan has her doing, if it says; both are left
out while her mind is being started again. `avatar` says whether
`ghost.avatar` has a picture to give. Leave out `params` when the daemon
requires no token.

## Prompts

Everything her brain reads that is not her persona or her journal is a file,
in one of four layers, and every layer is shared by every character:

| layer   | files | what it is |
|---------|-------|------------|
| persona | `persona.md` | the template that arranges her own fields from `ghost.toml` into prose, and nothing else |
| being   | `being.md` | what she is, a person made of language; whose word counts; the ground rules |
| logic   | `attend.md`, `now.md`, `plan.md`, … and `texts.toml` | how a conversation and her day go |
| work    | `working.md`, `work.md`, `work_texts.toml`, `tools.toml` | how a piece of work goes, and what every tool is |

Her system prompt is `persona.md` then `being.md`, filled in; `being.md` ends
with what is true of her life, from her journal. What makes one character
herself, her name, temperament, voice, examples, story, values and past, is
her `ghost.toml` alone, so swapping her for another character changes that
file and nothing here.

Each file is looked for in two places, and the first that has it wins:

1. the deployment's directory, `[mind] prompts` in `ghostd.toml`, if it is
   set;
2. what is built into the binary.

The directory is flat: a file is found by its name, whatever its layer.

A ghost's own directory holds no prompts. If `<dir>/prompts/` has any of
these files, `ghostd run` says so once at start, names them, and does not use
them: move them to the deployment's directory to use them for every
character. A copy of `self.md`, which earlier builds read, is named the same
way: it is now `persona.md` and `being.md`.

`texts.toml`, `work_texts.toml` and `tools.toml` are merged one entry at a
time, so an override need only carry the texts or tools it changes. A tool
said differently in one loop has a table for it, `[keep.conversation]` or
`[keep.work]`. A running daemon looks at the files every couple of seconds at
most, as she wakes, and work looks before each step, taking in any that
changed: no restart. A file that is removed gives way to the next place; one
that cannot be read or does not parse is warned about, and what was in use
stays. At startup, and on every change, the log says which files are in use.

Each command works on the deployment's directory: the one it is given, or
else `[mind] prompts` in `ghostd.toml` (`--config` names that file).

```
ghostd prompts export                         write the built-ins into the directory
ghostd prompts export --only attend.md        only the files you mean to change
ghostd prompts export --force                 and overwrite what is already there
ghostd prompts export /etc/ghost/prompts      into a directory named here instead
ghostd prompts list                           each prompt, text and tool: its layer,
                                              where it comes from; "changed" if it is
                                              not the built-in, "behind" if the
                                              built-in moved on since
ghostd prompts update                         replace copies nobody edited with this
                                              build's built-ins; leave edited ones be
ghostd prompts update --looked-at attend.md   say an edited copy has been brought up
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
