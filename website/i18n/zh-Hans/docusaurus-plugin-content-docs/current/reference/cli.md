---
title: 命令行
description: u0 命令、u0-server 和 --mcp。
---

# 命令行

在终端中有三种方式使用 u0：

- `u0`，u0 放到你 `PATH` 上的一个小脚本；
- 程序本身：u0.app 中应用的 `u0-desk`，以及没有窗口的服务器 `u0-server`；
- `--mcp`，两个程序都接受这个参数，用来为智能体提供 MCP（见 [u0 的工具](../tools-for-agents.md)）。

它们都使用 u0 的主目录：`~/.u0`，或 `U0_HOME` 指定的文件夹。

## 安装 `u0` 命令 {#installing-the-u0-command}

**设置 › 关于 › u0 命令** 会把脚本写入 `~/.local/bin/u0`。如果 `~/.local/bin` 不在你的 `PATH` 上，u0 会提示；请在你的 shell 配置文件中添加它。

## 命令 {#commands}

| 命令 | 作用 |
|---|---|
| `u0` | 在 u0 中将当前文件夹作为项目打开。 |
| `u0 <folder>…` | 在 u0 中将每个文件夹作为项目打开。macOS 会把它交给正在运行的 u0，或启动一个。 |
| `u0 serve` | 运行没有窗口的 u0 服务器，供别处的桌面端与之配对。在终端中，它会打印一个配对链接及其二维码（绝不写入日志）。按 Ctrl-C 停止。 |
| `u0 service install` | 仅限 macOS。将该服务器作为 launchd 代理在登录时运行，并让窗口在它上面打开。 |
| `u0 service uninstall` | 停止该服务并将其移出登录项。窗口重新运行它自己的服务器。 |
| `u0 service status` | 服务是否已安装并在运行，以及它的日志位置。 |
| `u0 status` | 此主目录的情况：已开启的智能体、服务器是否在监听、服务、计划任务。 |
| `u0 tasks` | 计划任务：编号、计划、下次运行时间和标题。 |
| `u0 doctor` | 此主目录中哪些地方不正常（设置、u0 保存的文件、已开启的智能体、服务），以及该怎么做；有任何不正常时以非零状态退出。 |
| `u0 thread list` | 保留的线程，最新的在前，每个都带有它的智能体和 ID。 |
| `u0 thread show <agent> <id>` | 线程中说过的内容。 |
| `u0 thread start <project> "<prompt>"` | 在项目中开始一个线程（`.`：当前文件夹）。`--agent <id>` 指定智能体（未指定：第一个已开启的），`--wait` 等待其回答并打印。 |
| `u0 thread send <agent> <id> "<prompt>"` | 在线程中说些什么，作为它的下一轮（正在进行的一轮会先让它结束）。`--wait` 同上。 |
| `u0 workflow list` | 工作流及其运行记录。 |
| `u0 workflow start <name> [input]` | 开始工作流的一次运行。 |
| `u0 help`、`u0 --help` | 上面的列表；`u0 <command> --help` 会详细说明某个命令。 |

`u0-server` 接受相同的子命令（`u0-server status` 等）。不带任何子命令时，它会像 `u0 serve` 一样提供服务。`u0 --version`（或 `u0-server --version`）会打印构建信息：版本、提交和渠道。u0 不认识的子命令会被拒绝，并说明它接受什么。

会执行操作的命令（`thread start`、`thread send`、`workflow start`）会请求这台 Mac 的服务，或者带 `--on <name>` 时请求这台 Mac 已配对的另一台 Mac，通过以这台 Mac 的密钥签名的连接，与窗口的方式相同。在那里只能读取的 Mac 会被拒绝。既没有服务也没有 `--on` 时，它们会提示你安装服务。

## launchd 服务 {#the-launchd-service}

`u0 service install` 会写入 `~/Library/LaunchAgents/local.u0.server.plist`（对于 `~/.u0` 以外的主目录，标签会带上专属后缀，因此两个主目录永远不会共用一个服务）。该代理会在登录时以你的 `U0_HOME` 运行 `u0 serve`，在它停止时重启它，并记录日志到 `<home>/logs/service.log`。

安装服务时，也会让这台 Mac 的窗口与它配对，就像任何桌面端与机器配对一样：一台名为“This Mac's window”的设备被允许接入，一台名为“This Mac's service”、位于 `127.0.0.1:<port>` 的机器被加入窗口的列表。从此窗口会与该服务通信，而不是在同一个主目录上启动第二个服务器，因此退出窗口后，计划任务和工作流仍会继续运行。

Linux 上没有 `u0 service`。请在 systemd 或你自己的其他进程管理器下运行 `u0-server`。

## `--mcp` {#--mcp}

```bash
/Applications/u0.app/Contents/MacOS/u0-desk --mcp
u0-server --mcp
```

在标准输入和输出上使用 MCP 通信，并把一切转交给使用同一主目录运行的 u0。`--mcp --on <name>` 则通过签名连接转交给这台 Mac 已配对的机器 `<name>`。见 [u0 的工具](../tools-for-agents.md)。

## 环境变量 {#environment}

| 变量 | 含义 |
|---|---|
| `U0_HOME` | u0 的主目录。默认为 `~/.u0`。 |
| `RUST_LOG` | 记录哪些日志，以 `tracing` 过滤器表示。默认为 `u0_desk=info,u0_core=info`。 |
| `U0_MCP_KEY` | 由 u0 为它交给线程的 `--mcp` 设置。请不要自己设置它。 |
