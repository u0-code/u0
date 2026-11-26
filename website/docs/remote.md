---
title: Remote control
description: Use the Ghost on one machine from a Mac elsewhere, pair devices by key, and hold several Macs in one window.
---

# Remote control

The Ghost on one machine can be used from a Mac elsewhere: its agents,
sessions, files, git and terminals, as if you sat at it. Every Ghost can
do both sides:

- **be controlled**: let other devices control this Mac (or a Linux box
  running `ghost-server`);
- **control**: hold other Macs in this window, beside this one.

Both are in **Settings › Devices**, and both are off at first.

## How a device comes in

A device is let in by a key, never by a password:

1. When you pair, the controlling Mac makes an Ed25519 key for that one
   server. The private half stays in its Keychain; only the public half is
   sent. The server lists it by its `SHA256:` fingerprint.
2. Pairing is allowed by a pairing code: 128 random bits, used once, valid
   for ten minutes, kept in memory only (a restart forgets it).
3. Each time the desk links afterwards, the server sends a fresh random
   nonce, and the desk signs it with its key. No secret crosses the wire
   or sits on the server.

Each call a device makes is checked here, on the machine controlled,
against what that device may do. A device that may only read watches
sessions, diffs and the files of your projects and their worktrees, and
changes nothing; anything else it asks is refused.

Turning listening off cuts every link at once and withdraws every unused
pairing code. **Revoke** on a device removes its key and closes its links,
as an access that ends does.

## Let other devices control this Mac

In **Settings › Devices**, turn on **Let other devices control this
Mac**. (The Ghost menu's **Show Pairing Code…** does the same and opens
the page.)

Ghost then listens on `127.0.0.1` only, at port 4317 (the `port` in
`<home>/devices.json`; give a second Ghost on the same machine another).
Nothing on the network reaches that port directly. A device reaches it
one of two ways:

- **A cloudflared tunnel.** Under **Reach › Tunnel (cloudflared)**,
  press **Start**. Ghost runs `cloudflared` from your `PATH` (install it
  with `brew install cloudflared`). By default it is a quick tunnel: a new
  `trycloudflare.com` address each time it starts, so devices must pair
  again with a new link. To keep one address, use a named tunnel of yours:
  enter its host name and its tunnel token, then **Use**. **Use a quick
  tunnel** goes back.
- **SSH port forwarding**, from the controlling Mac:
  `ssh -L 4317:127.0.0.1:4317 host`, then pair with the `127.0.0.1` link.
- **A network.** Under **Reach › Listen on**, choose **Tailscale**, or
  **Every network** (Tailscale too), beside **This Mac**. Ghost then also
  listens on that network's addresses, on the port after its own (4318
  by default), and speaks TLS there itself, with a certificate of its own
  kept in the Keychain. Reach lists each address it is reached at.

**Keep the Mac awake while serving** keeps it reachable with no one at it
(Ghost runs `caffeinate` while it listens).

### The pairing link

Under **Pair a device with this Mac**, Ghost shows a pairing link and its
QR code:

```
ghost://pair?host=<host>#<code>
```

The host is the way chosen under **Reached by**: **Best** (the tunnel
while it is up, then Tailscale, then the local network, then
`127.0.0.1:<port>`), or one of them. A link on a network also carries
`pin=`, the SHA-256 of this Mac's certificate, so the device trusts this
Mac alone there. The link works once, for ten minutes.

Above it, choose what the next link lets in; each choice makes a new
link at once:

- **What it may do**: **Everything**, or **Read only**.
- **For how long**: **Until revoked**, **A day**, **A week** or
  **30 days**. After that the device is turned away, and pairs again with
  a new link.
- **Ask me before letting it in**: the device pairs, then waits under
  **Devices that control this Mac** until you let it in. For a link you
  send someone. **Copy** copies
it and **New link** makes another. Open it on the other Mac, scan its QR
code, or paste it there.

### Devices that control this Mac

Each paired device is listed with the name it gave, what it may do and
until when, its key's fingerprint, when it paired and when it was last
here. **Revoke** cuts it off at once.

A device that waits shows **Let in** and **Turn away**. Check that its
fingerprint is the one the device shows before you let it in.

## Control other Macs

Turn on **Control other Macs**. Then, under **Pair with another Mac**,
paste the other machine's pairing link and press **Pair**. Opening a
`ghost://pair` link (from a browser, a QR scan or `open`) does the same.
The Ghost menu's **Pair a Device…** turns control on and takes you there.

Ghost asks before it pairs: it names the host and the fingerprint of the
key it made for it.

### Both ways at once

The confirmation has **Also let it control this Mac**. Ticked, this Mac
starts listening and the other pairs back with a key of its own, by a
one-time code this Mac makes. The other reaches this Mac at `127.0.0.1`
when it was itself reached there, otherwise only through this Mac's own
tunnel, which must be up. Where it cannot reach this Mac, the pairing
stays one way and says why. (The checkbox is not offered when this Mac's
window runs on the launchd service.)

### Several Macs in one window

With control on, every server you paired with is in the window beside
this Mac:

- The sidebar has a row for each server, this Mac first, with its
  projects and sessions under it and whether it is reached.
- New sessions, ⌘K and ⌘P name the server of each project.
- The settings dialog has a server picker at its top. A server's agents,
  projects, tasks, sharing and what it keeps are read from and saved on
  that server. The look and keys stay this Mac's.
- Agent sign-in that needs a terminal runs in a Ghost terminal on that
  server.

**Macs this Mac controls** lists them with your key's fingerprint, the
version each runs, and its state: in this window, reaching it, offline,
or turned away. **Reach again** retries at once; **Forget** removes the
server and its key from the Keychain.

### When a link drops

A server that drops is shown offline, with what the window had of it
still readable, and is reached again on its own (waiting 1 second, then
longer, up to 30 seconds between tries). The server keeps the last 16 MiB
of what it said; a desk that comes back to the same server run is told
everything it missed, in order. After the server restarted, or when more
was missed, the desk reads everything afresh. What you do while offline
is sent once the link is back. A server that turned this Mac away is not
retried until you press **Reach again**.

## A Linux server

`ghost-server` (see [Install](install.md#on-linux-ghost-server)) listens
as it starts, starts the tunnel if it was on, and prints:

```
Ghost serves on ws://127.0.0.1:4317
Pair a desk with it, once, in the next ten minutes: ghost://pair?host=…
```

followed by the link's QR code. Reach it through a tunnel or
`ssh -L`, and pair from a Mac as above.

## Files

| File | Where | Holds |
|---|---|---|
| `devices.json` | on the machine controlled | The devices let in (`label`, `key` as `ssh-ed25519 AAAA…`, `scope`: `full` or `read`, `until`: when its access ends, `waiting`: until you let it in, `paired`, `seen`), `port`, and whether it listens (`serve`) and on which networks (`bind`), runs the tunnel (`tunnel`), keeps awake (`awake`), and a named tunnel (`named`: `{token, host}`). |
| `servers.json` | on the controlling Mac | The servers paired with (`name`, `host`, `key`: the public half of the key made for it, `pin`: the certificate of one reached on a network), and `control`. Each private half is in the Keychain under `server/<fingerprint>`. |

Both refuse unknown fields. A file that does not read is left alone, and
until it is fixed or moved away no device comes in (`devices.json`) or
no server is reached (`servers.json`); pair again after moving it away.

:::caution

A named tunnel's token is stored in `devices.json` as plain text.

:::

:::note Coming

Opening a paired Mac's dev server here through its link, and
`ghost thread`, `ghost workflow` and `ghost doctor` acting over a signed
link are being finished and are not in the current build yet.

:::
