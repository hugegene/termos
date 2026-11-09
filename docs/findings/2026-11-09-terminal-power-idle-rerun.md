# Finding: re-run of baseline vs Terminator idle (2026-11-09)

## Question

In [the first run](2026-11-09-terminal-power.md), round 0's baseline and Terminator-idle runs caught extra
on-screen activity (iGPU busy 7.1% compared with 0.35% later). Terminator idle also came out *below* baseline
(−0.3% iGPU busy). Is an idle Terminator really free, and what was the extra activity? This decides whether
the idle row of the first finding can be trusted.

## Method

Same machine, versions and script as the first run:
[scripts/2026-11-09-terminal_power.py](scripts/2026-11-09-terminal_power.py). Two options were added for this
run, and both default to the old behaviour:

- **`START_DELAY`:** quiet time before the first run, so the chat window that started it has finished
  drawing.
- **`top_cpu_pct` and `top_gpu_pct`:** the 6 busiest processes on the whole machine during each run, read from
  every process's `/proc/PID/stat` and i915 `fdinfo`.

Only the two runs are repeated, 3 rounds. The order alternates (B,T / T,B / B,T), so every Terminator run
has a baseline right next to it. The laptop was on AC, and the battery had charged to 96%.

```sh
START_DELAY=30 ONLY=baseline,terminator:idle ROUNDS=3 \
  gnome-session-inhibit --inhibit idle --reason "terminal power re-run" \
  python3 -I docs/findings/scripts/2026-11-09-terminal_power.py target/release/tb \
  docs/findings/data/2026-11-09-terminal-power/rerun-idle.jsonl
```

Raw data: [data/2026-11-09-terminal-power/rerun-idle.jsonl](data/2026-11-09-terminal-power/rerun-idle.jsonl).

## Results

In run order. *desk* is Xorg + gnome-shell CPU %, and *busy* is whole-iGPU busy %:

```
0 baseline         pkg 5.658 core 1.055 unc 0.033 busy 2.94 desk 2.16  gpu xorg 1.073 gshell 0.874
    top cpu: kube-apiserver 7.1, zellij(10636) 6.8, etcd 4.0, kubelet 3.8, kube-controller 3.1, kubelet 3.0
0 terminator:idle  pkg 5.450 core 0.892 unc 0.016 busy 1.56 desk 1.31  gpu xorg 0.320 gshell 0.682 term 0.000
    top cpu: kube-apiserver 7.5, etcd 4.0, kubelet 3.8, kube-controller 3.3, kubelet 3.2, kubelet 3.0
1 terminator:idle  pkg 5.296 core 0.763 unc 0.000 busy 0.04 desk 0.16  gpu xorg 0.000 gshell 0.023 term 0.000
    top cpu: kube-apiserver 7.3, kubelet 4.2, etcd 4.2, kube-controller 3.2, kubelet 3.2, kubelet 3.2
1 baseline         pkg 5.318 core 0.788 unc 0.000 busy 0.03 desk 0.20  gpu xorg 0.001 gshell 0.012
    top cpu: kube-apiserver 7.6, etcd 4.1, kubelet 3.9, kube-controller 3.4, kubelet 3.2, kubelet 3.0
2 baseline         pkg 5.361 core 0.840 unc 0.001 busy 0.07 desk 0.18  gpu xorg 0.007 gshell 0.025
    top cpu: kube-apiserver 8.1, etcd 4.2, kubelet 4.1, kube-controller 3.3, kubelet 3.3, kubelet 3.1
2 terminator:idle  pkg 6.040 core 1.326 unc 0.084 busy 6.73 desk 5.46  gpu xorg 1.929 gshell 2.391 term 0.000
    top cpu: zellij(10636) 10.5, kube-apiserver 6.7, terminator(10540) 5.0, kubelet 3.8, etcd 3.7, Xorg 3.3
```

`zellij(10636)` is the user's own session, and `terminator(10540)` is the user's own window that shows it:

```
$ ps -o pid,args -p 10636,10540
  10540 /usr/bin/python3 /usr/bin/terminator
  10636 /snap/zellij/65/bin/zellij --server /run/user/1000/zellij/contract_version_1/prbot
$ pstree -p 10636   (threads left out)
zellij(10636)-+-bash ×2, python3(10708)
              |-claude(10863)   ← the Claude Code session running this measurement
              |-bash(16411)---yazi(16441)
claude 10737, 11075, 11211: parent zellij(10636)
```

A local Kubernetes cluster (kube-apiserver, etcd, kubelet) uses about 25% of a core in every run, steadily.

## Conclusions

**An idle Terminator costs nothing measurable.** Compare the runs where the user's window was quiet (round 1
both, round 2 baseline):

| | package W | iGPU busy % | Xorg + gnome-shell CPU % |
|---|---|---|---|
| baseline (r1, r2) | 5.318, 5.361 | 0.03, 0.07 | 0.20, 0.18 |
| terminator:idle (r1) | 5.296 | 0.04 | 0.16 |

**Ghostty idle against this clean baseline is about +1.3 points of iGPU busy.** From the first run's Ghostty
idle rows:

- **iGPU busy:** 1.39%, 1.26%, 1.36%, against 0.03–0.07% here.
- **Ghostty's own engine time:** 0.42%, 0.43%, 0.41% for the endless cursor blink. The rest is the GPU
  staying out of rc6 after each frame.
- **Not hit by the user's window:** those rows have Xorg + gnome-shell GPU at only 0.06–0.10%.
- **Power:** about +0.01 W of iGPU (uncore 0.011–0.012 W, against 0.000–0.002 W for Terminator).

It's positive, not negative. The earlier chat table wrote it as "~1%".

**The extra activity was the user's own terminal.** Two of the six runs caught it: round 0's baseline and
round 2's Terminator idle. In both, the user's zellij session and its Terminator window were redrawing.
That session holds four Claude Code instances (one of them running this measurement) and yazi. Their frames
go through Xorg and gnome-shell, just like the test window's:

```
 user's Terminator(10540) ← zellij "prbot"(10636) ← claude ×4, yazi
        │ 1. frames whenever a pane changes
        ▼
 Xorg ──▶ gnome-shell composites ──▶ panel        ← test window's frames take the same path
```

**Size:** when it's active, it adds about +0.3 to 0.7 W package and 3–7% iGPU busy (round 0 baseline: +0.34
W, round 2 Terminator idle: +0.72 W, each against the quiet runs). That's as large as the 50-lines-a-second
log workload in the first run.

**What this changes in [the first finding](2026-11-09-terminal-power.md):**

- **Terminator idle's −0.3% iGPU** was the user's window being busier during those baselines. It doesn't
  mean Terminator lowered anything. The real idle delta is 0.
- **The idle conclusions stand.** All four setups are near zero at idle, and the watts can't separate them.
- **The log rows** were measured while the same window could add up to about 0.7 W at random. Their watt
  differences of 0.2–0.3 W between setups are within that.

**For any re-run:**

- Hide or quiet the user's zellij/Claude Code window, for example by running the script from a terminal on
  another workspace or over ssh.
- Or drop runs whose `top_cpu_pct` contains `zellij(…)` or the user's terminal.
