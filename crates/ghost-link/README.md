# ghost-link

The client side of the Ghost Protocol.

**Is:** `presence`, behind the feature of that name, which is everything a
channel adapter needs besides the channel: `presence::run` (implement
`Platform`, describe yourself in a `Link`, and it keeps the connection alive,
holds messages while the daemon is away, and routes her actions to you), the
daemon arguments every adapter flattens into its own (`Daemon`), the start
every adapter makes (`init`: the `.env` file, then logging, then the
arguments), and the one wait between retries (`Backoff`). And `Console`, a
small client for tools that only watch.

**Is not:** dependent on anything else in Ghost. An adapter built on this never
sees her journal, her memory, or a brain.

Writing an adapter in another language is equally fine: speak the protocol in
[`ghost-protocol`](../ghost-protocol).
