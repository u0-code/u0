---
title: 命令行
description: ghost 命令、ghost-server 和 --mcp。
---

# 命令行

在终端中有三种方式使用 Ghost：

- `ghost`，Ghost 放到你 `PATH` 上的一个小脚本；
- 程序本身：Ghost.app 中应用的 `ghost-desk`，以及没有窗口的服务器 `ghost-server`；
- `--mcp`，两个程序都接受这个参数，用来为智能体提供 MCP（见 [Ghost 的工具](../tools-for-agents.md)）。

它们都使用 Ghost 的主目录：`~/.ghost`，或 `GHOST_HOME` 指定的文件夹。

## 安装 `ghost` 命令 {#installing-the-ghost-command}

**设置 › 关于 › ghost 命令** 会把脚本写入 `~/.local/bin/ghost`。如果 `~/.local/bin` 不在你的 `PATH` 上，Ghost 会提示；请在你的 shell 配置文件中添加它。

## 命令 {#commands}

| 命令 | 作用 |
|---|---|
| `ghost` | 在 Ghost 中将当前文件夹作为项目打开。 |
| `ghost <folder>…` | 在 Ghost 中将每个文件夹作为项目打开。macOS 会把它交给正在运行的 Ghost，或启动一个。 |
| `ghost serve` | 运行没有窗口的 Ghost 服务器，供别处的桌面端与之配对。它会打印一个配对链接及其二维码。按 Ctrl-C 停止。 |
| `ghost service install` | 仅限 macOS。将该服务器作为 launchd 代理在登录时运行，并让窗口在它上面打开。 |
| `ghost service uninstall` | 停止该服务并将其移出登录项。窗口重新运行它自己的服务器。 |
| `ghost service status` | 服务是否已安装并在运行，以及它的日志位置。 |
| `ghost status` | 此主目录的情况：已开启的智能体、服务器是否在监听、服务、计划任务。 |
| `ghost sessions` | 保留的（未归档的）会话，最新的在前：多久之前、智能体、项目和标题。 |
| `ghost tasks` | 计划任务：编号、计划、下次运行时间和标题。 |
| `ghost help` | 上面的列表。 |

`ghost-server` 接受相同的子命令（`ghost-server status` 等）。不带任何子命令时，它会像 `ghost serve` 一样提供服务。`ghost-server --version`（或应用包中的 `ghost-desk --version`）会打印构建信息：版本、提交和渠道。`ghost` 脚本只把表中的子命令传给 Ghost，因此要查看 `--version`，请直接调用程序本身。

## launchd 服务 {#the-launchd-service}

`ghost service install` 会写入 `~/Library/LaunchAgents/local.ghost.server.plist`（对于 `~/.ghost` 以外的主目录，标签会带上专属后缀，因此两个主目录永远不会共用一个服务）。该代理会在登录时以你的 `GHOST_HOME` 运行 `ghost serve`，在它停止时重启它，并记录日志到 `<home>/logs/service.log`。

安装服务时，也会让这台 Mac 的窗口与它配对，就像任何桌面端与服务器配对一样：一台名为“This Mac's window”的设备被允许接入，一台名为“This Mac's service”、位于 `127.0.0.1:<port>` 的服务器被加入窗口的列表。从此窗口会与该服务通信，而不是在同一个主目录上启动第二个服务器，因此退出窗口后，计划任务和工作流仍会继续运行。

Linux 上没有 `ghost service`。请在 systemd 或你自己的其他进程管理器下运行 `ghost-server`。

## `--mcp` {#--mcp}

```bash
/Applications/Ghost.app/Contents/MacOS/ghost-desk --mcp
ghost-server --mcp
```

在标准输入和输出上使用 MCP 通信，并把一切转交给使用同一主目录运行的 Ghost。`--mcp --on <name>` 则通过签名连接转交给这台 Mac 已配对的服务器 `<name>`。见 [Ghost 的工具](../tools-for-agents.md)。

## 环境变量 {#environment}

| 变量 | 含义 |
|---|---|
| `GHOST_HOME` | Ghost 的主目录。默认为 `~/.ghost`。 |
| `RUST_LOG` | 记录哪些日志，以 `tracing` 过滤器表示。默认为 `ghost_desk=info,ghost_core=info`。 |
| `GHOST_MCP_KEY` | 由 Ghost 为它交给会话的 `--mcp` 设置。请不要自己设置它。 |
