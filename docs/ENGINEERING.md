# Ghost 工程设计 v0

> 产品层的七层（见 ARCHITECTURE.md）落到工程上，只有一条原则：**ghostd 是唯一的真相，其他一切都是它的客户端。**

## 1. 拓扑：一个守护进程，一个协议，多个客户端

```
                  ┌───────────────────────────────────┐
                  │  ghostd  (Rust, 单二进制)           │
                  │  ├ identity  出生证明 + 封印校验     │
                  │  ├ journal   哈希链事件日志 (SQLite) │
                  │  ├ memory    派生索引 / 正典账本     │
                  │  ├ life      tick 调度 / 睡眠周期    │
                  │  ├ will      回不回 / 多久回 / 离开   │
                  │  └ brain     模型提供方 (可换)        │
                  └───────────────┬───────────────────┘
                                  │  Ghost Protocol
                                  │  JSON-RPC 2.0 over WebSocket
          ┌───────────────┼─────────────────┐
          │               │                 │
   ghost-telegram   ghost-discord       ghost CLI
   (Presence 适配器)  (Presence 适配器)    (终端 / 运维)
```

- **ghostd 不认识任何通道**。Telegram、Discord、X、电话都是外部进程，通过协议连进来。ghostd 里没有 telegram 这个词。
- **一个 Ghost 是磁盘上的一个目录**。拷走目录就是搬家。目录是人，二进制是身体。
- 每个 Ghost 独立运行一个 `ghostd` 进程。本地和远程客户端通过同一协议连接，进程由命令行或服务管理器启动。

这是 LSP 的模型：一个协议，任意客户端，任意语言。要写一个微信适配器，不需要懂 Rust，只需要会说 JSON-RPC。

## 2. 语言：核心用 Rust，边缘随意

选 Rust 的理由，按重要性排：

1. **单二进制即可运行**。`ghostd run ./lin` 就是她在活。没有 venv，没有 pip，没有版本地狱。她要跑好几年，身体得结实。
2. **类型系统守规矩**。"不可重置、不可改人格"这种约束在 Rust 里可以用类型表达：`SealedIdentity` 没有 `&mut` 方法。在 Python 里只是一个注释。
3. **开源形象**。2026 年的开源基础设施项目，Rust 核心是信任信号。

Rust 的代价和对策：

- 迭代慢，尤其是 prompt 密集的逻辑。对策：**所有 prompt 和人格相关的东西都是数据，不是代码**。模板文件、TOML、Markdown。改 prompt 不用重新编译。
- 贡献者池小。对策：协议是语言无关的。适配器、工具、实验都可以用 Python 写。核心小而稳，边缘大而活。
- 插件难。对策：不做进程内插件。所有扩展都是外部进程（通道适配器走 Ghost Protocol，工具走 MCP）。

Python 保留给：适配器原型、研究脚本、评估工具。仓库里会有 `tools/` 目录放这些。

## 3. 仓库结构（Cargo workspace）

```
Ghost/
├── Cargo.toml               # workspace
├── crates/
│   ├── ghost-core/          # 领域核心：identity / journal / memory / life / will
│   │                        # 不碰网络。只依赖 SQLite 和序列化。可被任何二进制嵌入。
│   ├── ghost-brain/         # 模型提供方抽象：Brain trait + anthropic / openai-compat
│   ├── ghost-protocol/      # Ghost Protocol 的类型定义，daemon 和客户端共用
│   ├── ghostd/              # 守护进程 + CLI 子命令 (birth / run / status / journal)
│   └── presence/
│       └── ghost-telegram/  # 第一个通道适配器，独立二进制
├── ghosts/
│   └── _template/           # 出生证明模板。不是真实 Ghost。
├── tools/                   # Python 脚本：评估、回放、研究
└── docs/
```

依赖方向只允许向内：`ghostd → ghost-brain → ghost-core`，`ghostd → ghost-protocol ← 客户端`。`ghost-core` 不依赖任何其他 crate。

## 4. 一个 Ghost 的目录

```
lin/
├── ghost.toml        # 出生证明。写一次。
├── seal             # 出生证明的 BLAKE3 哈希，由 `ghostd birth` 生成
├── journal.db        # 只追加事件日志，哈希链
├── derived/          # 可删除、可重建的一切：索引、正典账本、关系表
└── prompts/          # 她的叙述模板（可选覆盖默认）
```

**封印机制**：`ghostd birth` 读 `ghost.toml`，算哈希，写入 `seal`，并把 `Born { seal }` 作为 journal 的第一个事件。之后每次 `ghostd run` 启动时重新算哈希，和 Born 事件对比，不一致就拒绝启动，报错信息是 "this is not her"。没有 `--force`。

**哈希链**：每个事件记录 `prev_hash`，`hash = blake3(prev_hash ‖ seq ‖ ts ‖ kind ‖ payload)`。改历史会断链。这不是安全机制（本地文件谁都能重写整条链），是**诚实机制**：篡改要故意、要费力，并且可以被 World 检测。

## 5. ghost-core 的模块

