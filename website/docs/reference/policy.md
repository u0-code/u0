---
title: Managed policy
description: How an organisation sets, locks and limits Ghost on a machine.
---

# Managed policy

An administrator can give a machine a policy that sets Ghost's
settings, locks some of them, and limits what Ghost may do. Ghost reads it
once, as it starts.

| System | Where the policy comes from |
|---|---|
| macOS | A configuration profile for the preference domain `local.ghost.desk` (Ghost.app's bundle identifier). |
| Linux | The file `/etc/ghost/policy.json`. |

Both are set only by an administrator, and both have the same keys.

## On a Mac: a configuration profile

[`policy/ghost.mobileconfig`](https://github.com/u0-code/u0/blob/main/policy/ghost.mobileconfig)
in Ghost's repository is an example profile that names every key, with a
comment on each. To use it:

1. Change its values, and give the profile and its payload identifiers
   and UUIDs of your own.
2. Deploy it from your MDM, as a custom profile or as a Custom Settings
   payload for the domain `local.ghost.desk`; or install it by hand in
   **System Settings › General › Device Management**.

The profile's payload holds the keys below (`organization`,
`support_url`, `settings`, `limits`) as property-list values: a string,
a number, true or false, an array or a dict, as the JSON below has
them. A date or data value is an error.

Ghost reads them with CFPreferences, and counts only values a profile
**forces**: a value written with `defaults write` is not a policy.

## On Linux: policy.json

`/etc/ghost/policy.json` holds the same keys as JSON (below).

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

A policy that does not read (a `policy.json` that is not valid JSON, a
profile with a date or data value), has a key Ghost does not know, names
a setting that does not exist, or gives a value of the wrong type keeps
the settings from loading at all: Ghost starts on its defaults, says what is
wrong, and saves nothing until the policy is fixed. Nothing runs half
governed.
