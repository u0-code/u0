# bench

How the loop in `ghost-work` is measured against other agents.

**Benchmark:** [Terminal-Bench 2.1](https://github.com/harbor-framework/terminal-bench-2-1),
run with [Harbor](https://github.com/harbor-framework/harbor). 89 tasks in
containers (system administration, data processing, security, builds,
scientific computing, some coding), each graded pass or fail by the task's own
tests. It is the established public benchmark for multi-step work in a
terminal, and it is not only code editing.

**Same model for every agent:** DeepSeek `deepseek-flash`, each agent using
its own key-less defaults otherwise.

**Agents:**

| Agent | How it reaches the model |
|---|---|
| `ghost-work` (this repository) | OpenAI-compatible API, the loop on the host, its shell in the task container |
| `terminus-2` (Harbor's reference agent) | LiteLLM in the Harbor process, a tmux session in the container |
| `claude-code` | DeepSeek's Anthropic-compatible endpoint |

## Set up

```bash
uv tool install --python-preference only-managed --python 3.12 harbor --with socksio
git clone https://github.com/harbor-framework/terminal-bench-2-1
git -C terminal-bench-2-1 checkout 12b7565679ce5006230b820666353673a133db93
cargo build --release -p ghost-work

export HARBOR_TELEMETRY=off
export DEEPSEEK_API_KEY=...
export GHOST_WORK_BIN=$PWD/target/release/ghost-work
export PYTHONPATH=$PWD/bench
```

`--with socksio` is only needed behind a SOCKS proxy. Docker must be running.
On Apple silicon nearly all task images are amd64 and run emulated, so
timeouts are doubled for every agent (`--timeout-multiplier 2`) and tool
installation is given more time (`--agent-setup-timeout-multiplier 3`).

## Run

```bash
COMMON="-p terminal-bench-2-1/tasks $TASKS -n 2 -k 1 --timeout-multiplier 2 --agent-setup-timeout-multiplier 3 -o jobs"

harbor run $COMMON -a oracle --job-name oracle                     # the reference solutions, as a sanity check
harbor run $COMMON -a ghost_harbor.agent:GhostAgent -m deepseek/deepseek-flash --job-name ghost
harbor run $COMMON -a terminus-2  -m deepseek/deepseek-flash --job-name terminus2
harbor run $COMMON -a claude-code -m deepseek/deepseek-flash \
    --base-url https://api.deepseek.com/anthropic --job-name claude-code
```

`jobs/<job>/result.json` has the pass rate; each trial directory has the
reward, timings and, for `ghost-work`, `agent/state/log.jsonl` with one line
per step.

## Which tasks

Running all 89 for three agents takes most of a day on a laptop. The
comparison uses a fixed subset chosen before any agent was run on it:

1. Keep tasks a laptop can run: agent timeout at most 30 minutes, at most 4 GB
   and 2 CPUs, image at most 700 MB, no nested virtual machines. 60 remain.
2. Sort names, shuffle each difficulty level with `random.Random(20261112)`,
   take 2 easy, 8 medium, 5 hard.

```
TASKS="-i prove-plus-comm -i cobol-modernization -i chess-best-move -i query-optimize -i financial-document-processor -i git-leak-recovery -i largest-eigenval -i sanitize-git-repo -i pypi-server -i large-scale-text-editing -i protein-assembly -i cancel-async-tasks -i feal-differential-cryptanalysis -i password-recovery -i llm-inference-batching-scheduler"
```

Development and debugging of `ghost-work` used other tasks (`fix-git`,
`overfull-hbox`, `log-summary-date-ranges`, `break-filter-js-from-html`,
`fix-code-vulnerability`), never these fifteen.

With 15 pass-or-fail tasks the uncertainty on a pass rate is about ±25
points. Read the results task by task; a difference of one or two tasks
between agents is not a finding.

## Results

The comparison has not been run yet. So far only `ghost-work` has been run,
on the five development tasks above, and it passed all five. That says the
loop works and the adapter is wired correctly; it says nothing about how it
compares.

Known problems on an Apple-silicon laptop:

- `terminus-2` installs tmux in the task container with a fixed 120 second
  limit. Emulated and held to the task's one CPU, the install takes longer
  and setup fails. `--override-cpus 4` (for every agent) is the likely fix
  and is untested.
- `claude-code` 2.1.20 fails against DeepSeek's endpoint with a tool-result
  ordering error; 2.1.322 works.
