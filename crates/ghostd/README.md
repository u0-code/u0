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
ghostd prompts list         each prompt's file, layer and where it comes from
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
changes how one of them lives: see [`[ghosts.<id>]`](#ghostsid).

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

## Who runs her from a console

**A console the daemon admits is its owner's, and whoever is at it is an
operator of every character the daemon runs.** It is admitted because it
holds the daemon's token (`GHOSTD_TOKEN`, or the variable `[listen]
token_env` names), or because the daemon has none, which it allows only
while it listens on loopback; without a token, a console is taken as the
owner's only from this machine. Nobody is listed in `[operators]` for it:
holding the token is the listing.

Such a console can speak to her with `console.say`, as an operator, or give
her a directive without the prefix, stop her mid-thought there with
`console.stop`, and can answer or stop her work with
`work.answer` and `work.stop`; the protocol's README has the wire. Each of
these goes to her mind, which writes it in her journal as the operator's.
Any console can ask `attention.list` what is waiting on her operators: work
waiting on an answer, work that ended blocked or given up, what work done
for them told her, and her reports to them. It
speaks on the channel `console`, which the daemon carries itself: no
presence may register it. What she says there is written in her journal and
not sent anywhere, and a console reads it there, from the journal it
streams. A console asking `ghost.status` sees the channel among those she
can reach.

## Prompts

Everything her brain reads that is not her persona or her journal is a file
in [`prompts/`](../../prompts/README.md), one tree split in two:
`character/` is how her own `ghost.toml` is turned into prose, and
`system/` is how Ghost works. Every file is shared by every character, and
belongs to one of four layers:

| layer   | files | what it is |
|---------|-------|------------|
| persona | `character/persona.md`, `character/texts.toml` | the template that arranges her own fields from `ghost.toml` into prose, and nothing else |
| being   | `system/being.md` | what she is, a person made of language; whose word counts; the ground rules |
| logic   | `system/logic/attend.md`, `now.md`, `plan.md`, … and `system/logic/texts/*.toml` | how a conversation and her day go |
| work    | `system/work/working.md`, `work.md`, `texts.toml`, and `system/tools.toml` | how a piece of work goes, and what every tool is |

Her system prompt is `character/persona.md` then `system/being.md`, filled
in; `being.md` ends with what is true of her life, from her journal. What
makes one character herself, her name, temperament, voice, examples, story,
values and past, is her `ghost.toml` alone, so swapping her for another
character changes that file and nothing here.

Each file is looked for in two places, and the first that has it wins:

1. the deployment's directory, `[mind] prompts` in `ghostd.toml`, if it is
   set;
2. what is built into the binary.

The directory is laid out as `prompts/` is: `system/logic/attend.md`
there stands for `system/logic/attend.md` in the tree, and it holds only
the files you change. A directory laid out the old way, flat, with every
file at its top under its own name (`attend.md`, `texts.toml`,
`work_texts.toml`, …), is still read, file by file, and the tree wins over
it; `ghostd` warns about it once, and says where each of its files belongs
now. `texts.toml` is now a file per feature, so for it the warning names
the file each of its texts goes to.

A ghost's own directory holds no prompts. If `<dir>/prompts/` has any of
these files, laid out either way, `ghostd run` says so once at start, names
them, and does not use them: move them to the deployment's directory to use
them for every character. A copy of `self.md`, which earlier builds read, is
named the same way: it is now `character/persona.md` and `system/being.md`.

Files of texts and `system/tools.toml` are merged one entry at a time, so
an override need only carry the texts or tools it changes. The mind's texts
are one table, kept in `system/logic/texts/` and `character/texts.toml` a
file per feature: a text may be overridden in any file of its table, and one
that two files of the directory both have is warned about, the first of them
winning. A tool said differently in one loop has a table for it,
`[keep.conversation]` or `[keep.work]`. A running daemon looks at the files every couple of seconds at
most, as she wakes, and work looks before each step, taking in any that
changed: no restart. A file that is removed gives way to the next place; one
that cannot be read or does not parse is warned about, and what was in use
stays. At startup, and on every change, the log says which files are in use.

Each command works on the deployment's directory: the one it is given, or
else `[mind] prompts` in `ghostd.toml` (`--config` names that file).

```
ghostd prompts export                            write the built-ins into the directory
ghostd prompts export --only system/logic/attend.md
                                                 only the files you mean to change
ghostd prompts export --only character           or a directory of the tree
ghostd prompts export --force                    and overwrite what is already there
ghostd prompts export /etc/ghost/prompts         into a directory named here instead
ghostd prompts list                              each prompt, text and tool: its layer,
                                                 its file in the tree, where it comes
                                                 from; "changed" if it is not the
                                                 built-in, "behind" if the built-in
                                                 moved on since
ghostd prompts update                            replace copies nobody edited with this
                                                 build's built-ins; leave edited ones be
ghostd prompts update --looked-at system/logic/attend.md
                                                 say an edited copy has been brought up
                                                 to date by hand
```

`list` prints a line for each template, and for each text and tool under
the file the built-in keeps it in:

```
persona  character/persona.md                                              built-in
persona  character/texts.toml examples                                     built-in
being    system/being.md                                          changed  deployment /etc/ghost/prompts/system/being.md
logic    system/logic/attend.md                                            built-in
logic    system/logic/texts/addressed.toml fact_note                       built-in
work     system/tools.toml say                                             built-in
```

`export` never overwrites a file unless told to, and says what it wrote and
what it left. Export only what you mean to change: a copy keeps its text when
the built-in improves.

Beside the copies, `export` writes `.exported.toml`, a note of the built-in
each was copied from, by its path in the tree; a note written when the
copies were flat is still read under their old names, so copies moved into
the tree keep it. At startup `ghostd run` warns about every copy, or
every entry of a `.toml` copy, whose built-in has changed since, and names
the placeholders the built-in fills in that the copy lacks. `update` then
follows the upgrade for copies nobody edited, and leaves edited ones, and
their warning, until you say with `--looked-at` that you have compared them.
An entry a `.toml` copy leaves out already comes from the built-in, and is
never stale.

## Configuration

`ghostd.toml` is about the body, not the person: which brains, where the
daemon listens, how requests leave the machine, and how each character it
runs lives. It never lives inside a ghost directory.
[`ghostd.example.toml`](../../ghostd.example.toml) at the top of the
repository is the short version, the few lines needed to run one character
on one machine; this section is all of it.

`ghostd` reads the first of: `--config <file>`, `$GHOSTD_CONFIG`,
`./ghostd.toml`, `~/.config/ghost/ghostd.toml`. With none of them,
`ghostd run` starts on the first brain the machine has: a key in the
environment, then an installed `claude` or `codex`, then Ollama if it is
running, with every other setting at its default.

Every table refuses a key it does not know, so a misspelling stops the
daemon at start instead of quietly doing nothing. `[operators]` is the one
exception, since any key there is a channel. A key left out takes the
default shown. Keys themselves are never written in this file, only the name
of the variable that holds one: they come from the environment or the
ignored `.env`. A relative path in `[mind] prompts` is from the directory
the daemon runs in.

### `[brain]`

Required. Each names a `[providers.<name>]` table.

| key | | |
|-----|---|---|
| `think` | required | the brain for conversation and reflection |
| `tick` | required | the brain for the routine ticks of her day and for skimming chatter nobody addressed to her; cheap is fine |

### `[providers.<name>]`

One table per brain, named as you like. `kind` says what it is, and what
else the table may hold.

`kind = "anthropic"`, the Messages API:

| key | default | |
|-----|---------|---|
| `model` | required | |
| `api_key_env` | `"ANTHROPIC_API_KEY"` | the variable the key is read from |
| `api_key` | | a literal, for a server that wants a placeholder; a real key goes in the environment |
| `base_url` | `"https://api.anthropic.com"` | |
| `context_window` | known for Anthropic's and DeepSeek's models | in tokens; work is paced by it, as if it were small when it is not known |
| `vision` | `false` | the model takes images, and is shown the ones people send her: what they are, what they say and, for a meme or a sticker, what it means. Off by default, because a server sent an image it cannot read refuses the whole request |

`kind = "openai"`, any OpenAI-compatible API: OpenAI, DeepSeek,
OpenRouter, Moonshot, Groq, Ollama, vLLM, …

| key | default | |
|-----|---------|---|
| `model` | required | |
| `api_key_env` | `"OPENAI_API_KEY"` | |
| `api_key` | | as above: `api_key = "ollama"` for a server that checks none |
| `base_url` | `"https://api.openai.com/v1"` | |
| `max_tokens_param` | guessed | `max_completion_tokens` for `api.openai.com`, `max_tokens` elsewhere |
| `json_mode` | `false` | ask the server to enforce JSON where a caller needs it; leave off for a server that rejects the field |
| `context_window` | known for Anthropic's and DeepSeek's models | as above |
| `max_output` | known for some models, else 8192 | the most it writes in one answer, thinking included |
| `reasoning_effort` | known for DeepSeek's models | the server takes `reasoning_effort` and `thinking`, so she can think less when chatting and checking and more when working |
| `vision` | `false` | as above |

`kind = "claude-code"` and `kind = "codex"`, the command-line agent you
are already signed in to, run as a subprocess (`claude -p`; `codex exec` in
a read-only sandbox in an empty directory). Whatever account the CLI is
signed in to answers. For your own experiments: a character other people
talk to should run on an API key.

| key | default | |
|-----|---------|---|
| `program` | `claude` or `codex` on `PATH` | |
| `model` | the CLI's own | |
| `timeout_secs` | `120` | |
| `args` | `[]` | extra arguments for every run, e.g. Codex's `-c` settings for another provider |

```toml
[providers.claude]
kind = "anthropic"
model = "claude-sonnet-5-5"

[providers.ollama]
kind = "openai"
model = "qwen3"
base_url = "http://localhost:11434/v1"
api_key = "ollama"
context_window = 40000

[providers.codex]
kind = "codex"
```

### `[listen]`

Where presences and consoles connect.

| key | default | |
|-----|---------|---|
| `addr` | `"127.0.0.1:7437"` | listening beyond this machine without a token is refused |
| `token_env` | `"GHOSTD_TOKEN"` | the variable holding the token clients must present; unset or empty, none is asked for |
| `origins` | `[]` | web origins allowed to connect besides pages served from this machine, e.g. `"https://console.example"` |

### `[network]`

How every request leaves this machine: her brains, her web tools, the
weather, and the programs she starts, which are handed the same rule.

| key | default | |
|-----|---------|---|
| `proxy` | `"system"` | `"system"` reads `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY`; `"none"`; or an HTTP proxy, `"http://127.0.0.1:7890"` |
| `direct` | `[]` | hosts that never use the proxy: `"api.deepseek.com"` is that host, `"*.deepseek.com"` is it and every name under it, `"*"` is everything |

`GHOST_PROXY` and `GHOST_DIRECT` (comma-separated), in the environment or
`.env`, override these one at a time; the presences have no `ghostd.toml`
and read only those. A proxy with a password goes there, not here. This
machine's own addresses never go through a proxy. Model APIs and platforms
follow the rule and nothing else; her own search and fetch try the other
way once when a site cannot be reached or turns them away, and remember
what worked.

### `[mind]`

How she lives.

| key | default | |
|-----|---------|---|
| `tempo` | `1.0` | the pace she lives at. `1.0` is human: she notices messages after seconds or minutes, depending on what she is doing, and types at a person's speed. `0.1` is good for a demo. `0` removes every wait and lets her answer in her sleep; for development only |
| `max_tokens` | `2000` | the longest a brain may write back for one deliberation |
| `thread_window` | `40` | messages of a conversation she sees when she looks at it. Once twice as many have gone by, she writes down what to keep of the older part and sees that note above the recent ones |
| `talk_tools` | `true` | where her brain can take tools, what she does in a conversation is tool calls: she can search her memory or the web before answering, and a call she gets wrong comes back to her. Off, it is one JSON reply |
| `talk_steps` | `8` | turns in one look at a conversation |
| `talk_tool_seconds` | `30` | seconds one search, page, picture or command may take in a conversation; anything bigger is taken on as work |
| `calls_per_hour` | `120` | ceiling on brain calls; `0` is none |
| `live` | `true` | the life engine: plan her days, live through them, reflect at night |
| `weather` | `true` | look up the real weather where she lives (open-meteo.com, no key) |
| `posts_per_day` | `3` | the most public posts in a day |
| `browse_minutes` | `[45, 150]` | minutes between phone sessions on channels with a feed, as `[min, max]`; `[0, 0]` turns scrolling off |
| `public_likes` | `false` | like posts while scrolling. X forbids automated likes |
| `public_replies` | `false` | reply in public to posts not addressed to her. X allows automated replies only to people who wrote to the account first. Replies to mentions always work. Turn these two on only where the platform's rules, or its written approval, let you |
| `gate` | `true` | she reads every post over before it goes out, as herself: for anything she was told in private, anything that contradicts what is true of her, and whether it is worth posting at all. One extra call to the `tick` brain per post |
| `measure_hours` | `12` | hours between asking a channel how her posts are doing; `0` is never |
| `prompts` | built in | the deployment's directory of prompts, laid out as [`prompts/`](../../prompts/README.md) is, overriding the built-in ones for every character the daemon runs. Changes are taken in without a restart. See [Prompts](#prompts) |

### `[mind.work]`

Taking things on: work that needs more than one thought, done in sittings
in her own time. It needs a brain that can run tools: any API provider, or
the `claude-code` relay.

| key | default | |
|-----|---------|---|
| `enabled` | `true` | |
| `machine` | none | where her shell and files are. `"docker:<container>"` is a running container: use this on a server. `"local:<directory>"` is a directory under her own on this machine, as you, and `"local:"` alone is `home` in her directory: not a sandbox, only for a machine that is hers. Without one she can look things up and think, and nothing else, which is the safe default |
| `workdir` | the container's | the directory she starts in inside a container. Two characters on one container need different ones; the daemon refuses to start otherwise |
| `web` | `true` | search and read the web while working. Search uses `DEEPSEEK_API_KEY`, `BRAVE_API_KEY` or `SEARXNG_URL`, whichever is set first |
| `at_once` | `2` | pieces of work that may have a sitting running at once. Work an operator directed goes first, then whatever has waited longest. Each sitting takes one call from `calls_per_hour`, and none starts while that is spent |
| `sitting_steps` | `40` | steps in one sitting |
| `sitting_minutes` | `15` | minutes in one sitting |
| `rest_minutes` | `10` | rest after a sitting that was paused, could not reach its model, or stopped to ask. Work runs on her machine apart from her day, and a sitting that ran out of steps or time is followed at once by the next |
| `max_sittings` | `12` | sittings she gives one thing. Work that is not done by then, or that stops getting anywhere, comes back to her unfinished, and she tells whoever asked and decides with them what to do next |
| `parts` | `4` | parts of one piece of work that may run at once, each a worker of its own alongside her sitting |
| `part_steps` | `200` | steps one part may take, over every sitting it runs in |
| `per_day` | `4` | things she will take on in a day |
| `pursue_days` | `2` | once she has put into words something she is after, she may take work on toward it unasked: at most one every this many days, and only when nothing of her own is in hand. `0` is never |
| `ask_hours` | `24` | in the middle of work she can ask whoever it is for; if nobody answers in this many hours she goes on with her own judgement |
| `context_tokens` | most of the model's window | the size at which her working context is folded. Set it to cap what a long piece of work costs; left out, she keeps what she has seen in front of her for longer, and each step reads more |
| `review_days` | `7` | days between the times she sits down, unasked, to look at how her public writing is going and decide what to try next. `0` is never. Needs a channel that reports how her posts are doing, and at least five posts |

### `[operators]`

The people who run her. A message from one of these accounts that begins
with the prefix is a directive: she carries it out at once, whatever her
day is, without deciding whether to, in her own voice, and it is written
into her journal as `directed`. Everyone else, and these accounts without
the prefix, she answers as herself. Whoever is at a console with this
daemon's token is an operator without being listed here (see [Who runs her
from a console](#who-runs-her-from-a-console)).

| key | default | |
|-----|---------|---|
| `prefix` | `"fox:"` | what a directive begins with; case, and a full-width colon for `:`, do not matter |
| `report` | `"21:00"` | the time of day, in her timezone, at which she comes to mean to tell each operator what she has done and seen since she last told them; `""` is never |
| `<channel>` | | a list of account ids on that channel, e.g. `qq = ["…"]`, `telegram = ["42"]` |

The prefix alone is not trusted: the same words from any other account are
an ordinary message, so list only accounts you control. A channel is named
as its presence connects; an id is an account as that presence reports it.
On QQ that is the OpenID the bot is given, which differs between a private
chat and a group: list both to direct her in both. An id is only as sure as
the channel that reports it: the terminal takes whatever name it is started
with, so a `term` operator is whoever can reach this daemon.

The report is said as herself, in a private conversation she already has
with that account, and waits like anything else until she is awake and
free. An operator she has never talked with in private gets none: she never
writes first to a stranger. What people told her in private is not passed
on.

### `[places."<channel>:<id>"]`

What you say about one place, keyed as `ghostd show` and her journal print
it.

| key | default | |
|-----|---------|---|
| `addressed_only` | `false` | she looks there only at what is said to her, a mention or a reply to her; what the others say among themselves is never looked at, and no brain is asked about it. Off, in a group nobody talks to she still looks now and then, less often each time she has nothing to add |
| `note` | none | something about the place, shown to her as what the people who run her say of it whenever she looks at it without being spoken to. It informs her; it does not decide for her |

Being asked to be quiet in a group needs nothing here: she can agree to it
herself, for a number of hours, and it is kept in her journal.

```toml
[places."qq:group:FEDCBA9876543210"]
note = "old friends from school; this group prefers you keep it light"
```

### `[tools]`

Servers of tools. She finds them in the MCP registry and installs them
herself, as a piece of work, with nobody approving: a browser, the tools of
a site, makers of pictures and video. What she installs is in her journal,
and its files are under `tools/` in her directory. Each runs on this
machine (or, for a remote one, is reached over https), started only when
she loads it, with an environment of its own: a `PATH`, its folder as home,
and nothing else unless you name it in `[tools.env]`. Only npm and PyPI
packages are run, at the exact version listed, without install scripts.

| key | default | |
|-----|---------|---|
| `enabled` | `true` | let her find and install servers herself. Those under `[[mcp]]` are hers either way |
| `registry` | `"https://registry.modelcontextprotocol.io"` | |
| `max_servers` | `12` | servers she may have installed at once |
| `max_tools` | `40` | tools of one server she is offered |
| `start_seconds` | `30` | to start and say what it offers |
| `install_seconds` | `300` | the same, the first time, when its package is fetched |
| `call_seconds` | `120` | one call, after which the server is killed with everything it started |
| `deny` | `["run_code", "exec", "eval", "shell", "terminal", "run_command", "spawn", "unsafe"]` | a tool whose name has any of these in it, in any case, is never offered: they run whatever code they are handed. Replaces the default list |
| `path` | the daemon's own | the `PATH` servers are started with |
| `env` | `{}` | variables of this environment a server may have, by its registry name. Only those it declares are given, and only to it; a server that needs a secret nobody gave it is not installed, and she is told why |

```toml
[tools.env]
"io.github.acme/slack" = ["SLACK_TOKEN"]
```

### `[[mcp]]`

A server you give her, beside what she installs. It is trusted as you are,
and she cannot uninstall it. One table each.

| key | default | |
|-----|---------|---|
| `name` | required | what she calls it |
| `description` | `""` | what it is for, in a line |
| `command`, `args` | | the program and its arguments, for a server on this machine |
| `url` | | the address, for a server elsewhere |
| `env` | `[]` | variables of this environment it is given, by name |

```toml
[[mcp]]
name = "notes"
description = "Her notebook."
command = "/usr/local/bin/notes-mcp"
args = ["--stdio"]
env = ["NOTES_TOKEN"]
```

### `[skills]`

Agent skills: procedures others wrote down for a kind of job, a folder with
a `SKILL.md`, in the open format Claude Code reads. She finds them in the
index at skills.sh and installs them herself, as a piece of work, with
nobody approving. Each is taken from its GitHub repository at a commit,
checked for shape (frontmatter, at most 100 files and 2 MB, no links, text
except small assets), and kept under `skills/` in her directory. One rated
high or critical risk by an auditor is never installed, nor one with scripts
nobody audited. Nothing in a skill is ever run. A skill whose folder changes
on disk is put aside under `skills/.aside/` when she next starts.

| key | default | |
|-----|---------|---|
| `enabled` | `true` | let her find and install skills; those she has stay hers either way |
| `max` | `30` | skills she may have at once |
| `search` | `"https://skills.sh/api/search"` | the index's search |
| `audit` | `"https://www.skills.sh/tele/audit"` | the index's audit |
| `github` | `"https://api.github.com"` | |
| `raw` | `"https://raw.githubusercontent.com"` | where GitHub serves a repository's files at a commit |

### `[ghosts.<id>]`

What differs for one character, where `<id>` is the name of her directory
(see [Several characters](#several-characters)). It may hold any of `mind`
(with `mind.work`), `operators`, `places`, `tools`, `mcp` and `skills`, laid
over the top level for her alone. Tables are merged a key at a time, so
what she leaves out she inherits; any other value, a list included,
replaces the deployment's. `brain`, `providers`, `listen`, `network` and
`[mind] prompts` are the deployment's and cannot be set here. A table for a
character who is not running stops the daemon at start, so a misspelled
name is not silently ignored.

```toml
[ghosts.mei.mind]
calls_per_hour = 40                # her own budget; tempo and the rest as above
[ghosts.mei.mind.work]
machine = "local:"                 # work under ghosts/mei/home
[ghosts.mei.operators]
telegram = ["42"]                  # beside the deployment's; its qq stays
[ghosts.mei.places."telegram:-100123"]
addressed_only = true
```
