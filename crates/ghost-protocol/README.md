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
| `console` | `ghost.status`, `mind.agenda`, `ghost.identity`, `memory.people`, `memory.places`, `memory.canon`, `work.list`, `attention.list`, `journal.read`, `journal.place`, `journal.subscribe`, `console.say`, `console.stop`, `work.answer`, `work.stop` | answers, then `journal.entry` notifications |

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

### Stopping her

Having said something, an operator can stop her in that place, as one
stops a coding agent after sending it something:

```json
{"jsonrpc":"2.0","id":3,"method":"console.stop","params":{"place":"desk-1"}}

{"jsonrpc":"2.0","id":3,"result":{"stopped":true}}
```

`place` is as for `console.say`, with the same default; `params` may be
left out altogether. Whatever she is in the middle of there is called off:

- the look she is taking, or about to take: a brain call that is out is
  called off, and an answer that comes back anyway is not acted on;
- what she had decided to say there and not yet sent. A message already
  handed over to be sent is out of her hands, and is written as `said`;
- what was unread there is left read, so she does not look at it again;
- the work she took on there, directed or asked, as `work.stop` stops it.

`stopped` is `true` when anything was. Her journal then has a `stopped`
entry: the place, `by` (`console:<who>` in lower case), and, as each
applies, `"looking":true`, `unsaid` (what she did not say) and `works`
(the ids of the work stopped, each of which also ends `dropped`). She
remembers it, and the next look there is told she was stopped and why it
was meant, so she does not take up again unasked what was called off.

With nothing under way there, the result is `{"stopped":false}` and
nothing is written. Refused as `console.say` is.

## Answering and stopping her work

An operator at a console, as above, with `who` in its hello, can answer
what a piece of her work asked, and stop a piece of her work, without her.
Each is done the way she does it, and her journal says it was theirs.

`work.list` says what a piece of work waits on: `state` is `waiting`,
`question` what it asked, and, when the work said, `kind` what answering
takes and `options` what to choose from.

```json
{"id":"20261116-140000-1a2b","seq":812,"brief":"…","state":"waiting","steps":14,
 "question":"Which week should the rota cover?","kind":"choose",
 "options":["this week","next week"],"started":"…","last":"…"}
```

`kind` is `answer` (say something), `file` (give it a file only they have),
`act` (do something only they can, such as signing in, and say when it is
done) or `choose` (pick one of `options`). A client that does not know a
kind it is sent reads it as `other`.

```json
{"jsonrpc":"2.0","id":3,"method":"work.answer","params":{"work":"20261116-140000-1a2b","text":"next week"}}
{"jsonrpc":"2.0","id":3,"result":{}}

{"jsonrpc":"2.0","id":4,"method":"work.stop","params":{"work":"20261116-140000-1a2b"}}
{"jsonrpc":"2.0","id":4,"result":{}}
```

- `work.answer` puts `text` in the work's inbox, as what she passes on to
  her work is: it is in front of the work before its next step, and a work
  waiting on an answer goes on. Sent to work that is not waiting, it is
  added to what it was asked. Her journal has it as an `added` entry with
  `"by":"console:<who>"`.
- `work.stop` stops it as she stops work: a sitting at it ends after the
  step it is on, and with none it ends at once. It is `dropped`, and its
  summary says who stopped it.

`work` is the id `work.list` gives. Only her open work can be named: an id
she has none of, one that has ended, or a part's, is refused with
`-32005`. Blank `text`, or no `who`, with `-32602`.

## What is waiting on whoever runs her

`attention.list`, with no params, lists what is waiting on her operators,
newest first. Any console may ask; it changes nothing. It is made afresh
from her journal, her memory and her work's records each time, and which
items a person has already seen is the client's to keep, by `id`, which is
the same each time an item is listed and never another item's.

```json
{"jsonrpc":"2.0","id":5,"method":"attention.list"}

{"jsonrpc":"2.0","id":5,"result":[
  {"id":"report-905","kind":"report","at":"…","seq":905,"place":"qq:…","text":"…"},
  {"id":"told-871","kind":"told","at":"…","seq":871,"work":"20261116-140000-1a2b",
   "brief":"…","text":"halfway there","files":["/work/draft.md"]},
  {"id":"ended-850","kind":"ended","at":"…","seq":850,"work":"…","brief":"…",
   "text":"the site wants a login","outcome":"blocked"},
  {"id":"asked-812","kind":"question","at":"…","seq":812,"work":"…","brief":"…",
   "text":"Which week should the rota cover?","ask":"choose","options":["this week","next week"]}
]}
```

| `kind` | what | `seq` is | for how long |
|---|---|---|---|
| `question` | open work waiting on an answer; `work.answer` gives it one. `ask` and `options` as `work.list` has `kind` and `options` | its `asked` entry | as long as it waits |
| `ended` | work that ended `blocked` or `gave_up` (`outcome`), with why (`text`) | its `concluded` entry | three days |
| `told` | what work an operator asked for or directed told her while it ran, with the files it made | its `reported` entry | three days |
| `report` | what she said when she told an operator about her day, and where (`place`) | the `intended` entry that meant it | three days |

A client reads a `kind` it does not know as `other`.

The console payloads mirror the core's serialized types and are not frozen yet.
