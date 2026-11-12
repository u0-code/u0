# 做事：长程架构与接口规范

> 一个会话式的 agent 以"对话记录"为中心：记录不断变长，满了就压缩。Ghost 以"一件事"为中心：一件事可以跨好几天，中间夹着聊天、睡觉和别的事，任何时候进程停掉都能接上。

这份文档定义四个目标（长程任务能力、模型即装即用、跨平台上下文、公开生活与自我迭代）共用的骨架和接口。状态一栏写明每一项是已实现、已规范未实现，还是只有方向。

## 1. 五个时间尺度

| 尺度 | 是什么 | 持续 | 状态存在哪 |
|---|---|---|---|
| 一步 step | 一次模型调用，加它要求的工具调用 | 秒 | transcript |
| 一坐 sitting | 对一件事的一段专注工作，上下文从头组装 | 分钟到一小时 | transcript（每步落盘） |
| 一件事 undertaking | 目标、计划、笔记、结局、工作目录 | 小时到几周 | 目录（`state.json`） |
| 追求 pursuit | 几个月尺度的方向，派生出一件件事 | 月 | journal（未实现） |
| 人生 | 每晚、每周的反思，调整追求和手艺 | 一直 | journal |

只有一个循环。回一条消息、刷时间线、做一件事，是同一个循环配不同的工具集和预算。回消息是"很小的一坐"。

## 2. 模型接口（`ghost-brain`）✅ 已实现

```rust
trait Brain {
    fn label(&self) -> String;
    async fn complete(&self, req: &Completion) -> Result<Reply>;   // 文字进，文字出
    fn works(&self) -> bool;                                        // 能否带工具
    async fn step(&self, turn: &Turn) -> Result<Step>;              // 带工具的一步
}

struct Turn { system, items: Vec<Item>, tools: Vec<ToolSpec>, max_tokens, temperature }
enum   Item { User{text}, Assistant{text, calls, carry}, Result{id, name, output, failed} }
struct Step { text, calls: Vec<Call>, stop: End|Calls|Length, usage: {input, cached, output}, model, carry }
```

- `carry` 是提供方要求原样带回的东西（DeepSeek 的 `reasoning_content`、Anthropic 的 thinking 块），对其他所有人不透明。
- brain 永远不自己执行工具。循环在 `ghost-work` 里。
- 限流、过载、断线在 brain 内部退避重试（最多 5 次）。

模型名和"怎么到达它"是两个轴：

| 运行时 | 到达方式 | 对话（`complete`） | 做事 | 状态 |
|---|---|---|---|---|
| `openai` | 任何 OpenAI 兼容的 HTTP 接口（DeepSeek、OpenRouter、Ollama…） | ✅ | ✅ 自己的循环（`step`），已在 DeepSeek 上实测 | 已实现 |
| `anthropic` | Anthropic Messages API | ✅ | ✅ 自己的循环，未对真实 API 测过（没有 key） | 已实现 |
| `claude-code` | 本机的 `claude`，子进程 | ✅ 关掉它自带的工具，一次调用约 300 token | ✅ 由 CLI 推进每一步，工具是 Ghost 的（经本地 MCP 服务端），已实测 | 已实现 |
| `codex` | 本机的 `codex` | ✅ | ⬜ `codex app-server` 的 `dynamicTools`，见 `STUDY.md` | 已规范 |

两种"做事"的方式：

- **自己的循环**（`ghost_work::Worker`）：每一步都是一次 `Brain::step`，上下文怎么折叠、何时交接、完成后的独立审查都由 Ghost 掌控。
- **CLI 推进**（`ghost_work::claude`）：`claude` 以无头模式启动，`--tools ""` 关掉全部内置工具，只挂一个 MCP 服务端，就是 `ghost-work --serve-mcp`。系统提示、工具、落盘的"一件事"都是 Ghost 的；CLI 贡献的是它登录的那个模型，以及工具调用之间的推进。代价是上下文由 CLI 自己压缩，没有独立审查那一步。

CLI 运行时的规则（参照 OpenClaw 的做法）：复用本机已登录的 CLI，不读它的凭证；不加载运营者自己的 CLAUDE.md、记忆和 MCP 服务端，那些是运营者的，不是她的。

发现 ✅：`ghostd brains --discover` 列出本机可用的运行时（环境里的 key、PATH 上的 `claude` / `codex`、本地的 Ollama），并给出可直接粘贴进 `ghostd.toml` 的配置块。

## 3. 机器（`ghost-work::machine`）✅ 已实现

```rust
trait Machine {
    async fn exec(&self, script: &str, stdin: Option<Vec<u8>>, timeout: Duration) -> io::Result<Output>;
    fn describe(&self) -> String;
}
```

机器只有一个操作：跑一段 bash，拿回它打印的东西。读、写、改文件都建立在它上面，所以新增一种机器只需要一个小实现。

- `Local`：本机的一个目录。不是沙箱，命令以你的身份运行。
- `Docker`：一个正在运行的容器。超时在容器内执行，因为杀掉 `docker exec` 客户端并不会停掉它启动的进程。**服务器上她的 shell 应该在这里。**

## 4. 工具（`ghost-work::tool`）✅ 已实现

```rust
trait Tool {
    fn spec(&self) -> ToolSpec;                 // 名字、给模型看的说明、参数的 JSON Schema
    async fn call(&self, args: &Value) -> Outcome;   // { output, failed }
}
```

工具从不让循环失败：出什么错都以模型读得懂的文字返回。

