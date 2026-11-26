---
title: Ghost 为智能体提供的工具
description: Ghost 提供给每个会话的 MCP 服务器，以及 Ghost 之外的智能体如何使用它。
---

# Ghost 为智能体提供的工具

Ghost 自己没有智能体，但它为你的智能体提供工具：一个名为 `ghost` 的 MCP 服务器，挂接到 Ghost 打开的每个会话上。通过它，智能体可以读取 Ghost 保存的会话、列出和添加你的项目并在其中查找、读取审阅和合并拉取请求并读取其检查、运行你的工作流、更改你的计划任务、创建和移除工作树、把工作交给另一个智能体、打开拉取请求、安排任务、向你提问，以及使用其所在机器上的模拟器。

## 会话如何获得这些工具 {#how-a-session-gets-them}

Ghost 打开会话时，通过 ACP 的 `mcpServers` 把服务器作为一个要运行的程序交给智能体：即带 `--mcp` 的 Ghost 自身程序，附带 `GHOST_HOME` 和该会话独有的密钥（`GHOST_MCP_KEY`）。这个程序只是一个小管道：它在标准输入输出上讲 MCP，并把一切转发给 Ghost 服务器，后者只监听 `127.0.0.1`。这个管道从不发送它的密钥：服务器先发来一个一次性的随机数，管道用密钥对它（以及端口）算出 HMAC 作答。Ghost 没在运行时监听那个端口的程序，得不到任何可用的东西。

端口和供 Ghost 之外智能体使用的密钥写在 `<home>/mcp.json` 中，只有你能读取（权限 0600）。它们在每次启动之间保持不变，所以用它们设置好的应用会一直可用：

- 端口只选一次，每次启动时重新占用。如果期间另一个程序占用了其中一个，Ghost 会改在另一个端口监听，把它写进 `mcp.json`，并告诉你：用旧地址设置的应用需要换成新地址。如果那是 HTTP 端口，密钥也会重新生成，因为占着那个端口的程序可能已从某个应用那里听到了它（见下文）。
- 密钥保存在钥匙串中，直到你在 **设置 › 智能体 › Ghost 之外的智能体的密钥** 中按下 **新密钥**。之后旧密钥不再允许任何人进入，已用它连上的智能体也不例外（它的下一次调用会被拒绝），新的密钥会写进 `mcp.json`。没有钥匙串的地方（Linux 上的 `ghost-server`），每次启动都会重新生成密钥。

要让某个智能体的会话不带 Ghost 的工具，在 **设置 › 智能体** 中打开它的页面并把工具关掉。该智能体的 ID 会被加入 [settings.json](reference/settings.md) 的 `no_tools`。

## 工具 {#the-tools}

