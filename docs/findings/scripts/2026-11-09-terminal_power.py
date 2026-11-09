#!/usr/bin/env python3
"""CPU, iGPU and RAPL power of Terminator vs Ghostty, each with a plain shell and with `tb attach`.

Usage: python3 -I 2026-11-09-terminal_power.py TB_BINARY OUT.jsonl
Env:   SETTLE (s, default 10), MEASURE (s, default 45), ROUNDS (default 3), ONLY (comma list of run names),
       START_DELAY (s, default 0: quiet time before the first run)

Each run opens one fresh window (1200x800 px, focused), waits SETTLE, then reads counters over MEASURE:
  - CPU:  utime+stime from /proc/PID/stat (all threads) for the terminal, tb client, tb server, the
          shell/workload processes, Xorg and gnome-shell
  - iGPU: i915 per-client render-engine time from /proc/PID/fdinfo (same processes), and whole-GPU busy
          time from rc6 residency
  - power: RAPL energy (package-0, core, uncore = iGPU, psys) if energy_uj is readable
Runs rotate order each round, and every round has a baseline with no test window.
"""
import json, os, signal, subprocess, sys, tempfile, time, shutil

TB = os.path.abspath(sys.argv[1])
OUT = sys.argv[2]
SETTLE = float(os.environ.get("SETTLE", 10))
MEASURE = float(os.environ.get("MEASURE", 45))
ROUNDS = int(os.environ.get("ROUNDS", 3))
ONLY = [s for s in os.environ.get("ONLY", "").split(",") if s]
START_DELAY = float(os.environ.get("START_DELAY", 0))
CLK = os.sysconf("SC_CLK_TCK")
GEOM = (1200, 800, 100, 100)
RC6 = "/sys/class/drm/card1/gt/gt0/rc6_residency_ms"
RAPL = {"pkg": "intel-rapl:0", "core": "intel-rapl:0:0", "uncore": "intel-rapl:0:1", "psys": "intel-rapl:1"}

# The scrolling-log workload: 50 lines of ~100 characters a second, until killed.
GEN = r'''
import sys, time
n = 0; t0 = time.monotonic()
while True:
    n += 1
    sys.stdout.write(f"{n:08d} {time.strftime('%H:%M:%S')} INFO worker-3 processed request "
                     f"id={n * 7919 % 1000003:07d} in 12.4 ms status=200 bytes=4821 path=/api/v1/items\n")
    sys.stdout.flush()
    d = t0 + n / 50 - time.monotonic()
    if d > 0: time.sleep(d)
'''


def sh(*a, **kw):
    return subprocess.run(a, capture_output=True, text=True, **kw)


def stat(pid):
    """(ppid, utime+stime ticks, comm) or None."""
    try:
        s = open(f"/proc/{pid}/stat").read()
    except OSError:
        return None
    comm = s[s.index("(") + 1:s.rindex(")")]
    f = s[s.rindex(")") + 2:].split()
    return int(f[1]), int(f[11]) + int(f[12]), comm


def all_procs():
    out = {}
    for p in os.listdir("/proc"):
        if p.isdigit():
            st = stat(int(p))
            if st:
                out[int(p)] = st
    return out


def descendants(root, procs):
    kids = {}
    for pid, (ppid, _, _) in procs.items():
        kids.setdefault(ppid, []).append(pid)
    out, todo = [], [root]
    while todo:
        p = todo.pop()
        for c in kids.get(p, []):
            out.append(c)
            todo.append(c)
    return out


def cmdline(pid):
    try:
        return open(f"/proc/{pid}/cmdline", "rb").read().replace(b"\0", b" ").decode(errors="replace").strip()
    except OSError:
        return ""


def gpu_ns(pid):
    """Sum of i915 engine busy ns over this process's DRM clients (deduped by client id)."""
    seen = {}
    try:
        fds = os.listdir(f"/proc/{pid}/fdinfo")
    except OSError:
        return None
    for fd in fds:
        try:
            txt = open(f"/proc/{pid}/fdinfo/{fd}").read()
        except OSError:
            continue
        if "drm-driver:\ti915" not in txt:
            continue
        cid, ns = None, 0
        for line in txt.splitlines():
            if line.startswith("drm-client-id:"):
                cid = line.split()[1]
            elif line.startswith("drm-engine-") and line.endswith(" ns"):
                ns += int(line.split()[1])
        seen[cid] = ns
    return sum(seen.values())


def rapl():
    out = {}
    for k, d in RAPL.items():
        try:
            out[k] = int(open(f"/sys/class/powercap/{d}/energy_uj").read())
        except OSError:
            out[k] = None
    return out


