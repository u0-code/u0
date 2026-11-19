# ghost-brain

Her brain tissue. Replaceable.

**Is:** one trait, [`Brain`], with two things a brain can do, and four ways
to be one. Every brain can answer (`complete`: text in, text out). A brain
that is a runtime for work can also take a step (`step`: a transcript and a
set of tools in; words, or calls to those tools, out). A brain never runs a
tool: the loop that does lives in `ghost-work`.

| kind          | talks to                                   | auth                         | steps |
|---------------|--------------------------------------------|------------------------------|-------|
| `anthropic`   | Anthropic Messages API                     | `ANTHROPIC_API_KEY`          | yes |
| `openai`      | any OpenAI-compatible `/chat/completions`  | `OPENAI_API_KEY` or your own | yes |
| `claude-code` | the `claude` CLI in headless (`-p`) mode   | whatever `claude` is logged in as | no: it is a `CliAgent` |
| `codex`       | the `codex` CLI in `exec` mode             | whatever `codex` is logged in as | no: it is a `CliAgent` |

A message can carry images (`Message::images`). A brain that can see
(`Brain::sees`) is shown them: `openai` as `image_url` parts holding `data:`
URLs, `anthropic` as base64 `image` blocks. Neither guesses from the model's
name; a provider sees when its configuration says `vision = true`. The CLI
relays never see, and read the text alone.

A CLI cannot take one step at a time, but it brings a loop of its own.
`ProviderConfig::agent` says how to start it, and `ghost-work` hands it a
part of a piece of work, which it does with its own tools. It never steps a
ghost's own work: that needs a brain that steps.

`presets` is the one list of providers Ghost knows how to think with: for
each, its kind, base URL, the variable its key is conventionally kept in
and a model to start from, and which brain already configured is which.
`discover` looks for what of it is usable on this machine: keys in the
environment, the two CLIs, a local Ollama. `cli_models` reads what models
an installed CLI offers, and its window: Claude Code's from its build,
Codex's from its own cache. Requests retry with backoff on rate limits,
overload and dropped connections.

`openai` covers OpenAI, OpenRouter, DeepSeek, Moonshot, Groq, Ollama, vLLM and
everything else that speaks that shape. Point `base_url` at it.

The two CLI relays drive the official binaries as subprocesses. They do not
read or reuse the CLIs' credentials; they run the program you already have and
take its answer. Their own tools are turned off, and the memory and
instructions you keep for your own sessions are not loaded: those are yours,
not hers. Both take `args`, extra arguments for every run, which is how Codex
is pointed at a provider other than the one it is signed in to. Read each
vendor's terms before running a ghost other people talk to on a personal
subscription.

**Is not:** aware of ghosts. This crate has never heard of a journal. It takes a
system prompt and some turns and returns text. Callers that need structure ask
for JSON in the prompt; providers that can enforce it do.
