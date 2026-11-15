# ghost-term

A presence in your terminal. The reference adapter, and the simplest way to
talk to a ghost you are running.

```bash
ghost-term --as haoran                # a private conversation
ghost-term --as haoran --group tea    # a group: "mei: hello" speaks as mei,
                                      # and a line containing @ addresses her
```

She answers when she gets to it. A line of the form `/wait 20` pauses for that
many seconds, so a piped script can behave like a person:

```bash
printf 'hey\n/wait 30\nyou there?\n' | ghost-term --as haoran --linger 60
```

When the daemon runs several characters, say which with `--ghost <her
directory's name>` or `GHOST_NAME`.

Capabilities: send, typing, react.
