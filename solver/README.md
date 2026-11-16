# The solver

A deployment of `ghostd` for one ghost who is nobody in particular: a hand
on this machine, for whoever sits at the desk. `ghost-desk` opens on it.

The runtime is the same one every character lives on. What makes this one
a solver rather than a person is what every deployment can change and a
character cannot: the prompts. The desk's own, in
[`prompts/system/desk/`](../prompts/system/desk/), replace her persona
(`character/persona.md`) and her being (`system/being.md`) with a hand's.
They are part of the system, kept with every other prompt in
[`prompts/`](../prompts/), and laid out the way any deployment's directory
of prompts is. `ghostd.example.toml` points `[mind] prompts` at them, turns
off her day, her feeds and the weather, gives her this machine to work on,
and listens on its own port, 7438, so that it never gets in the way of a
daemon running real characters on 7437.

`ghost-desk` looks after all of it: the first time it runs it copies
`ghostd.example.toml` to `ghostd.toml` (not published, since it names you),
makes you the solver's operator on the `desk` channel so that it may use
the machine while you talk, gives birth to the solver from
`ghosts/_solver` if it has not been born, and starts its daemon. The brain
it thinks with is picked, and new ones added, from the line you type into;
a key is never written, only the name of the variable it is read from.

By hand, the same is:

```bash
cp solver/ghostd.example.toml solver/ghostd.toml   # add [operators] desk = ["<you>"]
cp -r ghosts/_solver ghosts/solver
./target/release/ghostd birth ghosts/solver
./target/release/ghostd run ghosts/solver --config solver/ghostd.toml
```

`machine = "local:"` is not a sandbox: it runs as you, under
`ghosts/solver/home`.
