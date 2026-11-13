# Results

Run on 2026-11-13, on one Apple-silicon laptop, every agent on DeepSeek
`deepseek-flash`.

## Aider polyglot, Python exercises (34), on this machine

### Correction to the first run

The table that stood here said the tests were hidden from the agents. They
were not, in effect. Both agents had a shell with the network behind it, and
in about half the exercises each of them went and got the exercise's real
tests: downloaded from the upstream repository, or, once, found in the
benchmark checkout on disk. Searching each run's record for the test file
(`saw_tests` in `bench/polyglot.py`) gives:

| First run, 2026-11-13 | `ghost-work` | Claude Code 2.1.322 |
|---|---|---|
| Exercises where it got the real tests | 17 of 34 | 14 of 34 |
| Passed at the first try, all 34 | 25 | 26 |
| Passed within two tries, all 34 | 33 | 34 |
| Passed at the first try, the 13 exercises neither saw the tests of | 11 | 11 |
| Passed within two tries, those 13 | 12 | 13 |

So the pass rates of the first run say little, and thirteen exercises are
too few to rank two agents on. What the first run does show:

- The explanation given here for `ghost-work` being twice as slow was wrong.
  It was put down to the check and review at the end. Those cost about 3 of
  17 steps. Most of the difference was 22 shell commands that searched the
  whole disk for the tests (`find / -name ...`) and ran into the two-minute
  limit: 2,307 of the 2,429 seconds its tools took in all.
- The reviewer passed all 34 first tries, including the 9 that then failed
  the tests. On exercises like these, where what fails is a guess about
  something the instructions do not say, it has nothing to go on.

The task text now forbids looking for the tests, the report marks every
exercise where an agent got them anyway, and those are left out of the pass
rates. The second run, below, is with that in place.

### Second run

Stopped partway: `ghost-work` on 26 of the 34 exercises, first tries only,
and no other agent. Not a result, but two things in it are worth keeping.

- With the tests really out of reach (one exercise of the 26 still went and
  got them), 10 of 26 passed at the first try. The failures are of one kind:
  the instructions do not say what the tests expect, and it guessed.
- The reviewer gave no verdict in 11 of the 26 runs. Six times the response
  could not be read off the connection, five times the model spent its whole
  output allowance thinking and wrote nothing. One run ended the same way in
  the loop itself. Requests are not streamed and the allowance (8,192 tokens)
  was set before these models thought at length; both need to change before
  this benchmark is run again.

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
