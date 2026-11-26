---
title: Scheduled tasks, workflows and webhooks
description: Run prompts on a schedule or from a webhook, and chain agents, commands and questions into workflows.
---

# Scheduled tasks, workflows and webhooks

Ghost's server runs tasks and workflows, not the window. They keep running
while the window is closed, as long as the server runs: in the app while
it is open, or all the time with `ghost service install` (see
[CLI](reference/cli.md#the-launchd-service)) or `ghost-server`.

## Scheduled tasks

A scheduled task says a prompt to an agent, in a project, on a schedule.
Each run is a session of its own (or the next turn of one session) and
shows in the sidebar like any other.

Open **Settings › Scheduled tasks** and choose **New task**:

| Field | Meaning |
|---|---|
| **Name** | The task's title. Left empty, the first line of the prompt (40 characters). |
| **Prompt** | What is said to the agent each time. |
| **Agent** | Who it is said to, and with which model. |
| **Project** | Where it works. |
| **Schedule** | **Every** (minutes, or a duration such as `2h`), **Daily** (a time, and the days of the week; none chosen means every day), **Once** (such as `tomorrow`, `5pm`, `in 3h`), or **On webhook**. |
| **Each run** | **A new session each run**, or **One session, continued** (each run is the next turn of the first run's session). |
| **In a worktree of its own** | With a new session each run: each run works on a new branch, away from your checkout. |

A new task starts as Daily at 09:00 on weekdays. Each task's ⋯ menu
has **Edit**, **Run now** and **Delete**, and its switch pauses it.

How runs behave:

- The server looks for due tasks every 20 seconds.
- A run is skipped while the previous run is still going.
- An **Every** task first runs one full interval after it is saved.
- A **Once** task removes itself after it runs successfully.
- A finished or failed run notifies you as any turn does (see
  `notify_on` in [settings](reference/settings.md#notifications-and-background)),
  and its session is marked unread.
- A failed run shows its error under the task.

`ghost tasks` lists the tasks from a terminal.

### tasks.json

Tasks are kept in `<home>/tasks.json`, written by the server. You can
edit it while Ghost is closed:

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
| `model`, `mode` | string or `null` | The model, and the session mode, to set. `null`: the agent's default. |
| `project` | path | |
| `schedule` | object | `{"every": minutes}`, `{"daily": {"minute": minutes after midnight, "days": [0–6, 0 is Sunday]}}` (an empty `days` is every day, in local time), `{"once": Unix seconds}`, or `{"hook": "<token>"}`. |
| `paused` | bool | |
| `same` | bool | One session, continued. |
| `worktree` | bool | Each run in a new worktree. |
| `session`, `next`, `last`, `runs` | | Kept by the server: the session it continues, the next run, the last run's result, the number of runs. |

An unknown field is an error. A file that does not read is left alone,
and no task runs or is saved until it is fixed or moved away.

### When a usage limit stops a turn

When an agent stops a turn because a usage limit was reached and says
when the limit resets, Ghost schedules the turn to go on then: a one-time
task that says "Continue" in the same session. The error card says when it
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
  It is the only thing that protects the address: anyone who has it can
  run the task. Keep it secret.
- The address is served where Ghost listens for other devices, so
  listening must be on (**Settings › Devices**, see
  [Remote control](remote.md)). The form shows
  `http://127.0.0.1:<port>/hook/<token>` and, while a tunnel is up,
  `https://<tunnel host>/hook/<token>`.
- The request body (up to 1 MiB) replaces each `{{body}}` in the prompt,
  wrapped in `<webhook-data untrusted>` … `</webhook-data>` so the agent
  knows it came from outside. Without `{{body}}` in the prompt the body is
  not used.
- Ghost answers `202 Accepted` when the task runs, and `404 Not Found` for
  an unknown token, a paused task, or when a [policy](reference/policy.md)
  forbids webhooks.
- A call made while Ghost is not running is not kept.

:::note Coming

Signed webhooks (an HMAC secret, filters, replay protection and rotation),
tasks that start a workflow or run on a session's events, and webhook
signing in the form are being finished and are not in the current build
yet.

:::

## Workflows

A workflow is a run in stages: steps that prompt an agent, run a command,
or ask you, one after another. The server runs it as one run that
survives a restart: each step's result is kept as the step ends, and a
step that fails pauses the run there.

Each workflow is a JSON file of yours in `<home>/workflows/`, named by its
file name (`release.json` is the workflow `release`). Ghost reads the
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
    { "run": { "name": "tag", "command": "git tag {{input}}" } }
  ]
}
```

Top level:

| Key | Meaning |
|---|---|
| `about` | A line shown with the workflow. |
| `agent` | The agent for agent steps that name none. |
| `project` | The folder for steps that name none. |
| `steps` | The steps, in order. Each step's `name` must be unique. |

Steps:

| Step | Keys | What it does |
|---|---|---|
| `agent` | `name`, `prompt`; optional `agent`, `model`, `project`, `worktree`, `after` | Says `prompt` to an agent in a new session titled "workflow · step". With `after` (an earlier agent step's name) it goes on in that step's session instead. The step's result is the agent's last answer. |
| `run` | `name`, `command`; optional `dir` | Runs `command` with `/bin/sh -c` in `dir` (or the project). The result is its standard output. A non-zero exit pauses the run with its error output. |
| `ask` | `name`, `title`, `choices`; optional `stop` | Asks you, with one button per choice. Your choice is the step's result. A choice listed in `stop` stops the run. |

Values: `{{input}}` is the run's input, and `{{<step name>}}` is an
earlier step's result (trimmed). In a `run` command each value is quoted
for the shell as one word, so it is never run as code.

### Running a workflow

In **Settings › Workflows**, type an input (optional) and press **Start**
beside the workflow. Under **Runs** you can show those that need you,
those going, or all. A run shows each finished step with the first line of
its result and **Open** (its session); a run waiting on an `ask` step
shows its choices; a paused run shows why, with **Resume** (run the step
again). **Cancel** stops a run and the turn in progress.

Runs are kept in `<home>/runs.json`. A run that was going when the
server stopped continues when it starts again.
