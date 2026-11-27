---
title: The thread and the composer
description: What the thread shows, how to write to an agent, choose its model and mode, and dictate.
---

# The thread and the composer

## The thread

The thread is the same for every agent:

- **What the agent says**, as Markdown. Mermaid diagrams (` ```mermaid `)
  are drawn natively, and with **Settings › Chat › Math** set to **Set as
  formulas**, so is TeX (`$…$`, `$$…$$`, ` ```math `).
- **Tool calls**, each a row with what it did: Read, Created, Edited,
  Deleted, Moved, Searched, Ran, Fetched, Ran a subagent, and so on. A row
  opens to its diff or output. A finished turn's calls fold into one line
  ("Read 3 files, ran 2 commands…"); a turn still running never folds.
- **The plan**, with how many steps are done, **Copy the plan**, and
  **Keep it as PLAN.md in the folder**.
- **Permission cards**: what the agent asks before it acts ("wants to run
  a command", "wants to edit a file", "proposed a plan"). The buttons are
  the agent's own options, and the default is the first that allows once.
  **No, and tell it…** rejects with a note; **Revise…** answers a plan
  with changes. Answered, the card becomes a line ("You allowed once").
- **Forms**: when an agent asks for input, the composer becomes a form,
  one question at a time, which you can **Decline**.
- **Subagents** at work or waiting get tabs over the thread; × hides a
  tab while the subagent goes on.

### The transcript view

⌃O, or the title's menu, goes round five views:

| View | Shows |
|---|---|
| Answers only | Your messages and the agent's answers. |
| Calls that went wrong | Also the calls that failed. |
| Normal (default) | Each run of calls folded into a line, answers in full. |
| Thinking | Also the agent's thinking. |
| Verbose | Every call and step. |

### The top bar

Click the title to rename the thread. Its menu has the transcript
views, **Continue with** another agent, **Open in your editor**, **Copy
the thread's id**, **Copy the pull request's address** and **Archive**.
The buttons beside it open the terminal drawer (⌘J) and show or hide the
right column (⌥⌘B).

### Find

⌘F finds in the thread: Return for the next match, ⇧Return for the
previous, Esc to close. ⌃⇧↑ and ⌃⇧↓ jump between your own messages.

### Turns: undo, restore, branch off

In a git repository, u0 checkpoints the files before and after each
turn (see [Worktrees and checkpoints](worktrees-and-checkpoints.md)).
Under a turn, while the thread is idle:

- **Diff** shows what the turn changed;
- **Undo** (the last turn) or **Restore to before** (an earlier one) puts
  the files back as they were before it;
- **Redo** puts the turn's changes back.

On each of your messages: **Branch off here, to say this again**, and
**Roll back to here** (with **files too** where the files can follow).

## The composer

Write a task or a question and press Return (see `send` in
[settings](reference/settings.md#chat-and-composer) for ⌘Return). ↑ and ↓
bring back messages you sent before.

### Chips

Under the text, chips show and change what the message goes with: the
project; for a new thread in a repository, **Worktree** and the branch
it starts **from**; the branch; the model; the mode; and a ring showing
how much of the agent's context is used.

### Attachments

Paste, drop or choose files and pictures (**+ › Add files or photos**),
or **Add a window** to attach a capture of a window. HEIC and very large
pictures are turned into JPEG. A paste of 32 KiB or more becomes an
attachment (⇧⌘V pastes it as text instead). A dropped folder is
@-mentioned. **+ › Add folder** gives a new thread another folder to
work in, where the agent supports it.

### @ mentions

Type `@` to name, from the project: a file, another thread (sent as its
conversation), an open pull request (sent as `gh` shows it), or a model
(which switches to it).

### / commands

Type `/` for the agent's own slash commands, and u0's: `clear`,
`usage-limits`, `model`, `branch` and `copy`.

### While the agent works

What you send while a turn goes on **waits its turn** by default: it is
queued and sent when the turn ends. The queue shows each message with
**Earlier**, **Send now** and **Edit**; ⌥↑ edits the last one. With
**Settings › Chat › What you send waits, or steers** set to **Steer**, a
message steers the turn in progress instead. ⌘-click Send, or ⇧⌘Return,
sends the other way.

**Say it later** (the clock chip) sends a message in 30 minutes, an hour,
3 hours, tomorrow at 9:00, or each day at 9:00. ⌘S puts a draft aside for
later.

With **Settings › Writing › Suggest what to say next** on, after each turn
the writer suggests your next message: Tab takes it, Esc dismisses it.

### Spelling

Misspelled words are underlined as you type, with the system's guesses on
their right-click menu. **Settings › Chat › Check spelling as you type**
turns it off.

### Dictation

⇧⌘D dictates into the composer with Apple's speech recognition, on this
Mac only. While it listens the composer shows **Listening…**; Esc
cancels. **Settings › Chat › Dictation** sets:

- **The dictation key (⇧⌘D)**: **Auto** (a press starts and a press stops;
  held, it listens until you let go), **Toggle**, or **Hold**;
- **Language**: the language listened for;
- **Dictionary**: words to listen for, one per line; `heard = written`
  writes what is heard otherwise;
- **Filler words**: left out (um, uh, er, hmm…) or kept;
- **Clean up**: as heard, or cleaned up by the writer.

**Edit › Speech › Start Speaking** reads aloud the text chosen in the
composer, or else the agent's last answer.

## Models and modes

⇧⌘M (or the model chip) opens the model picker: your favourites, the
agents, and each agent's models. Search with **Search models…**, star a
model to keep it among the favourites, and choose how hard it thinks
under **Effort** (or the agent's other **Options**). ⇧⌘E opens it at the
effort. Once a thread has begun, only its agent's models are offered.
**Send to all *n* agents** sends the message to every agent at once, each
in a thread of its own.

⇧⌘A opens the mode menu: the agent's modes (such as read-only, ask
first, auto), as the agent lists them, each with its description; ⌘1–⌘9
choose one.

Model ids of your own can be added on the agent's page (see
[First run](first-run.md#an-agents-page)). What you last chose for an
agent in a project is what its next new thread there starts with.

## Usage

⌘U opens **Usage**: what each thread cost and the tokens it took, by
agent, by model and by thread, over 24 hours, 7, 30 or 90 days, or all
(⇧⌘1 to ⇧⌘5). u0 computes nothing: it shows what the agents report.
**Limits** shows where each usage limit stands and when it resets, where
the agent says so (Claude's adapter does). A turn a limit stopped goes on
by itself when the limit resets (see
[Automation](automation.md#when-a-usage-limit-stops-a-turn)).

## Notifications

When a thread ends a turn, fails, or needs you while u0 is not in
front, macOS shows a notification (see `notify` and `notify_on` in
[settings](reference/settings.md#notifications-and-background)). In
front, u0 shows a notice at the top right instead.
