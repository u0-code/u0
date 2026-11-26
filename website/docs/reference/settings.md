---
title: settings.json
description: Every key of ~/.ghost/settings.json, its type, default and where to change it.
---

# settings.json

Ghost keeps its settings in `settings.json` in its home (`~/.ghost`, or
`GHOST_HOME`). Almost every key has a control in **Settings** (⌘,), and
changing it there writes the file. You can also edit the file while Ghost
is closed.

Rules for the file:

- A key Ghost does not know is an error (the file is read with
  `deny_unknown_fields`). Ghost then starts with the defaults, says why
  the file did not read, and **saves nothing** until you fix the file or
  move it away. It never writes over a file it could not read.
- A key left out takes its default.
- An organisation's [policy](policy.md) can set a key's default, or lock
  it. A locked setting shows disabled, with who manages it.
- In a window that controls several Macs, each server's settings are its
  own `settings.json`: the settings dialog shows a server picker at its
  top, and what you change is saved on that server. The look, the keys
  and the window's own state stay this Mac's.

## Agents

| Key | Type | Default | Meaning |
|---|---|---|---|
| `enabled` | list of agent ids | `["claude-acp", "codex-acp", "antigravity-acp"]` | The agents switched on, in the order listed. **Settings › Agents**. |
| `commands` | map: agent id → command line | `{}` | A command line of yours that starts an agent instead of the registry's way (your own build, a version manager's). On the agent's page. |
| `env` | map: agent id → (name → value or `null`) | `{}` | Variables each agent starts with. `null` means the value is a secret kept in the Keychain. On the agent's page. |
| `models` | map: agent id → list of model ids | `{}` | Model ids of your own, offered beside those the agent lists. |
| `favourites` | list of `{agent, model}` | `[]` | Models starred in the model picker, listed first there. |
| `outside_acts` | bool | `false` | Whether an agent outside Ghost may say something in a session or stop its turn through [Ghost's tools](../tools-for-agents.md#access-levels), each asked of you there. **Settings › Agents › Agents outside Ghost may act**. |
| `no_tools` | list of agent ids | `[]` | Agents whose sessions are not given [Ghost's tools](../tools-for-agents.md). The switch is on the agent's page. |
| `check_agents` | bool | `true` | Read the ACP registry again every hour for newer agent versions. **General › In the background**. |

## Projects

| Key | Type | Default | Meaning |
|---|---|---|---|
| `folders` | list of paths | `[]` | The projects, the latest added first. |
| `prefs` | map: folder → project prefs | `{}` | What you set for each project (below). **Settings › Projects**. |
| `projects_in` | path or `null` | `null` (`~/Documents/Ghost`) | Where new and cloned projects go. |
| `host_folders` | bool | `false` | A clone goes in `<host>/<owner>/<repo>` under that folder, as `ghq` lays them out. |
| `fetch_every` | minutes or `null` | `5` | How often each project's remote is fetched. `null`: never. **General › In the background**. |

Each entry of `prefs`:

| Key | Type | Default | Meaning |
|---|---|---|---|
| `name` | string or `null` | `null` | The name shown instead of the folder's. |
| `pinned` | bool | `false` | Pinned at the top of the sidebar. |
| `archived` | bool | `false` | Archived (out of the sidebar). |
| `agent` | agent id or `null` | `null` | The agent its new sessions begin with. |
| `worktree` | bool or `null` | `null` | Whether its new sessions work in a worktree. `null`: the last choice made anywhere. |
| `pull` | bool | `false` | Pull its default branch as Ghost starts and on each fetch, when it is checked out with nothing changed. |
| `icon` | `{"picture": path}` or `{"mark": {"text", "colour"}}` or `null` | `null` | Its icon. `null`: its own icon file where it has one. |
| `linear` | Linear team key or `null` | `null` | Read its issues from this Linear team instead of its repository's host. |
| `over` | object | `{}` | Settings this project holds otherwise than everywhere else. Only `merge`, `uncredit`, `submodules`, `branch_kind`, `resume_at_reset` and `tidy_after` may appear here; any other key is an error. |
| `last` | map: agent id → `{options, mode}` | `{}` | The options (model, effort, …) and mode last chosen in a new session of each agent here. |

## Sessions

| Key | Type | Default | Meaning |
|---|---|---|---|
| `settle_days` | days or `null` | `7` | A session left this long is marked done. `null`: never. **General › Done for now**. |
| `settle_merged` | bool | `true` | A session whose pull request merged or closed is done. |
| `keep_with_pr` | bool | `false` | A session with an open pull request is never done, however long it is left. |
| `snoozes` | list of strings | `["1h", "tomorrow", "next week"]` | The snooze choices offered first, each as you would type it in **Custom…**. |
| `resume_at_reset` | bool | `true` | A turn an agent's usage limit stopped goes on by itself when the limit resets, where the agent says when. **General › In the background**. |
| `sort` | `"used"`, `"updated"`, `"begun"`, `"name"` | `"used"` | How the sidebar orders sessions and projects: by your last message, by any activity, by when it began, or by name. |
| `views` | list of `{name, kind, state, part, search}` | `[]` | Saved views of the PRs and Issues pages. `kind` is `"Pr"` or `"Issue"`. |

## Worktrees and git

| Key | Type | Default | Meaning |
|---|---|---|---|
| `worktree` | bool | `false` | Whether a new session in a git repository works in a worktree of its own: the choice last made. **New sessions › In a repository**. |
| `branch_prefix` | string | `"ghost/"` | What a worktree's branch name begins with, before the name the writer gives it. |
| `branch_kind` | bool | `false` | Begin the branch with the kind of work instead (`feat/`, `fix/`). |
| `worktrees` | path or `null` | `null` (`<home>/worktrees`) | Where new worktrees are made. Those made before stay where they are. |
| `submodules` | `"recursive"`, `"top"`, `"skip"` | `"recursive"` | How far a new worktree's submodules are checked out. |
| `tidy_after` | days or `null` | `null` | Days an archived session's worktree is left before it is removed. A worktree with uncommitted changes stays, and its branch always does. `null`: kept. |
| `merge` | `"squash"`, `"merge"`, `"rebase"` | `"squash"` | How a pull request is merged from the right column. **General › Pull requests**. |
| `uncredit` | bool | `false` | Take agents' `Co-authored-by` lines and credit footers out of a squashed commit's message. **Source control › Merging**. |

## Chat and composer

| Key | Type | Default | Meaning |
|---|---|---|---|
| `send` | `"return"`, `"multiline"`, `"command"` | `"return"` | What sends: Return; Return while the message is one line and ⌘Return after; or ⌘Return always. Where Return does not send, it breaks the line. **Chat › Sending**. |
| `steer` | bool | `false` | What you send while a turn goes on steers it, instead of waiting for the turn to end. |
| `spell_check` | bool | `true` | Underline misspelled words in the composer, with the system's guesses on their menu. |
| `view` | `"zen"`, `"quiet"`, `"normal"`, `"thinking"`, `"verbose"` | `"normal"` | How an agent's work shows in the thread. ⌃O goes round them. |
| `paragraphs` | bool | `false` | An answer streams in a paragraph at a time, not word by word. |
| `math` | bool | `false` | TeX (`$…$`, `$$…$$`) in what an agent says is set as formulas. |

## Appearance

| Key | Type | Default | Meaning |
|---|---|---|---|
| `look` | `"dark"`, `"light"`, `"system"` | `"dark"` | Dark, light, or as the system is. |
| `theme_dark`, `theme_light` | theme name or `null` | `null` | The theme each look is drawn in. `null`: Ghost's own. |
| `backdrop` | path or `null` | `null` | A picture drawn faintly under the thread. |
| `scale` | `80`, `90`, `100`, `110`, `125`, `150`, `175`, `200` | `100` | How large the window is drawn, in percent. ⌘+ and ⌘− step through these. |
| `contrast` | bool | `false` | Draw less important text and edges stronger. |
| `glass` | bool | `false` | Let the desktop show through the window, blurred. |
| `mono` | `"jet_brains"`, `"sf_mono"`, `"menlo"` | `"jet_brains"` | The font for code and numbers. |
| `width` | `"comfortable"`, `"wide"`, `"full"` | `"comfortable"` | How wide the thread and composer read (680 pt, 880 pt, or the whole pane). |
| `blue_orange` | bool | `false` | Lines added and removed in blue and orange, not green and red. |
| `twelve` | bool | `false` | Times on a twelve-hour clock. |
| `language` | `"en"`, `"zh-CN"`, `"de"`, `"es"`, `"fr"`, `"ja"`, `"ko"`, `"pt-BR"`, `"ru"`, `"sv"` or `null` | `null` | The window's language. `null`: the system's where Ghost has it, else English. |
| `terminal_size` | `"small"`, `"medium"`, `"large"` | `"medium"` | Text size in terminals. |
| `shell` | `"login"`, `"zsh"`, `"bash"`, `"fish"` | `"login"` | The shell a terminal runs. `"login"`: your `$SHELL`. |
| `diff_wrap` | bool | `true` | Wrap a diff's long lines instead of cutting them at the edge. |
| `no_spacing` | bool | `false` | Leave changes only in spacing out of diffs. |
| `side_by_side` | bool | `false` | Open a file's diff side by side, widening the right column. |
| `diff_folded` | bool | `false` | Show a diff's files folded, each opened as wanted. |
| `hidden_details` | list of `"context"`, `"plan"`, `"background"`, `"servers"`, `"terminals"`, `"lineage"`, `"linked"` | `[]` | Sections of a session's details hidden from the right column. |

## Notifications and background

| Key | Type | Default | Meaning |
|---|---|---|---|
| `notify` | `"off"`, `"banner"`, `"banner_and_sound"` | `"banner"` | What macOS shows when a session ends a turn or needs you while Ghost is not in front. **General › While you are away**. |
| `notify_on` | `{finished, failed, asks}`, each bool | all `true` | Which events notify: a turn finished, one that failed, a question asked. |
| `awake` | bool | `true` | Keep the Mac from sleeping while an agent works. |
| `confirm` | object | see below | What Ghost asks before doing it. **General › Confirmations**. |

`confirm` holds `unpin` (default `false`), `archive` (`false`), `delete`
(`true`), `terminal` (`true`: before closing a terminal with something
running) and `quit`: what ⌘Q takes, `"hold"` (held a moment, or pressed
twice quickly; the default), `"direct"` (pressed once) or `"double"`
(pressed twice quickly). Quit in the menu always quits at once.

## The writer and dictation

`writer` (**Settings › Writing**) chooses who writes the text Ghost
writes for you (titles, branch names, commit messages, pull requests):

| Key | Type | Default | Meaning |
|---|---|---|---|
| `writer.agent` | agent id or `null` | `null` | The agent that writes. `null`: the session's own. |
| `writer.model` | string or `null` | `null` | Its model. `null`: the agent's default. |
| `writer.changes` | string | `""` | Your instructions for commit messages and pull requests. |
| `writer.titles` | string | `""` | Your instructions for session titles. |
| `writer.branches` | string | `""` | Your instructions for worktree branch names. |
| `writer.transcripts` | string | `""` | Your instructions for cleaning up dictation. |
| `writer.suggest` | bool | `false` | After each turn, write what you are likely to say next, taken with Tab. |

`voice` holds dictation's settings (see [The thread](../thread.md#dictation)):

| Key | Type | Default | Meaning |
|---|---|---|---|
| `voice.key` | `"auto"`, `"toggle"`, `"hold"` | `"auto"` | How ⇧⌘D listens: a press starts and a press stops, or held it listens until released (`auto`); press to start and stop (`toggle`); only while held (`hold`). |
| `voice.language` | locale or `null` | `null` | The language listened for (`"en-US"`). `null`: the system's. |
| `voice.dictionary` | list of strings | `[]` | Words to listen for, one per entry. `"heard = written"` writes what is heard differently. |
| `voice.no_fillers` | bool | `true` | Leave out "um", "uh" and the like. |
| `voice.clean` | bool | `false` | Have the writer clean up what was heard before it goes in. |

## Updates, logs, telemetry

| Key | Type | Default | Meaning |
|---|---|---|---|
| `check_updates` | bool | `true` | Look for an update as Ghost starts and every four hours. **About**. |
| `channel` | `"stable"`, `"nightly"` | `"stable"` | Which builds Ghost updates to. |
| `log_days` | number | `14` | Days of Ghost's log kept in `<home>/logs/`. **About › Keep logs**. |
| `telemetry` | `0`–`3` or `null` | `null` | How much Ghost tells its makers. `null` until you choose, as Ghost first opens. See [telemetry](../first-run.md#telemetry). |
| `editor` | command or `null` | `null` | The editor files open in, by its command. `null`: the first one found. **General › Files**. |

## Kept by the window

These record the window's own state. You rarely set them by hand.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `left` | points | `256` | The sidebar's width, as you dragged it. |
| `right` | points | `400` | The right column's width. |
| `frame` | `[x, y, width, height]` or `null` | `null` | Where the window was when Ghost last quit. `null`: centred. |
