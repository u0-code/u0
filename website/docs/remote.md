---
title: Remote control
description: Use u0 on one machine from a Mac elsewhere, pair devices by key, and hold several Macs in one window.
---

# Remote control

The u0 on one machine can be used from a Mac elsewhere: its agents,
threads, files, git and terminals, as if you sat at it. Every u0 can
do both sides:

- **be controlled**: let other devices control this Mac (or a Linux box
  running `u0-server`);
- **control**: hold other Macs in this window, beside this one.

Both are in **Settings › Devices**, and both are off at first.

## How a device comes in

A device is let in by a key, never by a password:

1. When you pair, the controlling Mac makes an Ed25519 key for that one
   machine. The private half stays in its Keychain; only the public half is
   sent. The machine lists it by its `SHA256:` fingerprint.
2. Pairing is allowed by a pairing code: 128 random bits, used once, valid
   for ten minutes, kept in memory only (a restart forgets it).
3. Each time the desk links afterwards, the machine sends a fresh random
   nonce, and the desk signs it with its key. No secret crosses the wire
   or sits on the machine.

Each call a device makes is checked here, on the machine controlled,
against what that device may do. A device that may only read watches
the threads, diffs and files of your projects and of the worktrees of
their threads (all of them: it is not limited to some), and changes
nothing; anything else it asks is refused, as are this Mac's processes,
log and storage. It is told nothing of a folder that is not one of
them, of a thread or a scheduled task elsewhere, of your terminals,
of notices, or of what another device asked for.

Turning listening off cuts every link at once and withdraws every unused
pairing code. **Revoke** on a device removes its key and closes its links,
as an access that ends does.

## Let other devices control this Mac

In **Settings › Devices**, turn on **Let other devices control this
Mac**. (The u0 menu's **Show Pairing Link…** does the same and opens
the pairing dialog directly.)

u0 then listens on `127.0.0.1` only, at port 4317 (the `port` in
`<home>/devices.json`), the same port each time it starts. **Port**,
under the switch, shows it: type another and press **Use** to listen
there from then on (from 1024 to 65535, and free). Where another program
holds the port as u0 starts, u0 listens at a free port the system gives
instead, keeps it as its `port`, and says so in a notice: devices paired
before reach it at the old port, so pair them again. Where another u0 on
the same machine holds it (one with a home of its own, or its service),
u0 leaves it to that one and says so under the switch: give this one
another port, or stop that one.
Nothing on the network reaches that port directly. A device reaches it
one of two ways:

- **A cloudflared tunnel.** Under **Reach › Tunnel (cloudflared)**,
  press **Start**. u0 runs `cloudflared` from your `PATH` (install it
  with `brew install cloudflared`). By default it is a quick tunnel: a new
  `trycloudflare.com` address each time it starts, so devices must pair
  again with a new link. To keep one address, use a named tunnel of yours:
  enter its host name and its tunnel token, then **Use**. **Use a quick
  tunnel** goes back.
- **SSH port forwarding**, from the controlling Mac:
  `ssh -L 4317:127.0.0.1:4317 host`, then pair with the `127.0.0.1` link.
- **A network.** Under **Reach › Listen on**, choose **Tailscale**, or
  **Every network** (Tailscale too), beside **This Mac**. u0 then also
  listens on that network's addresses, on the port after its own (4318
  by default), and speaks TLS there itself, with a certificate of its own
  kept in the Keychain. Reach lists each address it is reached at.

Until a device is let in, u0 holds at most 64 connections waiting at
once, each for a few seconds; a page in a browser is refused outright.
Anyone who reaches the tunnel can still keep that many busy and so keep
devices out for a while: the tunnel's address is not a secret.

**Keep the Mac awake while serving** keeps it reachable with no one at it
(u0 runs `caffeinate` while it listens).

### The pairing link

Under **Pair a device with this Mac**, u0 shows a pairing link and its
QR code:

```
u0://pair?host=<host>#<code>
```

The host is the way chosen under **Reached by**: **Best** (the tunnel
while it is up, then Tailscale, then the local network, then
`127.0.0.1:<port>`), or one of them. A link on a network also carries
`pin=`, the SHA-256 of this Mac's certificate, so the device trusts this
Mac alone there. The link works once, for ten minutes. The Welcome
page's **Your other devices** offers the same choice, each way shown with
the address it gives (`This Mac (127.0.0.1:4317)`).

Above it, choose what the next link lets in; each choice makes a new
link at once:

- **What it may do**: **Everything**, or **Read only**.
- **For how long**: **Until revoked**, **A day**, **A week** or
  **30 days**. After that the device is turned away, and pairs again with
  a new link.
- **Ask me before letting it in**: the device pairs, then waits under
  **Devices that control this Mac** until you let it in. For a link you
  send someone.
