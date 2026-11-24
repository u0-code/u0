# Security

## Reporting

Report a vulnerability privately, through
[GitHub's advisory form](https://github.com/HappyFox001/Ghost/security/advisories/new).
Please do not open a public issue. You will get an answer within a week.

## What to know before running an agent through Ghost

- **An agent runs as you.** Each agent is its own program, started by Ghost
  in the folder you choose, with your permissions and its own settings. What
  it may do without asking is the agent's mode, which you choose in the
  composer; Ghost does not sandbox it.
- **Questions are yours to answer.** When an agent asks before it acts
  (`session/request_permission`), Ghost shows the question and answers only
  with the option you click. Stopping a turn answers every open question as
  cancelled.
- **Ghost offers agents nothing of its own.** It declares no file system,
  terminal or other client capability, so an agent cannot read or write
  through Ghost.
- **Installs are checked.** An agent's archive is downloaded from the URL the
  ACP registry gives and compared with the registry's SHA-256 before it is
  unpacked; a mismatch installs nothing. Packages run through `npx` or `uvx`
  are fetched by those tools from their registries.
- **Credentials stay with the agent.** Sign-in is each agent's own, in its CLI
  or through its ACP `authenticate`. Ghost stores no key, token or account.
