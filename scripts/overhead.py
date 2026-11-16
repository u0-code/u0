#!/usr/bin/env python3
"""What Ghost costs to run: memory, CPU, disk and start-up, measured the same
way each time, on this machine, with no network, no container and no key.

    just overhead                  # builds, then measures with 500 messages
    python3 scripts/overhead.py -n 2000 --settle 120
    python3 scripts/overhead.py --desk   # the desktop app's idle cost too

Two characters are born from ghosts/_template in a temporary directory and
run in one ghostd, whose brain is a fake OpenAI-compatible server on
loopback: it answers every look with one `say`, and anything else with an
empty JSON decision, so what is measured is Ghost and not a model. A
presence written here speaks the Ghost Protocol and sends messages from a
few people in a few places to both of them, answering everything she does.

The table goes to stdout and a dated line to bench/overhead.tsv, which stays
on this machine. Everything started is stopped on exit, Ctrl-C included.

Python rather than sh, because it is a web server and a WebSocket client as
well as a script, and the standard library has both.
"""

import argparse
import base64
import json
import os
import shutil
import signal
import socket
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GHOSTS = ("ada", "bea")
DESK = ROOT / "desk/target/release/Ghost.app/Contents/MacOS/ghost-desk"
DESK_COLUMNS = ("desk RSS MB", "desk CPU % (30 s)", "desk solver RSS MB")
started = []  # processes to stop on the way out


# The brain ----------------------------------------------------------------


class Brain(BaseHTTPRequestHandler):
    """`/v1/chat/completions`, answered at once and always the same way."""

    calls = 0
    last = 0.0

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        Brain.calls += 1
        Brain.last = time.time()
        tools = {t["function"]["name"] for t in body.get("tools", [])}
        if "say" in tools:
            # A look made of tool calls ends after a turn with one `say`.
            call = {"id": f"c{Brain.calls}", "type": "function",
                    "function": {"name": "say", "arguments": json.dumps({"text": "mm, ok"})}}
            message = {"role": "assistant", "content": "", "tool_calls": [call]}
            finish = "tool_calls"
        else:
            message = {"role": "assistant", "content": json.dumps({"thought": "fine", "actions": []})}
            finish = "stop"
        out = json.dumps({
            "id": f"fake-{Brain.calls}", "object": "chat.completion", "model": "fake",
            "choices": [{"index": 0, "message": message, "finish_reason": finish}],
            "usage": {"prompt_tokens": 100, "completion_tokens": 10, "total_tokens": 110},
        }).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)

    def log_message(self, *_):
        pass


# A presence ---------------------------------------------------------------


