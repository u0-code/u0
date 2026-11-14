# ghost-work

The loop a ghost does things with: a model that can use tools, a set of
tools, and state that outlives any one context window.

**Is:** a [`Worker`] that takes a `ghost_brain::Brain`, a `Toolbox` and an
`Undertaking` and works until the undertaking is finished or something stops
it. A `Machine` to work on (a local directory, or a Docker container). The
standard tools: `shell`, `read`, `write`, `edit`, `plan`, `note`, `done`.

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
- A part of the work that is mostly reading can be handed to a helper
  (`delegate`) with a context of its own; only its report comes back.
  Calls that only look run side by side.
- Where there is someone to ask, the worker can stop and ask (`ask`), and
  goes on when the answer is added to the task.
- Talking without acting, repeating a call, and running low on steps are each
  said out loud to the model.

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


[`Worker`]: src/worker.rs
