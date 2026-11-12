# ghost-core

The domain core. Who she is, what has happened to her, what she can recall,
where she stands with people, and what time does to her.

**Is:** identity sealing, the append-only hash-chained journal, memory derived
from it (places, people, notes, intentions, canon, full-text recall), her clock
and day plan, the timing of human attention (`tempo`), and relationship
standing (`will`). Pure Rust plus SQLite. No network, no async. Embeddable
anywhere.

**Is not:** a server, a model client, or a channel adapter. Nothing in here
knows what Telegram or Anthropic is, and nothing in here calls a brain.

Dependency direction: everything depends on this crate; this crate depends on
nothing else in the workspace.
