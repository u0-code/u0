---
title: Ghost 是什么
slug: /
description: Ghost 是一个桌面应用，把你已经在用的编程智能体放在一起。
---

# Ghost 是什么

Ghost 是一个桌面应用，把你已经在用的编程智能体放在一起：Claude
Code、Codex、Antigravity，以及所有支持
[Agent Client Protocol](https://agentclientprotocol.com)（ACP）的智能体，
在同一个窗口里并排运行，以同样的方式呈现。

Ghost **不是**又一个智能体。它没有自己的模型、提示词或智能体。每个智能体都作为独立的程序运行，
有自己的工具、自己的登录和自己的会话，和在它的 CLI 中运行时完全一样。Ghost
负责启动它、用 ACP 与它通信，并展示它所做的事。当 Ghost 自己需要写些文字（会话标题、提交信息）时，
它会请你的某个智能体来写。

## 窗口 {#the-window}

窗口分为三栏：

- **侧边栏**，在左侧：你的项目（文件夹），每个项目下列出它的会话，无论会话属于哪个智能体；
  置顶的会话在最上方；稍后提醒和已标为完成的会话收在底部的分组里。最底部是：设置、拉取请求、
  议题、用量，以及你的智能体。
- **对话区**，在中间：你说了什么、智能体做了什么：它用 Markdown 写的回答，每次工具调用各占一行，
  展开可看到其差异或输出，它的计划，以及它在行动前提出的问题。输入框在底部。
- **右侧栏**：会话的成果：差异、文件、拉取请求、终端、模拟器、旁聊，以及会话的详情。

## 你能做什么 {#what-you-can-do}

- [安装](install.md) Ghost 并[启用你的智能体](first-run.md)；
  缺少的智能体，Ghost 会从 ACP 注册表安装。
- 在[项目中开始会话](projects-and-sessions.md)，并在[对话区](thread.md)中跟进：
  模型、模式、@ 提及、斜杠命令、听写。
- 让会话在[它自己的工作树](worktrees-and-checkpoints.md)中工作，并撤销任意一轮。
- [审查](review.md)改动，在代码行上给智能体留评论，然后提交、推送并创建拉取请求。
- 处理你在 GitHub、GitLab、Bitbucket、Azure DevOps 和 Linear 上的
  [拉取请求和议题](forges.md)。
- 操控 [iOS 模拟器和 Android 模拟器](simulators.md)。
- 按计划或由 Webhook 触发运行提示词，并把多个步骤串成[工作流](automation.md)。
- 通过 MCP 为智能体提供 [Ghost 的工具](tools-for-agents.md)。
- [从另一台 Mac](remote.md) 使用某台机器上的 Ghost。

## 工作原理 {#how-it-works}

与每个智能体的通信都通过 ACP：JSON-RPC，每行一条消息，走 Ghost 所启动程序的标准输入和标准输出。
[ACP 注册表](https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json)
是唯一的智能体列表：它们的名称、版本、徽标以及各自的分发方式。Ghost 读取它、保留一份副本，
并按它所说的方式安装和启动智能体。

Ghost 由两部分组成：**服务器**，负责运行智能体、保存会话并操作机器（文件、git、终端）；
以及**窗口**，负责把这些画出来。在应用中两者一起运行；服务器也可以单独运行
（`ghost serve`，在 Linux 上是 `ghost-server`），供其他 Mac 上的窗口使用。

Ghost 的源代码位于 [github.com/u0-code/u0](https://github.com/u0-code/u0)，
采用 Apache-2.0 许可证。
