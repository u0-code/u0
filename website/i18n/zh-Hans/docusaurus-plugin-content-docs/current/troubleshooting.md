---
title: 故障排查
description: 诊断信息、日志，以及某样东西无法启动或无法读取时该怎么办。
---

# 故障排查

## 从哪里查看 {#where-to-look}

- **设置 › 维护与诊断** 显示 **进程**：u0 运行的每个进程（智能体、终端、隧道）组成的树，附带其 CPU 和内存占用。智能体可以被放开（**丢弃**），其他进程可以被停止（**停止**）。其下方的 **日志中的警告和错误** 列出最近出错的情况。
- **设置 › 关于** 显示 **u0 存放文件的位置** 和 **u0 的日志**，各带一个 **显示**。
- 日志是 `<home>/logs/u0.<date>.log`，每天一个文件，保留最近 14 天（`log_days`）。launchd 服务还会记录到 `<home>/logs/service.log`。从终端启动 u0 前设置 `RUST_LOG`（例如 `RUST_LOG=u0_core=debug,u0_desk=debug`）可以记录更多内容。
- `u0 status` 说明主目录中有什么：已开启的智能体、服务器是否在监听、服务、任务，以及 `settings.json` 是否可读。
- u0 显示的每条错误都可以复制：选中其文字，或按它上面的 **复制**，复制的是全文，包括 **显示全部** 才展开的部分。带有下一步操作（例如 **重试**）的通知在文字下方有 **复制详情**。

从 Dock 打开的 u0 会读取你的登录 shell 设置的 `PATH`，因此它要找的程序（`claude`、`codex`、`npm`、`uv`、`gh`、`glab`、`cloudflared`、`op`、`adb`）会在你的终端能找到它们的地方被找到。启动时若没有代理设置，它也会取你的登录 shell 设置的代理（`https_proxy`、`http_proxy`、`all_proxy`、`no_proxy`，大小写均可），因此智能体联网的方式与你在终端里运行 `claude` 或 `codex` 时相同。

## 文件无法读取 {#a-file-does-not-read}

u0 不会去猜测它无法读取的文件，也从不覆盖它。每种情况都会说明哪里出错以及该怎么做：

| 文件 | 在修复或移走之前会发生什么 |
|---|---|
| `settings.json` | u0 使用默认值运行，且不保存任何内容。修复它，或将它移走以重新开始。 |
| [策略](reference/policy.md) | 同上：在策略可以读取之前，设置不会加载。 |
| `sessions.json` | 线程索引不会被覆盖。 |
| `tasks.json` | 不运行也不保存任何任务。 |
| `runs.json` | 不运行也不保存任何工作流。 |
| 某个工作流的文件 | 它会连同错误一起列出，且无法启动。 |
| `devices.json` | 没有设备能够接入。将它移走，然后重新配对每台设备。 |
| `servers.json` | 无法访问任何机器。将它移走，然后重新与每台机器配对。 |
| `keybindings.json` | 被忽略并记录到日志；设置 › 快捷键不会覆盖它。 |

u0 不会转换旧版本的文件：它不再支持的格式的文件会以同样的方式被拒绝。

## 智能体无法启动 {#an-agent-does-not-start}

- **安装**：这台机器上没有该智能体。点击它即可从注册表安装。以 npm 包发布的智能体需要 `npm`；为 `uvx` 发布的需要 `uv`。
- **没有适用于这台 Mac 的版本**：注册表中没有它适用于此平台的发行版。
- **登录**：该智能体需要你登录（见[首次运行](first-run.md#signing-in)）。在终端中登录后，点击 **我已登录**。
- **已停止** 或 **失败**：打开该智能体的页面。如果你用自己的命令启动它，**检查它是否支持 ACP** 会试运行它。u0 每五分钟会重新尝试一个已停止的智能体，如果它落后于注册表，会先更新它。
- 组织的策略可能只允许部分智能体；此时该智能体会说明策略不允许它。

## 某一轮无法撤销 {#a-turn-cannot-be-undone}

检查点要求该文件夹是（或包含）一个 git 仓库。如果检查点失败，该轮会显示 **这一轮无法撤销** 以及 git 给出的原因。

## macOS 反复请求文件夹访问权限 {#macos-asks-for-folder-access-again-and-again}

macOS 在应用读取 `~/Desktop` 或 `~/Documents` 下的文件夹之前会询问，并按应用的签名记住你的回答。临时签名（ad hoc）的构建（例如从源码构建的版本）每次重新构建后都会被再次询问；见[安装](install.md#from-source)。

## u0 的工具 {#u0s-tools}

当该主目录上没有运行 u0，或其 `mcp.json` 缺失时，`--mcp` 会显示 **u0 is not running with** *home*。请用相同的 `U0_HOME` 启动 u0（或 `u0 serve`）。

## 远程控制 {#remote-control}

| 消息 | 该怎么做 |
|---|---|
| The pairing link is used up or out of date: make another | 链接只能使用一次，十分钟内有效。生成一个 **新链接**。 |
| This device is not paired with this u0 | 它已被撤销，或者它的 `devices.json` 被移走了。重新配对。 |
| This u0 speaks protocol N and yours M: update the older | 更新两者之一，让双方运行相同的版本。 |
| cloudflared is not installed | `brew install cloudflared`。 |
| 快速隧道的地址变了 | 快速隧道每次启动都会获得新地址：重新配对，或使用命名隧道。 |
| 仅单向配对 | 对方机器无法访问这台机器：启动这台 Mac 的隧道（或在 `127.0.0.1` 访问它），然后重新配对。 |
| *Organisation*'s policy does not allow … | 某项[策略](reference/policy.md)禁止了此操作。 |
