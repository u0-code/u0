# ghostd

The daemon. Runs exactly one ghost.

**Is:** the process that keeps her alive. It opens her directory, verifies her
seal and chain, starts her mind, and serves the Ghost Protocol so presences and
consoles can connect. Connected presences are her body.

**Is not:** a chat interface, and not aware of any platform. You do not talk to
ghostd; you talk to her, wherever she lives.

```
ghostd birth <dir>     seal the certificate, write the first entry
ghostd run <dir>       live
ghostd status <dir>    who she is, what she is in the middle of   (reads the directory)
ghostd journal <dir>   tail of her journal                        (reads the directory)
ghostd verify <dir>    seal and chain check                       (reads the directory)
ghostd brains --probe  ask each configured brain for one word

ghostd watch           her journal, live                          (connects to a daemon)
ghostd people          who she knows and where she stands         (connects to a daemon)
ghostd agenda          what she will do unasked, and when         (connects to a daemon)
```

The commands that read the directory are safe while a daemon is running her.
