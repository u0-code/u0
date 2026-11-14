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
| `claude-code` | DeepSeek's Anthropic-compatible endpoint, running inside the task container |

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

### On Apple silicon

Nearly all task images are amd64 and run emulated. Three things keep a run
from stalling:

- **Rosetta, and room.** In Docker Desktop turn on "Use Rosetta for x86_64/amd64
  emulation" and give the machine most of its processors and memory.
- **More time and more processors for every agent.** `--timeout-multiplier 2
  --override-cpus 4 --override-memory-mb 4096`. The tasks ask for one
  processor; emulated, that is not the machine they were timed on.
- **Do not let an agent install itself in the container.** Harbor's
  `claude-code` agent installs nodejs with `apt` first, which under emulation
  did not finish in fourteen minutes. It skips installation when the right
  version of `claude` is already on the path, so download the Linux binary
  once on the host and mount it in:

  ```bash
  V=2.1.322; mkdir -p cc
  curl -fSL -o cc/claude https://downloads.claude.ai/claude-code-releases/$V/linux-x64/claude
  chmod +x cc/claude    # check it against $V/manifest.json; images on musl need linux-x64-musl
  MOUNT='[{"type":"bind","source":"'$PWD'/cc/claude","target":"/usr/local/bin/claude","read_only":true}]'
  ```

  `ghost-work` needs nothing in the container: its loop runs on the host.

Pull the task images before a run, so that no trial waits on a download.

If the host reaches the internet through a local proxy, exclude the model's
host from it (`NO_PROXY=api.deepseek.com`) for `ghost-work`, whose loop runs
on the host. Through one such proxy, one streamed answer in twenty stopped
partway and had to be asked for again. Agents inside a container do not go
through the host's proxy.

## Run

```bash
COMMON="-p terminal-bench-2-1/tasks $TASKS -n 2 -k 1 --timeout-multiplier 2 --override-cpus 4 --override-memory-mb 4096 -o jobs"

harbor run $COMMON -a oracle --job-name oracle                     # the reference solutions, as a sanity check
harbor run $COMMON -a ghost_harbor.agent:GhostAgent -m deepseek/deepseek-flash --job-name ghost
harbor run $COMMON -a claude-code -m deepseek/deepseek-flash \
    --base-url https://api.deepseek.com/anthropic --api-key-name DEEPSEEK_API_KEY \
    --ak version=$V --mounts "$MOUNT" --job-name claude-code
```

An interrupted job is picked up with `harbor jobs resume -p jobs/<job>`.
`jobs/<job>/result.json` has the pass rate; each trial directory has the
reward, timings and, for `ghost-work`, `agent/state/work.db`, whose `log`
table has one line per step.

## Which tasks

Running all 89 for two agents takes most of a day on a laptop. The
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

Results are not kept in this repository. The comparison that has been run is
the one below, which needs no containers; `python3 bench/polyglot.py --out
<dir> --report` prints its table. The Terminal-Bench comparison above has
only been run for `ghost-work` on five development tasks.

## Without containers: Aider polyglot

```bash
git clone https://github.com/Aider-AI/polyglot-benchmark
export DEEPSEEK_API_KEY=...
python3 bench/polyglot.py --bench polyglot-benchmark --out runs --agent ghost
python3 bench/polyglot.py --bench polyglot-benchmark --out runs --agent ghost --retry
python3 bench/polyglot.py --bench polyglot-benchmark --out runs --agent claude --claude-bin /path/to/claude
python3 bench/polyglot.py --bench polyglot-benchmark --out runs --agent claude --retry
python3 bench/polyglot.py --out runs --report
```

The 34 Python exercises, with their tests hidden from the agent. As in the
benchmark's own protocol there are two tries: after the first, an agent that
failed is shown what the tests printed and may fix its solution. The agents
run on this machine, in scratch directories, as you. Claude Code needs a
recent version (2.1.20 fails against DeepSeek's endpoint; 2.1.322 works) and
is given its own configuration directory, so your login is not touched.
