---
title: 托管策略
description: 组织如何在一台机器上设置、锁定和限制 Ghost。
---

# 托管策略

管理员可以给一台机器下发一份策略：一个 JSON 文件，用来设置 Ghost 的设置、锁定其中一些，并限制 Ghost 可以做什么。Ghost 在启动时读取它一次。

| 系统 | 策略从哪里读取 |
|---|---|
| macOS | `/Library/Application Support/Ghost/policy.json` |
| Linux | `/etc/ghost/policy.json` |

这两个位置都只有管理员可以写入。

## 格式 {#format}

```json
{
  "organization": "Example Corp",
  "support_url": "https://it.example.com/ghost",
  "settings": {
    "telemetry": { "value": 0, "locked": true },
    "check_updates": { "value": false, "locked": true },
    "branch_prefix": { "value": "corp/", "locked": false }
  },
  "limits": {
    "agents": ["claude-acp", "codex-acp"],
    "remote": false,
    "tunnel": false,
    "webhooks": false
  }
}
```

| 键 | 必填 | 含义 |
|---|---|---|
| `organization` | 是 | 由谁设置该策略。显示在它锁定的每项设置旁边（“由 … 管理”）。 |
| `support_url` | 否 | 可以去哪里咨询。 |
| `settings` | 否 | 设置，以它们在 [settings.json](settings.md) 中的键表示。每项为 `{"value": …, "locked": bool}`。 |
| `limits` | 否 | 任何设置都无法撤销的限制（见下文）。 |

### 设置 {#settings}

`settings` 的每个键都必须是 `settings.json` 的顶层键，且其 `value` 必须符合该设置的类型。

- **未锁定**（`"locked": false`，默认）：该值是一个默认值。它在用户没有做出任何选择的地方生效；用户自己的选择优先。
- **已锁定**（`"locked": true`）：无论 `settings.json` 怎么写，都使用该值。对应控件显示为禁用，并注明组织的名称。

### 限制 {#limits}

限制由 Ghost 的服务器在执行相应操作的地方强制执行。省略的限制不限制任何东西。

| 限制 | 效果 |
|---|---|
| `agents` | 只有列出的智能体（以其 ACP 注册表 ID 表示）可以启动。其他任何智能体都会拒绝启动，并说明策略不允许它。 |
| `remote: false` | 没有其他设备能连接到这台机器：Ghost 不监听。见[远程控制](../remote.md)。 |
| `tunnel: false` | 不运行 cloudflared 隧道。 |
| `webhooks: false` | 不运行任何 Webhook 任务。见[自动化](../automation.md#webhooks)。 |

## 策略无法读取时 {#when-the-policy-does-not-read}

如果策略不是有效的 JSON、含有 Ghost 不认识的键、指定了不存在的设置，或给出了类型错误的值，设置将完全无法加载：Ghost 以默认值启动，说明哪里出错，并在策略修复之前不保存任何内容。不会有任何东西在只受一半管控的状态下运行。

:::note 即将推出

在 macOS 上，策略正在迁移到配置描述文件（MDM）。该功能落地后，本页将介绍它们；Linux 上的文件保持不变。

:::
