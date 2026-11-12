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

| 运行时 | 到达方式 | `complete` | `step` | 状态 |
|---|---|---|---|---|
| `openai` | 任何 OpenAI 兼容的 HTTP 接口（DeepSeek、OpenRouter、Ollama…） | ✅ | ✅ 已在 DeepSeek 上实测 | 已实现 |
| `anthropic` | Anthropic Messages API | ✅ | ✅ 未对真实 API 测过（没有 key） | 已实现 |
| `claude-code` | 本机已登录的 `claude`，子进程 | ✅ | ⬜ 常驻子进程 + 把 Ghost 的工具经本地 MCP 桥交给它 | 已规范 |
| `codex` | 本机已登录的 `codex` | ✅ | ⬜ `codex app-server`（stdio 上的 JSON-RPC） | 已规范 |

CLI 运行时的规则（参照 OpenClaw 的做法）：复用本机已登录的 CLI，不读它的凭证；系统提示是 Ghost 的；CLI 自带的文件和 shell 工具关掉，只暴露 Ghost 的工具；权限判断留在 Ghost。走 CLI 时工具调用之间的内层循环在 CLI 里，所以 Ghost 的长程结构必须建立在外层（一件事、交接、落盘），不依赖掌控每一步。

发现（未实现）：`ghostd brains` 列出本机可用的运行时：PATH 上的 `claude` / `codex` 及其登录状态、环境里的 key、本地的 Ollama。

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

## 7. 接入 Mind ⬜ 已规范，未实现

- 新的 journal 事件：`Undertook { id, brief, origin }`、`Worked { id, sitting, summary }`、`Finished { id, outcome, summary }`。journal 只经过 Mind 写，一坐是派出去的异步任务，结果以 `Outcome` 回到 actor。
- 一件事有来源：她自己的追求，或者别人请她帮忙（她可以拒绝）。
- 一坐占着她的时候，她的 reach 是 busy，消息照常堆积；需要打断时置 `pause`，一步结束后停下，状态都在盘上。
- 现在"一次返回一组 JSON 动作"的 deliberation 改成同一个循环：工具是 `say`、`react`、`silence`、`later`、`note`、`feel`、`recall`…，预算很小。

## 8. 注意力与上下文 ⬜ 只有方向

- 同一时刻一个前台活动（聊天、刷、做事、休息），其余都是通知，按显著性排队。
- "看手机"是一次跨平台的分拣。
- 上下文分层组装，每层有预算：自我、此刻、工作记忆（跨平台的近况）、场景、检索。
- 记忆是层级的派生视图：journal → 对话片段摘要 → 每日反思 → 每个人的档案。
- 每条记忆带出处和"谁知道"。私下听到的不进公开场合，也不转给别人。

## 9. 手艺与公开生活 ⬜ 只有方向

- 身份封印不动，迭代只发生在手艺层：她从真实数据里总结的经验，以事件记录。
- 适配器新增 `metrics` 能力：帖子的数据、粉丝数、谁互动了。
- 每周复盘是一件事，用的就是第 6 节的循环。
- 平台规则是硬约束，见 `ACCESS.md`。有两条直接限制了这个目标的做法：X 上 AI 自动回复需要 X 的书面批准、禁止自动点赞；小红书封禁"AI 托管、无真人参与"的账号。
