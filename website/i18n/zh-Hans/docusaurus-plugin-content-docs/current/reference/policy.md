---
title: 托管策略
description: 组织如何在一台机器上设置、锁定和限制 Ghost。
---

# 托管策略

管理员可以给一台机器下发一份策略，用来设置 Ghost 的设置、锁定其中一些，并限制 Ghost 可以做什么。Ghost 在启动时读取它一次。

| 系统 | 策略来自哪里 |
|---|---|
| macOS | 针对偏好设置域 `local.ghost.desk`（Ghost.app 的 bundle identifier）的配置描述文件。 |
| Linux | 文件 `/etc/ghost/policy.json`。 |

两者都只能由管理员设置，键也完全相同。

## 在 Mac 上：配置描述文件 {#on-a-mac-a-configuration-profile}

Ghost 仓库中的 [`policy/ghost.mobileconfig`](https://github.com/u0-code/u0/blob/main/policy/ghost.mobileconfig) 是一个示例描述文件，列出了每个键，并为每个键附有注释。使用方法：

1. 修改其中的值，并为描述文件及其负载换上你自己的标识符和 UUID。
2. 通过你的 MDM 下发：作为自定义描述文件，或作为域 `local.ghost.desk` 的“自定设置”（Custom Settings）负载；也可以在 **系统设置 › 通用 › 设备管理** 中手动安装。

描述文件的负载以属性列表的值保存下文的各个键（`organization`、`support_url`、`settings`、`limits`）：字符串、数字、布尔值、数组或字典，与下面的 JSON 一一对应。日期或数据（data）类型的值会被视为错误。

Ghost 通过 CFPreferences 读取这些值，并且只认描述文件**强制**设定的值：用 `defaults write` 写入的值不算策略。

## 在 Linux 上：policy.json {#on-linux-policyjson}

`/etc/ghost/policy.json` 以 JSON 保存同样的键（见下文）。

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

如果策略无法读取（`policy.json` 不是有效的 JSON，或描述文件含有日期或数据类型的值）、含有 Ghost 不认识的键、指定了不存在的设置，或给出了类型错误的值，设置将完全无法加载：Ghost 以默认值启动，说明哪里出错，并在策略修复之前不保存任何内容。不会有任何东西在只受一半管控的状态下运行。
