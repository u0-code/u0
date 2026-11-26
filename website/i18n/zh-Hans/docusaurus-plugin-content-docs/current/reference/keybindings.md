---
title: 键盘快捷键
description: Ghost 的默认按键、所有可绑定的操作，以及 keybindings.json。
---

# 键盘快捷键

**设置 › 键盘** 列出每个操作及其按键，并允许你修改。你的修改会写入 Ghost 主目录中的 `keybindings.json`，你也可以手动编辑它。

## 默认按键 {#default-keys}

### 会话 {#sessions}

| 按键 | 操作 |
|---|---|
| ⌘N | 新会话 |
| ⇧⌘N | 在此项目中新建会话 |
| ⌥⌘N | 新建草稿会话（无项目） |
| ⇧⌘] / ⇧⌘[ (`cmd-}` / `cmd-{`) | 下一个会话 / 上一个会话 |
| ⌘1 … ⌘9 | 侧边栏中的第一到第九个会话 |
| ⇧⌘S | 标为完成或未完成 |
| ⇧⌘P | 置顶或取消置顶 |
| ⇧⌘C | 复制会话 ID |
| ⌘Z | 撤回删除或归档 |

### 对话 {#the-thread}

| 按键 | 操作 |
|---|---|
| ⌘F | 在会话中查找 |
| ⌘. | 停止本轮 |
| ⇧⌘M | 选择模型 |
| ⇧⌘A | 选择模式 |
| ⇧⌘E | 选择思考强度（effort） |
| ⇧⌘D | 听写 |
| ⌃⇧↑ / ⌃⇧↓ | 跳到你上一条消息 / 下一条消息 |
| ⌃O | 记录视图：仅回答、出错的调用、普通、思考、详细 |

### 导航 {#going-about}

| 按键 | 操作 |
|---|---|
| ⌘K | 前往任意位置（命令面板） |
| ⌘P | 前往文件 |
| ⇧⌘F | 搜索项目文件 |
| ⌥⌘[ / ⌥⌘] | 回到上一个查看的会话 / 再次前进 |
| F6 / ⇧F6 | 焦点移到窗口的下一部分 / 上一部分 |
| ⌘B | 显示或隐藏侧边栏 |
| ⌘, | 设置 |
| ⌘U | 用量：会话的花费 |

### 右侧栏 {#the-right-column}

| 按键 | 操作 |
|---|---|
| ⌘D | 审阅改动 |
| ⌥⌘B | 显示或隐藏右侧栏 |
| ⇧⌘B | 将右侧栏展开覆盖对话 |
| ⌘T | 在右侧栏打开标签页 |
| ⇧⌘T | 重新打开最近关闭的标签页 |
| ⌘W | 关闭右侧栏的标签页 |
| ⇧⌘K | 复制拉取请求的地址 |
| ⌘O | 在编辑器中打开会话的文件夹 |

### 终端 {#terminal}

| 按键 | 操作 |
|---|---|
| ⌘J | 终端（抽屉） |
| ⇧⌘] / ⇧⌘[ | 下一个终端 / 上一个终端（终端获得焦点时） |

### 窗口 {#the-window}

| 按键 | 操作 |
|---|---|
| ⌥⌘A | 浅色或深色 |
| ⌃⌘F | 进入或退出全屏 |
| ⌘M | 最小化 |
| ⌘+（或 ⌘=）/ ⌘− / ⌘0 | 全部放大 / 全部缩小 / 全部恢复实际大小 |
| ⌘Q | 退出（见[设置](settings.md#notifications-and-background)中的 `confirm.quit`） |
| ⌘H / ⌥⌘H | 隐藏 Ghost / 隐藏其他（不可重新绑定） |

### 在输入框中 {#in-the-composer}

| 按键 | 操作 |
|---|---|
| Return 或 ⌘Return | 发送（见[设置](settings.md#chat-and-composer)中的 `send`） |
| ⇧⌘Return | 以另一种方式发送：引导当前这一轮而不是等待，或者反过来 |
| ⌥↑ | 编辑最后一条排队的消息 |
| ⌘S | 把草稿放到一边 |
| ⇧⌘V | 以纯文本粘贴，无论多长 |
| ↑ / ↓ | 你之前 / 之后发送的消息 |
| Tab / Esc | 采用 / 忽略建议的下一条消息 |

## keybindings.json {#keybindingsjson}

`<home>/keybindings.json` 是一个绑定列表。你的绑定添加在默认绑定之后，因此会优先生效。

```json
[
  { "key": "cmd-k", "action": "palette" },
  { "key": "cmd-shift-r", "action": "script:dev" },
  { "key": "cmd-d", "action": "none" },
  { "key": "cmd-j", "action": "-drawer" },
  { "key": "ctrl-l", "action": "clear_terminal", "when": "Terminal" }
]
```

| 字段 | 含义 |
|---|---|
| `key` | 按键，按 GPUI 的写法：修饰键 `cmd`、`ctrl`、`alt`、`shift`，用 `-` 连接（`cmd-shift-p`）。以空格分隔的多个按键组成一个按键序列。 |
| `action` | 操作名称（见下文）；`"none"` 表示把该按键从任何占用它的操作上移除；`"-<name>"` 表示只从该操作上移除；或 `"script:<name>"` 表示运行仓库 `ghost.json` 中的该脚本（见[审阅](../review.md#repository-commands)）。 |
| `when` | 可选。绑定生效的位置，基于窗口的 `Terminal` 和 `Composer` 部分，可使用 `!`、`&&` 和 `||`。 |

未知字段会被视为错误。操作或 `when` 无法解析的绑定会被跳过，原因会记录到日志；完全无法读取的文件会被忽略（并记录到日志），**设置 › 键盘** 会保持它原样，而不是覆盖它。

## 操作名称 {#action-names}

| 分组 | 操作 |
|---|---|
| 会话 | `new_session`, `new_here`, `new_scratch`, `next_session`, `previous_session`, `settle`, `pin`, `copy_id` |
| 对话 | `find`, `stop`, `model`, `mode`, `effort`, `dictate`, `previous_turn`, `next_turn`, `transcript_view` |
| 导航 | `palette`, `go_to_file`, `search_words`, `back`, `ahead`, `next_area`, `toggle_sidebar`, `settings`, `usage` |
| 侧栏 | `review`, `right_column`, `open_surface`, `reopen_surface`, `close_surface`, `pull_requests`, `issues`, `copy_pr`, `open_folder` |
| 终端 | `drawer`, `next_terminal`, `previous_terminal`, `clear_terminal` |
| 窗口 | `light_or_dark`, `full_screen`, `minimize`, `quit` |
| 其他 | `take_back`, `previous_area`, `whole_right`, `zoom_in`, `zoom_out`, `actual_size`, `check_for_updates`, `pair_device`, `show_pairing_code` |
