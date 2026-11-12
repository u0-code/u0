# 从 Codex 和 Claude Code 学到的

这份文档记录读 Codex 源码（`openai/codex`，Rust 实现，2026-11-12 的 HEAD 和 0.133.0 两个版本）和 Claude Code 官方文档之后的结论：它们怎么做到能长时间干活，哪些 Ghost 采纳了，哪些没有，为什么。

Claude Code 不开源，下面关于它的内容来自官方文档和 Anthropic 的工程文章，没有读它的代码。

## 它们的共同点

把两者摆在一起看，让一个循环撑过几个小时的不是循环本身，是下面这几件事。

**1. 完成是要被证明的，不是被宣布的。**
Codex 的 `/goal` 模式在每一轮结束后要求"逐条需求给证据"，原文的态度是把完成视为未经证明；间接或薄弱的证据算没做完。Anthropic 的文章结论类似：单独的评估者比自我检查可靠。Claude Code 的 `/goal` 用另一个模型看着记录判断条件是否成立。

**2. 上下文会满，满了之后靠文件接上，而不是靠摘要。**
Codex 压缩时把历史换成一份"给另一个模型的交接摘要"，保留用户的原话，丢掉全部工具输出。Claude Code 压缩后从磁盘重新注入：项目说明、计划文件、最近动过的几个文件、还在跑的后台任务。Anthropic 明确说只靠压缩不够，长任务要有结构化的进度文件，新的上下文窗口从文件开始有时比压缩更好。两者都在提示词里告诉模型：压缩不等于任务结束，不要重做，不要急着收尾。

**3. 工具输出要裁，而且裁头尾留中间的标记。**
Codex 每次调用约 1 万 token，保留头尾各一半，标明原始大小。Claude Code 把大输出写进文件，只返回路径和预览。错误信息通常在末尾，所以不能只留开头。

**4. 进程在任何地方死掉都能续上。**
Codex 的会话是只追加的 JSONL，恢复时给没有结果的工具调用补一条"被中断，可能已部分执行"。Claude Code 的会话也是边跑边写的 JSONL。

**5. 长命令不该卡住循环，也不该被杀掉。**
Codex 的 `exec_command` 约 10 秒后返回一个会话 id，进程继续活着，用另一个工具轮询或写入标准输入。Claude Code 把超时的前台命令转入后台。

**6. 改文件的工具要对"看不见的差异"宽容，对"有歧义"严格。**
Codex 的补丁匹配是一个级联：精确匹配，忽略行尾空白，忽略首尾空白，Unicode 标点归一。Claude Code 要求被替换的字符串在文件里唯一。

**7. 能力按需加载。**
两者的 skills 都只把名字和一句描述放进上下文，正文用到才读。Claude Code 的子 agent 用独立的上下文做大量阅读，只把一两千 token 的报告交回来。

## Ghost 采纳了什么

| 做法 | 来源 | 在 Ghost 里 |
|---|---|---|
| 第一次说完成，回以任务原文并要求逐条核对 | Codex `/goal` | ✅ `done` 工具 |
| 说完成必须附证据：每条需求对应刚跑的命令和输出 | 两者 | ✅ 没有证据的 `finished` 被拒绝 |
| 独立审查：另一次模型调用只看任务、证据和最近命令的真实输出，判 PASS / FAIL，只审一次 | Anthropic、Claude Code `/goal` | ✅ `Worker::audit`，`--no-audit` 可关 |
| 交接是一份会被替换的文档，不是越积越多的笔记 | Codex 压缩 | ✅ `handoff` 工具，只在上下文将被清空时提供 |
| 折叠后从磁盘重建：任务、计划、交接、最近的笔记、最近改过的文件 | Claude Code | ✅ |
| 告诉模型：清空上下文是例行公事，不要重做、不要赶工 | 两者 | ✅ 系统提示和每次重建的开场 |
| 先丢旧的工具输出，再考虑交接，分级收紧 | Claude Code 的 tool-result clearing | ✅ |
| 用模型报告的 token 数量加上未发送部分的估算来决定何时折叠 | Codex | ✅ |
| 输出保留头尾，中间标明裁掉多少 | Codex | ✅ |
| 超长输出整份存到文件，告诉模型路径 | Claude Code | ✅ `/tmp/ghost-out-N.txt` |
| 记录被打断时，给没有结果的调用补一条"可能已部分执行" | Codex | ✅ `mend` |
| 编辑时原谅行尾空白，但仍然拒绝有歧义的匹配 | Codex 的级联（只取前两级） | ✅ |
| `blocked` 只在换过不止一种办法之后才算 | Codex `/goal` | ✅ 提示词 |
| 大任务先把"做完是什么样"写成清单，看到通过才打勾 | Anthropic 的 initializer 做法 | ✅ 提示词，没有做成强制的第一坐 |
| 限流、过载、断线退避重试，尊重 Retry-After | Codex | ✅ `ghost-brain` |
| 只有工作目录跨命令保留 | Claude Code | ✅ |
| 长时间的命令放后台，成为可等待、可取回增量输出的任务 | 两者 | ✅ `shell` 的 `background` 和 `job` |

