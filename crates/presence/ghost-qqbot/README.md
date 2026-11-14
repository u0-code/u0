# ghost-qqbot

Carries a ghost on QQ through Tencent's official
[QQ Bot API](https://bot.q.qq.com/wiki/): a bot account on the QQ open
platform, events over its WebSocket gateway, and messages sent over HTTPS.

## Setup

1. Sign in to the [QQ open platform](https://q.qq.com/) with QQ on your
   phone, and create a bot. Her name, avatar and description are set there.
   Say what she is in the description, so people know before they write.
2. On the bot's settings page, copy its **AppID** and **AppSecret**. The
   secret is shown once; if you leave the page without it, generate a new
   one.
3. Until the bot is reviewed and published, only the people and groups on
   its sandbox list can reach it. Add yourself there to try it.
4. Run it next to a running daemon:

```bash
export QQBOT_APPID=...
export QQBOT_SECRET=...
ghost-qqbot
```

It connects out to QQ's gateway, so it needs no public address. Run one
process per bot.

It registers as the `qq` channel, as `ghost-onebot` does. Run one or the
other: people have different ids in each, so they are not the same
conversations anyway.

## What she can do here

| | |
|---|---|
| private chats | yes |
| groups | only messages that @ her; QQ shows a bot nothing else in a group unless it has been granted more |
| answering | each message she receives can be answered a few times within minutes (5 in a group, 30 in a private chat); her reply is sent as an answer to the most recent one |
| writing to someone first | rarely; QQ limits what a bot sends unprompted, and refuses the rest |
| typing indicator | not used |
| reactions | no; the API has none |
| quoting a message | not carried; QQ quotes by an index of its own, not by message id |
| images, video, files | she sees that one was sent (`[image]`), not its content |
| voice messages | she sees QQ's transcript, `[voice message: …]`, when it gives one |
| names | QQ gives no names in private chats; the sender is `qq-` and the start of their id |

Nobody is known by their QQ number. QQ gives each bot its own id for each
person (an OpenID) and for each group, and those are what places and people
are keyed by here.

## Why the official API

The bot account is the interface Tencent provides for programs, and QQ marks
it as a bot wherever it appears. That label is the public disclosure the
[charter](../../../CHARTER.md) asks for. Running an ordinary QQ account from
a program, as OneBot implementations do, is against Tencent's terms and can
cost the account; see [`ghost-onebot`](../ghost-onebot).

## Status

Written against the QQ Bot API as Tencent's own SDK and plugins use it. The
gateway (identify, heartbeat, resume after a dropped socket) was exercised
against a local stand-in for it, and the message conversion and reply
allowance by tests. **Not yet run against the real QQ open platform with a
real bot.**