| 模块 | 职责 | v0 范围 |
|---|---|---|
| `identity` | 解析 ghost.toml，封印，提供只读视图 | 完整 |
| `journal` | 事件追加、读取、哈希链校验、订阅 | 完整 |
| `memory` | 从 journal 派生：正典账本、关系表、FTS 检索 | 正典 + FTS，embedding 后置 |
| `life` | 时钟：当前状态（睡/醒/忙/闲）、日计划、睡眠周期触发 | 固定作息 + 日计划事件 |
| `will` | 入站消息的处理决定：忽略 / 稍后 / 回复；关系状态机 | 规则 + brain 判断 |
| `narrate` | 把状态和记忆组装成 brain 的输入；模板驱动 | 完整 |

事件种类（v0）：`Born`、`Woke`、`Slept`、`Planned`、`Lived`（生活引擎生成的经历）、`Heard`（入站）、`Said`（出站）、`Decided`（will 的决定及理由）、`Reflected`（睡眠周期）、`Canon`（硬事实登记）。

所有状态都能从这些事件重放出来。`derived/` 删掉再跑一遍 `ghostd rebuild` 就回来。

## 6. Ghost Protocol

JSON-RPC 2.0，WebSocket，本地默认 unix socket，远程 TCP + token。

客户端分两类，连接时声明：
- **presence**：通道适配器。能发 `presence.inbound`，会收到 `presence.outbound` 通知。
- **console**：只读观察客户端角色，供 `ghostd watch`、`people`、`agenda` 等命令使用；与桌面应用无关。不能修改身份或删除事件，出生由 CLI 的 `birth` 子命令完成。

核心方法（v0）：

```
ghost.info                      → 她是谁、在哪、现在状态
ghost.state                     → life 当前状态
journal.tail  (subscription)    → 实时事件流
journal.read  {from, limit}     → 历史
memory.canon                    → 正典账本
memory.relations                → 关系表
presence.register {channel}     → 适配器注册
presence.inbound  {channel, from, text, ts}
← presence.outbound {channel, to, text}   (通知)
birth.create {dir, seed}        → 出生一个新 Ghost（只在 ghostd 未绑定 Ghost 时可用）
```

## 7. Brain

```rust
#[async_trait]
trait Brain: Send + Sync {
    fn label(&self) -> String;
    async fn complete(&self, req: &Completion) -> Result<Reply>;
}
```

四种实现，配置在 `ghostd.toml` 的 `[providers.*]` 里，**不在 ghost.toml 里**。模型不是她。

| kind | 走什么 | 认证 |
|---|---|---|
| `anthropic` | Anthropic Messages API | `ANTHROPIC_API_KEY` |
| `openai` | 任何 OpenAI 兼容的 `/chat/completions`，含 DeepSeek、OpenRouter、Moonshot、Ollama、vLLM | `OPENAI_API_KEY` 或自定义 |
| `claude-code` | 把官方 `claude` CLI 当子进程跑（`claude -p --output-format json`） | CLI 自己的登录态 |
| `codex` | 把官方 `codex` CLI 当子进程跑（`codex exec -s read-only -o <file>`，空目录） | CLI 自己的登录态 |

两个 CLI 转接的原则：**只驱动官方二进制，不碰它们存储的凭证**。抠 OAuth token 直接打 API 的做法违反两家的订阅条款，Anthropic 已经封过这么干的第三方工具。子进程转接走的是官方文档里的非交互模式，用你自己的订阅做实验没问题，给别人用的 Ghost 应该跑在 API key 上。

两档：`think`（对话、反思）和 `tick`（生活引擎日常），各指向一个 provider。`ghostd brains --probe` 会让每个 provider 说一个词，用来确认接通。

**narrate**（在 ghost-core 里）负责把身份、时间、最近的经历、和这个人的对话线程组装成 brain 的输入。模板是 `crates/ghost-core/prompts/self.md`，可以用 `[brain].self_template` 换掉。模板属于身体，不属于人：它是 brain 读证书的方式，证书本身仍然封印。

## 8. 建造顺序（每一步可单独推送）

1. **骨架**：workspace、ghost-core 的 identity + journal、`ghostd birth` / `ghostd run`。✅
2. **Brain**：四种 provider。✅
3. **Mind + Protocol + 终端通道**：见 [LIVING.md](LIVING.md)。议程、注意、动作、说话状态机、生活引擎、主动联系、刷信息流；JSON-RPC 服务端；`ghost-link`；`ghost-term`。✅
4. **平台适配器**：Telegram、Discord、OneBot（QQ）、X。
5. **World**：第二个 Ghost，Ghost 之间的协议。

> 第 1 到 7 节是最初的设计。实现过程中 crate 划分有调整（多了 `ghost-mind` 和 `ghost-link`，协议走 TCP 上的 WebSocket），以 LIVING.md 第 8 节和各 crate 的 README 为准。

## 9. 工程规矩

- Rust 2024 edition，`cargo clippy -D warnings`，`cargo fmt`。
- 每个 crate 有自己的 README，说清它是什么、不是什么。
- 提交信息用 Conventional Commits。
- 每个有效功能叠加一次提交一次推送。
- MIT 或 Apache-2.0 双许可。宪章单独一份文件，不是许可证的一部分。