| 工具 | 作用 | 状态 |
|---|---|---|
| `shell` | 跑 bash。工作目录跨调用保留；可设超时；可后台运行并返回日志路径 | ✅ |
| `read` / `write` / `edit` | 带行号读、整文件写、精确替换（必须唯一匹配） | ✅ |
| `plan` | 重写整份计划，每步带状态 | ✅ |
| `note` | 给以后的自己留一条事实 | ✅ |
| `done` | 结束：`finished` / `blocked` / `gave_up`，附证据 | ✅ |
| `search` / `fetch` | 搜索、读网页 | ⬜ 见 `ACCESS.md` |
| `recall` / `remember` | 查、写她自己的记忆 | ⬜ 接入 Mind 时 |
| `say` / `post` / `react` | 在各平台上说话。公开且不可撤回的动作过发布关 | ⬜ 接入 Mind 时 |

## 5. 一件事（`ghost-work::undertaking`）✅ 已实现

```
<dir>/state.json        要求原文、计划、笔记、结局、累计的步数/token/时间
<dir>/transcript.json   当前这一坐，每步之后保存
<dir>/log.jsonl         每步一行：调了什么、花了多少
```

要求（brief）只在第一次写入，之后不变。

## 6. 循环（`ghost-work::worker`）✅ 已实现

```rust
struct Worker { brain, tools, undertaking, limits, who, machine, pause }
async fn run(&self) -> Report   // { ended, summary, steps, sittings, usage, seconds }
enum Ended { Finished, Blocked, GaveUp, OutOfSteps, OutOfTime, Stalled, Paused, Failed(String) }
```

一步很简单：模型读 transcript，说话或要工具；工具运行；输出进 transcript。让它撑过几个小时的是外面这几条：

- **状态在上下文之外。** 计划、笔记、结局在 `state.json`，transcript 每步落盘。进程在任何地方停下都能续上。
- **上下文是折叠的，不是一直长的。** 快到预算时先丢掉旧的工具输出；还不够，就让她给自己写一份交接（只开放 `note` 和 `plan`），然后只带着要求、计划、笔记重新开始一坐。
- **完成要过一遍检查。** 第一次说 `finished`，得到的回答是要求原文和"逐条对着真实状态检查"。第二次才记录。
- **卡住会被说出来。** 只说不做、同一个调用重复三次、步数快用完，都会明确告诉模型。连续三次不行动则以 `Stalled` 结束。
- **不知道是谁在做。** `who` 是外面递进来的一段话。单独运行时它是一个任务 agent；在 Ghost 里是她做任何"不止一个念头"的事的方式。

命令行：

```bash
ghost-work --task-file task.md --state ./run1 --docker <容器名> --workdir /app
# 同一个 --state 再跑一次，就是接着做
```

## 7. 接入 Mind ✅ 已实现

- **动作 `undertake`**：对话里她可以决定接下一件需要真正动手的事。它成为 journal 里的 `Undertook { id, brief, asked_by, place }` 和 `work/<id>/` 目录。
- **一坐一坐地做**：每一坐是一次有上限的循环（默认 40 步或 15 分钟），作为异步任务派出去，结果以 `Outcome::Worked` 回到 actor。journal 仍然只有 Mind 在写。每坐结束记 `Worked`，了结时记 `Concluded { outcome, summary }`。
- **一次只做一件事**：一坐进行中她是 busy，消息等得更久。真去看手机时，先让这一坐在当前这步之后停下（状态都在盘上），看完、说完，再回来接着做。
- **做完去告诉对方**：了结后生成一条 `owed` 的念头。"答应了回来说结果"不算追着人发消息，所以不受"对方没回就不再追"的限制。说不说、怎么说，仍然是到时候她自己决定。
- **意志之下的底线**：她自己决定接不接。代码只守两条：有 shell 的情况下，请她做事的人至少得是朋友；每天接的事有上限。
- **机器**：`[mind.work] machine = "docker:<容器>"` 或 `"local:<目录>"`。不配就没有 shell，她只能查资料和思考。
- **没有做的**：对话本身仍然是"一次返回一组动作"，没有改成工具循环。原因是这样对只会答话的大脑（CLI 的对话通道）也成立；需要查东西再回答时，她用 `recall` 多想一轮，需要动手时用 `undertake`。

## 8. 注意力与上下文

已实现 ✅：

- **今天的其他事**：每次看一个对话，开头都带着她此刻的全貌：手上接了什么、做到哪了、最近几小时其他地方（跨平台）发生了什么、有多少没读。
- **出处**：每条笔记记着是在哪里知道的，回忆出来的内容也带着出处。来自别的私聊的笔记，在其他场合出现时标着"这是私下告诉你的"，并明确要求不复述、不表现出知情。
- **做事时的注意力**：见第 7 节。

只有方向 ⬜：

- "看手机"作为一次跨平台的统一分拣（现在仍然是每个地方各自到点，只是互相看得见）。
- 记忆的层级视图：对话片段摘要、每个人的档案。现在有的是每日反思和逐条笔记。
- 跨平台认人：别人声称"我是另一个平台上的某某"不可信，需要反向验证，仍未解决。

## 9. 手艺与公开生活 ⬜ 只有方向

- 身份封印不动，迭代只发生在手艺层：她从真实数据里总结的经验，以事件记录。
- 适配器新增 `metrics` 能力：帖子的数据、粉丝数、谁互动了。
- 每周复盘是一件事，用的就是第 6 节的循环。
- 平台规则是硬约束，见 `ACCESS.md`。有两条直接限制了这个目标的做法：X 上 AI 自动回复需要 X 的书面批准、禁止自动点赞；小红书封禁"AI 托管、无真人参与"的账号。
