# ghost-mind

The part that lives.

**Is:** a single-writer actor that owns a ghost. It keeps an agenda of when she
will look at each place with something unread, hands a look to a brain, turns
the actions that come back into journal entries, delivers what she says at the
pace a person types, plans and lives her day, pursues her own intentions,
scrolls feeds, and works on what she has taken on, a sitting at a time. The
prompts it uses are data, in [`prompts/`](prompts/): whole templates as
Markdown, and the short framings, nudges and refusals in `texts.toml`; what
each tool is said to do is in `ghost-work`'s `tools.toml`, one file for every
tool of both loops, read from the same directories. They are built in only as the last
resort; a file of the same name in her own `prompts/`, or in the configured
directory, is used instead, and taken in again when it changes.

What it adds to a look at a conversation, so that she is one person and not
one per platform: the rest of her day (what she has taken on, what has been
going on elsewhere), where each thing she knows was learned, and for each
person an account that keeps what they said in private apart from the rest.

In public she posts, is shown how her posts did, and writes down what that
taught her. Every post is read over before it goes out. What a platform does
not allow an automated account is off unless it is turned on.

**Is not:** aware of any platform or wire format. It acts through the `Body`
trait and perceives weather through `Senses`; the daemon supplies both. It can
be embedded in anything that can supply them.

Its behaviour is pinned by tests that run under a simulated clock, in
`src/mind/tests.rs`. Read those to see what she does and when.
