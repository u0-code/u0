---
title: Keyboard shortcuts
description: Ghost's default keys, every action you can bind, and keybindings.json.
---

# Keyboard shortcuts

**Settings › Keyboard** lists every action with its keys and lets you
change them. What you change is written to `keybindings.json` in Ghost's
home, which you can also edit by hand.

## Default keys

### Sessions

| Keys | Action |
|---|---|
| ⌘N | New session |
| ⇧⌘N | New session in this project |
| ⌥⌘N | New scratch session (no project) |
| ⇧⌘] / ⇧⌘[ (`cmd-}` / `cmd-{`) | Next / previous session |
| ⌘1 … ⌘9 | The first to ninth session in the sidebar |
| ⇧⌘S | Done, or not done |
| ⇧⌘P | Pin, or unpin |
| ⇧⌘C | Copy the session's id |
| ⌘Z | Take back what was deleted or archived |

### The thread

| Keys | Action |
|---|---|
| ⌘F | Find in the session |
| ⌘. | Stop the turn |
| ⇧⌘M | Choose the model |
| ⇧⌘A | Choose the mode |
| ⇧⌘E | Choose how hard it thinks (effort) |
| ⇧⌘D | Dictate |
| ⌃⇧↑ / ⌃⇧↓ | To what you said before / next |
| ⌃O | Transcript view: answers only, calls that went wrong, normal, thinking, verbose |

### Going about

| Keys | Action |
|---|---|
| ⌘K | Go to anything (the palette) |
| ⌘P | Go to a file |
| ⇧⌘F | Search the project's files |
| ⌥⌘[ / ⌥⌘] | Back to the session before / ahead again |
| F6 / ⇧F6 | Keys to the next / previous part of the window |
| ⌘B | Show or hide the sidebar |
| ⌘, | Settings |
| ⌘U | Usage: what sessions spent |

### The right column

| Keys | Action |
|---|---|
| ⌘D | Review the changes |
| ⌥⌘B | Show or hide the right column |
| ⇧⌘B | Widen the right column over the thread |
| ⌘T | Open a tab in the right column |
| ⇧⌘T | Reopen the tab closed last |
| ⌘W | Close the right column's tab |
| ⇧⌘K | Copy the pull request's address |
| ⌘O | Open the session's folder in your editor |

### Terminal

| Keys | Action |
|---|---|
| ⌘J | Terminal (the drawer) |
| ⇧⌘] / ⇧⌘[ | Next / previous terminal (while a terminal has focus) |

### The window

| Keys | Action |
|---|---|
| ⌥⌘A | Light or dark |
| ⌃⌘F | Full screen, or not |
| ⌘M | Minimize |
| ⌘+ (or ⌘=) / ⌘− / ⌘0 | Everything larger / smaller / at its actual size |
| ⌘Q | Quit (see `confirm.quit` in [settings](settings.md#notifications-and-background)) |
| ⌘H / ⌥⌘H | Hide Ghost / hide others (not rebindable) |

### In the composer

| Keys | Action |
|---|---|
| Return or ⌘Return | Send (see `send` in [settings](settings.md#chat-and-composer)) |
| ⇧⌘Return | Send the other way: steer the turn instead of waiting, or the reverse |
| ⌥↑ | Edit the last queued message |
| ⌘S | Put the draft aside |
| ⇧⌘V | Paste as plain text, however long |
| ↑ / ↓ | Earlier / later messages you sent |
| Tab / Esc | Take / dismiss the suggested next message |

## keybindings.json

`<home>/keybindings.json` is a list of bindings. Yours are added after
the defaults, so they win.

```json
[
  { "key": "cmd-k", "action": "palette" },
  { "key": "cmd-shift-r", "action": "script:dev" },
  { "key": "cmd-d", "action": "none" },
  { "key": "cmd-j", "action": "-drawer" },
  { "key": "ctrl-l", "action": "clear_terminal", "when": "Terminal" }
]
```

| Field | Meaning |
|---|---|
| `key` | The keystroke, as GPUI writes it: modifiers `cmd`, `ctrl`, `alt`, `shift`, joined with `-` (`cmd-shift-p`). Several keystrokes separated by spaces make a sequence. |
| `action` | An action name (below); `"none"` to take the key from whatever has it; `"-<name>"` to take it from that action only; or `"script:<name>"` to run that script from the repository's `ghost.json` (see [Review](../review.md#repository-commands)). |
| `when` | Optional. Where the binding holds, over the parts of the window `Terminal` and `Composer`, with `!`, `&&` and `||`. |

Unknown fields are an error. A binding whose action or `when` does not
read is skipped and the reason is logged; a file that does not read at
all is ignored (and logged), and **Settings › Keyboard** leaves it
untouched rather than writing over it.

## Action names

| Group | Actions |
|---|---|
| Sessions | `new_session`, `new_here`, `new_scratch`, `next_session`, `previous_session`, `settle`, `pin`, `copy_id` |
| The thread | `find`, `stop`, `model`, `mode`, `effort`, `dictate`, `previous_turn`, `next_turn`, `transcript_view` |
| Going about | `palette`, `go_to_file`, `search_words`, `back`, `ahead`, `next_area`, `toggle_sidebar`, `settings`, `usage` |
| The column beside | `review`, `right_column`, `open_surface`, `reopen_surface`, `close_surface`, `pull_requests`, `issues`, `copy_pr`, `open_folder` |
| Terminal | `drawer`, `next_terminal`, `previous_terminal`, `clear_terminal` |
| The window | `light_or_dark`, `full_screen`, `minimize`, `quit` |
| Other | `take_back`, `previous_area`, `whole_right`, `zoom_in`, `zoom_out`, `actual_size`, `check_for_updates`, `pair_device`, `show_pairing_code` |