- **Pair both ways** (for **Everything** alone): if the Mac that pairs
  asks, this Mac pairs back with it and controls it from here. For a Mac
  of yours; off at first. **Copy** copies
it and **New link** makes another. Open it on the other Mac, scan its QR
code, or paste it there.

### Devices that control this Mac

Each paired device is listed with the name it gave, what it may do and
until when, its key's fingerprint, when it paired, and that it is here now
while it has a link open, else when it was last here. **Revoke** cuts it
off at once.

A device that waits shows **Let in** and **Turn away**. Check that its
fingerprint is the one the device shows before you let it in.

## Control other Macs

Turn on **Control other Macs**. Then, under **Pair with another Mac**,
paste the other machine's pairing link and press **Pair**. Opening a
`u0://pair` link (from a browser, a QR scan or `open`) does the same.
The u0 menu's **Pair a Device…** turns control on and opens the pairing dialog.

u0 asks before it pairs: it names the host and the fingerprint of the
key it made for it.

### Both ways at once

The confirmation has **Also let it control this Mac**. Ticked, this Mac
starts listening and the other pairs back with a key of its own, by a
one-time code this Mac makes. The other reaches this Mac at `127.0.0.1`
when it was itself reached there, otherwise only through this Mac's own
tunnel, which must be up. Where it cannot reach this Mac, the pairing
stays one way and says why. The other Mac pairs back only if the link
was made with **Pair both ways**; and a machine of a name this Mac is
paired with elsewhere is refused, never put in its place. (The checkbox is not offered when this Mac's
window runs on the launchd service.)

### Several Macs in one window

With control on, every machine you paired with is in the window beside
this Mac:

- The sidebar has a row for each machine, this Mac first, with its
  projects and threads under it and whether it is reached.
- New threads, ⌘K and ⌘P name the machine of each project.
- The settings dialog has a machine picker at its top. A machine's agents,
  projects, tasks, sharing and what it keeps are read from and saved on
  that machine. The look and keys stay this Mac's.
- Agent sign-in that needs a terminal runs in a u0 terminal on that
  machine.

**Macs this Mac controls** lists them with your key's fingerprint, the
version each runs, and its state: in this window, reaching it, offline,
or turned away. **Reach again** retries at once; **Forget** removes the
machine and its key from the Keychain.

### A dev server on another Mac

A dev server that a terminal on another Mac runs opens here: **Open**
(in the column's Servers, the drawer's Open preview, or ⌘-click on its
address) brings its port to this Mac over a link of its own, signed in as
the window is, and opens it at `127.0.0.1` on a port of this Mac's: in
the Preview tab when opened from there or from Open preview, in your
browser otherwise. While a page in the Preview tab is of such a port, a
note under its strip says so, with **Stop**. Only a Mac that
may do everything there can open one.

### When a link drops

A machine that drops is shown offline, with what the window had of it
still readable, and is reached again on its own (waiting 1 second, then
longer, up to 30 seconds between tries). Its server keeps the last 16 MiB
of what it said; a window that comes back to the same run of it is told
everything it missed, in order. After its server restarted, or when more
was missed, the window reads everything afresh. What you do while offline
is sent once the link is back. A machine that turned this Mac away is not
retried until you press **Reach again**.

## A Linux server

`u0-server` (see [Install](install.md#on-linux-u0-server)) listens
as it starts, starts the tunnel if it was on, and, run in a terminal,
prints:

```
u0 serves on ws://127.0.0.1:4317
Pair a desk with it, once, in the next ten minutes: u0://pair?host=…
```

followed by the link's QR code. Run where what it prints goes to a log
(as the service), it says where it serves and prints no link: pair a desk
from a window on that Mac instead. Reach it through a tunnel or `ssh -L`,
and pair from a Mac as above.

## Files

| File | Where | Holds |
|---|---|---|
| `devices.json` | on the machine controlled | The devices let in (`label`, `key` as `ssh-ed25519 AAAA…`, `scope`: `full` or `read`, `until`: when its access ends, `waiting`: until you let it in, `paired`, `seen`), `port`, and whether it listens (`serve`) and on which networks (`bind`), runs the tunnel (`tunnel`), keeps awake (`awake`), and a named tunnel (`named`: `{token, host}`). |
| `servers.json` | on the controlling Mac | The machines paired with (`name`, `host`, `key`: the public half of the key made for it, `pin`: the certificate of one reached on a network), and `control`. Each private half is in the Keychain under `server/<fingerprint>`. |

Both refuse unknown fields. A file that does not read is left alone, and
until it is fixed or moved away no device comes in (`devices.json`) or
no machine is reached (`servers.json`); pair again after moving it away.

:::caution

A named tunnel's token is stored in `devices.json` as plain text.

:::
