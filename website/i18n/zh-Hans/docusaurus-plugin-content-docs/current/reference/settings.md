---
title: settings.json
description: ~/.ghost/settings.json 的每个键、它的类型、默认值以及在哪里修改。
---

# settings.json

Ghost 把设置保存在其主目录（`~/.ghost`，或 `GHOST_HOME`）中的 `settings.json` 里。几乎每个键在 **设置**（⌘,）中都有对应的控件，在那里修改就会写入该文件。你也可以在 Ghost 关闭时编辑该文件。

关于该文件的规则：

- Ghost 不认识的键会被视为错误（该文件以 `deny_unknown_fields` 读取）。此时 Ghost 以默认值启动，说明文件为何无法读取，并且在你修复或移走该文件之前**不保存任何内容**。它从不覆盖一个它无法读取的文件。
- 省略的键取其默认值。
- 组织的[策略](policy.md)可以设置某个键的默认值，或将其锁定。被锁定的设置显示为禁用，并注明由谁管理。
- 在控制多台 Mac 的窗口中，每台服务器的设置是它自己的 `settings.json`：设置对话框顶部显示一个服务器选择器，你所做的修改会保存在该服务器上。外观、按键以及窗口自身的状态仍属于这台 Mac。

## 智能体 {#agents}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `enabled` | 智能体 ID 列表 | `["claude-acp", "codex-acp", "antigravity-acp"]` | 已开启的智能体，按列出的顺序排列。**设置 › 智能体**。 |
| `commands` | 映射：智能体 ID → 命令行 | `{}` | 你自己的一条命令行，用它来启动智能体，而不是按注册表的方式（你自己的构建、版本管理器提供的版本）。在该智能体的页面中。 |
| `env` | 映射：智能体 ID →（名称 → 值或 `null`） | `{}` | 每个智能体启动时带的变量。`null` 表示该值是保存在钥匙串中的机密。在该智能体的页面中。 |
| `models` | 映射：智能体 ID → 模型 ID 列表 | `{}` | 你自己的模型 ID，与智能体列出的模型一起提供。 |
| `favourites` | `{agent, model}` 列表 | `[]` | 在模型选择器中加星标的模型，在那里排在最前。 |
| `no_tools` | 智能体 ID 列表 | `[]` | 其会话不提供 [Ghost 的工具](../tools-for-agents.md)的智能体。开关在该智能体的页面中。 |
| `check_agents` | bool | `true` | 每小时重新读取 ACP 注册表，查找更新的智能体版本。**通用 › 在后台**。 |

## 项目 {#projects}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `folders` | 路径列表 | `[]` | 项目列表，最近添加的排在最前。 |
| `prefs` | 映射：文件夹 → 项目偏好 | `{}` | 你为每个项目所做的设置（见下文）。**设置 › 项目**。 |
| `projects_in` | 路径或 `null` | `null`（`~/Documents/Ghost`） | 新建和克隆的项目放在哪里。 |
| `host_folders` | bool | `false` | 克隆的仓库放在该文件夹下的 `<host>/<owner>/<repo>` 中，与 `ghq` 的布局相同。 |
| `fetch_every` | 分钟数或 `null` | `5` | 多久获取（fetch）一次每个项目的远程仓库。`null`：从不。**通用 › 在后台**。 |

`prefs` 的每个条目：

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `name` | 字符串或 `null` | `null` | 代替文件夹名显示的名称。 |
| `pinned` | bool | `false` | 置顶在侧边栏顶部。 |
| `archived` | bool | `false` | 已归档（不在侧边栏中）。 |
| `agent` | 智能体 ID 或 `null` | `null` | 其新会话默认使用的智能体。 |
| `worktree` | bool 或 `null` | `null` | 其新会话是否在工作树中进行。`null`：沿用在任何地方最后一次所做的选择。 |
| `pull` | bool | `false` | 当默认分支已检出且没有任何改动时，在 Ghost 启动时以及每次获取时拉取（pull）它。 |
| `icon` | `{"picture": path}` 或 `{"mark": {"text", "colour"}}` 或 `null` | `null` | 它的图标。`null`：如果它有自己的图标文件，就用该文件。 |
| `linear` | Linear 团队标识或 `null` | `null` | 从这个 Linear 团队读取它的议题，而不是从它的仓库托管平台读取。 |
| `over` | 对象 | `{}` | 该项目中与其他地方不同的设置。这里只能出现 `merge`、`uncredit`、`submodules`、`branch_kind`、`resume_at_reset` 和 `tidy_after`；其他任何键都是错误。 |
| `last` | 映射：智能体 ID → `{options, mode}` | `{}` | 在此处每个智能体的新会话中最后选择的选项（模型、思考强度等）和模式。 |

