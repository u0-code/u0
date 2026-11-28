---
title: u0 为智能体提供的工具
description: u0 提供给每个线程的 MCP 服务器，以及 u0 之外的智能体如何使用它。
---

# u0 为智能体提供的工具

u0 自己没有智能体，但它为你的智能体提供工具：一个名为 `u0` 的 MCP 服务器，挂接到 u0 打开的每个线程上。通过它，智能体可以读取 u0 保存的线程、列出和添加你的项目并在其中查找、读取审阅和合并拉取请求并读取其检查、运行你的工作流、更改你的计划任务、创建和移除工作树、把工作交给另一个智能体、打开拉取请求、安排任务、向你提问，以及使用其所在机器上的模拟器。

## 线程如何获得这些工具 {#how-a-thread-gets-them}

u0 打开线程时，通过 ACP 的 `mcpServers` 把服务器作为一个要运行的程序交给智能体：即带 `--mcp` 的 u0 自身程序，附带 `U0_HOME` 和该线程独有的密钥（`U0_MCP_KEY`）。这个程序只是一个小管道：它在标准输入输出上讲 MCP，并把一切转发给 u0 服务器，后者只监听 `127.0.0.1`。这个管道从不发送它的密钥：服务器先发来一个一次性的随机数，管道用密钥对它（以及端口）算出 HMAC 作答。u0 没在运行时监听那个端口的程序，得不到任何可用的东西。

端口和供 u0 之外智能体使用的密钥写在 `<home>/mcp.json` 中，只有你能读取（权限 0600）。它们在每次启动之间保持不变，所以用它们设置好的应用会一直可用：

- 端口只选一次，每次启动时重新占用。如果期间另一个程序占用了其中一个，u0 会改在另一个端口监听，把它写进 `mcp.json`，并告诉你：用旧地址设置的应用需要换成新地址。如果那是 HTTP 端口，密钥也会重新生成，因为占着那个端口的程序可能已从某个应用那里听到了它（见下文）。占用端口时遇到的其他问题（不是被另一个程序占着）会让这些工具不提供服务，并记在 u0 的日志里。
- 密钥保存在钥匙串中，直到你在 **设置 › 智能体 › u0 之外的智能体的密钥** 中按下 **新密钥**。之后旧密钥不再允许任何人进入，已用它连上的智能体也不例外（它的下一次调用会被拒绝，正在进行的等待也会结束），新的密钥会写进 `mcp.json`。没有钥匙串的地方（Linux 上的 `u0-server`），每次启动都会重新生成密钥。

要让某个智能体的线程不带 u0 的工具，在 **设置 › 智能体** 中打开它的页面并把工具关掉。该智能体的 ID 会被加入 [settings.json](reference/settings.md) 的 `no_tools`。

## 工具 {#the-tools}

