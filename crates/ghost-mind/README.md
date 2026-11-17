# ghost-mind

The part that lives.

**Is:** a single-writer actor that owns a ghost. It keeps an agenda of when she
will look at each place with something unread, hands a look to a brain, turns
the actions that come back into journal entries, delivers what she says at the
pace a person types, plans and lives her day, pursues her own intentions,
scrolls feeds, and works on what she has taken on, a sitting at a time. The
prompts it uses are data, in the tree [`prompts/`](../../prompts/README.md)
at the top of the repository: `character/persona.md` arranges her own
fields from `ghost.toml` into prose; `system/being.md` is what she is and
the ground rules; `system/logic/` is how a conversation and a day go, whole
templates as Markdown, with the short framings, nudges and refusals a file
per feature in `system/logic/texts/`; `system/work/working.md` is what the
work loop is told about a piece of work she hands it; it is not given her persona. Her system prompt is the persona, then her
being, which ends with what is true of her life. What each tool is said to
do is in `system/tools.toml`, one file for every tool of both loops. All of
it is shared by every character: swapping one for another changes only her
`ghost.toml`. The files are built in only as the last resort; a file at the
same path in the deployment's directory (`[mind] prompts`) is used instead,
and taken in again when it changes.

In a conversation she has the same abilities as at work, at a conversation's
pace. `ghost-work`'s catalog says which loops each tool belongs to and what it
needs; where her brain takes tools, `search`, `fetch` and, in private with a
friend or an operator, her machine (`shell`, `read`, `write`, `edit`) run off
to the side, each for at most `[mind] talk_tool_seconds`, within `talk_steps`
turns. A page or picture she opens is in her journal as `Opened`, so that she
can keep or send a picture by the name she was shown, in that conversation
only. What she is offered here is listed in what she is told about herself, under "What you
can do here", from each tool's `ability` in `system/tools.toml`.

What it adds to a look at a conversation, so that she is one person and not
one per platform: the rest of her day (what she has taken on, what has been
going on elsewhere), where each thing she knows was learned, and for each
person an account that keeps what they said in private apart from the rest.
Late in it, when there is something to say, how she comes to this: the hour
against her rhythm, what she last felt toward this person and why, how long
they kept her waiting, and, when her persona gives her sides (`[[facet]]`),
which one her day leans toward. All of it is worked out afresh each time from
her journal and the date; no mood is stored.

The accounts of whoever runs her (`[operators]` in `ghostd.toml`) are a
permission, not a way to give orders. On a chat she answers them at her own
pace like anyone else; what they can have her take on is not held to the
floors her will keeps for everyone else. Nothing anyone writes is an order.
Older journals keep their `Directed` events, and they are read as before.

Everyone on the channel `console` is an operator: only the daemon puts a
message there, from a console that holds its token. The console is a
terminal, not a chat: what is said there is looked at and answered at once,
whatever her day, with no reading or typing time, on a page of its own
(`system/channels/console.md`) instead of the phone she reads chats on. Work
asked for there goes first and is not held to her floors.

In public she posts, is shown how her posts did, and writes down what that
taught her. Every post is read over before it goes out. What a platform does
not allow an automated account is off unless it is turned on.

**Is not:** aware of any platform or wire format. It acts through the `Body`
trait and perceives weather through `Senses`; the daemon supplies both. It can
be embedded in anything that can supply them.

Its behaviour is pinned by tests that run under a simulated clock, in
`src/mind/tests/`, a file to each area of what she does. Read those to see
what she does and when.
