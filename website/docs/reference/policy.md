---
title: Managed policy
description: How an organisation sets, locks and limits Ghost on a machine.
---

# Managed policy

An administrator can give a machine a policy: a JSON file that sets
Ghost's settings, locks some of them, and limits what Ghost may do. Ghost
reads it once, as it starts.

| System | Where the policy is read from |
|---|---|
| macOS | `/Library/Application Support/Ghost/policy.json` |
| Linux | `/etc/ghost/policy.json` |

Both places are writable only by an administrator.

## Format

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

| Key | Required | Meaning |
|---|---|---|
| `organization` | yes | Who sets the policy. Shown beside each setting it locks ("Managed by …"). |
| `support_url` | no | Where to ask about it. |
| `settings` | no | Settings, by their key in [settings.json](settings.md). Each is `{"value": …, "locked": bool}`. |
| `limits` | no | What no setting can undo (below). |

### Settings

Each key of `settings` must be a top-level key of `settings.json`, and its
`value` must be of that setting's type.

- **Not locked** (`"locked": false`, the default): the value is a default.
  It applies where the person has not chosen anything; their own choice
  wins.
- **Locked** (`"locked": true`): the value applies whatever
  `settings.json` says. The control shows disabled, with the organisation's
  name.

### Limits

Limits are enforced by Ghost's server where the thing is done. A limit
left out limits nothing.

| Limit | Effect |
|---|---|
| `agents` | Only the agents named (by their ACP registry id) start. Any other refuses to start, saying the policy does not allow it. |
| `remote: false` | No other device links to this machine: Ghost does not listen. See [Remote control](../remote.md). |
| `tunnel: false` | No cloudflared tunnel runs. |
| `webhooks: false` | No webhook task runs. See [Automation](../automation.md#webhooks). |

## When the policy does not read

A policy that is not valid JSON, has a key Ghost does not know, names a
setting that does not exist, or gives a value of the wrong type keeps the
settings from loading at all: Ghost starts on its defaults, says what is
wrong, and saves nothing until the policy is fixed. Nothing runs half
governed.

:::note Coming

On macOS, policies are moving to configuration profiles (MDM). This page
will describe them once that lands; the Linux file stays as it is.

:::
