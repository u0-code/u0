---
title: 计划任务、工作流与 Webhook
description: 按计划或由 Webhook 运行提示，并把智能体、命令和提问串成工作流。
---

# 计划任务、工作流与 Webhook

任务和工作流由 Ghost 的服务器运行，而不是窗口。只要服务器在运行，窗口关闭后它们也会继续运行：应用打开时服务器在应用中运行，或者通过 `ghost service install`（见 [CLI](reference/cli.md#the-launchd-service)）或 `ghost-server` 一直运行。

## 计划任务 {#scheduled-tasks}

计划任务会按计划，在某个项目中，向某个智能体发送一条提示。每次运行都是一个独立的会话（或同一会话的下一轮），像其他会话一样显示在侧边栏中。

打开 **设置 › 计划任务**，选择 **新任务**：

| 字段 | 含义 |
|---|---|
| **名称** | 任务的标题。留空时取提示的第一行（40 个字符）。 |
| **提示** | 每次对智能体说的话。 |
| **智能体** | 对谁说，以及使用哪个模型。 |
| **项目** | 在哪里工作。 |
| **每次运行** | 对智能体 **发送其提示**，或 **启动** 你的某个[工作流](#workflows)，以提示（按 Webhook 或事件的方式填好）为其输入。 |
| **计划** | **每隔**（分钟数，或 `2h` 这样的时长）、**每天**（一个时间和一周中的哪几天；一天都不选表示每天）、**一次**（如 `tomorrow`、`5pm`、`in 3h`），**通过 Webhook** 或 **在事件发生时**。 |
| **每次运行** | **每次运行新开会话**，或 **同一个会话，继续进行**（每次运行都是第一次运行所开会话的下一轮）。 |
| **在独立的工作树中** | 与“每次运行新开会话”一起用：每次运行都在一个新分支上工作，与你的检出隔开。 |

新任务默认为每天，工作日 09:00。每个任务的「⋯」菜单有 **编辑**、**立即运行** 和 **删除**，它的开关用于暂停任务。

运行的规则：

- 服务器每 20 秒检查一次到期的任务。
- 上一次运行还没结束时，跳过本次运行。
- **每隔** 任务在保存后过满一个完整间隔才第一次运行。
- **一次** 任务运行成功后会删除自己。
- 运行完成或失败时会像任何一轮那样通知你（见[设置](reference/settings.md#notifications-and-background)中的 `notify_on`），其会话会标为未读。
- 失败的运行会在任务下方显示错误。

`ghost tasks` 在终端中列出任务。

### 在事件发生时 {#on-an-event}

计划为 **在事件发生时** 的任务，会在你的会话中发生某事时运行：**一个回合结束**，或 **智能体提出请求**（向你提问）。**仅限此任务的项目** 将其限定在任务所在项目的会话中。它只在你的回合上运行，绝不会在 Ghost 自行发起的回合（任务的、工作流的，或某个智能体在另一个会话中说的话）上运行，因此任务不会因自身的运行而运行。智能体曾在其中说过话的会话仍是你的：你在那里的下一个回合会再次运行该任务。提示中的 `{{event}}` 是发生的事情，标记为来自外部的数据（`<event untrusted>`）。

### tasks.json {#tasksjson}

任务保存在 `<home>/tasks.json` 中，由服务器写入。Ghost 关闭时你可以编辑它：

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

| 字段 | 类型 | 含义 |
|---|---|---|
| `id` | 数字 | 任务的编号。 |
| `title`、`prompt` | 字符串 | |
| `agent` | 智能体 ID | |
| `model`、`mode` | 字符串或 `null` | 要设置的模型和会话模式。`null`：智能体的默认值。 |
| `project` | 路径 | |
| `schedule` | 对象 | `{"every": 分钟数}`、`{"daily": {"minute": 午夜后的分钟数, "days": [0–6，0 为周日]}}`（`days` 为空表示每天，按本地时间）、`{"once": Unix 秒数}`，或 `{"hook": {"token": "<token>", "signed": 布尔值, "only": [{"at": "body.action", "is": ["opened"]}]}}`，或 `{"on": {"moment": "turn_ended" 或 "asks", "here": 布尔值}}`。 |
| `workflow` | 字符串或 `null` | 每次运行启动的工作流，以填好的提示为输入。 |
| `paused` | 布尔值 | |
| `same` | 布尔值 | 同一个会话，继续进行。 |
| `worktree` | 布尔值 | 每次运行都在新工作树中。 |
| `session`、`next`、`last`、`runs` | | 由服务器维护：它继续的会话、下一次运行、上一次运行的结果、运行次数。 |

未知字段会报错。读不了的文件会原样保留，在修好或移走之前，不会运行或保存任何任务。

### 当用量限额中止一轮时 {#when-a-usage-limit-stops-a-turn}

当智能体因达到用量限额而中止一轮，并说明限额何时重置时，Ghost 会安排这一轮在那时继续：一个一次性任务，在同一会话中发送 "Continue"。错误卡片会显示何时继续，并提供 **不要继续** 来取消。可通过 **通用 › 在后台 › 限额重置后继续**（`resume_at_reset`）关闭此功能。

这只适用于你从窗口发起的轮次。由任务或工作流运行的轮次不会以这种方式继续。

## Webhook {#webhooks}

计划为 **通过 Webhook** 的任务，会在有东西向它的地址发送 HTTP `POST` 时运行：

```
POST /hook/<token>
```

- 令牌是你选择 **通过 Webhook** 时生成的 22 个随机字符。**新地址** 会给任务一个新的令牌：旧地址不再运行它。
- **已签名**（新 Webhook 默认开启）：只有正文用任务的密钥签名的调用才会运行它：正文的 HMAC-SHA256，十六进制，位于 `X-Hub-Signature-256`（`sha256=…`，与 GitHub 的签名方式相同）或 `X-Signature-256` 中。按发送方所持的样子输入密钥；它保存在运行该任务的 Mac 的钥匙串中（`hook/<token>`），并随 **新地址** 一起转移。不签名时，令牌是保护这个地址的唯一手段：任何拿到它的人都能运行该任务。
- 该地址由 Ghost 监听其他设备的地方提供，所以必须开启监听（**设置 › 设备**，见[远程控制](remote.md)）。表单会显示 `http://127.0.0.1:<port>/hook/<token>`，隧道开启时还会显示 `https://<tunnel host>/hook/<token>`。
- **仅当** 用于限定哪些调用会运行它：例如 `body.action = opened, closed; header.x-github-event = pull_request` 这样的规则，每条是 JSON 正文的一个字段（`body.<a>.<b>`）或一个请求头（`header.<name>`）以及它可取的值，用 `;` 分隔。每条规则都必须成立。留空时每个调用都会运行它。
- 发送方命名的投递（`X-GitHub-Delivery`、`X-GitLab-Event-UUID` 或 `X-Delivery-Id`）只运行一次；再次发送时什么也不运行。再次发送的已签名正文同样什么也不运行，无论它声称是哪次投递、签名以何种方式书写。收到过的调用会在 `<home>/heard.json` 中保留一个月，因此重启也不会忘记。一个月后，同一个已签名正文会再次运行：GitHub 式的签名不带时间，无法区分旧调用和新调用。
- 请求正文（最多 1 MiB）会替换提示中的每个 `{{body}}`，`{{body.<a>.<b>}}` 替换为它的一个字段，`{{header.<name>}}` 替换为一个请求头，每个都用 `<webhook-data untrusted>` … `</webhook-data>` 包起来，让智能体知道它来自外部；发送内容中的结束标记会写成 `<\/…`，因此只有 Ghost 的标记能结束它。它仍可能试图引导智能体：请像对待任何粘贴进来的内容一样看待它。提示中没有它们时，不使用正文。
- 任务运行时 Ghost 返回 `202 Accepted`；**仅当** 中的某条规则不成立时返回 `204 No Content`；之前已收到过的投递返回 `200 OK`；未按要求签名的调用返回 `401 Unauthorized`；正文过长返回 `413`；令牌未知、任务已暂停，或[策略](reference/policy.md)禁止 Webhook 时，返回 `404 Not Found`。
- Ghost 未运行时收到的调用不会被保留。


## 工作流 {#workflows}

工作流是分阶段的运行：一个接一个的步骤，向智能体发送提示、运行命令，或者询问你。服务器把它作为一次运行来执行，重启后仍能继续：每一步结束时都会保存其结果，失败的步骤会让运行在那里暂停。

每个工作流都是你放在 `<home>/workflows/` 中的一个 JSON 文件，以文件名命名（`release.json` 即工作流 `release`）。Ghost 每次列出或启动工作流时都会读取这些文件。

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

顶层：

| 键 | 含义 |
|---|---|
| `about` | 与工作流一起显示的一行说明。 |
| `agent` | 未指定智能体的智能体步骤所用的智能体。 |
| `project` | 未指定文件夹的步骤所用的文件夹。 |
| `steps` | 按顺序排列的步骤。每个步骤的 `name` 必须唯一。 |

步骤：

| 步骤 | 键 | 作用 |
|---|---|---|
| `agent` | `name`、`prompt`；可选 `agent`、`model`、`project`、`worktree`、`after` | 在一个标题为 “workflow · step” 的新会话中，对智能体说 `prompt`。有 `after`（前面某个智能体步骤的名称）时，改为在那个步骤的会话中继续。步骤的结果是智能体的最后一个回答。 |
| `run` | `name`、`command`；可选 `dir` | 在 `dir`（或项目）中用 `/bin/sh -c` 运行 `command`。结果是它的标准输出。退出码非零时，运行会暂停，并附上它的错误输出。 |
| `ask` | `name`、`title`、`choices`；可选 `stop` | 询问你，每个选项一个按钮。你的选择就是步骤的结果。选择 `stop` 中列出的选项会停止运行。 |

取值：`{{input}}` 是运行的输入，`{{<step name>}}` 是前面某一步的结果（去掉首尾空白）。在 `run` 命令中，每个值都作为参数交给 `/bin/sh`，每个 `{{name}}` 都变成对该参数的引用，并按它所在的位置（无引号、双引号或单引号中）加上引号：shell 绝不会把值读作命令的一部分。

命令本身仍可能把一个单词当作代码读取，因此 Ghost 会像 bash 那样解析命令（使用 [brush-parser](https://crates.io/crates/brush-parser)），只接受作为一个名字写得明白的命令的参数出现的 `{{name}}`：`echo`、`printf`（不能作为它的格式，也不能带 `-v`）、`cd` 或某个程序，可以不加引号、加引号或在 `$( )` 之内。把它放在其他任何地方的命令都会被拒绝（运行会暂停并说明原因）：其他内建命令之后（`eval`、`let`、`declare`、`export`、`read`、`test`、`trap`、`source` 等）、作为命令本身、算术之中（`$(( ))`、`$[ ]`、`(( ))`、数组下标、`${x:offset}`）、`[[ ]]` 之内、赋值、重定向、here 文档、`case` 模式或函数之中，以及无法解析的命令。命令所处理的值请放在 `--` 之后（`git fetch origin -- {{ref}}`），这样以 `-` 开头的值不会被当作选项；而把其参数当作代码运行的命令（`sh -c`、`ssh`、`xargs`）会把值当作代码运行。**设置 › 工作流** 的 **命令中的值** 也说明了这一点。

### 运行工作流 {#running-a-workflow}

在 **设置 › 工作流** 中，输入一个输入（可选），然后按工作流旁边的 **启动**。在 **运行记录** 下，你可以显示需要你处理的、正在进行的，或全部运行。一次运行会显示每个已完成步骤及其结果的第一行和 **打开**（其会话）；等待 `ask` 步骤的运行会显示其选项；已暂停的运行会显示原因，并提供 **继续**（重新运行该步骤）。**取消** 会停止运行及正在进行的一轮。

运行记录保存在 `<home>/runs.json` 中。服务器停止时正在进行的运行，会在服务器再次启动后继续。