## 会话 {#sessions}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `settle_days` | 天数或 `null` | `7` | 闲置这么久的会话会被标为完成。`null`：从不。**通用 › 暂时完成**。 |
| `settle_merged` | bool | `true` | 拉取请求已合并或已关闭的会话会被标为完成。 |
| `keep_with_pr` | bool | `false` | 拥有未关闭拉取请求的会话永远不会被标为完成，无论闲置多久。 |
| `snoozes` | 字符串列表 | `["1h", "tomorrow", "next week"]` | 优先提供的稍后提醒选项，每项的写法与你在 **自定义…** 中输入的一样。 |
| `resume_at_reset` | bool | `true` | 因智能体用量限制而中止的一轮，会在限制重置时自动继续（前提是智能体说明了重置时间）。**通用 › 在后台**。 |
| `sort` | `"used"`、`"updated"`、`"begun"`、`"name"` | `"used"` | 侧边栏如何排列会话和项目：按你的最后一条消息、按任何活动、按开始时间，或按名称。 |
| `views` | `{name, kind, state, part, search}` 列表 | `[]` | 拉取请求和议题页面中保存的视图。`kind` 为 `"Pr"` 或 `"Issue"`。 |

## 工作树与 git {#worktrees-and-git}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `worktree` | bool | `false` | git 仓库中的新会话是否在自己的工作树中进行：即最后一次所做的选择。**新会话 › 在仓库中**。 |
| `branch_prefix` | 字符串 | `"ghost/"` | 工作树分支名的开头，位于写作者给出的名称之前。 |
| `branch_kind` | bool | `false` | 改为以工作类型作为分支名的开头（`feat/`、`fix/`）。 |
| `worktrees` | 路径或 `null` | `null`（`<home>/worktrees`） | 新工作树创建在哪里。之前创建的工作树留在原处。 |
| `submodules` | `"recursive"`、`"top"`、`"skip"` | `"recursive"` | 新工作树的子模块检出到什么程度。 |
| `tidy_after` | 天数或 `null` | `null` | 已归档会话的工作树保留多少天后被移除。有未提交改动的工作树会保留，其分支始终保留。`null`：一直保留。 |
| `merge` | `"squash"`、`"merge"`、`"rebase"` | `"squash"` | 从右侧栏合并拉取请求时使用的方式。**通用 › 拉取请求**。 |
| `uncredit` | bool | `false` | 从压缩（squash）后的提交信息中去掉智能体的 `Co-authored-by` 行和署名脚注。**源代码管理 › 正在合并**。 |

## 对话与输入框 {#chat-and-composer}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `send` | `"return"`、`"multiline"`、`"command"` | `"return"` | 用什么发送：Return；消息只有一行时用 Return，之后用 ⌘Return；或始终用 ⌘Return。Return 不发送时，它会换行。**对话 › 发送中**。 |
| `steer` | bool | `false` | 在一轮进行中发送的内容会引导这一轮，而不是等这一轮结束。 |
| `spell_check` | bool | `true` | 在输入框中给拼写错误的单词加下划线，其菜单中提供系统给出的建议。 |
| `view` | `"zen"`、`"quiet"`、`"normal"`、`"thinking"`、`"verbose"` | `"normal"` | 智能体的工作在对话中如何显示。⌃O 依次切换。 |
| `paragraphs` | bool | `false` | 回答以段落为单位流式显示，而不是逐词显示。 |
| `math` | bool | `false` | 将智能体所说内容中的 TeX（`$…$`、`$$…$$`）排版为公式。 |

## 外观 {#appearance}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `look` | `"dark"`、`"light"`、`"system"` | `"dark"` | 深色、浅色，或跟随系统。 |
| `theme_dark`, `theme_light` | 主题名称或 `null` | `null` | 每种外观使用的主题。`null`：Ghost 自带的主题。 |
| `backdrop` | 路径或 `null` | `null` | 在对话下方淡淡绘制的一张图片。 |
| `scale` | `80`、`90`、`100`、`110`、`125`、`150`、`175`、`200` | `100` | 窗口绘制的大小，以百分比表示。⌘+ 和 ⌘− 在这些值之间切换。 |
| `contrast` | bool | `false` | 把次要文字和边框画得更明显。 |
| `glass` | bool | `false` | 让桌面以模糊效果透过窗口显示。 |
| `mono` | `"jet_brains"`、`"sf_mono"`、`"menlo"` | `"jet_brains"` | 代码和数字使用的字体。 |
| `width` | `"comfortable"`、`"wide"`、`"full"` | `"comfortable"` | 对话和输入框的显示宽度（680 pt、880 pt，或整个面板）。 |
| `blue_orange` | bool | `false` | 新增和删除的行用蓝色和橙色显示，而不是绿色和红色。 |
| `twelve` | bool | `false` | 时间使用十二小时制。 |
| `language` | `"en"`、`"zh-CN"`、`"de"`、`"es"`、`"fr"`、`"ja"`、`"ko"`、`"pt-BR"`、`"ru"`、`"sv"` 或 `null` | `null` | 窗口的语言。`null`：如果 Ghost 支持系统语言则使用系统语言，否则使用英语。 |
| `terminal_size` | `"small"`、`"medium"`、`"large"` | `"medium"` | 终端中的文字大小。 |
| `shell` | `"login"`、`"zsh"`、`"bash"`、`"fish"` | `"login"` | 终端运行的 shell。`"login"`：你的 `$SHELL`。 |
| `diff_wrap` | bool | `true` | diff 中的长行自动换行，而不是在边缘截断。 |
| `no_spacing` | bool | `false` | diff 中不显示仅涉及空白的改动。 |
| `side_by_side` | bool | `false` | 以并排方式打开文件的 diff，并加宽右侧栏。 |
| `diff_folded` | bool | `false` | diff 中的文件默认折叠显示，按需逐个展开。 |
| `hidden_details` | `"context"`、`"plan"`、`"background"`、`"servers"`、`"terminals"`、`"lineage"`、`"linked"` 的列表 | `[]` | 在右侧栏中隐藏的会话详情部分。 |

