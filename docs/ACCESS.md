# 外部接口：需要什么，怎么拿到

Ghost 自己不带任何账号。下面是她要用到的每一样外部东西：去哪里申请、怎么操作、拿到的凭证是什么、花多少钱、平台的限制。

信息于 2026-11-12 从各家官方页面核对。价格和规则会变，申请前再看一眼对应页面。标"未核实"的是当时没能在官方页面确认的。

## 现在就需要的

### DeepSeek（模型）

| | |
|---|---|
| 申请 | <https://platform.deepseek.com/api_keys> 注册、充值、创建 key |
| 凭证 | API key，环境变量 `DEEPSEEK_API_KEY` |
| 地址 | OpenAI 格式 `https://api.deepseek.com`；Anthropic 格式 `https://api.deepseek.com/anthropic` |
| 模型 | `deepseek-flash`（V4.1 Flash）、`deepseek-v4-pro`。旧名 `deepseek-chat` / `deepseek-reasoner` 官方已排期停用，不要再写进配置 |
| 能力 | 上下文 1M，两个模型都支持工具调用和 JSON 输出，默认开思考模式 |
| 价格（每百万 token，非高峰 / 高峰） | flash：缓存命中 $0.003 / $0.006，未命中 $0.15 / $0.3，输出 $0.6 / $1.2。v4-pro：$0.022 / $0.044，$0.66 / $1.32，$1.98 / $3.96 |
| 高峰时段 | 工作日 UTC 01:00–04:00 和 06:00–10:00（北京时间 9–12 点、14–18 点） |
| 注意 | 带工具时，模型返回的 `reasoning_content` 必须在之后每次请求里原样带回，否则 400。Ghost 的 `carry` 字段处理了这件事 |

### Docker（她的 shell 的沙箱）

- macOS：Docker Desktop，<https://docs.docker.com/desktop/setup/install/mac-install/>（Apple 芯片安装包 <https://desktop.docker.com/mac/main/arm64/Docker.dmg>）。个人和开源使用免费。
- Linux 服务器：Docker Engine，<https://docs.docker.com/engine/install/>，按发行版走 apt / dnf 源。
- 不需要凭证。`ghost-work --docker <容器名>` 让她的命令在容器里执行。

## 做"搜索和读网页"时需要

| 服务 | 申请 | 免费额度 | 价格 / 千次 | 鉴权 |
|---|---|---|---|---|
| Brave Search（建议默认） | <https://api-dashboard.search.brave.com/register> | 每月 $5 额度，约 1000 次 | $5 | 请求头 `X-Subscription-Token` |
| SearXNG（免费兜底，自建） | <https://docs.searxng.org/admin/installation-docker.html> | 无限 | $0 | 无。要在 `settings.yml` 的 `search.formats` 里加上 `json` |
| Tavily | <https://app.tavily.com> | 每月 1000 credits | $8 | `Authorization: Bearer tvly-…` |
| Exa | <https://dashboard.exa.ai/api-keys> | 每月 $10 额度 | $4–12 | `x-api-key` |
| Serper | <https://serper.dev> | 一次性 2500 次 | $1 起 | `X-API-KEY` |

读网页：静态页面直接 HTTP 抓取加正文提取就够。JS 渲染或拦截爬虫的页面可以用 Jina Reader 兜底：`GET https://r.jina.ai/<url>`，无 key 每分钟 20 次，免费 key 每分钟 500 次（<https://jina.ai/reader/>）。DeepSeek 在 2026-10-17 开放了自己的搜索和抓取接口，价格未核实。

**需要你做的**：注册 Brave Search，把 key 给我（环境变量名 `BRAVE_API_KEY`）。不想注册的话我用自建 SearXNG。

## 平台账号

### X（Twitter）

| | |
|---|---|
| 申请 | 用她的 X 账号登录 <https://console.x.com>，接受开发者协议，New App，权限选 "Read and write"（要私信则选带 DMs 的），保存凭证，充值 |
| 凭证 | API Key + Secret，Access Token + Secret（OAuth 1.0a，代表这个账号）。只显示一次 |
| 计费 | 官方文档现在只有按量付费。发帖 $0.015（**带链接的帖子 $0.20**），读首页时间线每条 $0.005，读自己的提及和帖子数据每条 $0.001，读用户信息（含粉丝数）$0.010，私信读 $0.010 / 发 $0.015。存卡并首充后送 $20 |
| 能拿到的数据 | 自己帖子的赞、转、回复、引用、展示量、收藏；粉丝数 |

规则（<https://help.x.com/en/rules-and-policies/x-automation>，2026 年 4 月版）对设计有硬约束：

- **AI 自动回复机器人需要 X 事先的书面批准。**
- 自动回复和 @ 只能发给主动找过你的人（回复过你、@ 过你、私信过你），每次互动一条。
- **禁止自动点赞。** 禁止批量关注和取关。禁止自动发热点话题。
- 自动私信只能发给先联系你的人。
- 必须打开账号的 "Automated" 标签，在简介里说明是机器人和运营者，并关联一个真人账号。
- 不走 API、模拟网页操作的，可能永久封号。

