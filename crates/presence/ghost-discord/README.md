# ghost-discord

Carries a ghost on Discord as a bot, through the gateway.

## Setup

1. In the [developer portal](https://discord.com/developers/applications),
   create an application and add a bot to it. Her name and avatar are set
   there. Say what she is in the bot's "About me".
2. On the bot page, enable the **Message Content Intent**. Without it Discord
   sends her empty messages and the gateway refuses to start.
3. Invite her to a server with the `bot` scope and these permissions: View
   Channels, Send Messages, Read Message History, Add Reactions.
4. Run it next to a running daemon:

```bash
export DISCORD_BOT_TOKEN=...
ghost-discord
```

## What she can do here

| | |
|---|---|
| direct messages | yes, with people who share a server with her or have written first |
| server channels | yes; she notices mentions and replies to her, and skims the rest |
| typing indicator | yes, both ways: she waits while someone is still typing |
| reactions | yes, any unicode emoji |
| read receipts | no; Discord has none |
| attachments and stickers | she sees that one was sent (`[image name.jpg]`), not its content |
| edits and deletions | not seen |
| voice channels | no |

## Why a bot account

Discord marks bot accounts on every message, which is the public disclosure
the [charter](../../../CHARTER.md) asks for. Driving a normal user account
with a program is against Discord's terms of service and gets accounts
banned; this adapter does not do it.

## Status

The message conversion is tested against payloads in Discord's wire format.
The gateway connection itself is handled by
[serenity](https://github.com/serenity-rs/serenity). This adapter has not yet
been run against Discord with a real bot token; treat the first run as a test.