## 通知与后台 {#notifications-and-background}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `notify` | `"off"`、`"banner"`、`"banner_and_sound"` | `"banner"` | 当 Ghost 不在前台时，某个会话结束一轮或需要你时，macOS 显示什么。**通用 › 你离开时**。 |
| `notify_on` | `{finished, failed, asks}`，每项为 bool | 全部为 `true` | 哪些事件发出通知：一轮结束、一轮失败、提出了问题。 |
| `awake` | bool | `true` | 智能体工作时阻止 Mac 进入睡眠。 |
| `confirm` | 对象 | 见下文 | Ghost 在执行哪些操作前先询问。**通用 › 确认**。 |

`confirm` 包含 `unpin`（默认 `false`）、`archive`（`false`）、`delete`（`true`）、`terminal`（`true`：在关闭有程序正在运行的终端之前）和 `quit`：⌘Q 的触发方式，`"hold"`（按住片刻，或快速按两次；默认值）、`"direct"`（按一次）或 `"double"`（快速按两次）。菜单中的“退出”总是立即退出。

## 写作者与听写 {#the-writer-and-dictation}

`writer`（**设置 › 写作**）决定由谁来撰写 Ghost 为你写的文本（标题、分支名、提交信息、拉取请求）：

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `writer.agent` | 智能体 ID 或 `null` | `null` | 负责撰写的智能体。`null`：会话自己的智能体。 |
| `writer.model` | 字符串或 `null` | `null` | 它的模型。`null`：智能体的默认模型。 |
| `writer.changes` | 字符串 | `""` | 你对提交信息和拉取请求的指示。 |
| `writer.titles` | 字符串 | `""` | 你对会话标题的指示。 |
| `writer.branches` | 字符串 | `""` | 你对工作树分支名的指示。 |
| `writer.transcripts` | 字符串 | `""` | 你对整理听写内容的指示。 |
| `writer.suggest` | bool | `false` | 每轮结束后，写出你接下来可能要说的话，按 Tab 采用。 |

`voice` 包含听写的设置（见[对话](../thread.md#dictation)）：

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `voice.key` | `"auto"`、`"toggle"`、`"hold"` | `"auto"` | ⇧⌘D 如何监听：按一下开始、再按一下停止，或按住时一直监听直到松开（`auto`）；按一下开始、再按一下停止（`toggle`）；仅在按住时（`hold`）。 |
| `voice.language` | 语言区域或 `null` | `null` | 要识别的语言（`"en-US"`）。`null`：系统的语言。 |
| `voice.dictionary` | 字符串列表 | `[]` | 要识别的词语，每项一个。`"heard = written"` 会把听到的内容以另一种写法写出。 |
| `voice.no_fillers` | bool | `true` | 省略“um”、“uh”之类的填充词。 |
| `voice.clean` | bool | `false` | 让写作者在插入之前先整理听到的内容。 |

## 更新、日志、遥测 {#updates-logs-telemetry}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `check_updates` | bool | `true` | 在 Ghost 启动时以及每四小时检查一次更新。**关于**。 |
| `channel` | `"stable"`、`"nightly"` | `"stable"` | Ghost 更新到哪类构建。 |
| `log_days` | 数字 | `14` | Ghost 的日志在 `<home>/logs/` 中保留的天数。**关于 › 保留日志**。 |
| `telemetry` | `0`–`3` 或 `null` | `null` | Ghost 向其开发者报告多少信息。在 Ghost 首次打开、你做出选择之前为 `null`。见[遥测](../first-run.md#telemetry)。 |
| `editor` | 命令或 `null` | `null` | 用来打开文件的编辑器，以其命令指定。`null`：找到的第一个编辑器。**通用 › 文件**。 |

## 由窗口保存 {#kept-by-the-window}

这些键记录窗口自身的状态。你很少需要手动设置它们。

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `left` | 点数 | `256` | 侧边栏的宽度，即你拖动后的宽度。 |
| `right` | 点数 | `400` | 右侧栏的宽度。 |
| `frame` | `[x, y, width, height]` 或 `null` | `null` | Ghost 上次退出时窗口的位置。`null`：居中。 |
