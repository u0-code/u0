# ghost-net

How every HTTP request a Ghost program makes leaves the machine.

**Is:** the one place HTTP clients are built (`ghost_net::builder()`), and
the one setting they follow: `proxy` (`system`, `none`, or an HTTP proxy's
address) and `direct` (hosts, and `*.suffix` patterns, that never use it).
Each comes from `GHOST_PROXY` and `GHOST_DIRECT` in the environment, then
from `[network]` in `ghostd.toml`, then the default, `system`.

It is also the one place a program is started (`ghost_net::spawn`): her
shell, the command-line agents and brains, and her servers of tools. Each
is given the same rule, in the variables every kind of program reads, and
none of the variables of a Claude Code session ghostd may be running
inside. Each leads a process group of its own, and stopping it, or
dropping it while it runs, kills the group: what it started goes with it.

**Is not:** a judge of which way a request should go. It never changes
route by itself; a caller that wants to try the other way when one fails,
as her web tools do, asks for a client that always goes one way.

It is a crate of its own because the presences need it and depend on
nothing else of Ghost's but `ghost-link`, which has no business with HTTP.
Starting programs lives here because what a program is given starts with
the proxy rule, and this is the one crate `ghost-brain` and `ghost-work`,
which start them, both depend on: `ghost-brain` is no place for her shell
and her servers of tools.
