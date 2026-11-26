---
title: 隐私与安全
description: Ghost 存储什么、存在哪里、发送什么，以及如何把智能体和设备约束在边界之内。
---

# 隐私与安全

## 智能体以你的身份运行 {#agents-run-as-you}

智能体以你的身份在你的机器上运行，能做什么取决于它自己的设置。Ghost 不会对它进行沙箱隔离，也不读取它的凭据：登录是智能体自己的事。智能体在行动前提出的问题，Ghost 会转达给你，并且只按你的选择作答。Ghost 自己的[工具](tools-for-agents.md)在执行操作前也会以同样的方式先询问你。

## Ghost 发送什么 {#what-ghost-sends}

- **遥测**，仅限你允许的范围：每个级别具体发送什么，见[首次运行](first-run.md#telemetry)。不包含你或智能体写下的任何内容、任何路径，也不包含项目、仓库、分支或会话的名称。
- **ACP 注册表** 从 `cdn.agentclientprotocol.com` 读取，智能体从它指明的位置下载。
- **更新** 在该仓库的 GitHub releases 上查找。
- **你的代码托管平台**（GitHub、GitLab、Bitbucket、Azure DevOps、Linear）会被用你的令牌查询你的拉取请求和议题。

你的智能体向它们自己的服务发送什么，是你与它们之间的事。

## Ghost 保存什么 {#what-ghost-keeps}

一切都在 Ghost 的主目录 `~/.ghost`（或 `GHOST_HOME`）中：

| 路径 | 内容 |
|---|---|
| `settings.json` | 你的[设置](reference/settings.md)。标记为机密的智能体变量只在这里记下名称，从不存储其值。 |
| `sessions.json` | 见过的会话的索引：标题、置顶、归档、工作树。 |
| `transcripts/<agent>/<id>.jsonl` | 每个会话的记录：你和智能体说过的话，按发生顺序。 |
| `keybindings.json` | 你的[按键](reference/keybindings.md)。 |
| `tasks.json`、`runs.json`、`workflows/` | [计划任务、工作流运行记录、你的工作流](automation.md)。 |
| `devices.json` | 可以控制这台机器的设备（它们的公钥）、端口、共享设置，以及命名隧道的令牌。 |
| `servers.json` | 这台 Mac 控制的服务器（其密钥的公钥）。 |
| `mcp.json` | Ghost 工具的端口，以及供 Ghost 之外的智能体使用的密钥。每次启动时重新生成，只有你可读（0600）。 |
| `telemetry.json` | 当天的计数、本次安装的随机 ID，以及首次启动时是否已告知。 |
| `registry.json`、`icons/`、`agents/` | 最近一次读取的注册表、智能体的标志、已安装的智能体。 |
| `accounts/` | 某个智能体额外[账户](first-run.md#an-agents-page)的登录文件夹。 |
| `worktrees/` | 会话的工作树，除非 `worktrees` 指定了另一个文件夹。 |
| `themes/` | 你的主题。 |
| `logs/` | Ghost 的日志，每天一个文件，保留 `log_days` 天。 |

**设置 › 会话 › Ghost 保存的内容** 显示每项占用多少空间，并清理没有会话在用的内容。在仓库中，Ghost 还会在 `refs/ghost/checkpoints/` 下保存[检查点](worktrees-and-checkpoints.md#checkpoints)。

除 `mcp.json` 外的文件都按你通常的权限（你的 umask）创建，Ghost 重写的文件会保留它原有的权限。Ghost 每次都整体写入文件：崩溃之后，文件要么是旧的，要么是新的，绝不会是写了一半的。

## 机密 {#secrets}

机密保存在 macOS 钥匙串中，从不放在 Ghost 的文件里，服务名为 `Ghost <home>`（因此两个主目录互不干扰）：

| 账户 | 机密 |
|---|---|
| `env/<agent>/<NAME>` | 标记为 **机密** 的智能体变量。 |
| `token/<host>` | GitHub、GitLab、Bitbucket、Azure DevOps 或 Linear 的令牌。 |
| `server/SHA256:…` | 这台 Mac 为它所控制的服务器生成的密钥的私钥（PKCS#8）。 |

这些值都可以是 1Password 引用（`op://vault/item/field`）；每次使用时，Ghost 都会用 1Password 的 `op` CLI 读取它。

钥匙串是 macOS 的功能。Linux 上的 `ghost-server` 不保存任何机密：标记为机密的智能体变量或令牌无法在那里使用。

## 远程控制 {#remote-control}

- Ghost 只在 `127.0.0.1` 上监听，并且只在你打开监听时才监听。除非你运行隧道或转发端口，否则网络上的任何东西都无法访问它。
- 设备凭 Ed25519 密钥接入，从不凭密码：每次建立连接时，它都会对一个新的随机数（nonce）签名。服务器只保存公钥。
- 配对码是 128 位随机数，只能使用一次，十分钟内有效，从不写入磁盘。
- **撤销** 会立即删除设备的密钥并关闭它的连接；关闭监听会关闭所有连接并作废所有配对码。
- [策略](reference/policy.md)可以禁止监听、隧道、Webhook 和智能体。

详见[远程控制](remote.md)，其中包括命名隧道的令牌以明文形式存储在 `devices.json` 中这一点。

## Webhook {#webhooks}

Webhook 任务的地址只受其随机令牌保护：任何拥有该地址的人都可以运行该任务。Webhook 的请求体会向智能体标明为不可信内容。见[自动化](automation.md#webhooks)。

## 报告漏洞 {#reporting-a-vulnerability}

请不要为漏洞提交议题。见
[SECURITY.md](https://github.com/u0-code/u0/blob/main/SECURITY.md)。
