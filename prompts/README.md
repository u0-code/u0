# prompts

Every word a brain is given that is not a character's own or her journal.
The binaries are built with these files in them, so a lone binary runs; a
deployment changes them without a rebuild.

```
system/                 Ghost's own: the same for every character
  being.md              what she is, whose word counts, the ground rules
  channels/             the page she reads a place on, one per channel:
                        console, qq, discord, telegram, x, and chat for any other
  logic/                how her day goes, and what every page shares: a template each
  logic/texts/          the short framings, nudges and refusals, a file per feature
  work/                 how a piece of work goes: working.md (handed over),
                        work.md and texts.toml (the work loop's)
  tools.toml            what every tool is, in a conversation and at work
  desk/                 Ghost, the mascot the desktop app runs
character/              how a character's own ghost.toml is turned into prose
  persona.md            the persona template
  texts.toml            the texts only it puts around her fields
```

**System** is how Ghost works, and every character a deployment runs is
told the same. The desk's Ghost, the mascot, is part of it: what the desktop app runs
is a deployment like any other, a `ghostd` with a home of its own, and
`system/desk/` holds the two files it replaces, laid out as any
deployment's directory is. `ghostd` builds them in, and gives them to the
ghosts of a home in place of the built-in ones.

**Character** is the arrangement and nothing else. Her name, temperament,
voice, examples, story, values and past are in her `ghost.toml`, which is
in her journal; `persona.md` puts them in order, so that two characters
differ in what fills it and in nothing else. Swapping one character for
another changes her `ghost.toml` and none of these files. That is also why a
character never holds prompts in her own directory: a copy there would make
her work unlike the others. Nothing reads one there.

Her system prompt is `character/persona.md`, then `system/being.md`, filled
in. Everything else is put into a request by the code that needs it.

## Texts

The `.toml` files of texts are tables of named pieces. The code asks for a
text by its name, never by its file: the files of `system/logic/texts/` and
`character/texts.toml` are read as one table, and `system/work/texts.toml`
is the work loop's. So a name is in one file of its table and no other; a
test holds the built-ins to that, and a deployment that has one name in two
files is warned about it. A `{{name}}` is filled in by that code, once:
what goes in is never read again for placeholders. A text said to be "only
when" is left out altogether, framing and all, when what fills it is empty.
Mind the newlines at the start and end of some of them: they are what keeps
a piece apart from its neighbours.

## Changing them

A person editing these files in the repository changes what every build
says. A deployment changes them for itself: `[mind] prompts` in
`ghostd.toml` names a directory laid out as this one is, holding only the
files it changes, and each file there wins over the built-in.
`ghostd prompts export --only <file or directory>` writes this build's copy
to start from, `ghostd prompts list` says where each prompt in use comes
from, and a file changed while she runs is taken in without a restart. A
file of texts or tools there need only carry the entries it changes. See
[crates/ghostd](../crates/ghostd/README.md#prompts).

Prompts are in English, and say why. A prompt that only gives orders gets
worse behaviour than one that explains.