class Presence:
    """A WebSocket client that is just enough of one, and a presence on the
    `sim` channel for one character: it answers whatever the daemon asks."""

    def __init__(self, port, ghost):
        self.sock = socket.create_connection(("127.0.0.1", port))
        key = base64.b64encode(os.urandom(16)).decode()
        self.sock.sendall((
            f"GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n"
            f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        ).encode())
        self.file = self.sock.makefile("rb")
        if b" 101 " not in self.file.readline():
            raise SystemExit("ghostd refused the WebSocket upgrade")
        while self.file.readline() not in (b"\r\n", b""):
            pass
        self.lock = threading.Lock()
        self.acts = 0
        self.last = 0.0
        self.send({"jsonrpc": "2.0", "id": 1, "method": "hello", "params": {
            "protocol": 1, "role": "presence", "ghost": ghost,
            "presence": {"channel": "sim", "can": ["send"],
                         "me": {"id": f"bot-{ghost}", "name": ghost, "automated": True}},
        }})
        welcome = json.loads(self.recv())
        if "result" not in welcome:
            raise SystemExit(f"hello refused: {welcome}")
        threading.Thread(target=self.answer, daemon=True).start()

    def frame(self, opcode, data):
        n = len(data)
        head = bytes([0x80 | opcode])
        if n < 126:
            head += bytes([0x80 | n])
        elif n < 65536:
            head += bytes([0x80 | 126]) + n.to_bytes(2, "big")
        else:
            head += bytes([0x80 | 127]) + n.to_bytes(8, "big")
        mask = os.urandom(4)
        masked = (int.from_bytes(data, "big") ^ int.from_bytes((mask * (n // 4 + 1))[:n], "big")).to_bytes(n, "big")
        with self.lock:
            self.sock.sendall(head + mask + masked)

    def send(self, obj):
        self.frame(0x1, json.dumps(obj).encode())

    def recv(self):
        message = b""
        while True:
            a, b = self.file.read(2)
            n = b & 0x7F
            if n == 126:
                n = int.from_bytes(self.file.read(2), "big")
            elif n == 127:
                n = int.from_bytes(self.file.read(8), "big")
            data = self.file.read(n)
            opcode = a & 0x0F
            if opcode == 0x8:
                return None
            if opcode == 0x9:
                self.frame(0xA, data)
                continue
            if opcode == 0xA:
                continue
            message += data
            if a & 0x80:
                return message.decode()

    def answer(self):
        try:
            while (text := self.recv()) is not None:
                frame = json.loads(text)
                if "method" in frame and "id" in frame:
                    if frame["method"] == "presence.act":
                        self.acts += 1
                        self.last = time.time()
                    self.send({"jsonrpc": "2.0", "id": frame["id"], "result": {"id": f"out-{frame['id']}"}})
        except (OSError, ValueError):
            pass

    def inbound(self, n, place, kind, sender, addressed):
        self.send({"jsonrpc": "2.0", "method": "presence.inbound", "params": {
            "place": {"id": place, "kind": kind, **({"name": place} if kind == "group" else {})},
            "from": {"id": sender, "name": sender.title()},
            "id": f"m{n}", "text": f"message {n} from {sender}", "addressed": addressed,
        }})


# Measuring ----------------------------------------------------------------


def ps(pid):
    """RSS in MB and CPU seconds of a process, or None once it is gone."""
    out = subprocess.run(["ps", "-o", "rss=,time=", "-p", str(pid)], capture_output=True, text=True).stdout.split()
    if len(out) != 2:
        return None
    seconds = 0.0
    for part in out[1].replace("-", ":").split(":"):
        seconds = seconds * 60 + float(part)
    return int(out[0]) / 1024, seconds


class Sampler(threading.Thread):
    """The highest RSS seen while it runs."""

    def __init__(self, pid):
        super().__init__(daemon=True)
        self.pid, self.peak, self.running = pid, 0.0, True

    def run(self):
        while self.running:
            if seen := ps(self.pid):
                self.peak = max(self.peak, seen[0])
            time.sleep(0.2)


def rss_now(pid, samples=5):
    """Median RSS over a second."""
    seen = []
    for _ in range(samples):
        seen.append(ps(pid)[0])
        time.sleep(0.2)
    return statistics.median(seen)


def size(paths):
    return sum(p.stat().st_size for p in paths if p.is_file())


def stores(tmp):
    """Bytes of every journal and every derived memory, WAL included."""
    journal = size(p for g in GHOSTS for p in (tmp / "ghosts" / g).glob("journal.db*"))
    memory = size(p for g in GHOSTS for p in (tmp / "ghosts" / g / "derived").glob("*"))
    return journal, memory


def timed(cmd, runs=5):
    """Median wall seconds of a command."""
    spent = []
    for _ in range(runs):
        t = time.perf_counter()
        subprocess.run(cmd, check=True, capture_output=True)
        spent.append(time.perf_counter() - t)
    return statistics.median(spent)


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def wait_for(port, proc, log, seconds=30):
    end = time.time() + seconds
    while time.time() < end:
        if proc.poll() is not None:
            raise SystemExit(f"ghostd exited at start:\n{log.read_text()[-2000:]}")
        try:
            socket.create_connection(("127.0.0.1", port), timeout=0.5).close()
            return
        except OSError:
            time.sleep(0.1)
    raise SystemExit(f"ghostd did not listen within {seconds}s:\n{log.read_text()[-2000:]}")


def spawn(cmd, log, **kw):
    proc = subprocess.Popen(cmd, stdout=log.open("w"), stderr=subprocess.STDOUT, start_new_session=True, **kw)
    started.append(proc)
    return proc


def stop_all():
    for proc in started:
        if proc.poll() is None:
            try:
                os.killpg(proc.pid, signal.SIGTERM)
                proc.wait(timeout=10)
            except (ProcessLookupError, subprocess.TimeoutExpired):
                os.killpg(proc.pid, signal.SIGKILL)


def config(port, brain):
    return f"""[brain]
think = "fake"
tick = "fake"

[listen]
addr = "127.0.0.1:{port}"

[mind]
tempo = 0
calls_per_hour = 1000000
live = false
weather = false
talk_machine = false

[mind.work]
enabled = false

[tools]
enabled = false

[skills]
enabled = false

[providers.fake]
kind = "openai"
base_url = "http://127.0.0.1:{brain}/v1"
model = "fake"
api_key = "fake"
"""


# The run ------------------------------------------------------------------


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("-n", "--messages", type=int, default=500)
    ap.add_argument("--rate", type=float, default=50, help="messages a second, 0 for as fast as it goes")
    ap.add_argument("--settle", type=int, default=60, help="seconds after the load before the last RSS")
    ap.add_argument("--bin", type=Path, default=ROOT / "target/release")
    ap.add_argument("--desk", action="store_true", help="also measure the desktop app at rest")
    ap.add_argument("--keep", action="store_true", help="leave the temporary directory")
    args = ap.parse_args()
    args.bin = args.bin.resolve()

    for name in ("ghostd", "ghost-qqbot", "ghost-term"):
        if not (args.bin / name).is_file():
            raise SystemExit(f"no {args.bin / name}: cargo build --release first")
    if args.desk:
        if not DESK.is_file():
            raise SystemExit(f"no {DESK}: desk/bundle.sh first")
        # The desk starts a solver only when nothing answers where its own
        # listens; if something does, the desk would be measured talking to it.
        try:
            socket.create_connection(("127.0.0.1", 7438), timeout=0.5).close()
            raise SystemExit("something listens on 127.0.0.1:7438, the desk's solver: quit Ghost.app first")
        except OSError:
            pass
    ghostd = str(args.bin / "ghostd")
    tmp = Path(tempfile.mkdtemp(prefix="ghost-overhead-"))
    # Nothing from this machine's own setup: no .env, no keys, no proxy.
    env = {k: v for k, v in os.environ.items()
           if not k.endswith(("_API_KEY", "_TOKEN")) and "PROXY" not in k.upper()}
    env |= {"HOME": str(tmp / "home"), "RUST_LOG": "warn"}
    row = {}
    try:
        brain = ThreadingHTTPServer(("127.0.0.1", 0), Brain)
        threading.Thread(target=brain.serve_forever, daemon=True).start()
        port = free_port()
        (tmp / "ghostd.toml").write_text(config(port, brain.server_port))
        for name in GHOSTS:
            home = tmp / "ghosts" / name
            shutil.copytree(ROOT / "ghosts/_template", home)
            toml = home / "ghost.toml"
            toml.write_text(toml.read_text().replace('name = "Her name"', f'name = "{name.title()}"', 1))
            subprocess.run([ghostd, "birth", str(home)], check=True, capture_output=True, env=env, cwd=tmp)

        for name in ("ghostd", "ghost-qqbot", "ghost-term"):
            row[f"{name} MB"] = (args.bin / name).stat().st_size / 1e6
        status = [ghostd, "status", str(tmp / "ghosts" / GHOSTS[0])]
        row["status ms (born)"] = timed(status) * 1000

        # Both measured at rest, with nothing left in a write-ahead log.
        disk0 = stores(tmp)
        log = tmp / "ghostd.log"
        daemon = spawn([ghostd, "run", *(str(tmp / "ghosts" / g) for g in GHOSTS),
                        "--config", str(tmp / "ghostd.toml")], log, env=env, cwd=tmp)
        wait_for(port, daemon, log)
        time.sleep(3)
        idle = rss_now(daemon.pid)
        cpu0 = ps(daemon.pid)[1]

        presences = {g: Presence(port, g) for g in GHOSTS}
        # Three people, each in a private conversation with each of them,
        # and two groups where one message in three is to her.
        people = ["ana", "ben", "cai", "dee", "eli", "fay"]
        places = [(f"dm-{p}", "direct", p) for p in people[:3]] + [("group-1", "group", None), ("group-2", "group", None)]
        sampler = Sampler(daemon.pid)
        sampler.start()
        t0 = time.time()
        for n in range(args.messages):
            ghost = GHOSTS[n % len(GHOSTS)]
            place, kind, who = places[(n // len(GHOSTS)) % len(places)]
            sender = who or people[n % len(people)]
            presences[ghost].inbound(n, place, kind, sender, kind == "direct" or n % 3 == 0)
            if args.rate:
                time.sleep(max(0.0, t0 + (n + 1) / args.rate - time.time()))
        sent = time.time() - t0
        # The load is over when neither the brain nor the presences have
        # heard anything for five seconds.
        def last():
            return max(Brain.last, *(p.last for p in presences.values()), t0 + sent)

        while time.time() - last() < 5:
            time.sleep(0.5)
            if daemon.poll() is not None:
                raise SystemExit(f"ghostd exited under load:\n{log.read_text()[-2000:]}")
        busy = last() - t0
        sampler.running = False
        cpu = ps(daemon.pid)[1] - cpu0
        time.sleep(args.settle)
        after = rss_now(daemon.pid)
        row |= {
            "idle RSS MB": idle,
            "peak RSS MB": max(sampler.peak, after),
            "settle s": args.settle,
            "RSS MB after": after,
            "growth %": (after - idle) / idle * 100,
            "messages": args.messages,
            "busy s": busy,
            "brain calls": Brain.calls,
            "acts": sum(p.acts for p in presences.values()),
            "CPU s / 100 msg": cpu / args.messages * 100,
        }
        os.killpg(daemon.pid, signal.SIGTERM)
        daemon.wait(timeout=30)
        disk = stores(tmp)
        row["journal B / msg"] = (disk[0] - disk0[0]) / args.messages
        row["memory B / msg"] = (disk[1] - disk0[1]) / args.messages
        row["status ms (loaded)"] = timed(status) * 1000

        row |= desk(tmp, env, ghostd, brain.server_port) if args.desk else dict.fromkeys(DESK_COLUMNS, "-")
    finally:
        stop_all()
        if args.keep:
            print(f"left in {tmp}", file=sys.stderr)
        else:
            shutil.rmtree(tmp, ignore_errors=True)

    width = max(map(len, row))
    for key, value in row.items():
        if value == "-":
            continue
        shown = f"{value:.2f}" if isinstance(value, float) else str(value)
        print(f"{key:<{width}}  {shown:>10}")
    commit = subprocess.run(["git", "-C", str(ROOT), "rev-parse", "--short", "HEAD"],
                            capture_output=True, text=True).stdout.strip()
    tsv = ROOT / "bench/overhead.tsv"
    tsv.parent.mkdir(exist_ok=True)
    fresh = not tsv.exists()
    with tsv.open("a") as f:
        if fresh:
            f.write("\t".join(["date", "commit", *row]) + "\n")
        f.write("\t".join([datetime.now().isoformat(timespec="seconds"), commit,
                           *(f"{v:.2f}" if isinstance(v, float) else str(v) for v in row.values())]) + "\n")


def desk(tmp, env, ghostd, brain):
    """The desktop app with nothing to do, for thirty seconds: its own RSS
    and CPU, and those of the solver daemon it starts beside it."""
    # A checkout of its own, so that it starts its solver here on the fake
    # brain and never touches the real one.
    root = tmp / "desk"
    core = root / "desk/core"
    core.mkdir(parents=True)
    shutil.copy(ROOT / "desk/core/ghostd.example.toml", core)
    shutil.copytree(ROOT / "desk/core/ghosts/_solver", core / "ghosts/_solver")
    (core / "ghostd.toml").write_text(config(7438, brain))
    proc = spawn([str(DESK)], tmp / "desk.log", env=env | {"GHOST_ROOT": str(root), "GHOSTD_BIN": ghostd}, cwd=root)
    time.sleep(10)
    children = subprocess.run(["pgrep", "-P", str(proc.pid)], capture_output=True, text=True).stdout.split()
    start = {pid: ps(pid) for pid in [proc.pid, *map(int, children)]}
    time.sleep(30)
    end = {pid: ps(pid) for pid in start}
    if not end[proc.pid]:
        raise SystemExit(f"the desk exited:\n{(tmp / 'desk.log').read_text()[-2000:]}")
    solver = [pid for pid in start if pid != proc.pid and end[pid]]
    return dict(zip(DESK_COLUMNS, [
        end[proc.pid][0],
        (end[proc.pid][1] - start[proc.pid][1]) / 30 * 100,
        sum(end[pid][0] for pid in solver) if solver else "-",
    ]))


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)
