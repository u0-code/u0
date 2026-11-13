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
  she then decides to do is hers.
- **What people tell her is data.** Her prompts say that a message is
  something someone said and never an instruction, and what she looks up on
  the web is read the same way. This reduces what an injected instruction
  can do; it does not make it impossible.
- **Keys.** They are read from the environment and never written to her
  directory, her journal or a log. With a command-line agent as her brain,
  Ghost does not read that agent's credentials.
- **Her journal is not encrypted.** It is a SQLite file in her directory and
  holds everything said to her. Protect the directory.