def rapl_max(k):
    return int(open(f"/sys/class/powercap/{RAPL[k]}/max_energy_range_uj").read())


def read_int(path):
    try:
        return int(open(path).read())
    except OSError:
        return None


def pgrep(*a):
    r = sh("pgrep", *a)
    return [int(x) for x in r.stdout.split()]


def dgpu_status():
    """NVIDIA runtime PM state from sysfs; nvidia-smi would wake the card and skew the watts."""
    try:
        return open("/sys/bus/pci/devices/0000:01:00.0/power/runtime_status").read().strip()
    except OSError:
        return None


XORG = pgrep("-u", str(os.getuid()), "-x", "Xorg")[0]
SHELL_PID = pgrep("-u", str(os.getuid()), "-x", "gnome-shell")[0]


def launch(term, use_tb, env, session, gen):
    w, h, x, y = GEOM
    inner = [TB, "attach", session] if use_tb else (["python3", "-I", gen] if gen else [])
    if term == "terminator":
        cmd = ["terminator", "-u", f"--geometry={w}x{h}+{x}+{y}", "--title=tftest-power"]
        if inner:
            cmd += ["-x", *inner]
    else:
        cmd = ["ghostty", "--gtk-single-instance=false", "--title=tftest-power"]
        if inner:
            cmd += ["-e", *inner]
    p = subprocess.Popen(cmd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL, start_new_session=True)
    r = sh("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(p.pid), timeout=20)
    wid = r.stdout.split()[-1]
    sh("xdotool", "windowsize", wid, str(w), str(h))
    sh("xdotool", "windowmove", wid, str(x), str(y))
    time.sleep(1.5)
    sh("xdotool", "windowactivate", "--sync", wid, timeout=10)
    return p, wid


def grid(term_pid, procs):
    """Rows x cols of the terminal's pty, read from its first child (shell or tb client)."""
    for c in descendants(term_pid, procs):
        try:
            tty = os.readlink(f"/proc/{c}/fd/0")
        except OSError:
            continue
        if tty.startswith("/dev/pts/"):
            return sh("stty", "-F", tty, "size").stdout.strip()
    return None


def snapshot(groups):
    procs = all_procs()
    cpu = {g: sum(procs[p][1] for p in pids if p in procs) for g, pids in groups.items()}
    gpu = {g: sum((gpu_ns(p) or 0) for p in pids) for g, pids in groups.items()}
    # every readable process, to name whatever else was busy during the run
    every = {p: (st[2], st[1], gpu_ns(p) or 0) for p, st in procs.items()}
    return {"t": time.monotonic(), "cpu": cpu, "gpu": gpu, "rapl": rapl(), "rc6": read_int(RC6), "every": every}


def top(a, b, dt, i, scale, n=6):
    d = []
    for p, (comm, *v) in b["every"].items():
        if p in a["every"]:
            x = (v[i - 1] - a["every"][p][i]) / scale / dt * 100
            if x > 0:
                d.append((round(x, 3), f"{comm}({p})"))
    return sorted(d, reverse=True)[:n]


def run_one(name, term, use_tb, workload, tbdirs, rnd):
    env = {k: v for k, v in os.environ.items() if k not in ("TB_SESSION", "TB_PANE_ID")}
    env["TB_SOCKET_DIR"], env["TB_STATE_DIR"] = tbdirs[:2]
    session = f"tftest-pw{rnd}-{int(time.time() * 1000) % 1000000}"  # fresh name: a reused one is restored
    p = wid = None
    server = []
    info = {"name": name, "round": rnd, "term": term, "tb": use_tb, "workload": workload}
    try:
        if term:
            gen = tbdirs[2] if workload == "log" else None
            p, wid = launch(term, use_tb, env, session, gen)
            info["focused"] = sh("xdotool", "getactivewindow").stdout.strip() == wid
            info["window"] = sh("xdotool", "getwindowgeometry", wid).stdout.split("Geometry:")[-1].strip()
            if workload == "log" and use_tb:
                # wait for the shell prompt inside tb's pane, then type the command into it
                for _ in range(100):
                    if "$" in sh(TB, "-s", session, "dump-screen", env=env).stdout:
                        break
                    time.sleep(0.1)
                sh(TB, "action", "-s", session, "write-chars", f"python3 -I {tbdirs[2]}\n", env=env)
        time.sleep(SETTLE)
        procs = all_procs()
        groups = {"xorg": [XORG], "gnome_shell": [SHELL_PID]}
        if term:
            groups["term"] = [p.pid]
            tb_client = [c for c in descendants(p.pid, procs) if cmdline(c).startswith(f"{TB} attach")]
            server = pgrep("-f", f"{TB} __server {session}")
            groups["tb_client"] = tb_client
            groups["tb_server"] = server
            rest = set(descendants(p.pid, procs)) | set(d for s in server for d in descendants(s, procs))
            rest -= set(tb_client) | set(server)
            work = [c for c in rest if "python3 -I" in cmdline(c)]
            groups["workload"] = work
            groups["shell"] = sorted(rest - set(work))
            info["grid"] = grid(p.pid, procs)
            info["pids"] = {g: v for g, v in groups.items()}
            if workload == "log" and not work:
                info["workload_missing"] = True
        a = snapshot(groups)
        time.sleep(MEASURE / 2)
        info["dgpu"] = dgpu_status()
        time.sleep(MEASURE / 2)
        b = snapshot(groups)
        dt = b["t"] - a["t"]
        info["seconds"] = round(dt, 2)
        info["top_cpu_pct"] = top(a, b, dt, 1, CLK)
        info["top_gpu_pct"] = top(a, b, dt, 2, 1e9)
        info["cpu_pct"] = {g: round(100 * (b["cpu"][g] - a["cpu"][g]) / CLK / dt, 2) for g in groups}
        info["gpu_pct"] = {g: round(100 * (b["gpu"][g] - a["gpu"][g]) / 1e9 / dt, 3) for g in groups}
        if a["rc6"] is not None:
            info["igpu_busy_pct"] = round(100 * (1 - (b["rc6"] - a["rc6"]) / 1000 / dt), 2)
        info["watts"] = {}
        for k in RAPL:
            if a["rapl"][k] is None:
                continue
            d = b["rapl"][k] - a["rapl"][k]
            if d < 0:
                d += rapl_max(k)
            info["watts"][k] = round(d / 1e6 / dt, 3)
    finally:
        if use_tb:
            r = sh(TB, "kill", session, env=env)
            info["tb_kill"] = [r.returncode, r.stderr.strip()[-200:]]
            server = server or pgrep("-f", f"{TB} __server {session}")
        if p:
            for sig in (signal.SIGTERM, signal.SIGKILL):
                try:
                    os.killpg(p.pid, sig)
                except ProcessLookupError:
                    break
                try:
                    p.wait(5)
                    break
                except subprocess.TimeoutExpired:
                    pass
        # wait for the tb server to exit so it can't leak into the next run, then kill any leftovers
        for _ in range(50):
            if not any(os.path.exists(f"/proc/{q}") for q in server):
                break
            time.sleep(0.1)
        else:
            info["tb_server_killed"] = True
        for q in server + pgrep("-f", f"python3 -I {tbdirs[2]}"):
            try:
                os.kill(q, signal.SIGKILL)
            except ProcessLookupError:
                pass
    return info


RUNS = [("baseline", None, False, "none")]
for wl in ("idle", "log"):
    for term in ("terminator", "ghostty"):
        for use_tb in (False, True):
            RUNS.append((f"{term}{'+tb' if use_tb else ''}:{wl}", term, use_tb, wl))
if ONLY:
    RUNS = [r for r in RUNS if r[0] in ONLY]


def main():
    d = tempfile.mkdtemp(prefix="tbpw.", dir="/tmp")  # short path: socket paths must stay under ~100 bytes
    os.makedirs(f"{d}/s")
    gen = f"{d}/gen.py"
    open(gen, "w").write(GEN)
    tbdirs = (f"{d}/s", f"{d}/state", gen)
    meta = {"meta": True, "tb": sh(TB, "--version").stdout.strip(),
            "terminator": sh("terminator", "--version").stdout.strip(),
            "ghostty": sh("ghostty", "--version").stdout.splitlines()[0],
            "kernel": os.uname().release, "settle": SETTLE, "measure": MEASURE, "rounds": ROUNDS,
            "profile": sh("powerprofilesctl", "get").stdout.strip(),
            "ac": open("/sys/class/power_supply/AC0/online").read().strip(),
            "battery": open("/sys/class/power_supply/BAT0/capacity").read().strip(),
            "rapl_readable": rapl()["pkg"] is not None}
    meta["start_delay"] = START_DELAY
    time.sleep(START_DELAY)
    with open(OUT, "a") as f:
        f.write(json.dumps(meta) + "\n")
        print(json.dumps(meta), flush=True)
        for rnd in range(ROUNDS):
            k = (rnd * 3) % len(RUNS)
            for name, term, use_tb, wl in RUNS[k:] + RUNS[:k]:
                info = run_one(name, term, use_tb, wl, tbdirs, rnd)
                f.write(json.dumps(info) + "\n")
                f.flush()
                print(json.dumps(info), flush=True)
    shutil.rmtree(d, ignore_errors=True)


main()
