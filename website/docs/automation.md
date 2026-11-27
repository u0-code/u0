---
title: Scheduled tasks, workflows and webhooks
description: Run prompts on a schedule or from a webhook, and chain agents, commands and questions into workflows.
---

# Scheduled tasks, workflows and webhooks

u0's server runs tasks and workflows, not the window. They keep running
while the window is closed, as long as the server runs: in the app while
it is open, or all the time with `u0 service install` (see
[CLI](reference/cli.md#the-launchd-service)) or `u0-server`.

## Scheduled tasks

A scheduled task says a prompt to an agent, in a project, on a schedule.
Each run is a thread of its own (or the next turn of one thread) and
shows in the sidebar like any other.

Open **Settings › Scheduled tasks** and choose **New task**:

| Field | Meaning |
|---|---|
| **Name** | The task's title. Left empty, the first line of the prompt (40 characters). |
| **Prompt** | What is said to the agent each time. |
| **Agent** | Who it is said to, and with which model. |
| **Project** | Where it works. |
| **Each run** | **Says its prompt** to the agent, or **Begins** one of your [workflows](#workflows), the prompt (filled as a webhook's or an event's is) its input. |
| **Schedule** | **Every** (minutes, or a duration such as `2h`), **Daily** (a time, and the days of the week; none chosen means every day), **Once** (such as `tomorrow`, `5pm`, `in 3h`), **On webhook**, or **On an event**. |
| **Each run** | **A new thread each run**, or **One thread, continued** (each run is the next turn of the first run's thread). |
| **In a worktree of its own** | With a new thread each run: each run works on a new branch, away from your checkout. |

A new task starts as Daily at 09:00 on weekdays. Each task's ⋯ menu
has **Edit**, **Run now** and **Delete**, and its switch pauses it.

How runs behave:

- The server looks for due tasks every 20 seconds.
- A run is skipped while the previous run is still going.
- An **Every** task first runs one full interval after it is saved.
- A **Once** task removes itself after it runs successfully.
- A finished or failed run notifies you as any turn does (see
  `notify_on` in [settings](reference/settings.md#notifications-and-background)),
  and its thread is marked unread.
- A failed run shows its error under the task.

`u0 tasks` lists the tasks from a terminal.

### On an event

A task **On an event** runs as something happens in a thread of yours:
**A turn ends**, or **An agent asks** you something. **In this task's
project alone** limits it to threads in the task's project. It runs only
on your turns, never on a turn u0 began on its own (a task's, a
workflow's, or one an agent said in another thread), so no task runs on
its own runs. A thread an agent once said something in is still yours:
your next turn there runs the task again. `{{event}}` in the
prompt is what happened, marked as data from outside (`<event
untrusted>`).

### tasks.json

Tasks are kept in `<home>/tasks.json`, written by the server. You can
edit it while u0 is closed:

```json
{
  "tasks": [
    {
      "id": 1,
      "title": "Morning triage",
      "prompt": "Look at the issues opened since yesterday",
      "agent": "claude-acp",
      "project": "/Users/me/app",
      "schedule": { "daily": { "minute": 540, "days": [1, 2, 3, 4, 5] } },
      "worktree": true
    },
    {
      "id": 2,
      "title": "CI watch",
      "prompt": "Check the CI of main",
      "agent": "codex-acp",
      "project": "/Users/me/app",
      "schedule": { "every": 30 },
      "paused": true
    }
  ]
}
```

| Field | Type | Meaning |
|---|---|---|
| `id` | number | The task's number. |
| `title`, `prompt` | string | |
| `agent` | agent id | |
| `model`, `mode` | string or `null` | The model, and the mode, to set. `null`: the agent's default. |
| `project` | path | |
| `schedule` | object | `{"every": minutes}`, `{"daily": {"minute": minutes after midnight, "days": [0–6, 0 is Sunday]}}` (an empty `days` is every day, in local time), `{"once": Unix seconds}`, or `{"hook": {"token": "<token>", "signed": bool, "only": [{"at": "body.action", "is": ["opened"]}]}}`, or `{"on": {"moment": "turn_ended" or "asks", "here": bool}}`. |
| `workflow` | string or `null` | The workflow each run begins, its filled prompt the input. |
| `paused` | bool | |
| `same` | bool | One thread, continued. |
| `worktree` | bool | Each run in a new worktree. |
| `session`, `next`, `last`, `runs` | | Kept by the server: the thread it continues, the next run, the last run's result, the number of runs. |

An unknown field is an error. A file that does not read is left alone,
and no task runs or is saved until it is fixed or moved away.

### When a usage limit stops a turn

When an agent stops a turn because a usage limit was reached and says
when the limit resets, u0 schedules the turn to go on then: a one-time
task that says "Continue" in the same thread. The error card says when it
goes on, with **Don't go on** to cancel it. Turn this off with **General ›
In the background › Go on when a limit resets** (`resume_at_reset`).

This covers turns you started from the window. Turns run by a task or a
workflow are not resumed this way.

## Webhooks

A task whose schedule is **On webhook** runs when something sends an HTTP
`POST` to its address:

```
POST /hook/<token>
```

- The token is 22 random characters made when you choose **On webhook**.
  **New address** gives the task a new one: the old address runs it no
  more.
- **Signed** (on for a new webhook): only a call whose body is signed with the task's secret
  runs it: the body's HMAC-SHA256, in hex, in `X-Hub-Signature-256`
  (`sha256=…`, as GitHub signs) or `X-Signature-256`. Type the secret as
  the sender has it; it is kept in the Keychain of the Mac the task runs
  on (`hook/<token>`), and moves with **New address**. Unsigned, the token
  is the only thing that protects the address: anyone who has it can run
  the task.
- The address is served where u0 listens for other devices, so
  listening must be on (**Settings › Devices**, see
  [Remote control](remote.md)). The form shows
  `http://127.0.0.1:<port>/hook/<token>` and, while a tunnel is up,
  `https://<tunnel host>/hook/<token>`.
- **Only when** narrows the calls that run it: rules such as
  `body.action = opened, closed; header.x-github-event = pull_request`,
  each a field of the JSON body (`body.<a>.<b>`) or a header
  (`header.<name>`) and the values it may be, separated by `;`. Every rule
  must hold. Empty, every call runs it.
- A delivery its sender names (`X-GitHub-Delivery`,
  `X-GitLab-Event-UUID` or `X-Delivery-Id`) runs it once; sent again, it
  runs nothing. A signed body sent again runs nothing either, whatever
  delivery it names and however its signature is written. What was heard
  is kept for a month in `<home>/heard.json`, so a restart forgets none.
  After a month the same signed body runs again: a GitHub-style signature
  carries no time, so nothing tells an old call from a new one.
- The request body (up to 1 MiB) replaces each `{{body}}` in the prompt,
  `{{body.<a>.<b>}}` a field of it and `{{header.<name>}}` a header, each
  wrapped in `<webhook-data untrusted>` … `</webhook-data>` so the agent
  knows it came from outside; a closing tag in what was sent is written
  `<\/…`, so only u0's ends the mark. It can still try to steer the
  agent: read it as you would anything pasted in. Without them in the
  prompt the body is not used.
- u0 answers `202 Accepted` when the task runs; `204 No Content` when
  a rule of **Only when** does not hold; `200 OK` for a delivery heard
  before; `401 Unauthorized` for a call not signed as it must be;
  `413` for a body too long; and `404 Not Found` for an unknown token, a
  paused task, or when a [policy](reference/policy.md) forbids
  webhooks.
- A call made while u0 is not running is not kept.


## Workflows

A workflow is a run in stages: steps that prompt an agent, run a command,
or ask you, one after another. The server runs it as one run that
survives a restart: each step's result is kept as the step ends, and a
step that fails pauses the run there.

Each workflow is a JSON file of yours in `<home>/workflows/`, named by its
file name (`release.json` is the workflow `release`). u0 reads the
files each time it lists or starts one.

```json
{
  "about": "Prepare a release",
  "agent": "claude-acp",
  "project": "/Users/me/app",
  "steps": [
    { "agent": { "name": "notes", "prompt": "Write release notes for {{input}}" } },
    { "agent": { "name": "polish", "prompt": "Make these shorter: {{notes}}", "after": "notes" } },
    { "ask": { "name": "ship", "title": "Ship {{input}}?", "choices": ["Ship", "Hold"], "stop": ["Hold"] } },
    { "run": { "name": "tag", "command": "git tag -m \"Release {{input}}\" -- {{input}}" } }
  ]
}
```

Top level:

| Key | Meaning |
|---|---|
| `about` | A line shown with the workflow. |
| `agent` | The agent for agent steps that name none. |
| `project` | The folder for steps that name none. |
| `worktree` | When `true`, each run creates an isolated git worktree of `project` that all steps in that run share. |
| `steps` | The steps, in order. Each step's `name` must be unique. |

Steps:

| Step | Keys | What it does |
|---|---|---|
| `agent` | `name`, `prompt`; optional `agent`, `model`, `mode`, `project`, `worktree`, `after` | Says `prompt` to an agent in a new thread titled "workflow · step". With `after` (an earlier agent step's name) it goes on in that step's thread instead. The step's result is the agent's last answer. |
| `run` | `name`, `command`; optional `dir`, `continue_on_error` | Runs `command` with `/bin/sh -c` in `dir` (or the run's worktree / project). The result is its standard output. A non-zero exit pauses the run with its error output unless `continue_on_error` is `true`. |
| `ask` | `name`, `title`, `choices`; optional `stop` | Asks you, with one button per choice. Your choice is the step's result. A choice listed in `stop` stops the run. |

Values: `{{input}}` is the run's input, and `{{<step name>}}` is an
earlier step's result (trimmed). In a `run` command each value is given
to `/bin/sh` as an argument, and each `{{name}}` becomes a reference to
it, quoted for where it stands (bare, in double quotes or in single
quotes): the shell never reads a value as words of the command.

A command can still read a word as code itself, so u0 reads the
command as bash would (with [brush-parser](https://crates.io/crates/brush-parser))
and takes a `{{name}}` only as an argument of a command named plainly:
`echo`, `printf` (not as its format, and not with `-v`), `cd` or a
program, bare, in quotes or within `$( )`. A command that puts one
anywhere else is refused (the run pauses, saying why), as is one that makes an alias or holds a
here-document (where `/bin/sh` may read a command otherwise than the
parser does): after another
builtin (`eval`, `let`, `declare`, `export`, `read`, `test`, `trap`,
`source`, ...), as the command itself, in arithmetic (`$(( ))`, `$[ ]`,
`(( ))`, an array index, `${x:offset}`), within `[[ ]]`, in an
assignment, a redirection, a here-document, a `case` pattern or a
function, and in a command that does not parse. Give a value a command works on after `--`
(`git fetch origin -- {{ref}}`), so one that begins with `-` is not taken
for an option; and a command that runs its words as code (`sh -c`,
`ssh`, `xargs`) runs a value as code. **Settings › Workflows** says so
under **Values in commands**.

### Editing and running a workflow

Open **Workflows** from the sidebar (or **Settings › Workflows**). Choose **New workflow** or **Edit** on any workflow tab to add, reorder or edit agent, command and question steps in place, or press **Run now** to start a run.

The **Runs** column lists every run of the selected workflow; selecting a run shows its step-by-step timeline:
- Finished agent steps link to their thread; finished commands show their duration.
- A failed step shows its exit code and output, with **Fix in a thread** (opens a thread in the run's worktree with the failure context), **Re-run from step N** (truncates completed steps back to step *N* and resumes), and **Full log**.
- A step waiting on an `ask` question shows its choice buttons, and **Cancel** stops an active run.

Runs are kept in `<home>/runs.json`. A run that was going when the
server stopped continues when it starts again.
