# ghost-protocol

The wire types of the Ghost Protocol: JSON-RPC 2.0 over a WebSocket, one JSON
object per text frame.

**Is:** `serde` types and method names. Nothing else.

**Is not:** the journal format. The two look alike and are kept apart: the
journal is forever, this is versioned by `PROTOCOL`.

A connection opens with a `hello` request declaring a role.

| role | sends | receives |
|---|---|---|
| `presence` | `presence.inbound`, `presence.typing` notifications | `presence.act` requests, answered with what the platform said |
| `console` | `ghost.status`, `mind.agenda`, `ghost.identity`, `memory.people`, `memory.places`, `memory.canon`, `journal.read`, `journal.place`, `journal.subscribe` | answers, then `journal.entry` notifications |

One presence carries a channel at a time. When a newer one registers the same
channel it takes over, and the older is sent `presence.displaced` and closed, so
a window left open or an adapter that was restarted cannot hold a channel
against its replacement.

There is no method that edits her identity or removes a journal entry.

A browser may connect only from a page served on the same machine, from the
desktop console, or from an origin the daemon lists. Native clients send no
origin and are not asked.

The console payloads mirror the core's serialized types and are not frozen yet.