| 工具 | 作用 | 谁可以调用 |
|---|---|---|
| `list_sessions` | u0 保存的线程，最新的在前（最多 100 个）：每个线程的智能体、ID、标题、项目、工作文件夹以及是否已归档。可选的 `project` 按文件夹筛选。 | 任何智能体 |
| `read_session` | 线程中说过的内容：你的消息（以 "You:" 开头）和智能体的回答。参数为 `agent` 和 `session`。 | 任何智能体 |
| `read_diff` | 文件夹中尚未提交的内容，以统一差异格式给出。`folder` 默认为调用方线程的工作位置；对 u0 之外的智能体，它必须位于你的某个项目或其工作树中。 | 任何智能体 |
| `delegate_task` | 把任务交给一个新 u0 线程中的智能体（你可以旁观），并等待它的回答。参数为 `prompt`、`title`，可选 `agent`、`project` 和 `worktree`。返回新线程的 ID 和它的最终回答。 | u0 中的线程；先询问你 |
| `open_pr` | 提交线程工作位置中未提交的内容，推送（如在默认分支上则推到独立分支），并用 `gh` 打开拉取请求。参数为 `title`、`body`，可选提交信息 `message`。 | u0 中的线程；先询问你 |
| `schedule_task` | 为智能体安排一条提示，每次都在新线程中运行：一次（`in_minutes`）或重复（`every_minutes`）。它会出现在 **设置 › 计划任务** 中，你可以在那里暂停或删除它。 | u0 中的线程；先询问你 |
| `ask_user` | 在调用方线程中向你提问并等待回答，可以是自由文本，也可以是 `choices` 之一。 | u0 中的线程 |
| `list_devices` | 服务器所在机器上的 iOS 模拟器和 Android 模拟器：ID、名称、平台、是否在运行；以及缺什么才能用上它们。 | 任何智能体 |
| `device_screen` | 正在运行的设备当前显示的内容，以图片形式给出。 | 任何智能体 |
| `use_device` | 启动或停止设备、轻点某一点、按住某一点、从一点滑到另一点、朝某一侧滚动、向左或向右旋转、输入文本、按主屏幕键（Android 上还有返回或最近任务），或设为深色或浅色。 | u0 中的线程；先询问你 |
| `send_to_session` | 在 u0 保存的某个线程（`agent`、`session`）中说 `prompt`，作为它的下一轮：正在进行的一轮会先让它结束，绝不会被中断。带 `wait` 时返回其智能体的回答；否则由 `wait_for_turn` 返回。卡片会写明那个线程，并在一行中写出要说的话。 | u0 中的线程；先在那个线程中询问你 |
| `answer_request` | 回答另一个线程中智能体提出、正在等待的问题（`agent`、`session`；`request` 编号和一个 `option` id，都由 `events` 给出）。卡片会写明问题和答案。 | u0 中的线程；每次都先询问你 |
| `organize_session` | 整理 u0 保存的一个线程（`agent`、`session`）：`pin` 置顶或取消置顶，`archive` 归档或恢复，给它一个 `title`，设置它的 `tags`。每个窗口都会立刻显示这一变化。 | u0 中的线程；先在那个线程中询问你 |
| `wait_for_turn` | 等到线程中正在进行的一轮结束（`seconds`，未给出时为十分钟，最多一小时），并返回其智能体在最后一轮中说的话。 | 任何智能体 |
| `interrupt_session` | 停止线程中正在进行的一轮。 | u0 中的线程；先在那个线程中询问你 |
| `events` | u0 运行的线程中发生的事（一轮结束、智能体向你提问：附问题的 `request` 编号和 `options`），每件都有编号；指定了 `agent`、`session` 或 `project` 时只给出与之相关的。用返回的 `last`（作为 `after`）再次调用以继续跟进，带 `seconds` 则等待能通过筛选的新事发生。`missed` 表示其中有些已不再保留。 | 任何智能体 |
| `list_projects` | 你的项目：它们的文件夹，与侧边栏列出的一致。 | 任何智能体 |
| `add_project` | 把一个 `folder`（完整路径）设为你的项目，排在列表最前；连到那个 u0 的每个窗口都会立刻显示它。不能是根目录、你的主文件夹或包含它的文件夹，也不能是 u0 的主目录或其中的文件夹。 | u0 中的线程；先询问你 |
| `search_sessions` | 标题或其中所说内容包含 `query`（不分大小写）的线程，最新的在前，格式与 `list_sessions` 相同。 | 任何智能体 |
| `find_files` | `folder` 中 git 能看到的文件里，路径按顺序包含 `query` 各字母的那些，与 u0 的文件查找方式相同（`smr` 能找到 `src/main.rs`），最匹配的在前，最多五十个。 | 任何智能体 |
| `search_code` | `words` 出现在 `folder` 中 git 能看到的文件里的位置：`path:line` 和那一行，最多五十处；`case`、`word` 和 `regex` 与文件标签页的搜索相同。 | 任何智能体 |
| `list_prs` | 项目远程所在托管平台（GitHub、GitLab、Bitbucket、Azure DevOps）上的拉取请求：`state`（未给出时为 open，还有 closed、merged、all），以及按平台语法写的 `search`。 | 任何智能体 |
| `read_pr` | 完整读取一个拉取请求：它的描述、其中和各行上的讨论、检查、审阅者，以及是否有冲突；带 `diff` 时还包括补丁。 | 任何智能体 |
| `review_pr` | 审阅一个拉取请求：`verdict` 为 approve、request-changes 或 comment，附上 `body`，以及 `lines`（对新版本各行的评论）。 | u0 中的线程；先询问你 |
| `merge_pr` | 合并一个拉取请求，之后删除其分支：`how`（squash、merge 或 rebase；未给出时按 **通用 › 拉取请求 › 合并方式** 的设置），立即合并或在检查通过后合并（`auto`）。 | u0 中的线程；先询问你 |
| `reply_to_thread` | 用 `body` 回复拉取请求某些行上的一段讨论（`thread`，即 `read_pr` 给出的 id，只接受它列出的）。 | u0 中的线程；先询问你 |
| `resolve_thread` | 把拉取请求某些行上的一段讨论标为已解决，或重新打开（`reopen`）。 | u0 中的线程；先询问你 |
| `rerun_ci` | 重新运行拉取请求检查中某次 `run` 失败的作业（GitHub Actions）。 | u0 中的线程；先询问你 |
| `pr_checks` | 拉取请求的检查：每项的名称、状态、运行位置；在 GitHub Actions 上失败的检查还给出 `ci_log` 可读取的 `run`。 | 任何智能体 |
| `ci_log` | 检查中失败的某次 `run` 输出的末尾部分。 | 任何智能体 |
| `list_issues` | 项目的议题，来自其远程仓库的托管方或它绑定的 Linear 团队：`state`（未给出时为 open，或 closed、all）以及托管方能理解的 `search`。 | 任何智能体 |
| `read_issue` | 完整读取一个议题：它的内容、其中的讨论、指派给了谁。 | 任何智能体 |
| `create_issue` | 用 `title` 和 `body` 新建一个议题。 | u0 中的线程；先询问你 |
| `comment_on_issue` | 用 `body` 评论一个议题（`number`）。 | u0 中的线程；先询问你 |
| `set_issue_state` | 关闭一个议题，或重新打开它（`open`）。 | u0 中的线程；先询问你 |
| `list_workflows` | 你的工作流：每个的名称、用途和步骤，或其文件无法读取的原因；以及它们的运行，包括各自的状态和已完成的步骤。只能读取的智能体拿不到 `run` 步骤的命令，也拿不到各步骤的结果。 | 任何智能体 |
| `start_workflow` | 用 `input` 开始一次 `workflow` 的运行：返回它的 ID。卡片会完整显示输入，与它的步骤得到的一样。 | u0 中的线程；先询问你 |
| `list_tasks` | 计划任务：每个的 ID、标题、提示、智能体、项目、计划、是否已暂停，以及上次运行。绝不会给出 webhook 的地址。 | 任何智能体 |
| `change_task` | 暂停一个任务（`id`）、恢复它、立即运行或删除它（`change`）。 | u0 中的线程；先询问你 |
| `list_worktrees` | u0 为某个项目创建、位于其工作树文件夹中的工作树：每个的仓库、路径和分支。 | 任何智能体 |
| `make_worktree` | 像新线程那样为项目创建一个工作树：从远程的默认分支开始，分支以 `about` 命名；返回它的路径和分支。 | u0 中的线程；先询问你 |
| `remove_worktree` | 移除 u0 创建的一个工作树（`path`），除非有 u0 线程在其中工作，或其中有未提交的内容；它的分支会保留。 | u0 中的线程；先询问你 |

