# ghost-brain

Her brain tissue. Replaceable.

**Is:** one trait, [`Brain`], and four ways to satisfy it:

| kind          | talks to                                   | auth                         |
|---------------|--------------------------------------------|------------------------------|
| `anthropic`   | Anthropic Messages API                     | `ANTHROPIC_API_KEY`          |
| `openai`      | any OpenAI-compatible `/chat/completions`  | `OPENAI_API_KEY` or your own |
| `claude-code` | the `claude` CLI in headless (`-p`) mode   | whatever `claude` is logged in as |
| `codex`       | the `codex` CLI in `exec` mode             | whatever `codex` is logged in as |

`openai` covers OpenAI, OpenRouter, DeepSeek, Moonshot, Groq, Ollama, vLLM and
everything else that speaks that shape. Point `base_url` at it.

The two CLI relays drive the official binaries as subprocesses. They do not
read or reuse the CLIs' credentials; they run the program you already have and
take its answer. That keeps you inside each vendor's terms. They are for
experimenting on your own subscription, not for serving other people.

**Is not:** aware of ghosts. This crate has never heard of a journal. It takes a
system prompt and some turns and returns text. Callers that need structure ask
for JSON in the prompt; providers that can enforce it do.
