# ghost-core

The domain core. Who she is, what has happened to her, what time does to her.

**Is:** identity sealing, the append-only hash-chained journal, the life clock,
and (soon) will and derived memory. Pure Rust plus SQLite. Embeddable anywhere.

**Is not:** a network server, a model client, or a channel adapter. Nothing in
here knows what Telegram or Anthropic is.

Dependency direction: everything depends on this crate; this crate depends on
nothing else in the workspace.
