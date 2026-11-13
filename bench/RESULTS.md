# Results

Run on 2026-11-13, on one Apple-silicon laptop, every agent on DeepSeek
`deepseek-flash`.

## Aider polyglot, Python exercises (34), on this machine

`bench/polyglot.py`. Tests hidden from the agent; two tries, the second after
being shown what the tests printed. Same task text and the same limits (60
steps, 8 minutes a try) for both agents.

| | `ghost-work` | Claude Code 2.1.322 |
|---|---|---|
| Passed at the first try | 25 / 34 | 26 / 34 |
| Passed within two tries | 33 / 34 | 34 / 34 |
| Median time per exercise, both tries | 177 s | 91 s |
| Steps per exercise, both tries | 21.9 | 14.9 |
| Tokens read per exercise (cached) | 324k (93%) | 265k (92%) |
| Tokens written per exercise | 20.0k | 13.1k |
| Cost of the whole run | $0.55 | $0.39 |

What this says:

- On pass rate the two are level. One exercise apart on 34 is inside the
  noise: the nine or so exercises each agent missed at the first try are
  nearly all cases where the instructions do not say what the hidden tests
  expect (a list or a string, cents or dollars, the exact wording of an
  error), and which way an agent guessed.
- `ghost-work` takes about twice as long and costs about 1.4 times as much.
  Most of that is deliberate: it will not accept its own first claim of being
  finished, and a reviewer then reads the evidence. On short exercises like
  these that is overhead with nothing to catch. Whether it pays for itself on
  long work is what this benchmark cannot show.
- The one exercise `ghost-work` did not get in two tries is `beer-song`: on
  the second try it returned a list of verses where the tests want a flat
  list of lines.

Caveats:

- Claude Code's token counts are low by a little: three of its eight second
  tries lost their counts to a crash in the benchmark script, since fixed.
  Its pass results were regraded from the files it left and are not affected.
- The laptop slept for some hours in the middle of Claude Code's run, which
  is why times are given as medians.
- `ghost-work`'s prompt was changed once after two exercises had been seen
  failing in a smoke test (to compare output with expected text mechanically,
  and to give its reviewer room to answer). The numbers above are from the
  full run after that change.
- Codex 0.133 was also started on the same exercises through DeepSeek's
  Responses API and passed 25 of 34 at the first try. Its run was stopped
  before its second tries finished and is not in the table.

## A long task, not code

One run, not a comparison. The task and the report it produced are in
[`samples/`](samples/): compare five agent runtimes on four points each, from
primary sources, every claim followed by the page it was read on.

- 160 steps in 6.5 minutes: it used its whole step allowance and was stopped
  before it called `done`. The report was complete on disk by then.
- 2459 words, 31 cited URLs, all five runtimes and the comparison table.
- Old tool output was dropped three times; once that was not enough, and it
  wrote itself a handoff and went on from a fresh context.
- 5.7M tokens read, 96.5% of them from cache: about $0.09.

Nobody has checked the report's claims against its sources one by one.

## Terminal-Bench 2.1, in containers

Development runs only, `ghost-work` alone, no other agent: `fix-git`,
`overfull-hbox`, `log-summary-date-ranges`, `break-filter-js-from-html`,
`fix-code-vulnerability`. All five passed. The fifteen-task comparison
described in the README has not been run: on this laptop the task images are
emulated, Harbor's reference agent could not finish installing tmux inside
its fixed two minutes, and the work moved to benchmarks that run without
containers.
