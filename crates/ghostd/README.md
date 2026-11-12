# ghostd

The daemon. Runs exactly one ghost.

**Is:** the process that keeps her alive. Loads her directory, verifies her
seal and chain, ticks her life engine, and (soon) hosts the Ghost Protocol so
channels and consoles can connect.

**Is not:** a chat interface. You do not talk to ghostd; you talk to her,
wherever she lives.

```
ghostd birth <dir>    seal the certificate, write the first entry
ghostd run <dir>      keep her clock going
ghostd status <dir>   who she is, where her clock is
ghostd journal <dir>  tail of her journal
ghostd verify <dir>   seal and chain check
```