| 工具 | 作用 | 谁可以调用 |
|---|---|---|
| `list_sessions` | Ghost 保存的会话，最新的在前（最多 100 个）：每个会话的智能体、ID、标题、项目、工作文件夹以及是否已归档。可选的 `project` 按文件夹筛选。 | 任何智能体 |
| `read_session` | 会话中说过的内容：你的消息（以 "You:" 开头）和智能体的回答。参数为 `agent` 和 `session`。 | 任何智能体 |
| `read_diff` | 文件夹中尚未提交的内容，以统一差异格式给出。`folder` 默认为调用方会话的工作位置；对 Ghost 之外的智能体，它必须位于你的某个项目或其工作树中。 | 任何智能体 |
| `delegate_task` | 把任务交给一个新 Ghost 会话中的智能体（你可以旁观），并等待它的回答。参数为 `prompt`、`title`，可选 `agent`、`project` 和 `worktree`。返回新会话的 ID 和它的最终回答。 | Ghost 中的会话；先询问你 |
| `open_pr` | 提交会话工作位置中未提交的内容，推送（如在默认分支上则推到独立分支），并用 `gh` 打开拉取请求。参数为 `title`、`body`，可选提交信息 `message`。 | Ghost 中的会话；先询问你 |
| `schedule_task` | 为智能体安排一条提示，每次都在新会话中运行：一次（`in_minutes`）或重复（`every_minutes`）。它会出现在 **设置 › 计划任务** 中，你可以在那里暂停或删除它。 | Ghost 中的会话；先询问你 |
| `ask_user` | 在会话的对话中向你提问并等待回答，可以是自由文本，也可以是 `choices` 之一。 | Ghost 中的会话 |
| `list_devices` | 服务器所在机器上的 iOS 模拟器和 Android 模拟器：ID、名称、平台、是否在运行；以及缺什么才能用上它们。 | 任何智能体 |
| `device_screen` | 正在运行的设备当前显示的内容，以图片形式给出。 | 任何智能体 |
| `use_device` | 启动或停止设备、轻点某一点、输入文本、按主屏幕键（Android 上还有返回或最近任务），或设为深色或浅色。 | Ghost 中的会话；先询问你 |
| `send_to_session` | 在 Ghost 保存的某个会话（`agent`、`session`）中说 `prompt`，作为它的下一轮：正在进行的一轮会先让它结束，绝不会被中断。带 `wait` 时返回其智能体的回答；否则由 `wait_for_turn` 返回。 | Ghost 中的会话；先在那个会话中询问你 |
| `wait_for_turn` | 等到会话中正在进行的一轮结束（`seconds`，未给出时为十分钟，最多一小时），并返回其智能体在最后一轮中说的话。 | 任何智能体 |
| `interrupt_session` | 停止会话中正在进行的一轮。 | Ghost 中的会话；先在那个会话中询问你 |
| `events` | Ghost 运行的会话中发生的事（一轮结束、智能体向你提问），每件都有编号：用最后一个编号（`after`）再次调用以继续跟进，带 `seconds` 则等待新事发生。`missed` 表示其中有些已不再保留。 | 任何智能体 |
| `list_projects` | 你的项目：它们的文件夹，与侧边栏列出的一致。 | 任何智能体 |
| `add_project` | 把一个 `folder`（完整路径）设为你的项目，排在列表最前；连到那个 Ghost 的每个窗口都会立刻显示它。不能是根目录、你的主文件夹或包含它的文件夹，也不能是 Ghost 的主目录或其中的文件夹。 | Ghost 中的会话；先询问你 |
| `search_sessions` | 标题或其中所说内容包含 `query`（不分大小写）的会话，最新的在前，格式与 `list_sessions` 相同。 | 任何智能体 |
| `find_files` | `folder` 中 git 能看到的文件里，路径按顺序包含 `query` 各字母的那些，与 Ghost 的文件查找方式相同（`smr` 能找到 `src/main.rs`），最匹配的在前，最多五十个。 | 任何智能体 |
| `search_code` | `words` 出现在 `folder` 中 git 能看到的文件里的位置：`path:line` 和那一行，最多五十处；`case`、`word` 和 `regex` 与文件标签页的搜索相同。 | 任何智能体 |
| `list_prs` | 项目远程所在托管平台（GitHub、GitLab、Bitbucket、Azure DevOps）上的拉取请求：`state`（未给出时为 open，还有 closed、merged、all），以及按平台语法写的 `search`。 | 任何智能体 |
| `read_pr` | 完整读取一个拉取请求：它的描述、其中和各行上的讨论、检查、审阅者，以及是否有冲突；带 `diff` 时还包括补丁。 | 任何智能体 |
| `review_pr` | 审阅一个拉取请求：`verdict` 为 approve、request-changes 或 comment，附上 `body`，以及 `lines`（对新版本各行的评论）。 | Ghost 中的会话；先询问你 |
| `merge_pr` | 合并一个拉取请求，之后删除其分支：`how`（squash、merge 或 rebase；未给出时按 **通用 › 拉取请求 › 合并方式** 的设置），立即合并或在检查通过后合并（`auto`）。 | Ghost 中的会话；先询问你 |
| `pr_checks` | 拉取请求的检查：每项的名称、状态、运行位置；在 GitHub Actions 上失败的检查还给出 `ci_log` 可读取的 `run`。 | 任何智能体 |
| `ci_log` | 检查中失败的某次 `run` 输出的末尾部分。 | 任何智能体 |
| `list_workflows` | 你的工作流：每个的名称、用途和步骤，或其文件无法读取的原因；以及它们的运行，包括各自的状态和已完成的步骤。只能读取的智能体拿不到 `run` 步骤的命令，也拿不到各步骤的结果。 | 任何智能体 |
| `start_workflow` | 用 `input` 开始一次 `workflow` 的运行：返回它的 ID。 | Ghost 中的会话；先询问你 |
| `list_tasks` | 计划任务：每个的 ID、标题、提示、智能体、项目、计划、是否已暂停，以及上次运行。绝不会给出 webhook 的地址。 | 任何智能体 |
| `change_task` | 暂停一个任务（`id`）、恢复它、立即运行或删除它（`change`）。 | Ghost 中的会话；先询问你 |
| `list_worktrees` | Ghost 为某个项目创建、位于其工作树文件夹中的工作树：每个的仓库、路径和分支。 | 任何智能体 |
| `make_worktree` | 像新会话那样为项目创建一个工作树：从远程的默认分支开始，分支以 `about` 命名；返回它的路径和分支。 | Ghost 中的会话；先询问你 |
| `remove_worktree` | 移除 Ghost 创建的一个工作树（`path`），除非有 Ghost 会话在其中工作，或其中有未提交的内容；它的分支会保留。 | Ghost 中的会话；先询问你 |

