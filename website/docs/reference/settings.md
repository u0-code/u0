---
title: settings.json
description: Every key of ~/.u0/settings.json, its type, default and where to change it.
---

# settings.json

u0 keeps its settings in `settings.json` in its home (`~/.u0`, or
`U0_HOME`). Almost every key has a control in **Settings** (⌘,), and
changing it there writes the file. You can also edit the file while u0
is closed.

Rules for the file:

- A key u0 does not know is an error (the file is read with
  `deny_unknown_fields`). u0 then starts with the defaults, says why
  the file did not read, and **saves nothing** until you fix the file or
  move it away. It never writes over a file it could not read.
- A key left out takes its default.
- An organisation's [policy](policy.md) can set a key's default, or lock
  it. A locked setting shows disabled, with who manages it.
- In a window that controls several Macs, each machine's settings are its
  own `settings.json`: the settings dialog shows a machine picker at its
  top, and what you change is saved on that machine. The look, the keys
  and the window's own state stay this Mac's.

## Agents

| Key | Type | Default | Meaning |
|---|---|---|---|
| `enabled` | list of agent ids | `["claude-acp", "codex-acp", "antigravity-acp"]` | The agents switched on, in the order listed. **Settings › Agents**. |
| `commands` | map: agent id → command line | `{}` | A command line of yours that starts an agent instead of the registry's way (your own build, a version manager's). On the agent's page. |
| `env` | map: agent id → (name → value or `null`) | `{}` | Variables each agent starts with. `null` means the value is a secret kept in the Keychain. On the agent's page. |
| `models` | map: agent id → list of model ids | `{}` | Model ids of your own, offered beside those the agent lists. |
| `favourites` | list of `{agent, model}` | `[]` | Models starred in the model picker, listed first there. |
| `outside_acts` | bool | `false` | Whether an agent outside u0 may say something in a thread or stop its turn through [u0's tools](../tools-for-agents.md#access-levels), each asked of you there. **Settings › Agents › Agents outside u0 may act**. |
| `no_tools` | list of agent ids | `[]` | Agents whose threads are not given [u0's tools](../tools-for-agents.md). The switch is on the agent's page. |
| `check_agents` | bool | `true` | Read the ACP registry again every hour for newer agent versions. **About**. |
| `hidden_models` | map: agent id → list of model ids | `{}` | Agent-listed models hidden from the model picker. |
| `custom_agents` | map: id → `{name, command}` | `{}` | Standalone custom ACP agents added outside the ACP registry. |
| `health_check_mins` | number | `5` | Minutes between background agent health checks. |
| `compact_idle` | bool | `true` | Compact idle threads in the background. |

## Projects

| Key | Type | Default | Meaning |
|---|---|---|---|
| `folders` | list of paths | `[]` | The projects, the latest added first. |
| `prefs` | map: folder → project prefs | `{}` | What you set for each project (below). **Settings › Projects**. |
| `projects_in` | path or `null` | `null` (`~/Documents/u0`) | Where new and cloned projects go. |
| `host_folders` | bool | `false` | A clone goes in `<host>/<owner>/<repo>` under that folder, as `ghq` lays them out. |
| `fetch_every` | minutes or `null` | `5` | How often each project's remote is fetched. `null`: never. **General › In the background**. |

Each entry of `prefs`:

| Key | Type | Default | Meaning |
|---|---|---|---|
| `name` | string or `null` | `null` | The name shown instead of the folder's. |
| `pinned` | bool | `false` | Pinned at the top of the sidebar. |
| `archived` | bool | `false` | Archived (out of the sidebar). |
| `agent` | agent id or `null` | `null` | The agent its new threads begin with. |
| `worktree` | bool or `null` | `null` | Whether its new threads work in a worktree. `null`: the last choice made anywhere. |
| `pull` | bool | `false` | Pull its default branch as u0 starts and on each fetch, when it is checked out with nothing changed. |
| `icon` | `{"picture": path}` or `{"mark": {"text", "colour"}}` or `null` | `null` | Its icon. `null`: its own icon file where it has one. |
| `linear` | Linear team key or `null` | `null` | Read its issues from this Linear team instead of its repository's host. |
| `over` | object | `{}` | Settings this project holds otherwise than everywhere else (`merge`, `uncredit`, `auto_pull`, `sign_commits`, `run_git_hooks`, `pr_drafts`, `pr_templates`, `commit_style`, `review_guides`, `submodules`, `branch_prefix`, `branch_kind`, `branch_template`, `worktree_from`, `copy_untracked`, `worktree_cleanup`, `keep_dirty_worktrees`, `tidy_after`, `worktree`, `ask_worktree`, `default_agent`, `default_model`, `ask_before_push`, `streaming`, `settle_days`, `resume_at_reset`). Any other key is an error. |
| `last` | map: agent id → `{options, mode}` | `{}` | The options (model, effort, …) and mode last chosen in a new thread of each agent here. |
| `dictionary` | list of strings | `[]` | Project-specific dictation words. |
| `external_folders` | list of paths | `[]` | External folders linked into this project. |
| `excluded_submodules` | list of paths | `[]` | Submodule paths excluded when creating worktrees. |
| `env` | map: name → value | `{}` | Environment variables and `secret:<NAME>` references given to agents and actions in this project. |
| `closed` | `{changes, publish}` | `{}` | What you closed here with ×: `changes`, by folder (the project's or a worktree's), the mark of the changes whose bar above the composer you closed (it shows again once they change); `publish`, the right column's offer to publish it (offered again once it has had a remote and lost it). |

## Threads

| Key | Type | Default | Meaning |
|---|---|---|---|
| `settle_days` | days or `null` | `7` | A thread left this long is marked done. `null`: never. **General › Done for now**. |
| `settle_merged` | bool | `true` | A thread whose pull request merged or closed is done. |
| `keep_with_pr` | bool | `false` | A thread with an open pull request is never done, however long it is left. |
| `snoozes` | list of strings | `["1h", "tomorrow", "next week"]` | The snooze choices offered first, each as you would type it in **Custom…**. |
| `resume_at_reset` | bool | `true` | A turn an agent's usage limit stopped goes on by itself when the limit resets, where the agent says when. **General › In the background**. |
| `sort` | `"used"`, `"updated"`, `"begun"`, `"name"` | `"used"` | How the sidebar orders threads and projects: by your last message, by any activity, by when it began, or by name. |
| `project_grouping` | `"by_project"`, `"by_machine"`, `"flat"` | `"by_project"` | How threads are grouped in the sidebar. |
| `project_order` | `"manual"`, `"activity"`, `"name"` | `"manual"` | How projects are ordered in the sidebar. |
| `working_section` | bool | `true` | Keep threads that are running or need you at the top of the sidebar. |
| `two_line_projects` | bool | `false` | Show the branch under each project name in the sidebar. |
| `ask_worktree` | bool | `false` | Ask whether a new thread works locally or in a new worktree each time. |
| `default_agent` | agent id or `null` | `null` | Default agent for new threads (`null`: first enabled). |
| `default_model` | model id or `null` | `null` | Default model for new threads (`null`: the agent's default). |
| `default_modes` | map: agent id → mode id | `{}` | Default mode per agent for new threads. |
| `ask_before_push` | bool | `true` | Ask before `git push` runs in a thread. |
| `streaming` | bool | `true` | Stream replies as they are written. |
| `archive_after` | days or `null` | `30` | Days after settling before a thread is archived (`null`: never). |
| `snooze_until_reset` | bool | `true` | Offer snoozing until usage limits reset. |
| `views` | list of `{name, kind, state, part, search}` | `[]` | Saved views of the PRs and Issues pages. `kind` is `"Pr"` or `"Issue"`. |

## Worktrees and git

| Key | Type | Default | Meaning |
|---|---|---|---|
| `worktree` | bool | `false` | Whether a new thread in a git repository works in a worktree of its own: the choice last made. **New threads › In a repository**. |
| `branch_prefix` | string | `"u0/"` | What a worktree's branch name begins with, before the name the writer gives it. |
| `branch_kind` | bool | `false` | Begin the branch with the kind of work instead (`feat/`, `fix/`). |
| `worktrees` | path or `null` | `null` (`<home>/worktrees`) | Where new worktrees are made. Those made before stay where they are. |
| `submodules` | `"recursive"`, `"top"`, `"skip"` | `"recursive"` | How far a new worktree's submodules are checked out. |
| `tidy_after` | days or `null` | `null` | Days an archived thread's worktree is left before it is removed. A worktree with uncommitted changes stays, and its branch always does. `null`: kept. |
| `merge` | `"squash"`, `"merge"`, `"rebase"` | `"squash"` | How a pull request is merged from the right column. **Source control & issues**. |
| `uncredit` | bool | `false` | Take agents' `Co-authored-by` lines and credit footers out of a squashed commit's message. **Source control & issues**. |
| `worktree_from` | `"origin_default"`, `"local_main"`, `"current_branch"` | `"origin_default"` | What a new worktree checks out from by default. |
| `branch_template` | string | `"{type}/{thread}"` | Branch naming template for new worktrees. |
| `copy_untracked` | string | `".env, .env.local"` | Comma-separated untracked files copied into new worktrees. |
| `worktree_cleanup` | `"when_archived"`, `"when_merged"`, `"never"` | `"when_archived"` | When finished worktrees are removed. |
| `keep_dirty_worktrees` | bool | `true` | Skip worktrees with uncommitted changes during cleanup. |
| `auto_pull` | bool | `false` | Automatically pull clean default branches on startup and fetch. |
| `sign_commits` | bool | `false` | Sign commits made from u0. |
| `run_git_hooks` | bool | `true` | Run git hooks on commits and pushes made from u0. |
| `pr_drafts` | bool | `false` | Open pull requests as drafts by default. |
| `pr_templates` | bool | `true` | Follow repository pull request templates. |
| `commit_style` | `"match_history"`, `"conventional"`, `"plain"` | `"match_history"` | Default commit message style. |
| `review_guides` | bool | `true` | Enable guided pull request review tours. |

## Chat and composer

| Key | Type | Default | Meaning |
|---|---|---|---|
| `send` | `"return"`, `"multiline"`, `"command"` | `"return"` | What sends: Return; Return while the message is one line and ⌘Return after; or ⌘Return always. Where Return does not send, it breaks the line. **Chat › Sending**. |
| `steer` | bool | `false` | What you send while a turn goes on steers it, instead of waiting for the turn to end. |
| `spell_check` | bool | `true` | Underline misspelled words in the composer, with the system's guesses on their menu. |
| `view` | `"zen"`, `"quiet"`, `"normal"`, `"thinking"`, `"verbose"` | `"normal"` | How an agent's work shows in the thread. ⌃O goes round them. |
| `paragraphs` | bool | `false` | An answer streams in a paragraph at a time, not word by word. |
| `math` | bool | `false` | TeX (`$…$`, `$$…$$`) in what an agent says is set as formulas. |
| `rich_composer` | bool | `true` | Formatting, code blocks and inline chips in the composer. |
| `ask_follow_up` | bool | `false` | Ask each time whether a follow-up message queues or steers. |
| `slash_skills` | bool | `true` | List provider and repository skills in the `/` menu. |
| `open_subagents` | `"right_panel"`, `"split"`, `"new_window"` | `"right_panel"` | Where subagents and side chats open. |
| `announcements` | `"replies_and_alerts"`, `"alerts_only"`, `"off"` | `"replies_and_alerts"` | Screen reader announcements in threads. |
| `title_status` | bool | `false` | Put the active thread's status in the window title. |

## Appearance

| Key | Type | Default | Meaning |
|---|---|---|---|
| `look` | `"dark"`, `"light"`, `"system"` | `"dark"` | Dark, light, or as the system is. |
| `theme_dark`, `theme_light` | theme name or `null` | `null` | The theme each look is drawn in (`"g100"` / `"g90"` for dark, `"white"` / `"g10"` for light, or a theme in `themes/`). `null`: Carbon G100 for dark, Carbon White for light. |
| `backdrop` | path or `null` | `null` | A picture drawn faintly under the thread. |
| `scale` | `80`, `90`, `100`, `110`, `125`, `150`, `175`, `200` | `100` | How large the window is drawn, in percent. ⌘+ and ⌘− step through these. |
| `contrast` | bool | `false` | Draw less important text and edges stronger. |
| `glass` | bool | `false` | Let the desktop show through the window, blurred. |
| `motion` | `"system"`, `"reduce"`, `"full"` | `"system"` | Motion in the window: as macOS's Reduce motion says, cut down (panels fade instead of sliding), or in full. |
| `mono` | `"plex_mono"`, `"jet_brains"`, `"sf_mono"`, `"menlo"` | `"plex_mono"` | The font for code and numbers. |
| `width` | `"comfortable"`, `"wide"`, `"full"` | `"comfortable"` | How wide the thread and composer read (720 pt, 880 pt, or the whole pane). |
| `blue_orange` | bool | `false` | Lines added and removed in blue and orange, not green and red. |
| `twelve` | bool | `false` | Times on a twelve-hour clock. |
| `language` | `"en"`, `"zh-CN"`, `"de"`, `"es"`, `"fr"`, `"ja"`, `"ko"`, `"pt-BR"`, `"ru"`, `"sv"` or `null` | `null` | The window's language. `null`: the system's where u0 has it, else English. |
| `terminal_size` | `"small"`, `"medium"`, `"large"` | `"medium"` | Text size in terminals. |
| `shell` | `"login"`, `"zsh"`, `"bash"`, `"fish"` | `"login"` | The shell a terminal runs. `"login"`: your `$SHELL`. |
| `diff_wrap` | bool | `true` | Wrap a diff's long lines instead of cutting them at the edge. |
| `no_spacing` | bool | `false` | Leave changes only in spacing out of diffs. |
| `side_by_side` | bool | `false` | Open a file's diff side by side, widening the right column. |
| `diff_folded` | bool | `false` | Show a diff's files folded, each opened as wanted. |
| `diff_theme_colors` | bool | `false` | Use theme colors in diffs. |
| `diff_auto_layout` | bool | `true` | Choose unified vs split diff layout automatically from width. |
| `diff_context` | number | `3` | Unchanged lines shown around each change in a diff. |
| `highlight_words` | bool | `true` | Highlight changed words inside modified lines. |
| `collapse_large_diffs` | bool | `true` | Collapse diff files over 400 changed lines. |
| `hide_test_files` | bool | `false` | Collapse test files under one summary row in diffs. |
| `open_prs_here` | bool | `true` | Open pull request links in u0's pull request view. |
| `proactive_panels` | bool | `true` | Open diff and pull request panels proactively. |
| `hidden_details` | list of `"context"`, `"plan"`, `"background"`, `"servers"`, `"terminals"`, `"lineage"`, `"linked"` | `[]` | Sections of a thread's details hidden from the right column. |

### Themes

**Settings › Appearance › Themes** lists the themes: Carbon's four that u0
brings and yours, each a file in `themes/` in gpui-kit's theme format.
Choosing a card draws its look (dark or light) in it; **Light theme** and
**Dark theme** above say which one each look uses.

- **Customize** on a card opens the theme in a panel over the settings:
  its main colours with a colour picker, how well its text reads on its
  grounds (contrast), a preview, and every colour by its hex. Edits are
  saved as you make them. A theme u0 brings is customized as a copy of
  yours. **Duplicate**, a reset, and **Remove theme** are at its foot.
- **Import a VS Code theme…** and **Import a gpui-kit theme JSON…** make a
  theme file of another app's yours. When you already have a theme of that
  name, u0 asks first: replace it, or keep both, the import under the next
  free name.
- The search finds your themes, and, from two letters on, colour theme
  extensions on [Open VSX](https://open-vsx.org); **Add** brings one and
  imports the themes it holds as above.
- **Ask an agent** has the writer (one of your agents; see
  [The writer and dictation](#the-writer-and-dictation)) write a theme
  from what you describe, for the look drawn now. u0 has no model of its
  own.
- **New theme** starts one of yours from the theme drawn now.

**Reduce motion** (`motion`) is under **Motion and effects**.

## Notifications and background

| Key | Type | Default | Meaning |
|---|---|---|---|
| `notify` | `"off"`, `"banner"`, `"banner_and_sound"` | `"banner"` | What macOS shows when a thread ends a turn or needs you while u0 is not in front. **General › While you are away**. |
| `notify_on` | `{finished, failed, asks}`, each bool | all `true` | Which events notify: a turn finished, one that failed, a question asked. |
| `in_app_notify` | bool | `true` | Show in-app toast notifications for unfocused threads. |
| `awake` | bool | `true` | Keep the Mac from sleeping while an agent works. |
| `background_activity` | `"battery_saver"`, `"balanced"`, `"full_speed"` | `"balanced"` | How aggressively background tasks run while the window is hidden. |
| `continue_after_restart` | bool | `true` | Resume working threads after an update or restart. |
| `confirm` | object | see below | What u0 asks before doing it. **General › Confirmations**. |

`confirm` holds `unpin` (default `false`), `archive` (`false`), `delete`
(`true`), `terminal` (`true`: before closing a terminal with something
running) and `quit`: what ⌘Q takes, `"hold"` (held a moment, or pressed
twice quickly; the default), `"direct"` (pressed once) or `"double"`
(pressed twice quickly). Quit in the menu always quits at once.

## The writer and dictation

`writer` (**Settings › Writing**) chooses who writes the text u0
writes for you (titles, branch names, commit messages, pull requests):

| Key | Type | Default | Meaning |
|---|---|---|---|
| `writer.agent` | agent id or `null` | `null` | The agent that writes. `null`: the thread's own. |
| `writer.model` | string or `null` | `null` | Its model. `null`: the agent's default. |
| `writer.changes` | string | `""` | Your instructions for commit messages and pull requests. |
| `writer.titles` | string | `""` | Your instructions for thread titles. |
| `writer.branches` | string | `""` | Your instructions for worktree branch names. |
| `writer.transcripts` | string | `""` | Your instructions for cleaning up dictation. |
| `writer.guides` | string | `""` | Your instructions for pull request review guides. |
| `writer.suggest` | bool | `false` | After each turn, write what you are likely to say next, taken with Tab. |

`voice` holds dictation's settings (see [The thread](../thread.md#dictation)):

| Key | Type | Default | Meaning |
|---|---|---|---|
| `voice.key` | `"auto"`, `"toggle"`, `"hold"` | `"auto"` | How ⇧⌘D listens: a press starts and a press stops, or held it listens until released (`auto`); press to start and stop (`toggle`); only while held (`hold`). |
| `voice.language` | locale or `null` | `null` | The language listened for (`"en-US"`). `null`: the system's. |
| `voice.dictionary` | list of strings | `[]` | Words to listen for, one per entry. `"heard = written"` writes what is heard differently. |
| `voice.no_fillers` | bool | `true` | Leave out "um", "uh" and the like. |
| `voice.fillers` | string | `"um, uh, like, you know"` | Filler words removed locally when `voice.no_fillers` is on. |
| `voice.clean` | bool | `false` | Have the writer clean up what was heard before it goes in. |
| `voice.microphone` | string or `null` | `null` | Chosen input device (`null`: system default). |
| `voice.live_words` | bool | `true` | Show partial recognized words while speaking. |
| `voice.correction_cue` | string | `"scratch that"` | Spoken phrase that replaces what was just said. |

## Browser, snapshots, devices and labs

| Key | Type | Default | Meaning |
|---|---|---|---|
| `open_pages` | `"preview"`, `"browser"` | `"preview"` | Where a page you open shows: the Preview tab (macOS) or your browser (**Settings › Browser › Open pages in**). |
| `browser` | path or `null` | `null` | The browser `.app` **Open in your browser** uses (`null`: system default). |
| `ask_browser` | bool | `false` | Ask which browser to open each link in. |
| `cmd_click_urls` | bool | `true` | `⌘`-clicking a terminal URL opens it where pages open; `⌥⌘`-click, in your browser. |
| `dev_server` | `"show"`, `"open"`, `"ignore"` | `"show"` | What happens when a dev server starts. |
| `forward_port` | number | `41873` | First local port used when forwarding a remote dev server. |
| `snapshots` | `{shortcut, include_text, never_capture, sound, flash, animations}` | see defaults | Desktop window snapshot capture and feedback settings. |
| `use_devices` | bool | `true` | Show the Simulators & devices surface in the right panel. |
| `agent_devices` | `"ask"`, `"allowed"`, `"off"` | `"ask"` | Whether agents may drive simulators and devices over MCP. |
| `local_simulators` | bool | `true` | Boot local simulators when a thread asks for one. |
| `max_simulators` | number | `2` | Maximum simulators running at once. |
| `machine_id` | `"color_and_name"`, `"name_only"`, `"off"` | `"color_and_name"` | How remote machines are marked in the title bar. |
| `hidden_servers` | list of strings | `[]` | Paired server names hidden from this window's sidebar. |
| `pr_sharing` | bool | `false` | Share pull request state with read-only paired Macs. |
| `phone_confirm`, `phone_live_activities`, `phone_dev_server` | bool | `false` | Preview toggles for paired phone features. |
| `otlp_endpoint` | string or `null` | `null` | OpenTelemetry OTLP collector endpoint. |
| `agent_feedback` | bool | `false` | Allow agents to file feedback about u0. |
| `workflows_enabled` | bool | `true` | Enable multi-stage workflows. |
| `skipped_runs_days` | number | `7` | Days concurrency-skipped runs remain in history. |
| `fast_worktree` | bool | `true` | Clone worktrees with copy-on-write where supported. |
| `simlock_pools` | bool | `false` | Lease simulators from shared Simlock pools. |

## Updates, logs, telemetry

| Key | Type | Default | Meaning |
|---|---|---|---|
| `check_updates` | bool | `true` | Look for an update as u0 starts and every four hours. **About**. |
| `channel` | `"stable"`, `"nightly"` | `"stable"` | Which builds u0 updates to. |
| `log_days` | number | `14` | Days of u0's log kept in `<home>/logs/`. **About › Keep logs**. |
| `telemetry` | `0`–`3` or `null` | `null` | How much u0 tells its makers. `null` until you choose, as u0 first opens. See [telemetry](../first-run.md#telemetry). |
| `editor` | command or `null` | `null` | The editor files open in, by its command. `null`: the first one found. **General › Files**. |

## Kept by the window

These record the window's own state. You rarely set them by hand.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `left` | points | `256` | The sidebar's width, as you dragged it. |
| `right` | points | `400` | The right column's width. |
| `frame` | `[x, y, width, height]` or `null` | `null` | Where the window was when u0 last quit. `null`: centred. |
