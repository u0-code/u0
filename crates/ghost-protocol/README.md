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
| `console` | `ghost.status`, `mind.agenda`, `memory.people`, `journal.read`, `journal.subscribe` | answers, then `journal.entry` notifications |

There is no method that edits her identity or removes a journal entry.

The console payloads mirror the core's serialized types and are not frozen yet.
