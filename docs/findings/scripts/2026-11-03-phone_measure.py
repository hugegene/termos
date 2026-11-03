"""Measure the finder prototype at phone-sized terminals.

For each terminal size: start a throwaway zellij session in a pty, launch `finder` into it,
then drive scripted actions and count the bytes the zellij client writes to the terminal
(= what SSH would carry, before encryption). Also dumps each pane's screen and its geometry.

usage: python3 phone_measure.py <sample_dir> <out_dir> COLSxROWS [COLSxROWS ...]
"""
import fcntl, json, os, pty, select, struct, subprocess, sys, termios, threading, time, zlib

SAMPLE, OUT = sys.argv[1], sys.argv[2]
SIZES = [tuple(map(int, s.split("x"))) for s in sys.argv[3:]]
ENV = {k: v for k, v in os.environ.items() if not k.startswith("ZELLIJ")}
ENV.update(TERM="xterm-256color", COLORTERM="truecolor")
FINDER = os.path.expanduser("~/terminal-browser/finder")


class Stream:
    def __init__(self, fd):
        self.fd, self.chunks, self.last, self.alive = fd, [], time.time(), True
        self.lock = threading.Lock()
        threading.Thread(target=self.pump, daemon=True).start()

    def pump(self):
        while self.alive:
            if select.select([self.fd], [], [], 0.05)[0]:
                try:
                    data = os.read(self.fd, 1 << 16)
                except OSError:
                    return
                with self.lock:
                    self.chunks.append(data)
                    self.last = time.time()

    def mark(self):
        with self.lock:
            return len(self.chunks)

    def since(self, m):
        with self.lock:
            return b"".join(self.chunks[m:])

    def settle(self, quiet=0.5, cap=6.0):
        t0 = time.time()
        time.sleep(0.15)
        while time.time() - t0 < cap:
            with self.lock:
                if time.time() - self.last >= quiet:
                    return
            time.sleep(0.05)


def run(cols, rows):
    ses = f"tftest-phone-{cols}x{rows}"
    subprocess.run(["zellij", "delete-session", ses, "--force"], capture_output=True, env=ENV)
    pid, fd = pty.fork()
    if pid == 0:
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        os.chdir(SAMPLE)
        os.execvpe("zellij", ["zellij", "-s", ses], ENV)
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    st = Stream(fd)
    time.sleep(3)
    za = lambda *a: subprocess.run(["zellij", "-s", ses, "action", *a], capture_output=True,
                                   text=True, env=ENV).stdout
    comp = zlib.compressobj(6)  # stateful, like `ssh -C`
    res = {"size": f"{cols}x{rows}", "actions": []}

    def record(name, m, n=1, note=""):
        raw = st.since(m)
        z = len(comp.compress(raw) + comp.flush(zlib.Z_SYNC_FLUSH))
        res["actions"].append({"action": name, "count": n, "bytes": len(raw), "gzip": z,
                               "bytes_each": round(len(raw) / n), "gzip_each": round(z / n), "note": note})

    def key(b, quiet=0.5):
        os.write(fd, b)
        st.settle(quiet)

    m = st.mark()
    subprocess.run([FINDER, SAMPLE], env={**ENV, "ZELLIJ": "0", "ZELLIJ_SESSION_NAME": ses})
    time.sleep(2.0)
    st.settle(1.0, 10)
    record("open finder (first full draw)", m)

    panes = {p["title"]: p for p in json.loads(za("list-panes", "-j")) if p.get("tab_name") == "finder"}
    geo = {t: f'{p["pane_content_columns"]}x{p["pane_content_rows"]}' for t, p in panes.items()}
    res["pane_content_size"] = geo
    pid_of = lambda t: f'terminal_{panes[t]["id"]}'
    dump = lambda t: za("dump-screen", "-p", pid_of(t))
    header = lambda: (dump("preview").splitlines() or [""])[0].strip()

    def save(tag):
        for t in ("files", "preview"):
            if t in panes:
                with open(f"{OUT}/{cols}x{rows}-{tag}-{t}.txt", "w") as f:
                    f.write(dump(t))

    save("start")
    # browse: hover each file once (preview re-render on every move)
    seen = []
    for _ in range(6):
        m = st.mark(); key(b"j", 1.2); record("move to next file (+preview)", m, note=header()[:40])
        seen.append(header()[:30])
    res["browse_headers"] = seen
    # photo.png is last in the list; capture image preview
    save("photo")
    # long.md: top, then down 3 (a.md, b.txt, c.csv, long.md)
    key(b"gg", 0.6); key(b"jjj", 0.8)
    res["long_header"] = header()[:40]
    save("long")
    za("focus-pane-id", pid_of("preview")); st.settle(0.5)
    m = st.mark()
    for _ in range(20):
        key(b"j", 0.25)
    record("preview: scroll 1 line", m, 20)
    m = st.mark()
    for _ in range(5):
        key(b" ", 0.35)
    record("preview: page down", m, 5)
    key(b"h", 0.5)  # back to files
    # edit notes.md: from long.md down 1
    key(b"j", 0.8)
    res["edit_target"] = header()[:40]
    m = st.mark(); key(b"\r", 1.0); time.sleep(1.5); st.settle(0.6); record("open editor (Neovim)", m)
    save("editor")
    text = b"hello from the phone " * 3
    m = st.mark()
    for ch in text:
        os.write(fd, bytes([ch])); time.sleep(0.06)
    st.settle(0.5)
    record("editor: type 1 character", m, len(text))
    os.write(fd, b"\x1b"); time.sleep(0.4)
    key(b"\x1bq", 1.0); key(b"n", 1.5)  # Alt+q → confirm → No (don't save)
    m = st.mark(); time.sleep(5); record("idle 5 s", m)
    st.alive = False
    subprocess.run(["zellij", "kill-session", ses], capture_output=True, env=ENV)
    subprocess.run(["zellij", "delete-session", ses, "--force"], capture_output=True, env=ENV)
    return res


results = [run(c, r) for c, r in SIZES]
with open(f"{OUT}/results.json", "w") as f:
    json.dump(results, f, indent=1)
for r in results:
    print(f"\n=== {r['size']}  panes {r['pane_content_size']}")
    print("   browse:", r["browse_headers"], "| long:", r["long_header"], "| edit:", r["edit_target"])
    for a in r["actions"]:
        print(f"   {a['action']:32} n={a['count']:<3} bytes={a['bytes']:>8,} gzip={a['gzip']:>7,}"
              f"  each={a['bytes_each']:>7,} / gz {a['gzip_each']:>6,}  {a['note']}")
