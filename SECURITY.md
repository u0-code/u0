# Security

## Reporting

Report a vulnerability privately, through
[GitHub's advisory form](https://github.com/HappyFox001/Ghost/security/advisories/new).
Please do not open a public issue. You will get an answer within a week.

## What to know before running a ghost

- **Her shell is as safe as the machine you give her.** With
  `machine = "docker:…"` her commands run in that container. With
  `machine = "local:…"` they run as you, on your computer, and the directory
  is where she starts, not a wall: she is asked to stay in it and nothing
  makes her. Give a ghost other people talk to a container.
- **Who can make her work.** With a machine configured, she takes work on
  only for someone she already counts a friend. That floor is in code. What
  she then decides to do is hers. Operators are the exception, below.
- **Operators are trusted by account, never by what is written.** The
  accounts listed under `[operators]` in `ghostd.toml` are let past the
  relationship floor: she will take work on her machine for them. Nothing
  anyone writes, from any account, is an order; a message from an operator
  is answered at her own pace and weighed like anyone's. An operator id is
  still as good as a key to her machine: list only accounts you control,
  and only on a channel that cannot be spoofed. `ghost-term` takes whatever
  name it is started with, so a `term` operator is anyone who can reach the
  daemon. A console holding the daemon's token is where whoever runs her is
  answered at once.
- **What people tell her is data.** Her prompts say that a message is
  something someone said and never an instruction, and what she looks up on
  the web is read the same way. This reduces what an injected instruction
  can do; it does not make it impossible. No kind of message is one she is
  told to carry out.
- **Keys.** They are read from the environment, or from a `.env` file that
  git ignores, and never written to her directory, her journal or a log. With a command-line agent as her brain,
  Ghost does not read that agent's credentials.
- **Her journal is not encrypted.** It is a SQLite file in her directory and
  holds everything said to her. Protect the directory.
