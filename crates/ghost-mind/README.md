# ghost-mind

The part that lives.

**Is:** a single-writer actor that owns a ghost. It keeps an agenda of when she
will look at each place with something unread, hands a look to a brain, turns
the actions that come back into journal entries, delivers what she says at the
pace a person types, plans and lives her day, pursues her own intentions,
scrolls feeds, and works on what she has taken on, a sitting at a time. The
prompts it uses are data, in [`prompts/`](prompts/), a directory per layer:
`persona/persona.md` arranges her own fields from `ghost.toml` into prose;
`being/being.md` is what she is and the ground rules; `logic/` is how a
conversation and a day go, whole templates as Markdown and the short
framings, nudges and refusals in `texts.toml`; `work/working.md` is what she
is told about a piece of work. Her system prompt is the persona, then her
being, which ends with what is true of her life. What each tool is said to do
is in `ghost-work`'s `tools.toml`, one file for every tool of both loops, read
from the same directory. All of it is shared by every character: swapping
one for another changes only her `ghost.toml`. The files are built in only
as the last resort; a file of the same name in the deployment's directory
(`[mind] prompts`) is used instead, and taken in again when it changes.

In a conversation she has the same abilities as at work, at a conversation's
pace. `ghost-work`'s catalog says which loops each tool belongs to and what it
needs; where her brain takes tools, `search`, `fetch` and, in private with a
friend or an operator, her machine (`shell`, `read`, `write`, `edit`) run off
to the side, each for at most `[mind] talk_tool_seconds`, within `talk_steps`
turns. A page or picture she opens is in her journal as `Opened`, so that she
can keep or send a picture by the name she was shown, in that conversation
only. What she is offered here is listed in what she is told about herself, under "What you
can do here", from each tool's `ability` in `tools.toml`.

What it adds to a look at a conversation, so that she is one person and not
one per platform: the rest of her day (what she has taken on, what has been
going on elsewhere), where each thing she knows was learned, and for each
person an account that keeps what they said in private apart from the rest.

A message from an operator's account (`[operators]` in `ghostd.toml`) that
begins with the configured prefix is a directive. It is recorded as
`Directed`, looked at at once whatever her day is (the hourly budget still
holds), shown to her as a directive with a section of `texts.toml` saying she
carries it out, and work she takes on for it is not held to the floors her
will keeps for everyone else. The prefix from anyone else is a message.

In public she posts, is shown how her posts did, and writes down what that
taught her. Every post is read over before it goes out. What a platform does
not allow an automated account is off unless it is turned on.

**Is not:** aware of any platform or wire format. It acts through the `Body`
trait and perceives weather through `Senses`; the daemon supplies both. It can
be embedded in anything that can supply them.

Its behaviour is pinned by tests that run under a simulated clock, in
`src/mind/tests.rs`. Read those to see what she does and when.
