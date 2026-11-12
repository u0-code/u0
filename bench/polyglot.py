#!/usr/bin/env python3
"""Run agents on the Python exercises of the Aider polyglot benchmark.

Each exercise is copied to a fresh directory with its tests and reference
solution left out. An agent is told to implement the solution from the
instructions. When it stops, the exercise's own tests are put back and run.
Passing means every test passes.

Runs on this machine, with no container: the agent's commands run as you, in
a scratch directory. Both agents are given the same task text and the same
limits.

    git clone https://github.com/Aider-AI/polyglot-benchmark
    export DEEPSEEK_API_KEY=...
    python3 bench/polyglot.py --bench polyglot-benchmark --out runs --agent ghost
    python3 bench/polyglot.py --bench polyglot-benchmark --out runs --agent claude \
        --claude-bin /path/to/claude
    python3 bench/polyglot.py --out runs --report

`--agent claude` points Claude Code at DeepSeek's Anthropic-compatible
endpoint, and `--agent codex` points Codex at DeepSeek's Responses API. Each
gets its own configuration directory under `--out`, so neither touches your
login or settings. For Codex, "steps" counts commands run and files changed.
"""

import argparse
import concurrent.futures
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

TASK = """This directory holds a programming exercise.

Read `.docs/instructions.md`{append}. Implement the solution in {files}. Keep the names and signatures of the functions and classes already declared there: the tests import them. Use only the Python standard library.

There are tests for this exercise that you cannot see. They will be run against your file after you stop, and every one of them has to pass. Work out from the instructions what the edge cases are and make sure your implementation handles them."""

MAX_STEPS = 60
MAX_MINUTES = 8


def prepare(exercise: Path, ws: Path):
    """Copy the exercise without its tests and reference solution."""
    meta = json.loads((exercise / ".meta" / "config.json").read_text())["files"]
    if ws.exists():
        shutil.rmtree(ws)
    ws.mkdir(parents=True)
    shutil.copytree(exercise / ".docs", ws / ".docs")
    for name in meta["solution"]:
        shutil.copy(exercise / name, ws / name)
    return meta


def task_for(ws: Path, meta) -> str:
    append = " and `.docs/instructions.append.md`" if (ws / ".docs" / "instructions.append.md").exists() else ""
    files = ", ".join(f"`{name}`" for name in meta["solution"])
    return TASK.format(append=append, files=files)


def run_ghost(args, ws: Path, state: Path, task: str):
    if state.exists():
        shutil.rmtree(state)
    out = subprocess.run(
        [args.ghost_bin, "--task", task, "--state", str(state), "--local", str(ws),
         "--model", args.model, "--max-steps", str(MAX_STEPS), "--max-minutes", str(MAX_MINUTES)],
        capture_output=True, text=True, timeout=MAX_MINUTES * 60 + 120,
    )
    (state.parent / "agent.log").write_text(out.stderr)
    report = json.loads(out.stdout)
    usage = report["usage"]
    return {
        "ended": report["ended"]["why"],
        "steps": report["steps"],
        "read": usage["input"], "cached": usage["cached"], "wrote": usage["output"],
    }


def run_claude(args, ws: Path, home: Path, task: str):
    env = dict(os.environ)
    for name in ("CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT"):
        env.pop(name, None)
    env.update({
        "CLAUDE_CONFIG_DIR": str(home),
        "ANTHROPIC_BASE_URL": "https://api.deepseek.com/anthropic",
        "ANTHROPIC_AUTH_TOKEN": os.environ["DEEPSEEK_API_KEY"],
        "ANTHROPIC_MODEL": f"{args.model}[1m]",
        "ANTHROPIC_DEFAULT_OPUS_MODEL": f"{args.model}[1m]",
        "ANTHROPIC_DEFAULT_SONNET_MODEL": f"{args.model}[1m]",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL": args.model,
        "CLAUDE_CODE_SUBAGENT_MODEL": args.model,
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
    })
    home.mkdir(parents=True, exist_ok=True)
    out = subprocess.run(
        [args.claude_bin, "-p", task, "--output-format", "json",
         "--dangerously-skip-permissions", "--max-turns", str(MAX_STEPS)],
        cwd=ws, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True,
        timeout=MAX_MINUTES * 60 + 120,
    )
    (home.parent / "agent.log").write_text(out.stderr + "\n" + out.stdout[-4000:])
    result = json.loads(out.stdout)
    usage = result.get("usage", {})
    cached = usage.get("cache_read_input_tokens", 0)
    return {
        "ended": "error" if result.get("is_error") else result.get("subtype", "?"),
        "steps": result.get("num_turns", 0),
        "read": usage.get("input_tokens", 0) + usage.get("cache_creation_input_tokens", 0) + cached,
        "cached": cached, "wrote": usage.get("output_tokens", 0),
    }


def run_codex(args, ws: Path, home: Path, task: str):
    env = dict(os.environ)
    env["CODEX_HOME"] = str(home)
    home.mkdir(parents=True, exist_ok=True)
    provider = ('model_providers.deepseek={name="DeepSeek",base_url="https://api.deepseek.com",'
                'env_key="DEEPSEEK_API_KEY",wire_api="responses"}')
    out = subprocess.run(
        [args.codex_bin, "exec", "--json", "--skip-git-repo-check", "-C", str(ws),
         "-s", "workspace-write", "-m", args.model,
         "-c", 'model_provider="deepseek"', "-c", provider, task],
        cwd=ws, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True,
        timeout=MAX_MINUTES * 60 + 120,
    )
    (home.parent / "agent.log").write_text(out.stderr + "\n" + out.stdout[-6000:])
    steps = read = cached = wrote = 0
    ended = "no turn"
    for line in out.stdout.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        kind = event.get("type")
        if kind == "item.completed" and event["item"].get("type") in ("command_execution", "file_change", "mcp_tool_call"):
            steps += 1
        elif kind == "turn.completed":
            ended = "success"
            usage = event.get("usage", {})
            read += usage.get("input_tokens", 0)
            cached += usage.get("cached_input_tokens", 0)
            wrote += usage.get("output_tokens", 0)
        elif kind in ("turn.failed", "error"):
            ended = "error"
    return {"ended": ended, "steps": steps, "read": read, "cached": cached, "wrote": wrote}


