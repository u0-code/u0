# ghost-mind

The part that lives.

**Is:** a single-writer actor that owns a ghost. It keeps an agenda of when she
will look at each place with something unread, hands a look to a brain, turns
the actions that come back into journal entries, delivers what she says at the
pace a person types, plans and lives her day, pursues her own intentions, and
scrolls feeds. The prompts it uses are data, in [`prompts/`](prompts/).

**Is not:** aware of any platform or wire format. It acts through the `Body`
trait and perceives weather through `Senses`; the daemon supplies both. It can
be embedded in anything that can supply them.

Its behaviour is pinned by tests that run under a simulated clock, in
`src/mind/tests.rs`. Read those to see what she does and when.