对 Ghost 的影响：原创发帖是允许的；`ghost-x` 现在的自动点赞要默认关掉；"主动回复别人的帖子"在拿到 X 的批准之前不能做。

### Telegram

- Bot：在 <https://t.me/botfather> 发 `/newbot`，起名，用户名以 `bot` 结尾。拿到形如 `110201543:AAH…` 的 token。免费。
- Bot 的限制：不能主动给没找过它的人发消息；群里默认只看得到命令、对它的回复和 @（关掉 privacy mode 或设为管理员才能看全部）；永远带 "bot" 标记。
- 用户号路线（MTProto）：在 <https://my.telegram.org> 的 "API development tools" 拿 `api_id` / `api_hash`。技术上可行，但非官方客户端登录的账号会被重点观察，刷屏会永久封号，封的是手机号。要用就用专门的号、低频、只回复。Ghost 目前没有实现这条路线。

### Discord

1. <https://discord.com/developers/applications> → Create。
2. Bot 页 → Reset Token，复制（只显示一次）。
3. Bot 页打开 `MESSAGE CONTENT` 特权意图，否则除了私信和 @ 她的消息，内容都是空的。
4. Installation 页选 `bot` 和 `applications.commands`，选权限，用生成的链接把她加进服务器。

免费。超过 100 个服务器后特权意图需要审核。用普通用户号做自动化（self-bot）是明令禁止的，会封号。

### QQ

- 官方开放平台：<https://q.qq.com>，QQ 号加实名认证，拿 `AppID` / `AppSecret`。个人主体最多 5 个机器人，不能公开使用；未认证时只能在自己是群主或管理员的群里用。被动回复有时间窗口（私聊 60 分钟内 4 条，群 5 分钟内 5 条）。Ghost 目前没有实现官方平台的适配器。
- OneBot 路线（Ghost 现有的 `ghost-onebot`）：用普通 QQ 号登录第三方实现。**NapCat 的仓库在 2026-11-12 被清空，上游已停止**；还在维护的有 LLBot（<https://llonebot.com>，支持 OneBot 11）和 Lagrange（V2 转向 Milky 协议）。这条路线不被腾讯认可，有风控和封号风险，只能用小号。

### 小红书

- **没有面向个人创作者的官方发布或数据接口。** <https://open.xiaohongshu.com> 是给电商店铺的。
- 现有的自动化项目都是用无头浏览器操作创作者网页，例如 <https://github.com/xpzouying/xiaohongshu-mcp>（扫码登录，能发图文和视频、读笔记数据、评论）。
- 平台规则（经媒体报道核对，未读到平台原文）：2026 年 3 月起打击"AI 托管"账号；4 月 27 日的规则要求所有 AI 参与创作的内容主动标识，并明确"对于明确由 AI 托管运营、无真实人类参与的账号，平台将予以封禁"。国家层面《人工智能生成合成内容标识办法》也要求发布者主动声明。

对 Ghost 的影响：一个完全自主运营的角色正好是被封禁的那一类。在小红书上站得住的做法只有一种：她写、她配图，由人审核并点发布，并打上 AI 标识。这需要你决定是否接受。

## 之后可能需要的

### 生图（头像、配图，保持角色一致）

| 选项 | 申请 | 价格 | 参考图 |
|---|---|---|---|
| FLUX（Black Forest Labs） | <https://bfl.ai> | Kontext [pro] $0.04 / 张 | 支持，官方明确说能在多次编辑中保持角色一致 |
| Gemini 图像 | <https://aistudio.google.com> | 约 $0.03–0.13 / 张 | 支持图像输入，一致性未核实 |
| OpenAI GPT Image | <https://platform.openai.com/api-keys> | 约 $0.006–0.21 / 张 | 支持多张参考图，官方承认角色一致性偶尔不稳 |

做法：先定一张角色设定图，之后每次生成都把它作为参考图传进去。二次元风格的实际效果三家都没有官方说法，需要试。

### 其他模型

- Anthropic：<https://platform.claude.com/settings/keys>，环境变量 `ANTHROPIC_API_KEY`。
- OpenAI：<https://platform.openai.com/api-keys>，环境变量 `OPENAI_API_KEY`。
- Claude Code / Codex：本机安装并登录即可，Ghost 不读它们的凭证。Anthropic 的 Agent SDK 文档写明未经批准第三方不得提供 claude.ai 登录或订阅额度，所以面向公众长期运行的角色建议用 API key。

### 托管沙箱（不想自己管 Docker 时）

E2B：<https://e2b.dev>，免费档送一次性 $100 用量、会话最长 1 小时；用量约每 vCPU 小时 $0.05。

## 没能核实的

- `deepseek-chat` / `deepseek-reasoner` 是否已经实际下线（今天用 `deepseek-chat` 仍然能通）。
- Brave 免费额度是否需要绑卡。
- X 控制台里权限设置和 Automated 标签开关的确切位置。
- QQ 官方平台"主动消息每天 1000 条"的数字。
- 小红书蒲公英和专业号的接口范围；平台公告的原文。
