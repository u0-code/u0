# Running a ghost on a Linux server

What is here: two systemd units and a backup script, and this page on how
they fit together. They are written for a Linux with systemd (Debian and
Ubuntu are what the package names below are for). They have not yet been run
on a server as part of this repository; the backup script has been run
against a ghost on macOS, and `ghostd` stopping in order on SIGTERM is
tested. Treat the rest as a starting point and read it before you use it.

## Her own account

She runs tools she installs herself, and there is no sandbox around them: the
shell she is given on a `local:` machine runs as whoever runs `ghostd`. So
give her an account of her own that owns her directory and nothing else.

```bash
sudo useradd --system --create-home --home-dir /var/lib/ghost \
  --shell /usr/sbin/nologin ghost
```

Never run her as root, and do not put this account in the `docker` group:
that group is root on the host. If her machine is a container, use rootless
Docker, or keep her on `local:` under this account.

## Layout

```
/opt/ghost/bin/          ghostd, ghost-qqbot, ...               root's; she cannot change them
/etc/ghost/ghostd.toml   which brains, where to listen, [network]   root:ghost, 640
/etc/ghost/ghost.env     keys and tokens (the .env)              root:root, 600
/etc/ghost/prompts/      prompts you changed, for every character   root:ghost, optional
/var/lib/ghost/          her account's home: caches of the tools she installs
/var/lib/ghost/core-id   which core this is, for consoles: made the first time
/var/lib/ghost/weiwei/   her directory: ghost.toml, assets/, journal.db, work.db,
                         media/, skills/, tools/, derived/
```

Her directory holds what is hers alone: her persona in `ghost.toml`, her
avatar in `assets/`, and what she has lived. How she works is the same for
every character and is not kept there. If you change any of the words her
brain is given, export them with `ghostd prompts export /etc/ghost/prompts
--only <file>`, where the file is its path in the tree, such as
`system/channels/chat.md`; edit them there, and set
`prompts = "/etc/ghost/prompts"` under `[mind]` in `ghostd.toml`. The
directory is laid out as [`prompts/`](../prompts/README.md) is.

```bash
cargo build --release
sudo install -d /opt/ghost/bin /etc/ghost
sudo install -m 755 target/release/ghostd target/release/ghost-qqbot \
  /opt/ghost/bin/
sudo install -m 640 -g ghost ghostd.toml /etc/ghost/ghostd.toml
sudo install -m 600 .env /etc/ghost/ghost.env
# Her directory: an existing one, moved with her journal...
sudo cp -r ghosts/weiwei /var/lib/ghost/weiwei
sudo chown -R ghost:ghost /var/lib/ghost/weiwei
# ...or a new one, born there from a ghost.toml you put in it first.
sudo -u ghost /opt/ghost/bin/ghostd birth /var/lib/ghost/weiwei
```

systemd reads `ghost.env` before it drops to her account, so the file can
stay root's alone. What `ghostd` is given, the programs her shell starts
inherit, keys included; a container for her machine is what keeps them
apart.

## systemd

```bash
sudo cp deploy/systemd/ghostd@.service deploy/systemd/ghost-presence@.service \
  /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now ghostd@weiwei
sudo systemctl enable --now ghost-presence@qqbot
journalctl -u ghostd@weiwei -f
```

`ghostd@<name>` runs `/var/lib/ghost/<name>`; `ghost-presence@<platform>` runs
`/opt/ghost/bin/ghost-<platform>`. Each line of hardening in the units says
why it is there and what was left off so as not to take away her abilities.
`ghostd` stops her in order on SIGTERM, which is what `systemctl stop` sends.

`ghost-x` remembers how far it has read only when given `--state`. Point it
into the directory systemd makes for it:

```bash
sudo systemctl edit ghost-presence@x
# [Service]
# ExecStart=
# ExecStart=/opt/ghost/bin/ghost-x --state /var/lib/ghost-presence/x/cursor.json
```

### More than one character

Both ways are supported:

- **One per process**, as above: `ghostd@lin`, `ghostd@mei`. A crash, a
  stuck brain or a full disk in one is that process alone, and each can be
  stopped and upgraded on its own. But each needs its own `ghostd.toml`
  with its own `[listen] addr`, since two daemons cannot share a port: edit
  the unit's `ExecStart` for each (`systemctl edit ghostd@mei`) to name
  `/etc/ghost/mei.toml`, and give each presence `--url` for its daemon.
- **Several in one**: `ghostd run /var/lib/ghost/lin /var/lib/ghost/mei`,
  or a directory holding them. One port, one `ghostd.toml` with
  `[ghosts.<id>]` for whatever differs, one unit to look after. A mind that
  fails is started again inside the process without touching the others,
  but they share a process: whatever kills it, or a restart of it, takes
  every character down together.

