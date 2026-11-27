---
title: What u0 is
slug: /
description: u0 is one desktop app for the coding agents you already use.
---

# What u0 is

u0 is one desktop app for the coding agents you already use: Claude
Code, Codex, Antigravity, and every other agent that speaks the
[Agent Client Protocol](https://agentclientprotocol.com) (ACP), side by
side in one window, drawn the same way.

u0 is **not** another agent. It has no model, no prompts and no agent
of its own. Each agent runs as its own program, with its own tools, its
own sign-in and its own threads, exactly as it would in its CLI. u0
starts it, speaks ACP to it, and shows what it does. When u0 itself
needs words written (a thread's title, a commit message), it asks one of
your agents.

## The window

The window has three columns:

- **The sidebar**, on the left: your projects (folders), with each
  project's threads under it, whichever agent each is with; pinned
  threads at the top; snoozed and settled threads on shelves at the
  foot. At the very foot: Settings, Pull requests, Issues, Usage, and your
  agents.
- **The thread**, in the middle: what you said and what the agent did:
  its answers in Markdown, each tool call as a row that opens to its diff
  or output, its plan, and the questions it asks before it acts. The
  composer is at the bottom.
- **The right column**: what the thread came to: the diff, the files, the
  pull request, terminals, simulators, a side chat and the thread's
  details.

## What you can do

- [Install](install.md) u0 and [switch on your agents](first-run.md);
  u0 installs the ones that are missing, from the ACP registry.
- Start [threads in projects](projects-and-threads.md), and follow them
  in [the thread](thread.md): models, modes, @-mentions, slash commands,
  dictation.
- Let a thread work in [a worktree of its own](worktrees-and-checkpoints.md),
  and undo any turn.
- [Review](review.md) what changed, comment on lines for the agent, then
  commit, push and open a pull request.
- Work through your [pull requests and issues](forges.md) on GitHub,
  GitLab, Bitbucket, Azure DevOps and Linear.
- Drive [iOS simulators and Android emulators](simulators.md).
- Run prompts on a schedule or from a webhook, and chain steps into
  [workflows](automation.md).
- Give agents [u0's tools](tools-for-agents.md) over MCP.
- Use one machine's u0 [from another Mac](remote.md).

## How it works

Every agent is spoken to over ACP: JSON-RPC, one message a line, on the
standard input and output of a program u0 starts. The
[ACP registry](https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json)
is the one list of agents: their names, versions, logos and how each is
distributed. u0 reads it, keeps a copy, and installs and starts agents
the way it says.

u0 is two parts: a **server**, which runs the agents, keeps the
threads and touches the machine (files, git, terminals), and the
**window**, which draws it. In the app both run together; the server can
also run alone (`u0 serve`, `u0-server` on Linux) for windows on
other Macs to use.

u0's source is at [github.com/u0-code/u0](https://github.com/u0-code/u0),
under the Apache-2.0 licence.
