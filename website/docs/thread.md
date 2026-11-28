---
title: The thread and the composer
description: What the thread shows, how to write to an agent, choose its model and mode, and dictate.
---

# The thread and the composer

## The thread

The thread is the same for every agent:

- **What the agent says**, as Markdown. Mermaid diagrams (` ```mermaid `)
  are drawn natively, and with **Settings › Chat › Math** set to **Set as
  formulas**, so is TeX (`$…$`, `$$…$$`, ` ```math `). A link in it opens
  a web address in your browser, a `mailto:` address in your mail app,
  and a file of the thread's folder in the right column's viewer, as does
  a file that went with a message; any other link (an app, a script, a
  share, another app's address) is not opened, and u0 says so. A file the
  viewer cannot show is offered in Finder, never opened.
- **Tool calls**, each a row with what it did: Read, Created, Edited,
  Deleted, Moved, Searched, Ran, Fetched, Ran a subagent, and so on. A row
  opens to its diff or output. A finished turn's calls fold into one line
  ("Read 3 files, ran 2 commands…"); a turn still running never folds.
- **The plan**, with how many steps are done, **Copy the plan**, and
  **Keep it as PLAN.md in the folder**.
- **Permission cards**: what the agent asks before it acts ("wants to run
  a command", "wants to edit a file", "proposed a plan"), along with its
  structured arguments when it calls a u0 tool. The buttons are the
  agent's own options, and the default is the first that allows once.
  **No, and tell it…** opens a note on the card to reject once and send
  what to do instead; **Revise…** answers a plan with changes; sending in
  the composer while a card waits also rejects once and sends your
  message. Answered, the card becomes a line ("You allowed once").
  ⌘K offers the waiting request's allow-once option; anything that
  stands past it is chosen on the card, so where the agent offers no
  allow-once, ⌘K offers **Review the waiting request** and shows the
  card.
- **Forms**: when an agent asks for input, an inline question card appears
  right in the thread, one question at a time, which you can **Decline** while
  keeping the composer open below.
- **Subagents** the agent sets to work (Claude Code's Task and Agent
  tools, in the foreground or the background) are each a card that a
  finished turn never folds away: the kind of agent it is, what it was set
  to do, **Running**, **Needs you**, **Done**, **Failed** or **Stopped**,
  how long it took, **Open chat** and, while it runs, **Stop it**. Under
  the card, its calls fold into one line as a turn's do, and open to what
  it was told and each call; below them is what it answered. **Open chat**
  shows its whole transcript, starting from the task its agent gave it
  ("From the agent that set it to work"). Subagents at work or waiting
  also show a summary bar in the thread and get tabs over it; × hides a tab
  while the subagent goes on. A session loaded from Claude Code has no
  times for its subagents, so their cards show none.

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

Beside the title, the bar names the thread's project, its branch and the
server it runs on, and **Needs you** while it waits for an answer.
Click the title to rename the thread. Its menu has **Rename**, **Pin**,
**Settle**, **Snooze…**, **Open beside** (placing two threads side by side, or
press ⇧Return on a thread in ⌘K), **Branch a new thread from here**, **New
side chat**, **Copy thread link**, **Share read only with a device…**, **Link
a pull request…**, **Tags…**, the transcript views, **Continue with** another
agent, **Open in your editor**, **Archive** and **Delete…**.

In the middle of the bar, the box that reads **Search threads, projects,
commands and settings** opens the palette under it, as ⌘K does; when the
bar is short of room, only its magnifier is left. At the right, while
the folder has uncommitted changes, **Commit N files** opens the commit
form in the Diff tab; for a thread on this Mac, **Share this Mac
read-only…** makes a pairing link with which a device reads every project
and thread on this Mac, not this thread alone (as the title menu's
**Share read-only link…** does); the
last three buttons show or hide the sidebar, the terminal drawer (⌘J) and
the right column (⌥⌘B).

### Find

⌘F finds in the thread: Return for the next match, ⇧Return for the
previous, Esc to close. ⌃⇧↑ and ⌃⇧↓ jump between your own messages.

### Turns: undo, restore, branch off

Over each answer, a line names the agent and its model, how long the
turn took and when you asked; its buttons copy the answer, quote it in
your reply, or branch a new thread off after it.

In a git repository, u0 checkpoints the files before and after each
turn (see [Worktrees and checkpoints](worktrees-and-checkpoints.md)).
Under a turn that changed files, a card lists them with what each gained
and lost; a file, or **Show in diff**, opens the turn's changes in the
Diff tab. While the thread is idle:

- **Undo** (the last turn) or **Restore to before** (an earlier one) puts
  the files back as they were before it;
- **Redo** puts the turn's changes back.

An edit's own **Show in diff** shows just that edit.

### Plans

A plan the agent keeps shows each step done, under way or still to do,
and how many are done; copy it, or keep it as PLAN.md in the folder. In
a mode that only plans (as the agent lists its modes), its latest plan
has **Implement plan**, which switches to the mode the agent lists for
working and asks it to carry the plan out, and **Keep planning**, which
puts you in the composer, whose line reads **Ask for changes to the
plan**. A line over the composer names the mode and the keys (⇧⌘A) that
open the modes.

On each of your messages: **Branch off here, to say this again**, and
**Roll back to here** (with **files too** where the files can follow).

## The composer

Write a task or a question and press Return (see `send` in
[settings](reference/settings.md#chat-and-composer) for ⌘Return). ↑ and ↓
bring back messages you sent before.

### Chips

Under the text, chips show and change what the message goes with: for a
new thread, the project, and in a repository **Worktree** and the branch
it starts **from**; the model; the mode; and a ring showing how much of
the agent's context is used.

### Attachments

Paste, drop or choose files and pictures (**+ › Add files or photos**),
or **Add a window** to attach a capture of an open window (or choose one
on screen). Click a picture's thumbnail to see it at full size; Escape or
a click outside closes it. HEIC and very large pictures are turned into JPEG. A paste of
32 KiB or more becomes an attachment (⇧⌘V pastes it as text instead), and
a message over the length limit offers **Attach as file**. A dropped
folder is @-mentioned. **+ › Add folder** gives a new thread another
folder to work in, where the agent supports it.

### @ mentions

Type `@` to name, from the project: a file, another thread (sent as its
conversation), an open pull request or issue, or a model (which switches
to it).

### / commands

Type `/` for the agent's own slash commands, and u0's: `clear`,
`usage-limits`, `model`, `branch` and `copy`.

### While the agent works

What you send while a turn goes on **waits its turn** by default: it is
queued and sent when the turn ends. The queue shows each message with
**Earlier**, **Send now**, **Edit** (which edits it in place with **Save
to queue**) and **Remove from queue**; ⌥↑ edits the last one. A message
being edited keeps its place however the queue moves, and the queue does
not send it, or pass it, until you save or press Esc. With
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
Mac only. While it listens the composer shows what it hears; Esc
cancels. **Settings › Voice** sets:

- **Dictation**: the **Language** listened for; the **Dictation key** (its
  **Change** opens it in Keybindings); **How the key works**: **Auto** (a
  press starts and a press stops; held, it listens until you let go),
  **Toggle**, or **Hold**; and **Show words while you speak** (off, the
  composer only says it is listening);
- **Cleanup**: **Remove filler words**, the **Fillers** taken out (um,
  uh, er, hmm…); the **Correction cue** (“scratch that”), which takes back
  the sentence you said before it; **Tidy with the writer**, and the
  **Writer**, chosen in **Writing**;
- **Dictionary**: **Your words**, which dictation listens for; a word
  `heard = written` writes what is heard otherwise.

**Edit › Speech › Start Speaking** reads aloud the text chosen in the
composer, or else the agent's last answer.

## Models and modes

⇧⌘M (or the model chip) opens the model picker: **Favourites** and the
agents down its left, the tab's models in the middle (each with what its
agent says of it, ⌘1–⌘9 beside the first nine), that agent's options at
the right (another agent's tab shows what a thread of it begins with, or
none until it has said; a search, or **Favourites**, shows the thread's
own), and **Agent settings** under the tabs. On a new thread, changing
another agent's option moves the thread to that agent, as choosing its
model does. An agent that is
starting, installing, not installed, failed or signed out says so on its tab,
and one that needs you has a button above its models (**Agent settings** or
**Sign in**); choosing a tab clears the search. In a narrow window the tabs show only the
agents' marks. Search with **Search models…** (across every agent), star a
model to keep it among the favourites, and choose how hard it thinks under
the agent's own name for it (Codex's **Reasoning effort**). A speed option
the agent offers, such as Codex's **Fast mode**, is a switch under
**Speed**, and a bolt beside the effort chip says it is on; any other
option is a row that opens a menu of its choices. ⇧⌘E opens the picker
too. Once a thread has begun, only its agent's models are offered.
On a new thread, **+** beside any model adds it to **Also run on** (and
**Send to all *n* agents** adds every usable agent), starting a thread
and a worktree for each: adding one turns **Worktree** on, turning
**Worktree** off leaves the thread on its own agent, and outside a git
repository it stays on one, so several agents never work in one folder.
**Start in the background** leaves you on a fresh draft after sending.

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
front, u0 shows a notice at the top right instead. It goes by itself
after a few seconds (it waits while the pointer is on it); one that says
a thread needs you stays until you answer.
