# The desk's core

A deployment of `ghostd` for Ghost's own little ghost, 小幽灵 (Little
Ghost): it lives on this machine and helps whoever sits at the desk with
whatever they bring. `ghost-desk` opens on it, and it belongs to the desk,
so it is kept here rather than beside the characters Ghost runs. In the
code and on disk it is still called the solver.

```
desk/core/
  README.md               this page
  ghostd.example.toml     the core's configuration, as published
  ghostd.toml             yours, copied from the example (not published)
  ghosts/_solver/         the template the solver is born from
  ghosts/solver/          the solver, once born (not published)
```

The runtime is the same one every character lives on. What makes this one
Ghost's own little ghost rather than a character is what every deployment can change and a
character cannot: the prompts. The desk's own, in
[`prompts/system/desk/`](../../prompts/system/desk/), replace her persona
(`character/persona.md`) and her being (`system/being.md`) with a hand's.
They are part of the system, kept with every other prompt in
[`prompts/`](../../prompts/), and laid out the way any deployment's
directory of prompts is. `ghostd.example.toml` points `[mind] prompts` at
them, turns off her day, her feeds and the weather, gives her this machine
to work on, and listens on its own port, 7438, so that it never gets in the
way of a daemon running real characters on 7437. Its paths are relative to
the checkout, which is where the daemon runs.

`ghost-desk` looks after all of it: the first time it runs it copies
`ghostd.example.toml` to `ghostd.toml` (not published: it is yours to
change), gives birth to the solver from `ghosts/_solver` if it has not
been born, and starts its daemon. You are its operator, and so it may use
the machine while you talk, because the desk speaks to it through a
console that holds its daemon's token, or from this machine when it has
none: nobody is listed in `[operators]` for it. The brain
it thinks with is picked, and new ones added, from the line you type into;
a key is never written, only the name of the variable it is read from.

By hand, from the checkout, the same is:

```bash
cp desk/core/ghostd.example.toml desk/core/ghostd.toml
cp -r desk/core/ghosts/_solver desk/core/ghosts/solver
./target/release/ghostd birth desk/core/ghosts/solver
./target/release/ghostd run desk/core/ghosts/solver --config desk/core/ghostd.toml
```

`machine = "local:"` is not a sandbox: it runs as you, under the solver's
`home`.

## From before the core moved here

The core used to be kept at the top of the checkout: its configuration in
`solver/ghostd.toml`, the solver born in `ghosts/solver/`. The desk carries
both over without asking:

- A `solver/ghostd.toml` with no `desk/core/ghostd.toml` beside it is moved
  here the first time the desk wants it: copied, and removed only once the
  copy is in place, so the brains you added and the one you chose are kept.
- A solver born in `ghosts/solver/`, with none born here, goes on being used
  where it is. The desk never moves her: her journal is hers for good, and
  the files her past work reported are named by where she was. To have her
  here all the same, quit the desk and
  `mv ghosts/solver desk/core/ghosts/solver` yourself.
