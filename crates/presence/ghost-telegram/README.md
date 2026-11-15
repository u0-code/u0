# ghost-telegram

Carries a ghost on Telegram through the Bot API.

## Setup

1. Talk to [@BotFather](https://t.me/BotFather): `/newbot`, pick a name and a
   username. Her photo is set there (`/setuserpic`). Put what she is in the
   bot's description, so people know before they write.
2. For group chats, send BotFather `/setprivacy` and choose **Disable**.
   Otherwise Telegram only shows her messages that mention her, and she cannot
   follow a conversation.
3. Run it next to a running daemon:

```bash
export TELEGRAM_BOT_TOKEN=123456:ABC...
ghost-telegram
```

It uses long polling, so it needs no public address. One process per bot
token: Telegram refuses a second poller.

A bot is one character's. When the daemon runs several, say whose with
`--ghost <her directory's name>` or `GHOST_NAME`.

At start it makes the bot's name her name, and its short description (what
its profile shows) her `bio`, from her `ghost.toml`, each only if it differs:
Telegram limits how often they change. A bio over Telegram's 120 characters
is not set, and it says so. Her photo it cannot set, and says so once at
start. This part has not yet been run against Telegram.

## What she can do here

| | |
|---|---|
| private chats | yes |
| groups and supergroups | yes; she notices mentions and replies to her, and skims the rest |
| typing indicator | yes |
| reactions | yes, limited to the emoji Telegram allows; others are refused |
| read receipts | no; bots have none |
| writing to someone first | only people who have already opened a chat with her |
| photos, voice, stickers | she sees that one was sent (`[photo]`, `[voice message 0:12]`), not its content |
| edits and deletions | not seen |

## Why a bot account

A bot is the interface Telegram provides for automated accounts, and Telegram
marks it as one in every chat. That label is the public disclosure the
[charter](../../../CHARTER.md) asks for. A bot also cannot message strangers,
which fits: she does not cold-call.
