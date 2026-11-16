# ghost-protocol

The wire types of the Ghost Protocol: JSON-RPC 2.0 over a WebSocket, one JSON
object per text frame.

**Is:** `serde` types and method names. Nothing else.

**Is not:** the journal format. The two look alike and are kept apart: the
journal is forever, this is versioned by `PROTOCOL`.

A connection opens with a `hello` request declaring a role. `console` is
the role of whatever watches her and of whoever runs her: CLI tools, the
desk.

| role | sends | receives |
|---|---|---|
| `presence` | `presence.inbound`, `presence.typing` notifications | `presence.act` requests, answered with what the platform said |
| `console` | `ghost.status`, `mind.agenda`, `ghost.identity`, `memory.people`, `memory.places`, `memory.canon`, `work.list`, `journal.read`, `journal.place`, `journal.subscribe`, `console.say` | answers, then `journal.entry` notifications |

One presence carries a channel at a time. When a newer one registers the same
channel it takes over, and the older is sent `presence.displaced` and closed, so
a window left open or an adapter that was restarted cannot hold a channel
against its replacement.

There is no method that edits her identity or removes a journal entry.

A browser may connect only from a page served on the same machine or from
an origin the daemon lists. Native clients send no
origin and are not asked.

## Speaking from a console

A console the daemon admits is its owner's: it holds the daemon's token, or
the daemon has none, which it allows only while it listens on loopback, and
then only from this machine. So whoever is at a console is an operator of
every character the daemon runs, without being listed in `ghostd.toml`.

A console that speaks says who is at it in its hello, with `who`:

```json
{"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol":1,"role":"console","token":"…","ghost":"weiwei","who":"Fox"}}
```

and then says something with `console.say`:

```json
{"jsonrpc":"2.0","id":2,"method":"console.say","params":{"place":"desk-1","text":"send me the rota","mode":"directive"}}

{"jsonrpc":"2.0","id":2,"result":{"place":"console:desk-1","id":"console-1763280000000-0"}}
```

- `text` is what is said. It may not be blank.
- `place` is a place on the channel `console`, by its id. Each is a private
  conversation of its own. Left out, it is the one named after `who` in
  lower case.
- `mode` is `say`, the default, or `directive`. `say` reaches her as
  anything an operator says; `directive` as an operator's directive does
  from a platform with the prefix: looked at at once, whatever her day, and
  carried out rather than weighed.

She knows whoever is at the console as `console:` and `who` in lower case.
The result is the place's key and the message's id, as her journal's
`heard` entry has them.

Nothing is sent back on the connection. What she says in answer is written
in her journal as a `said` entry with `"channel":"console"` and the place's
id, and a console reads it there: from `journal.subscribe`, or with
`journal.place` and the key `console:<place>`. It reaches every console,
including one that connects later. No presence can register the channel
`console`; the daemon carries it itself.

A console with no `who` is refused with `-32602`, and so is blank `text`;
one that is not the owner's, which can only be one without a token from
another machine, with `-32001`.

The console payloads mirror the core's serialized types and are not frozen yet.