智能体不必一再调用 `events`：通过 `u0 --mcp` 连接的智能体可以订阅资源 `u0://events`（MCP 的 `subscriptions/listen`），每当有事发生，u0 都会告诉它（`notifications/resources/updated`），它再调用 `events` 即可。读取这个资源，得到的是最近发生的事。通过 HTTP 时每个请求各自独立，所以无从订阅。

### 访问级别 {#access-levels}

调用方有两类：

- **u0 中的线程。** 它可以调用所有工具。会执行操作的工具（`delegate_task`、`open_pr`、`schedule_task`、`use_device`、`send_to_session`、`interrupt_session`、`organize_session`、`answer_request`、`add_project`、`review_pr`、`merge_pr`、`reply_to_thread`、`resolve_thread`、`rerun_ci`、`create_issue`、`comment_on_issue`、`set_issue_state`、`start_workflow`、`change_task`、`make_worktree`、`remove_worktree`）会先在该线程中用权限卡片询问你，这与智能体自身提问所用的卡片相同，有 **Allow**、**Always allow** 和 **Reject**。**Always allow** 在该线程中对该工具一直有效，直到线程的连接结束。`review_pr` 和 `merge_pr` 的卡片会写明拉取请求、它所在的仓库（以其地址）以及要做的全部事情：审阅结论，或合并方式、是否等检查通过，以及要删除的分支。`merge_pr`、`start_workflow`、`change_task`、`remove_worktree`、`answer_request` 和 `resolve_thread` 从不提供 **Always allow**：它们做的事无法撤销，而智能体读到的文字（拉取请求、CI 日志、代码）可能左右它向这些工具提出的请求。作用于另一个线程的工具（`send_to_session`、`interrupt_session`、`organize_session`、`answer_request`）也不提供：它的卡片会写明那个线程的标题、智能体和项目，而 **Always allow** 就等于允许它作用于任何线程。
- **u0 之外的智能体**（见下文）。它可以读取：`list_sessions`、`read_session`、`read_diff`、`wait_for_turn`、`events`、`list_devices`、`device_screen`，以及上面每个列出、查找或读取的工具（它指定的文件夹必须在你的某个项目中，或是其工作树；它列出、读取、搜索、等待或得知的线程，都是在其中工作或开始的线程）。打开 **设置 › 智能体 › u0 之外的智能体可以执行操作**（`outside_acts`）后，它还可以 `send_to_session`、`interrupt_session`、`organize_session` 和 `answer_request`（只对这样的线程），每次都在那个线程的卡片上询问你，只有 **Allow** 和 **Reject**：u0 之外的每个智能体都用 `mcp.json` 中的同一个密钥进入，**Always allow** 会让它们全都获准，所以没有这一项。其他会执行操作或向你提问的工具会回答：只有 u0 线程中的智能体才能这样做。
- **另一台 Mac 上的智能体**，通过已配对设备的连接访问这台 Mac 的工具（`--mcp --on`，见下文）：它可以读取，并在该设备允许的范围内执行操作（只读设备的智能体只能读取）。