### 访问级别 {#access-levels}

调用方有两类：

- **Ghost 中的会话。** 它可以调用所有工具。会执行操作的工具（`delegate_task`、`open_pr`、`schedule_task`、`use_device`、`send_to_session`、`interrupt_session`、`add_project`、`review_pr`、`merge_pr`、`start_workflow`、`change_task`、`make_worktree`、`remove_worktree`）会先在该会话的对话中用权限卡片询问你，这与智能体自身提问所用的卡片相同，有 **Allow**、**Always allow** 和 **Reject**。**Always allow** 在该会话中对该工具一直有效，直到会话的连接结束。`review_pr` 和 `merge_pr` 的卡片会写明拉取请求、它所在的仓库（以其地址）以及要做的全部事情：审阅结论，或合并方式、是否等检查通过，以及要删除的分支。`merge_pr`、`start_workflow`、`change_task` 和 `remove_worktree` 从不提供 **Always allow**：它们做的事无法撤销，而智能体读到的文字（拉取请求、CI 日志、代码）可能左右它向这些工具提出的请求。
- **Ghost 之外的智能体**（见下文）。它可以读取：`list_sessions`、`read_session`、`read_diff`、`wait_for_turn`、`events`、`list_devices`、`device_screen`，以及上面每个列出、查找或读取的工具（它指定的文件夹必须在你的某个项目中，或是其工作树）。打开 **设置 › 智能体 › Ghost 之外的智能体可以执行操作**（`outside_acts`）后，它还可以 `send_to_session` 和 `interrupt_session`，每次都在那个会话的卡片上询问你，只有 **Allow** 和 **Reject**：Ghost 之外的每个智能体都用 `mcp.json` 中的同一个密钥进入，**Always allow** 会让它们全都获准，所以没有这一项。其他会执行操作或向你提问的工具会回答：只有 Ghost 会话中的智能体才能这样做。
- **另一台 Mac 上的智能体**，通过已配对设备的连接访问这台 Mac 的工具（`--mcp --on`，见下文）：它可以读取，并在该设备允许的范围内执行操作（只读设备的智能体只能读取）。

## 从 Ghost 之外的智能体使用 {#using-them-from-an-agent-outside-ghost}

你在终端中运行的智能体，只要 Ghost 在同一个主目录上运行，就能通过同一个服务器读取 Ghost 保存的内容：

```bash
claude mcp add ghost -- /Applications/Ghost.app/Contents/MacOS/ghost-desk --mcp
codex mcp add ghost -- /Applications/Ghost.app/Contents/MacOS/ghost-desk --mcp
```

在没有窗口的机器上，使用 `ghost-server --mcp`。要访问另一台机器上的 Ghost，通过 SSH 在那里运行它：`ssh host ghost-server --mcp`。

如果没有以该主目录运行的 Ghost，`--mcp` 会退出并说明原因。

### 通过 HTTP {#over-http}

使用 MCP 可流式 HTTP 的智能体，可以在以下地址使用相同的工具：

```
http://127.0.0.1:<http>/mcp
```

`<http>` 和密钥都来自 `<home>/mcp.json`，密钥以 `Authorization: Bearer <key>` 发送。两者在重启后保持不变（见上文），所以地址和密钥只需设置一次。HTTP 客户端会把密钥发给在那个地址上监听的任何程序，所以 Ghost 没在运行时不要使用它；应用能运行命令时，`ghost --mcp` 是更安全的接入方式。它与不带会话的 `--mcp` 是同一类调用方：可以读取，并在 `outside_acts` 允许时执行操作。Ghost 不保存 MCP 会话；它只响应以 `127.0.0.1`、`localhost` 和 `::1` 为名的请求，并拒绝来自网页的请求（带有 `Origin` 的请求）。

### 在另一台 Mac 上 {#on-another-mac}

在与另一台 Mac 配对的 Mac 上，智能体可以通过窗口所用的连接访问那台 Mac 的工具，以这台 Mac 为它生成的密钥签名：

```bash
ghost --mcp --on <name>
```

`<name>` 是服务器的名称，即 **这台 Mac 控制的 Mac** 中列出的名称。
