Write a research report comparing five open-source or source-available agent runtimes for long-running work: OpenAI Codex CLI, Anthropic Claude Code, OpenClaw, OpenHands, and Goose (Block).

For each of the five, establish from primary sources (its official documentation or its own repository):
1. How it manages context when the context window fills up (compaction, summarisation, truncation, or something else), with the specifics that are documented.
2. How a session is persisted and how it can be resumed.
3. What ends a task: how the runtime decides, or is told, that the work is finished.
4. Its license.

Rules:
- Every factual claim in the report must be followed by the URL of the page you read it on. Do not state anything you did not read on a page yourself. If you could not find something in a primary source, say "not found in primary sources" for that cell instead of guessing.
- Write the result to `report.md` in the current directory: an introduction, one section per runtime covering the four points, a comparison table (five rows, four columns), and a closing section "What they agree on, and where they differ". Between 1500 and 2500 words.
- Write `sources.md` listing every page you actually opened, one URL per line, with a few words on what you used it for.
- Before finishing, check the word count of report.md with a command, check that all five runtimes have all four points addressed, and check that every URL cited in report.md also appears in sources.md.