## 从 u0 之外的智能体使用 {#using-them-from-an-agent-outside-u0}

你在终端中运行的智能体，只要 u0 在同一个主目录上运行，就能通过同一个服务器读取 u0 保存的内容：

```bash
claude mcp add u0 -- /Applications/u0.app/Contents/MacOS/u0-desk --mcp
codex mcp add u0 -- /Applications/u0.app/Contents/MacOS/u0-desk --mcp
```

在没有窗口的机器上，使用 `u0-server --mcp`。要访问另一台机器上的 u0，通过 SSH 在那里运行它：`ssh host u0-server --mcp`。

如果没有以该主目录运行的 u0，`--mcp` 会退出并说明原因。

### 通过 HTTP {#over-http}

使用 MCP 可流式 HTTP 的智能体，可以在以下地址使用相同的工具：

```
http://127.0.0.1:<http>/mcp
```

`<http>` 和密钥都来自 `<home>/mcp.json`，密钥以 `Authorization: Bearer <key>` 发送。两者在重启后保持不变（见上文），所以地址和密钥只需设置一次。HTTP 客户端会把密钥发给在那个地址上监听的任何程序，所以 u0 没在运行时不要使用它；应用能运行命令时，`u0 --mcp` 是更安全的接入方式。它与不带线程的 `--mcp` 是同一类调用方：可以读取，并在 `outside_acts` 允许时执行操作。u0 不保留 MCP 会话；它只响应以 `127.0.0.1`、`localhost` 和 `::1` 为名的请求，并拒绝来自网页的请求（带有 `Origin` 的请求）。

### 在另一台 Mac 上 {#on-another-mac}

在与另一台 Mac 配对的 Mac 上，智能体可以通过窗口所用的连接访问那台 Mac 的工具，以这台 Mac 为它生成的密钥签名：

```bash
u0 --mcp --on <name>
```

`<name>` 是机器的名称，即 **这台 Mac 控制的 Mac** 中列出的名称。
