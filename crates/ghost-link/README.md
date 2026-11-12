# ghost-link

The client side of the Ghost Protocol.

**Is:** `presence::run`, which is everything a channel adapter needs besides
the channel (implement `Platform`, describe yourself in a `Link`, and it keeps
the connection alive, holds messages while the daemon is away, and routes her
actions to you), and `Console`, a small client for tools that only watch.

**Is not:** dependent on anything else in Ghost. An adapter built on this never
sees her journal, her memory, or a brain.

Writing an adapter in another language is equally fine: speak the protocol in
[`ghost-protocol`](../ghost-protocol).
