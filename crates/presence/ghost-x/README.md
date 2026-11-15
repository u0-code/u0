# ghost-x

Carries a ghost on X.

X is where she has a public life. Mentions reach her like any message. Direct
messages are private conversations. Her home timeline is something she scrolls
when she picks up her phone: most of the time nothing comes of it, sometimes
she likes something, rarely she replies, and now and then something from her
own day is worth posting.

Everything goes through the official API v2, signed with OAuth 1.0a as her
account.

## Setup

1. Create her account. In its settings, mark it as **automated** and name the
   account that runs it; X then shows that label on her profile. Say what she
   is in her bio. The [charter](../../../CHARTER.md) requires both.
2. In the [developer portal](https://developer.x.com), create an app with
   **Read and write and Direct message** permissions, then generate an access
   token and secret for her account.
3. Run it next to a running daemon:

```bash
export X_API_KEY=...  X_API_SECRET=...
export X_ACCESS_TOKEN=...  X_ACCESS_SECRET=...
ghost-x --state ~/.local/state/ghost-x.json
```

An account is one character's. When the daemon runs several, say whose with
`--ghost <her directory's name>` or `GHOST_NAME`. Her name, bio and picture
are set by hand in the account's settings: API v2 has no call that changes
a profile, and it says so once at start.

Without `--state` each run starts from now. With it, a restart picks up what
arrived while the adapter was not running. On its very first look she does not
answer old mentions; she starts from that moment.

## What it costs

X meters its API. Posting is available on the free tier. **Reading** mentions,
direct messages and the timeline is not; those need a paid tier or
pay-per-use credits, and each poll counts. The adapter reports a `403` once
and keeps going, so on a tier that can only post she will still post and
simply never hear anyone.

Tune the polling to your tier:

```bash
ghost-x --poll-mentions 300 --poll-dms 0
```

How often she scrolls and how much she posts are the daemon's settings
(`browse_minutes` and `posts_per_day` under `[mind]`), not this adapter's.

## What she can do here

| | |
|---|---|
| mentions | yes, polled; she replies in the thread, and several messages become a chain |
| direct messages | yes, polled |
| likes | yes; any reaction she makes becomes a like |
| posting | yes; text too long for one post becomes a thread |
| scrolling her timeline | yes; she remembers what she has already scrolled past |
| typing indicator, read receipts | no; X has none through the API |
| images and video | not seen |
| reposts, quotes, follows | not yet |

## Status

The request signing is tested against the worked example in X's own
documentation. The adapter was run end to end against a local stand-in for the
API with a live brain: a mention answered, a timeline scrolled, a post liked,
a post replied to, clickbait ignored. It has not yet been run against X.
