---
title: settings.json
description: ~/.u0/settings.json 的每个键、它的类型、默认值以及在哪里修改。
---

# settings.json

u0 把设置保存在其主目录（`~/.u0`，或 `U0_HOME`）中的 `settings.json` 里。几乎每个键在 **设置**（⌘,）中都有对应的控件，在那里修改就会写入该文件。你也可以在 u0 关闭时编辑该文件。

关于该文件的规则：

- u0 不认识的键会被视为错误（该文件以 `deny_unknown_fields` 读取）。此时 u0 以默认值启动，说明文件为何无法读取，并且在你修复或移走该文件之前**不保存任何内容**。它从不覆盖一个它无法读取的文件。
- 省略的键取其默认值。
- 组织的[策略](policy.md)可以设置某个键的默认值，或将其锁定。被锁定的设置显示为禁用，并注明由谁管理。
- 在控制多台 Mac 的窗口中，每台机器的设置是它自己的 `settings.json`：设置对话框顶部显示一个机器选择器，你所做的修改会保存在该机器上。外观、按键以及窗口自身的状态仍属于这台 Mac。

## 智能体 {#agents}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `enabled` | 智能体 ID 列表 | `["claude-acp", "codex-acp", "antigravity-acp"]` | 已开启的智能体，按列出的顺序排列。**设置 › 智能体**。 |
| `commands` | 映射：智能体 ID → 命令行 | `{}` | 你自己的一条命令行，用它来启动智能体，而不是按注册表的方式（你自己的构建、版本管理器提供的版本）。在该智能体的页面中。 |
| `env` | 映射：智能体 ID →（名称 → 值或 `null`） | `{}` | 每个智能体启动时带的变量。`null` 表示该值是保存在钥匙串中的机密。在该智能体的页面中。 |
| `models` | 映射：智能体 ID → 模型 ID 列表 | `{}` | 你自己的模型 ID，与智能体列出的模型一起提供。 |
| `favourites` | `{agent, model}` 列表 | `[]` | 在模型选择器中加星标的模型，在那里排在最前。 |
| `outside_acts` | 布尔值 | `false` | u0 之外的智能体是否可以通过 [u0 的工具](../tools-for-agents.md#access-levels)在线程中发言或停止其一轮，每次都在那里询问你。**设置 › 智能体 › u0 之外的智能体可以执行操作**。 |
| `no_tools` | 智能体 ID 列表 | `[]` | 其线程不提供 [u0 的工具](../tools-for-agents.md)的智能体。开关在该智能体的页面中。 |
| `check_agents` | bool | `true` | 每小时重新读取 ACP 注册表，查找更新的智能体版本。**关于**。 |
| `hidden_models` | 映射：智能体 ID → 模型 ID 列表 | `{}` | 在模型选择器中隐藏的模型。 |
| `custom_agents` | 映射：ID → `{name, command}` | `{}` | 在 ACP 注册表之外添加的独立自定义 ACP 智能体。 |
| `health_check_mins` | 数字 | `5` | 后台智能体健康检查的分钟间隔。 |
| `compact_idle` | bool | `true` | 在后台压缩空闲线程。 |

## 项目 {#projects}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `folders` | 路径列表 | `[]` | 项目列表，最近添加的排在最前。 |
| `prefs` | 映射：文件夹 → 项目偏好 | `{}` | 你为每个项目所做的设置（见下文）。**设置 › 项目**。 |
| `projects_in` | 路径或 `null` | `null`（`~/Documents/u0`） | 新建和克隆的项目放在哪里。 |
| `host_folders` | bool | `false` | 克隆的仓库放在该文件夹下的 `<host>/<owner>/<repo>` 中，与 `ghq` 的布局相同。 |
| `fetch_every` | 分钟数或 `null` | `5` | 多久获取（fetch）一次每个项目的远程仓库。`null`：从不。**通用 › 在后台**。 |

`prefs` 的每个条目：

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `name` | 字符串或 `null` | `null` | 代替文件夹名显示的名称。 |
| `pinned` | bool | `false` | 置顶在侧边栏顶部。 |
| `archived` | bool | `false` | 已归档（不在侧边栏中）。 |
| `agent` | 智能体 ID 或 `null` | `null` | 其新线程默认使用的智能体。 |
| `worktree` | bool 或 `null` | `null` | 其新线程是否在工作树中进行。`null`：沿用在任何地方最后一次所做的选择。 |
| `pull` | bool | `false` | 当默认分支已检出且没有任何改动时，在 u0 启动时以及每次获取时拉取（pull）它。 |
| `icon` | `{"picture": path}` 或 `{"mark": {"text", "colour"}}` 或 `null` | `null` | 它的图标。`null`：如果它有自己的图标文件，就用该文件。 |
| `linear` | Linear 团队标识或 `null` | `null` | 从这个 Linear 团队读取它的议题，而不是从它的仓库托管平台读取。 |
| `over` | 对象 | `{}` | 该项目中与其他地方不同的设置（`merge`、`uncredit`、`auto_pull`、`sign_commits`、`run_git_hooks`、`pr_drafts`、`pr_templates`、`commit_style`、`review_guides`、`submodules`、`branch_prefix`、`branch_kind`、`branch_template`、`worktree_from`、`copy_untracked`、`worktree_cleanup`、`keep_dirty_worktrees`、`tidy_after`、`worktree`、`ask_worktree`、`default_agent`、`default_model`、`ask_before_push`、`streaming`、`settle_days`、`resume_at_reset`）。其他任何键都是错误。 |
| `last` | 映射：智能体 ID → `{options, mode}` | `{}` | 在此处每个智能体的新线程中最后选择的选项（模型、思考强度等）和模式。 |
| `dictionary` | 字符串列表 | `[]` | 项目专属听写词条。 |
| `external_folders` | 路径列表 | `[]` | 关联到该项目的外部文件夹。 |
| `excluded_submodules` | 路径列表 | `[]` | 创建工作树时排除的子模块路径。 |
| `env` | 映射：名称 → 值 | `{}` | 提供给该项目中智能体与动作的环境变量及 `secret:<NAME>` 引用。 |
| `closed` | `{changes, publish}` | `{}` | 你在这里用 × 关掉的东西：`changes`，目前没有任何功能读取它（它记下关闭的输入框上方改动栏已不存在；保留它，是为了让带有此项的已保存设置文件仍能读取）；`publish`，右栏中发布该项目的提议（等它有过远程又失去后才再次提供）。 |

## 线程 {#threads}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `settle_days` | 天数或 `null` | `7` | 闲置这么久的线程会被标为完成。`null`：从不。**通用 › 暂时完成**。 |
| `settle_merged` | bool | `true` | 拉取请求已合并或已关闭的线程会被标为完成。 |
| `keep_with_pr` | bool | `false` | 拥有未关闭拉取请求的线程永远不会被标为完成，无论闲置多久。 |
| `snoozes` | 字符串列表 | `["1h", "tomorrow", "next week"]` | 优先提供的稍后提醒选项，每项的写法与你在 **自定义…** 中输入的一样。 |
| `resume_at_reset` | bool | `true` | 因智能体用量限制而中止的一轮，会在限制重置时自动继续（前提是智能体说明了重置时间）。**通用 › 在后台**。 |
| `sort` | `"used"`、`"updated"`、`"begun"`、`"name"` | `"used"` | 侧边栏如何排列线程和项目：按你的最后一条消息、按任何活动、按开始时间，或按名称。 |
| `project_grouping` | `"by_project"`、`"by_machine"`、`"flat"` | `"by_project"` | 侧边栏如何对线程分组。 |
| `project_order` | `"manual"`、`"activity"`、`"name"` | `"manual"` | 侧边栏如何排列项目。 |
| `working_section` | bool | `true` | 将运行中或等待你的线程置顶显示。 |
| `two_line_projects` | bool | `false` | 在侧边栏项目名下显示当前分支。 |
| `ask_worktree` | bool | `false` | 每次新建线程时询问是在本地检出还是新工作树中工作。 |
| `default_agent` | 智能体 ID 或 `null` | `null` | 新线程的默认智能体（`null`：首个已开启的智能体）。 |
| `default_model` | 模型 ID 或 `null` | `null` | 新线程的默认模型（`null`：智能体的默认模型）。 |
| `default_modes` | 映射：智能体 ID → 模式 ID | `{}` | 各智能体新线程的默认模式。 |
| `ask_before_push` | bool | `true` | 在线程中执行 `git push` 前询问确认。 |
| `streaming` | bool | `true` | 流式显示智能体回复。 |
| `archive_after` | 天数或 `null` | `30` | 线程标为完成后多少天自动归档（`null`：从不）。 |
| `snooze_until_reset` | bool | `true` | 遇到用量限制时提供“稍后提醒至限制重置”。 |
| `views` | `{name, kind, state, part, search}` 列表 | `[]` | 拉取请求和议题页面中保存的视图。`kind` 为 `"Pr"` 或 `"Issue"`。 |

## 工作树与 git {#worktrees-and-git}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `worktree` | bool | `false` | git 仓库中的新线程是否在自己的工作树中进行：即最后一次所做的选择。**新线程 › 在仓库中**。 |
| `branch_prefix` | 字符串 | `"u0/"` | 工作树分支名的开头，位于写作者给出的名称之前。 |
| `branch_kind` | bool | `false` | 改为以工作类型作为分支名的开头（`feat/`、`fix/`）。 |
| `worktrees` | 路径或 `null` | `null`（`<home>/worktrees`） | 新工作树创建在哪里。之前创建的工作树留在原处。 |
| `submodules` | `"recursive"`、`"top"`、`"skip"` | `"recursive"` | 新工作树的子模块检出到什么程度。 |
| `tidy_after` | 天数或 `null` | `null` | 已归档线程的工作树保留多少天后被移除。有未提交改动的工作树会保留，其分支始终保留。`null`：一直保留。 |
| `merge` | `"squash"`、`"merge"`、`"rebase"` | `"squash"` | 从右侧栏合并拉取请求时使用的方式。**源代码管理与议题**。 |
| `uncredit` | bool | `false` | 从压缩（squash）后的提交信息中去掉智能体的 `Co-authored-by` 行和署名脚注。**源代码管理与议题**。 |
| `worktree_from` | `"origin_default"`、`"local_main"`、`"current_branch"` | `"origin_default"` | 新工作树默认从何处检出。 |
| `branch_template` | 字符串 | `"{type}/{thread}"` | 新工作树的分支命名模板。 |
| `copy_untracked` | 字符串 | `".env, .env.local"` | 复制到新工作树中的未跟踪文件列表。 |
| `worktree_cleanup` | `"when_archived"`、`"when_merged"`、`"never"` | `"when_archived"` | 已完成工作树的清理时机。 |
| `keep_dirty_worktrees` | bool | `true` | 清理工作树时跳过仍有未提交改动的工作树。 |
| `auto_pull` | bool | `false` | 启动和获取时自动拉取干净的默认分支。 |
| `sign_commits` | bool | `false` | 对 u0 发起的提交进行签名。 |
| `run_git_hooks` | bool | `true` | 在提交和推送时运行仓库的 git 钩子。 |
| `pr_drafts` | bool | `false` | 默认以草稿形式创建拉取请求。 |
| `pr_templates` | bool | `true` | 遵循仓库的拉取请求模板。 |
| `commit_style` | `"match_history"`、`"conventional"`、`"plain"` | `"match_history"` | 默认提交信息风格。 |
| `review_guides` | bool | `true` | 启用拉取请求评审导览。 |

## 对话与输入框 {#chat-and-composer}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `send` | `"return"`、`"multiline"`、`"command"` | `"return"` | 用什么发送：Return；消息只有一行时用 Return，之后用 ⌘Return；或始终用 ⌘Return。Return 不发送时，它会换行。**对话 › 发送**。 |
| `steer` | bool | `false` | 在一轮进行中发送的内容会引导这一轮，而不是等这一轮结束。 |
| `spell_check` | bool | `true` | 在输入框中给拼写错误的单词加下划线，其菜单中提供系统给出的建议。 |
| `view` | `"zen"`、`"quiet"`、`"normal"`、`"thinking"`、`"verbose"` | `"normal"` | 智能体的工作在对话中如何显示。⌃O 依次切换。 |
| `paragraphs` | bool | `false` | 回答以段落为单位流式显示，而不是逐词显示。 |
| `math` | bool | `false` | 将智能体所说内容中的 TeX（`$…$`、`$$…$$`）排版为公式。 |
| `rich_composer` | bool | `true` | 输入框内富文本格式、代码块和内联芯片。 |
| `ask_follow_up` | bool | `false` | 一轮进行中发送消息时每次询问排队或引导。 |
| `slash_skills` | bool | `true` | 在 `/` 菜单中列出智能体和仓库技能。 |
| `open_subagents` | `"right_panel"`、`"split"`、`"new_window"` | `"right_panel"` | 子智能体和侧聊的打开位置。 |
| `announcements` | `"replies_and_alerts"`、`"alerts_only"`、`"off"` | `"replies_and_alerts"` | 屏幕阅读器在线程中的播报内容。 |
| `title_status` | bool | `false` | 在窗口标题中放入当前线程状态。 |

## 外观 {#appearance}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `look` | `"dark"`、`"light"`、`"system"` | `"dark"` | 深色、浅色，或跟随系统。 |
| `theme_dark`, `theme_light` | 主题名称或 `null` | `null` | 每种外观使用的主题（深色可选 `"g100"` / `"g90"`，浅色可选 `"white"` / `"g10"`，或 `themes/` 中的主题）。`null`：深色为 Carbon G100，浅色为 Carbon White。 |
| `backdrop` | 路径或 `null` | `null` | 在对话下方淡淡绘制的一张图片。 |
| `scale` | `80`、`90`、`100`、`110`、`125`、`150`、`175`、`200` | `100` | 窗口绘制的大小，以百分比表示。⌘+ 和 ⌘− 在这些值之间切换。 |
| `contrast` | bool | `false` | 把次要文字和边框画得更明显。 |
| `glass` | bool | `false` | 让桌面以模糊效果透过窗口显示。 |
| `motion` | `"system"`、`"reduce"`、`"full"` | `"system"` | 窗口里的动效：跟随 macOS 的“减弱动态效果”，始终减弱（面板淡入而不滑动），或完整显示。 |
| `mono` | `"plex_mono"`、`"jet_brains"`、`"sf_mono"`、`"menlo"` | `"plex_mono"` | 代码和数字使用的字体。 |
| `width` | `"comfortable"`、`"wide"`、`"full"` | `"comfortable"` | 对话和输入框的显示宽度（720 pt、880 pt，或整个面板）。 |
| `blue_orange` | bool | `false` | 新增和删除的行用蓝色和橙色显示，而不是绿色和红色。 |
| `twelve` | bool | `false` | 时间使用十二小时制。 |
| `language` | `"en"`、`"zh-CN"`、`"de"`、`"es"`、`"fr"`、`"ja"`、`"ko"`、`"pt-BR"`、`"ru"`、`"sv"` 或 `null` | `null` | 窗口的语言。`null`：如果 u0 支持系统语言则使用系统语言，否则使用英语。 |
| `terminal_size` | `"small"`、`"medium"`、`"large"` | `"medium"` | 终端中的文字大小。 |
| `shell` | `"login"`、`"zsh"`、`"bash"`、`"fish"` | `"login"` | 终端运行的 shell。`"login"`：你的 `$SHELL`。 |
| `diff_wrap` | bool | `true` | diff 中的长行自动换行，而不是在边缘截断。 |
| `no_spacing` | bool | `false` | diff 中不显示仅涉及空白的改动。 |
| `side_by_side` | bool | `false` | 以并排方式打开文件的 diff，并加宽右侧栏。 |
| `diff_folded` | bool | `false` | diff 中的文件默认折叠显示，按需逐个展开。 |
| `diff_theme_colors` | bool | `false` | 在 diff 中使用主题配色。 |
| `diff_auto_layout` | bool | `true` | 按面板宽度自动选择统一或并排 diff 布局。 |
| `diff_context` | 数字 | `3` | diff 中每处改动周围显示的未改动行数。 |
| `highlight_words` | bool | `true` | 高亮改动行内部变更的词与字符。 |
| `collapse_large_diffs` | bool | `true` | 自动折叠超过 400 行改动的 diff 文件。 |
| `hide_test_files` | bool | `false` | 在 diff 中将测试文件折叠为单行汇总。 |
| `open_prs_here` | bool | `true` | 在 u0 内置拉取请求视图中打开 PR 链接。 |
| `proactive_panels` | bool | `true` | 编辑文件或关联 PR 时主动展开右侧面板。 |
| `hidden_details` | `"context"`、`"plan"`、`"background"`、`"servers"`、`"terminals"`、`"lineage"`、`"linked"` 的列表 | `[]` | 在右侧栏中隐藏的线程详情部分。 |

### 主题 {#themes}

**设置 › 外观 › 主题** 列出所有主题：u0 自带的 Carbon 四款，以及你的主题——每个都是 `themes/` 中采用 gpui-kit 主题格式的文件。点选一张卡片，即用它绘制对应的外观（深色或浅色）；上方的 **浅色主题** 和 **深色主题** 显示每种外观使用哪一个。

- 卡片上的 **自定义** 会在设置之上打开一个面板：主要颜色和取色器、文字在背景上的可读性（对比度）、预览，以及每种颜色的十六进制值。修改即时保存。u0 自带的主题会作为你的副本来自定义。面板底部有 **复制一份**、重置和 **移除主题**。
- **导入 VS Code 主题…** 和 **导入 gpui-kit 主题 JSON…** 把其他应用的主题文件变成你的主题。如果你已有同名主题，u0 会先询问：替换它，还是两个都保留（导入的使用下一个可用的名称）。
- 搜索框会查找你的主题；输入两个字符以上时，也会搜索 [Open VSX](https://open-vsx.org) 上的颜色主题扩展。**添加** 会下载它，并按上述方式导入其中的主题。
- **让智能体来写** 请写作者（你的某个智能体，见[写作者与听写](#the-writer-and-dictation)）按你的描述，为当前外观写一个主题。u0 自己没有模型。
- **新建主题** 以当前绘制的主题为起点创建你的主题。

**减弱动态效果**（`motion`）位于 **动效与视觉效果** 中。

## 通知与后台 {#notifications-and-background}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `notify` | `"off"`、`"banner"`、`"banner_and_sound"` | `"banner"` | 当 u0 不在前台时，某个线程结束一轮或需要你时，macOS 显示什么。**通用 › 你离开时**。 |
| `notify_on` | `{finished, failed, asks}`，每项为 bool | 全部为 `true` | 哪些事件发出通知：一轮结束、一轮失败、提出了问题。 |
| `in_app_notify` | bool | `true` | 对未聚焦线程显示应用内通知提示。 |
| `awake` | bool | `true` | 智能体工作时阻止 Mac 进入睡眠。 |
| `background_activity` | `"battery_saver"`、`"balanced"`、`"full_speed"` | `"balanced"` | 窗口隐藏时后台任务的活跃程度。 |
| `continue_after_restart` | bool | `true` | 更新或重启后自动恢复工作中的线程。 |
| `confirm` | 对象 | 见下文 | u0 在执行哪些操作前先询问。**通用 › 确认**。 |

`confirm` 包含 `unpin`（默认 `false`）、`archive`（`false`）、`delete`（`true`）、`terminal`（`true`：在关闭有程序正在运行的终端之前）和 `quit`：⌘Q 的触发方式，`"hold"`（按住片刻，或快速按两次；默认值）、`"direct"`（按一次）或 `"double"`（快速按两次）。菜单中的“退出”总是立即退出。

## 写作者与听写 {#the-writer-and-dictation}

`writer`（**设置 › 写作**）决定由谁来撰写 u0 为你写的文本（标题、分支名、提交信息、拉取请求）：

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `writer.agent` | 智能体 ID 或 `null` | `null` | 负责撰写的智能体。`null`：线程自己的智能体。 |
| `writer.model` | 字符串或 `null` | `null` | 它的模型。`null`：智能体的默认模型。 |
| `writer.changes` | 字符串 | `""` | 你对提交信息和拉取请求的指示。 |
| `writer.titles` | 字符串 | `""` | 你对线程标题的指示。 |
| `writer.branches` | 字符串 | `""` | 你对工作树分支名的指示。 |
| `writer.transcripts` | 字符串 | `""` | 你对整理听写内容的指示。 |
| `writer.guides` | 字符串 | `""` | 你对拉取请求评审导览的指示。 |
| `writer.suggest` | bool | `false` | 每轮结束后，写出你接下来可能要说的话，按 Tab 采用。 |

`voice` 包含听写的设置，位于 **设置 › 语音**（见[对话](../thread.md#dictation)）：

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `voice.key` | `"auto"`、`"toggle"`、`"hold"` | `"auto"` | ⇧⌘D 如何监听：按一下开始、再按一下停止，或按住时一直监听直到松开（`auto`）；按一下开始、再按一下停止（`toggle`）；仅在按住时（`hold`）。 |
| `voice.language` | 语言区域或 `null` | `null` | 要识别的语言（`"en-US"`）。`null`：系统的语言。 |
| `voice.dictionary` | 字符串列表 | `[]` | 要识别的词语，每项一个。`"heard = written"` 会把听到的内容以另一种写法写出。 |
| `voice.no_fillers` | bool | `true` | 省略 `voice.fillers` 中的词。**移除填充词**。 |
| `voice.fillers` | 字符串 | `"um, umm, uh, uhm, er, erm, ah, hmm, mm"` | 填充词，用逗号分隔；开启 `voice.no_fillers` 时在这台 Mac 上移除。**填充词**。 |
| `voice.clean` | bool | `false` | 让写作者在插入之前先整理听到的内容。**由写作者整理**。 |
| `voice.microphone` | 字符串或 `null` | `null` | 目前没有任何地方读取：听写使用系统的麦克风。 |
| `voice.live_words` | bool | `true` | 输入框随听随显示听到的内容；`false` 时只显示正在聆听。**说话时显示文字**。 |
| `voice.correction_cue` | 字符串 | `"scratch that"` | 听写时说出它，会撤回它之前说的那句话。`""`：不启用。**更正提示语**。 |

## 浏览器、快照、设备与实验室 {#browser-snapshots-devices-and-labs}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `open_pages` | `"preview"`、`"browser"` | `"preview"` | 你打开的网页在哪里显示：预览标签页（macOS）或你的浏览器（**设置 › 浏览器 › 网页打开位置**）。 |
| `browser` | 路径或 `null` | `null` | **在浏览器中打开** 使用的浏览器 `.app`（`null`：系统默认）。 |
| `ask_browser` | bool | `false` | 目前没有任何功能读取它：链接在 `browser` 中打开。保留它，是为了让带有此项的已保存设置文件仍能读取。 |
| `cmd_click_urls` | bool | `true` | 按住 `⌘` 点击终端打印的地址时在网页打开的位置打开；按住 `⌥⌘` 点击则在你的浏览器中打开。 |
| `dev_server` | `"show"`、`"open"`、`"ignore"` | `"show"` | 开发服务器启动时的处理方式。 |
| `forward_port` | 数字 | `41873` | 转发远程开发服务器端口时的起始本地端口。 |
| `snapshots` | `{shortcut, include_text, never_capture, sound, flash, animations}` | 见默认值 | 桌面窗口快照捕获与反馈设置。 |
| `use_devices` | bool | `true` | 在右侧栏显示模拟器与设备面板。 |
| `agent_devices` | `"ask"`、`"allowed"`、`"off"` | `"ask"` | 智能体是否可通过 MCP 操控模拟器与设备。 |
| `local_simulators` | bool | `true` | 线程请求时允许启动本机模拟器。 |
| `max_simulators` | 数字 | `2` | 本机同时运行的模拟器上限。 |
| `machine_id` | `"color_and_name"`、`"name_only"`、`"off"` | `"color_and_name"` | 标题栏如何标识其他机器。 |
| `hidden_servers` | 字符串列表 | `[]` | 在当前窗口侧边栏中暂时隐藏的已配对服务器名称。 |
| `pr_sharing` | bool | `false` | 向只读配对的 Mac 共享拉取请求状态。 |
| `phone_confirm`, `phone_live_activities`, `phone_dev_server` | bool | `false` | 已配对手机功能的预览开关。 |
| `otlp_endpoint` | 字符串或 `null` | `null` | OpenTelemetry OTLP 收集器端点。 |
| `agent_feedback` | bool | `false` | 允许智能体提交关于 u0 本身的反馈。 |
| `workflows_enabled` | bool | `true` | 启用多阶段工作流。 |
| `skipped_runs_days` | 数字 | `7` | 因并发限制跳过的运行在历史中保留的天数。 |
| `fast_worktree` | bool | `true` | 在文件系统支持时使用写时复制克隆工作树。 |
| `simlock_pools` | bool | `false` | 从共享 Simlock 池租用模拟器。 |

## 更新、日志、遥测 {#updates-logs-telemetry}

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `check_updates` | bool | `true` | 在 u0 启动时以及每四小时检查一次更新。**关于**。 |
| `channel` | `"stable"`、`"nightly"` | `"stable"` | u0 更新到哪类构建。 |
| `log_days` | 数字 | `14` | u0 的日志在 `<home>/logs/` 中保留的天数。**关于 › 保留日志**。 |
| `telemetry` | `0`–`3` 或 `null` | `null` | u0 向其开发者报告多少信息。在 u0 首次打开、你做出选择之前为 `null`。见[遥测](../first-run.md#telemetry)。 |
| `editor` | 命令或 `null` | `null` | 用来打开文件的编辑器，以其命令指定。`null`：找到的第一个编辑器。**通用 › 文件**。 |

## 由窗口保存 {#kept-by-the-window}

这些键记录窗口自身的状态。你很少需要手动设置它们。

| 键 | 类型 | 默认值 | 含义 |
|---|---|---|---|
| `left` | 点数 | `256` | 侧边栏的宽度，即你拖动后的宽度。 |
| `right` | 点数 | `400` | 右侧栏的宽度。 |
| `frame` | `[x, y, width, height]` 或 `null` | `null` | u0 上次退出时窗口的位置。`null`：居中。 |