Either way each platform account is one character's: run one presence per
account, and tell it whose it is with `GHOST_NAME` in its environment file
(`/etc/ghost/presence-<platform>.env`), which a daemon running several
requires. To run two accounts on one platform, make a copy of
`ghost-presence@.service` per account, or start the second by hand with its
own credentials and `--ghost`. `GHOSTD_TOKEN` is one for the deployment;
tokens per character are not offered.

Her errors and warnings are in the journal of the unit, not in her
directory: `journalctl -u ghostd@weiwei -p warning -n 50`. With several
characters in one process, each line from one of them begins
`ghost{id=<her directory's name>}`. What she has on
hand, read without disturbing her: `sudo -u ghost /opt/ghost/bin/ghostd
status /var/lib/ghost/weiwei`.

## Who can reach the daemon

Keep `[listen] addr` on `127.0.0.1`. The presences on the same machine
connect there, and a console from your own machine goes through ssh:

```bash
ssh -N -L 7437:127.0.0.1:7437 you@server &
ghostd watch --last 20
ghost-term --as yourname
```

If it ever has to listen beyond this machine, `ghostd` refuses to start
without a token: set `GHOSTD_TOKEN` in `ghost.env`, where the presences read
it too.

## Backups

`journal.db` is her life, append-only and forever, and nothing can rebuild
it. Back it up, with `work.db`, `assets/`, `media/`, `skills/` and `tools/`,
often and off the machine. Keep `/etc/ghost`, with any prompts you changed,
with the rest of the machine's configuration.
`deploy/backup.sh` does it while she runs, with SQLite's online backup:

```bash
sudo install -m 755 deploy/backup.sh /opt/ghost/bin/ghost-backup
sudo crontab -u ghost -e
# 17 * * * * /opt/ghost/bin/ghost-backup /var/lib/ghost/weiwei /var/lib/ghost/backups 14
```

It copies `journal.db` first and `work.db` after, on purpose: a piece of
work's record is always written before her journal says what came of it,
so with the copies in that order the record is never behind the journal,
and a work that ended on its record but not in the copied journal is ended
there, as its record says, when she next looks at her work. Restore the two
from the same run; copy them by hand in the same order.

Then copy `/var/lib/ghost/backups` somewhere else. For a journal that is
never more than seconds behind, [Litestream](https://litestream.io) can
replicate `journal.db` to object storage continuously. `derived/` is rebuilt
from the journal when she starts, and is not backed up.

## What her abilities need on the host

Only what you mean her to use:

- **Tool servers she installs herself** are usually started with `npx` or
  `uvx`: install Node.js and [uv](https://docs.astral.sh/uv/).
- **A headless browser**: Chromium and the libraries it needs, for instance
  with `npx playwright install --with-deps chromium` run once as root.
- **Video**: `ffmpeg`.
- **A command-line agent** (`claude-code` or `codex` in `ghostd.toml`), as
  a brain for talking or as an agent her work hands parts to: the CLI
  installed, and logged in as her account, because it answers as whoever it
  is signed in as. With no browser on the server: `sudo -u ghost -H claude
  setup-token` and the token in `CLAUDE_CODE_OAUTH_TOKEN` in her `.env`;
  `sudo -u ghost -H codex login --device-auth`. `ghostd brains` then says
  whether each is ready, and what to run if not.
- **Backups**: `sqlite3`.

## Platforms

- **QQ.** The QQ open platform can refuse API calls from an address that is
  not on the bot's IP whitelist. Check the bot's settings on the platform and
  add the server's public address before you start the presence.
- **Where the server is** decides what it reaches directly. A server in
  mainland China may not reach Telegram, Discord, X, Anthropic or OpenAI
  without a proxy; one elsewhere may reach QQ or DeepSeek slowly.

## The network setting

Every HTTP request ghostd, the presences and the programs she starts make
follows one setting. In `ghostd.toml`:

```toml
[network]
proxy = "http://127.0.0.1:7890"   # or "system" (the default) or "none"
direct = ["*.deepseek.com"]       # hosts that never use the proxy
```

`GHOST_PROXY` and `GHOST_DIRECT` (comma-separated) in `ghost.env` override
those two, one at a time, and are the only way the presences, which have no
`ghostd.toml`, are told. This machine's own addresses never go through a
proxy. A proxy with a password belongs in `GHOST_PROXY`, not in
`ghostd.toml`. Only HTTP proxies are taken.

Model APIs and platforms go the way the setting says and no other. Her own
`search` and `fetch` try the other way once when a site cannot be reached or
turns them away, and remember which way worked. Websockets are not HTTP
requests and are made directly whatever the setting: the gateways of QQ
and Discord, OneBot, and each presence's own link to `ghostd`.
