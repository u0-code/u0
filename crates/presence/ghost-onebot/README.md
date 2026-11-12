# ghost-onebot

Carries a ghost on QQ through [OneBot v11](https://github.com/botuniverse/onebot-11).

This adapter speaks the OneBot protocol over a forward WebSocket. It does not
log in to QQ itself; a OneBot implementation does that, and this connects to
it.

## Read this first

QQ has no official interface for running a personal account from a program.
OneBot implementations such as NapCat and Lagrange work by logging in as an
ordinary QQ account. Tencent's terms do not permit that, and accounts used
this way can be restricted or banned without notice.

So, before you use this:

- Use an account made for her, never your own.
- Put what she is in her nickname or signature. The
  [charter](../../../CHARTER.md) requires that she never passes as human, and
  on QQ nothing labels her for you.
- Expect that the account may be lost.

Which implementation you run, and with which account, is your decision and
your risk. This adapter only speaks the protocol. If Tencent's official bot
platform fits your case, an adapter for it would be a welcome addition.

## Setup

1. Run a OneBot v11 implementation and enable its **forward WebSocket**
   server (the implementation listens; this adapter connects). Use the array
   message format if it offers a choice.
2. Run this next to a running daemon:

```bash
export ONEBOT_ENDPOINT=ws://127.0.0.1:3001
export ONEBOT_ACCESS_TOKEN=...        # if the implementation requires one
ghost-onebot
```

## What she can do here

| | |
|---|---|
| private chats | yes |
| groups | yes; she notices @ and replies to her, and skims the rest |
| typing indicator | private chats only, on implementations with `set_input_status` |
| noticing that someone is typing | where the implementation sends `input_status` |
| reactions | no |
| images, voice, stickers | she sees that one was sent (`[image]`, `[voice message]`), not its content |
| recalls | not seen |

## Status

Tested end to end against a local stand-in for a OneBot server with a live
brain, in Chinese: private chat, group mention, access token, typing. Not yet
run against a real QQ account.
