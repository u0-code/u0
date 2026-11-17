# ghost-work

The loop a ghost does things with: a model that can use tools, a set of
tools, and state that outlives any one context window.

**Is:** a [`Worker`] that takes a `ghost_brain::Brain`, a `Toolbox` and an
`Undertaking` and works until the undertaking is finished or something stops
it. A `Machine` to work on (a local directory, or a Docker container). The
standard tools: `shell`, `read`, `write`, `edit`, `plan`, `note`, `done`. The
catalog (`src/catalog.rs`) lists every tool of a ghost's conversations and of
work, the loops each belongs to and what each needs.

**Is not:** a person. It does not know who is working; that is a paragraph
handed in by whoever starts it. `ghost-mind` is what makes it hers.

What makes it hold up over hours:

- The plan, the notes and the outcome live in a SQLite database, `work.db`,
  and the transcript is saved there after every step. Kill the process
  anywhere; run it again with the same state directory and it carries on.
  One database holds any number of undertakings by id: a ghost keeps all of
  hers in `<ghost>/work.db`, beside her journal. Only the bookkeeping is
  there; the files the work produces are wherever its machine put them.
- The transcript may fill most of the model's context window. When it nears
  that, long tool output is trimmed to its head and tail, then old output is
  dropped. If that is not enough the worker writes itself a handoff and goes
  on from the task, the plan, its notes and the last stretch of the
  transcript word for word.
- Nothing is lost to that: every item the transcript ever held is appended to
  the `history` table, which refuses any change to what is in it.
- How to finish is said beforehand, the way Codex does it: the instructions
  say that checking is part of the work and what `finished` means, and work
  that has outlived a context is told again, at length, when it is picked
  up. `done` then ends the work as reported. Nothing is sent back and nobody
  reads it over.
- A self-contained part of the work can be handed to another worker
  (`delegate`) with a context of its own, and runs alongside while the
  worker goes on. What the part tells, asks and finally reports reaches the
  worker between its steps, through the same inbox as what is added to its
  task; `steer`, `stop`, `wait` and `parts` are how it answers, ends, waits
  on and looks over its parts. A few run at once (`parts` in the limits,
  4 by default) and the rest queue. A part is kept in `work.db` like any
  undertaking and resumes at the parent's next sitting; it hands out no
  parts of its own, and the work cannot be called `finished` while one is
  still going. Calls that only look run side by side.
- A part can instead be handed to an agent installed on this machine,
  Claude Code or Codex (`delegate` with `agent`, offered only for the ones
  there are). It runs in the machine's directory with its own loop and its
  own tools, given only the task. What it does is written to the part's log
  as steps, its answer is the part's report, and its session is kept:
  what is passed on to the part is given to it when it next stops, in the
  same session, and a part that outlives a sitting is resumed. It asks
  nobody anything and keeps nothing (`agents.rs`).
- Where there is someone to ask, the worker can stop and ask (`ask`), and
  goes on when the answer is added to the task. It may say what answering
  takes, `answer`, `file`, `act` or `choose` with `options`; the record
  keeps it beside the question until the answer comes, for whoever shows
  the question to them.
- Talking without acting, repeating a call, and running low on steps are each
  said out loud to the model.

## Servers of tools

A ghost gets more tools the way a person gets an app: `find_tools` searches
the [MCP registry](https://registry.modelcontextprotocol.io), `install`
installs a server from it, `uninstall` takes one out, and `load_tools` makes
one server's tools callable as `<server>__<tool>` for the rest of a sitting
or a look at a conversation, so that tools nobody is using cost nothing.
Nobody approves an install; what keeps a server in its place is checked in
`src/servers/`: only the current listing of a name, a GitHub namespace only
with its owner's repository, exact versions with no install scripts, an
environment holding nothing but what it was given by name, https remotes on
the public internet only, bounded tool lists with nothing that runs
arbitrary code, a kill of the whole process group when a call runs over, and
a hash that puts a server aside when it starts describing its tools
differently from the day it was installed. The protocol is spoken by
[`rmcp`](https://crates.io/crates/rmcp), behind `src/servers/client.rs`.
Installs are recorded in the ghost's journal by `ghost-mind`, and a command-
line agent stepping the work does not get these tools.

## Skills

A skill is a folder with a `SKILL.md`, in the open format Claude Code reads:
a procedure someone wrote down for a kind of job. `find_skills` searches the
index at [skills.sh](https://skills.sh), with what its auditors say of each
skill; `install_skill` takes one skill's folder from its GitHub repository
at the commit the default branch points at; `uninstall_skill` takes one out;
and `use_skill` reads a skill's instructions, or one of its files, when a job
calls for it. Each skill she has costs one line until it is read. Nobody
approves an install; what is checked, in `src/skills/`: only skills the index
lists, from the repository their id names; none an auditor rates high or
critical, and none with scripts that nobody audited; frontmatter as the
format says, named as its folder; at most 100 files and 2 MB, no links,
nothing outside its folder, text except assets of up to 1 MB. Nothing in a
skill is run, and everything it says reaches the model marked as outside
content, naming the repository and commit. A folder that changes on disk is
put aside when the ghost next starts.

## Run one

```bash
export DEEPSEEK_API_KEY=...
docker run -d --name sandbox -w /work python:3.12-slim sleep infinity
cargo run --release -p ghost-work -- \
  --task "Write fizzbuzz.py and a test for it; run both." \
  --state ./run1 --docker sandbox --workdir /work
```

The report is JSON on stdout: why it ended, steps, sittings, tokens (read,
cached, written) and seconds. The `log` table in `./run1/work.db` has a line
per step:

```bash
sqlite3 ./run1/work.db "SELECT line FROM log WHERE id = 'main' ORDER BY n"
```

`--local <dir>` works in a directory on this computer instead. That is not a
sandbox: commands the model writes run as you. Use a container for anything
you did not write the task for yourself.

Any OpenAI-compatible server works (`--base-url`, `--model`,
`--api-key-env`); `--kind anthropic` uses the Anthropic API.

## What it is told

Every word the model reads from this crate is a file in the tree
[`prompts/`](../../prompts/README.md) at the top of the repository: the
working instructions in `system/work/work.md`, what is said along the way
in `system/work/texts.toml`, and in `system/tools.toml` what every tool is,
for work and for a ghost's conversations alike. They are the work layer of
the system, the same for every character, and are built into the binary.
What is built in is only the last resort: a file at the same path in
`--prompts <dir>`, laid out as the tree is, wins, file by file, and the
texts and tools entry by entry. Nothing in `--state` is
read as a prompt. The files are looked at before each step, so an edit
reaches work that is already running.


[`Worker`]: src/worker/mod.rs