## 没有采纳，以及为什么

- **前台命令自动让出控制权（10 秒后返回会话 id）。** 没有照搬：它要求命令在独立的会话里跑，和"工作目录跨调用保留"的简单实现冲突。采纳的是它的效果：`background` 把命令变成一个有编号的任务，永不被超时杀掉；`shell` 带 `job` 参数可以等它结束，并只取回上次看过之后新打印的内容。
- **保留用户原话跨越折叠。** Codex 这么做是因为对话里用户会中途改需求。Ghost 的一件事只有一份不变的 brief。等"做事途中对方又补充了要求"接进来时再做。
- **补丁语法（`apply_patch`）。** 是为特定模型训练过的格式，Codex 自己也只对目录里登记的模型提供。DeepSeek 上精确替换更稳。
- **子 agent。** 对大量阅读的任务有用，但会让预算和可恢复性复杂一倍。先把单个循环做稳。
- **只读工具并行执行。** 省时间，不改变能力。之后再说。
- **读过才能改。** 精确匹配本身已经要求模型知道文件里现在是什么。
- **skills。** Ghost 里对应的东西是"手艺"，由她自己从经验里写出来，见 `WORK.md` 第 9 节。

## 把它们当后端

**Claude Code。** 可以做到只跑 Ghost 的工具：

```bash
claude -p --bare --input-format stream-json --output-format stream-json --verbose \
  --system-prompt-file system.md --tools "" \
  --mcp-config mcp.json --strict-mcp-config \
  --allowedTools "mcp__ghost__*" --permission-mode dontAsk
```

`--tools ""` 去掉所有内置工具，Ghost 的工具通过一个 stdio 的 MCP 服务端交给它。`--bare`（2.1.81 起）跳过用户自己的设置、记忆、插件。旧版本没有 `--bare`，要用空的 `CLAUDE_CONFIG_DIR` 代替。`claude -p` 在 2.1.79 之前不关标准输入会一直等。

**Codex。** 只支持 Responses 接口，`wire_api="chat"` 直接报错，所以只能对接提供 `/responses` 的服务。`codex exec --json` 加 `-c mcp_servers.ghost.command=…` 可以挂上 Ghost 的工具，`--disable shell_tool` 去掉它自己的 shell；但 0.133 去不掉 `update_plan` 等几个内置工具，做不到"只有我的工具"。`codex app-server` 的 `dynamicTools` 更合适：Codex 回调进 Ghost 的进程，不需要 MCP 服务端，不过它标着实验性。

**条款。** Anthropic 的 Agent SDK 文档写明未经批准第三方不得提供 claude.ai 登录或订阅额度，法律页补充说用户用自己的订阅登录未经修改的官方程序不受限制。所以 Ghost 只复用本机已登录的 CLI，不碰凭证；面向公众长期运行的角色建议用 API key。

## 还没验证的

- 上面的后端做法来自文档和源码，除了"Claude Code 2.1.322 接 DeepSeek 兼容端点能跑通一个任务"之外，没有实际跑过。
- Codex 0.133 的默认提示词和压缩常量是否与 HEAD 完全一致，只抽查了几项。