def grade(exercise: Path, ws: Path, meta) -> bool:
    for name in meta["test"]:
        shutil.copy(exercise / name, ws / name)
    modules = [Path(name).stem for name in meta["test"]]
    try:
        out = subprocess.run([sys.executable, "-m", "unittest", *modules], cwd=ws,
                             capture_output=True, text=True, timeout=180)
    except subprocess.TimeoutExpired:
        return False
    (ws.parent / "tests.log").write_text(out.stdout + out.stderr)
    return out.returncode == 0


def one(args, exercise: Path):
    base = Path(args.out).resolve() / args.agent / exercise.name
    ws = base / "ws"
    meta = prepare(exercise, ws)
    task = task_for(ws, meta)
    started = time.time()
    try:
        if args.agent == "ghost":
            run = run_ghost(args, ws, base / "state", task)
        elif args.agent == "codex":
            run = run_codex(args, ws, base / "codex-home", task)
        else:
            run = run_claude(args, ws, base / "claude-home", task)
    except subprocess.TimeoutExpired:
        run = {"ended": "timeout", "steps": 0, "read": 0, "cached": 0, "wrote": 0}
    except (json.JSONDecodeError, KeyError) as e:
        run = {"ended": f"no report: {e}", "steps": 0, "read": 0, "cached": 0, "wrote": 0}
    run.update(name=exercise.name, agent=args.agent, seconds=round(time.time() - started, 1),
               passed=grade(exercise, ws, meta))
    print(f"{'pass' if run['passed'] else 'FAIL'}  {run['name']:<28} {run['seconds']:>6}s  "
          f"{run['steps']:>3} steps  {run['ended']}", flush=True)
    return run


# USD per million tokens, deepseek-flash off-peak, for the cost column.
PRICE = {"cached": 0.003, "fresh": 0.15, "wrote": 0.60}


def report(out: Path):
    rows = {}
    for path in sorted(out.glob("*/results.jsonl")):
        for line in path.read_text().splitlines():
            row = json.loads(line)
            rows.setdefault(row["agent"], {})[row["name"]] = row
    if not rows:
        print("no results yet")
        return
    names = sorted({name for agent in rows.values() for name in agent})
    agents = sorted(rows)
    print(f"{'exercise':<28} " + " ".join(f"{a:>10}" for a in agents))
    for name in names:
        cells = []
        for a in agents:
            r = rows[a].get(name)
            cells.append(f"{'-' if r is None else ('pass' if r['passed'] else 'FAIL'):>10}")
        print(f"{name:<28} " + " ".join(cells))
    print()
    for a in agents:
        rs = list(rows[a].values())
        n = len(rs)
        cost = sum(r["cached"] * PRICE["cached"] + (r["read"] - r["cached"]) * PRICE["fresh"]
                   + r["wrote"] * PRICE["wrote"] for r in rs) / 1e6
        print(f"{a}: {sum(r['passed'] for r in rs)}/{n} passed, "
              f"{sum(r['seconds'] for r in rs) / n:.0f}s and {sum(r['steps'] for r in rs) / n:.1f} steps per exercise, "
              f"{sum(r['read'] for r in rs) / n / 1000:.0f}k tokens read ({sum(r['cached'] for r in rs) * 100 // max(1, sum(r['read'] for r in rs))}% cached) "
              f"and {sum(r['wrote'] for r in rs) / n / 1000:.1f}k written per exercise, ${cost:.2f} in all")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--bench", help="a checkout of Aider-AI/polyglot-benchmark")
    p.add_argument("--out", required=True)
    p.add_argument("--agent", choices=["ghost", "claude", "codex"])
    p.add_argument("--model", default="deepseek-flash")
    p.add_argument("--ghost-bin", default=os.environ.get("GHOST_WORK_BIN", "target/release/ghost-work"))
    p.add_argument("--claude-bin", default="claude")
    p.add_argument("--codex-bin", default="codex")
    p.add_argument("--jobs", type=int, default=3)
    p.add_argument("--only", nargs="*", help="exercise names; default is all of them")
    p.add_argument("--report", action="store_true", help="print the table of what has been run")
    args = p.parse_args()
    out = Path(args.out).resolve()
    if args.report:
        return report(out)
    if not args.bench or not args.agent:
        p.error("--bench and --agent are needed to run")
    args.ghost_bin = str(Path(args.ghost_bin).resolve())
    exercises = sorted((Path(args.bench) / "python" / "exercises" / "practice").iterdir())
    if args.only:
        exercises = [e for e in exercises if e.name in args.only]
    (out / args.agent).mkdir(parents=True, exist_ok=True)
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        results = list(pool.map(lambda e: one(args, e), exercises))
    with open(out / args.agent / "results.jsonl", "w") as f:
        for row in results:
            f.write(json.dumps(row) + "\n")
    report(out)


if __name__ == "__main__":
    main()
