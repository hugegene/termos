# Finding: CPU, GPU and power of Terminator vs Ghostty, with and without tb (2026-11-09)

## Question

How much CPU, GPU and battery does each terminal cost on this laptop, and how much does running `tb attach`
inside it add? This decides which terminal to run tb in on battery. It also decides whether tb's idle wake-ups
need work: the server wakes every 500 ms (`REFRESH`, `crates/tb/src/server/mod.rs:82`), and the attach client
polls every 50 ms (`crates/tb/src/client.rs:572`).

## Method

**Machine:** 12th Gen Intel Core i7-12650H (16 CPUs) with Intel UHD (Alder Lake-P GT1) and an RTX 4070
Laptop GPU. PRIME is `on-demand`: the terminals render on the iGPU, and the NVIDIA card stayed
runtime-suspended in every run. The desktop is GNOME on X11 (rootless Xorg) on the built-in 2048×1152 60 Hz
panel. Linux 6.8.0-149, governor `powersave`, EPP `balance_performance`, power profile `balanced`. The laptop
was **on AC and charging**, so power comes from RAPL rather than battery drain.

**Versions:** Terminator 2.1.3 (GTK3/VTE, its config sets only colours), Ghostty 1.3.1 (GTK4/OpenGL, no
config), and `target/release/tb` (`tb 0.1.0 (extension API 1)`) built from `558849a`.

**Setups:** each run opens one fresh 1200×800 px window at +100+100 and focuses it:

```
 baseline      no test window
 terminator    terminator -u (own process, not the D-Bus instance)  →  bash
 terminator+tb terminator -u -x tb attach S                          →  tb client ─unix socket─ tb server → bash
 ghostty       ghostty --gtk-single-instance=false (own process)      →  bash
 ghostty+tb    ghostty --gtk-single-instance=false -e tb attach S     →  tb client ─unix socket─ tb server → bash
```

At this size, Terminator's grid is 35×118 and Ghostty's is 29×107 (default fonts).

**Workloads:**

- **idle:** a shell prompt with a blinking cursor. The window is focused and nothing is typed.
- **log:** a Python script prints 50 lines of about 110 characters a second (a log tail). Plain terminals
  run it as the window's command. Under tb, it's typed into tb's shell pane with
  `tb action write-chars` once the prompt shows.

**Counters:** for each run, the script waits 10 s to settle, then reads counters at the start and end of
45 s:

- **CPU:** `utime + stime` from `/proc/PID/stat`, covering all threads, for:
  - the terminal process
  - the tb client and tb server
  - the shell and the workload, kept separate
  - Xorg and gnome-shell
- **iGPU:** the i915 per-client engine time (`drm-engine-*` in `/proc/PID/fdinfo`) for the same processes, plus
  whole-iGPU busy time (1 − rc6 residency, `/sys/class/drm/card1/gt/gt0/rc6_residency_ms`).
- **Power:** RAPL `energy_uj` for `package-0` (CPU and iGPU), `uncore` (the iGPU) and `psys`. This needed
  `sudo chmod a+r /sys/class/powercap/intel-rapl:*/energy_uj`, which resets on reboot.
  - `psys` read *lower* than `package-0` (3.7 W vs 6.9 W in a 2 s check), so it's probably not calibrated
    on this laptop. Package power is the figure used.

**Order:** 3 rounds of all 9 runs (baseline plus 4 setups × 2 workloads). The order rotates each round, and
every round has its own baseline. Desktop (Xorg + gnome-shell), iGPU-busy and watt figures are the difference
from that round's baseline. Tables show the median over rounds with the min..max range.

**Avoided:**

- `nvidia-smi`, because it wakes the dGPU from runtime suspend. A smoke test showed it going from P8 to P0
  mid-run. The script reads `/sys/bus/pci/devices/0000:01:00.0/power/runtime_status` instead.
- The user's tb sessions. tb runs use their own `TB_SOCKET_DIR` and `TB_STATE_DIR`, and a fresh session name
  each run, because a reused name restores the killed session's layout.
- Screen blanking. The whole run sits under `gnome-session-inhibit --inhibit idle`.

```sh
sudo chmod a+r /sys/class/powercap/intel-rapl:*/energy_uj
gnome-session-inhibit --inhibit idle --reason "terminal power measurement" \
  python3 -I docs/findings/scripts/2026-11-09-terminal_power.py target/release/tb \
  docs/findings/data/2026-11-09-terminal-power/runs.jsonl
python3 -I docs/findings/scripts/2026-11-09-terminal_power_summary.py \
  docs/findings/data/2026-11-09-terminal-power/runs.jsonl
```

Raw per-run JSON is in [data/2026-11-09-terminal-power/runs.jsonl](data/2026-11-09-terminal-power/runs.jsonl).

## Results

**Units:**

- **CPU %:** percent of one CPU core. The machine has 16.
- **iGPU %:** engine busy time.
- **(Δ):** the difference from the same round's baseline.
- **Battery %/h:** an **estimate**, the package Δ watts divided by the battery's 41 Wh (charge_full 2.7 Ah ×
  15.2 V). The battery itself wasn't measured.

The run started on AC with the battery at 38%. No runs were flagged: every window was focused, every log
workload ran, the dGPU stayed `suspended`, and every tb server exited on `tb kill`.

```
Baselines: {0: {'pkg_w': 6.285, 'psys_w': 3.589, 'igpu_busy': 7.14, 'desk_cpu': 5.1},
            1: {'pkg_w': 5.473, 'psys_w': 2.977, 'igpu_busy': 0.36, 'desk_cpu': 0.4},
            2: {'pkg_w': 5.985, 'psys_w': 3.1, 'igpu_busy': 0.34, 'desk_cpu': 0.3}}
```

Median over 3 rounds (min..max):

| setup | terminal CPU % | tb CPU % | Xorg+shell CPU % (Δ) | total CPU % | terminal iGPU % | Xorg+shell iGPU % (Δ) | iGPU busy % (Δ) | package W (Δ) | iGPU W (Δ) | psys W (Δ) | battery %/h (est.) |
|---|---|---|---|---|---|---|---|---|---|---|---|
| terminator:idle | 0.0 (0.0..0.0) | 0.0 (0.0..0.0) | -0.1 (-0.2..3.3) | -0.1 (-0.2..3.3) | 0.0 (0.0..0.0) | -0.1 (-0.2..0.9) | -0.3 (-0.3..0.6) | -0.09 (-0.14..-0.04) | -0.00 (-0.00..0.01) | -0.03 (-0.04..-0.02) | -0.2 |
| terminator+tb:idle | 0.0 (0.0..0.0) | 0.3 (0.3..0.3) | -0.2 (-2.7..-0.1) | 0.2 (-2.4..0.2) | 0.0 (0.0..0.0) | -0.2 (-2.9..-0.1) | -0.3 (-5.3..-0.2) | -0.03 (-0.93..0.48) | -0.00 (-0.08..-0.00) | -0.05 (-0.20..0.18) | -0.1 |
| ghostty:idle | 0.5 (0.4..0.5) | 0.0 (0.0..0.0) | 0.4 (-4.2..0.4) | 0.9 (-3.7..0.9) | 0.4 (0.4..0.4) | -0.1 (-3.8..0.0) | 0.9 (-5.8..1.0) | -0.10 (-0.92..0.07) | 0.01 (-0.10..0.01) | -0.06 (-0.22..0.29) | -0.3 |
| ghostty+tb:idle | 0.5 (0.5..0.6) | 0.3 (0.2..0.3) | 0.4 (-3.9..0.5) | 1.2 (-3.0..1.2) | 0.4 (0.4..0.6) | -0.0 (-3.7..0.0) | 1.0 (-5.2..1.0) | -0.05 (-0.89..0.07) | 0.01 (-0.09..0.01) | -0.08 (-0.24..0.10) | -0.1 |
| terminator:log | 15.4 (14.9..16.4) | 0.0 (0.0..0.0) | 16.9 (13.7..17.0) | 32.4 (28.6..33.5) | 0.0 (0.0..0.0) | 15.8 (13.4..16.1) | 29.2 (23.5..29.4) | 0.79 (0.31..0.85) | 0.30 (0.22..0.30) | 0.06 (-0.02..0.24) | 1.9 |
| terminator+tb:log | 20.5 (20.4..21.3) | 5.0 (5.0..5.2) | 18.1 (13.6..18.3) | 43.7 (39.3..44.5) | 0.0 (0.0..0.0) | 16.2 (12.9..16.7) | 29.4 (23.1..29.9) | 0.94 (0.14..0.97) | 0.30 (0.20..0.30) | 0.24 (-0.06..0.25) | 2.3 |
| ghostty:log | 21.4 (19.9..21.4) | 0.0 (0.0..0.0) | 23.8 (19.8..23.8) | 45.2 (39.8..45.2) | 14.3 (14.2..14.3) | 6.2 (6.2..6.6) | 51.0 (48.9..51.6) | 0.76 (0.51..0.76) | 0.33 (0.29..0.34) | 0.18 (-0.01..0.19) | 1.9 |
| ghostty+tb:log | 17.4 (17.1..17.6) | 5.0 (5.0..5.1) | 18.2 (14.7..18.4) | 40.4 (37.3..40.7) | 16.6 (16.1..16.6) | 4.9 (4.7..5.0) | 54.5 (54.4..57.2) | 0.63 (0.03..0.75) | 0.36 (0.31..0.36) | 0.15 (-0.11..0.16) | 1.5 |

The round-0 baseline caught other on-screen activity (desktop at 5.1% CPU, compared with 0.3–0.4% later).
That pulls round 0's deltas down and accounts for most of the wide min..max ranges.

**Idle iGPU per process** (busy = 1 − rc6; *other* = busy minus the engine time of the processes listed):

```
1 baseline             igpu_busy  0.36  gpu: xorg 0.024 gshell 0.154 term 0.000  other 0.18
1 terminator:idle      igpu_busy  0.09  gpu: xorg 0.018 gshell 0.033 term 0.000  other 0.04
1 ghostty:idle         igpu_busy  1.26  gpu: xorg 0.055 gshell 0.066 term 0.426  other 0.71
2 baseline             igpu_busy  0.34  gpu: xorg 0.024 gshell 0.147 term 0.000  other 0.17
2 terminator:idle      igpu_busy  0.03  gpu: xorg 0.000 gshell 0.015 term 0.000  other 0.01
2 ghostty:idle         igpu_busy  1.36  gpu: xorg 0.068 gshell 0.106 term 0.411  other 0.77
```

Two things here explain the table's idle iGPU figures:

- **Negative idle deltas:** the desktop drew *less* with an idle Terminator window focused than with no test
  window at all. gnome-shell went from 0.15% to 0.02–0.03%. The likely cause is something else on screen being
  covered or losing focus; the baseline gives focus back to the user's own window. Other GPU clients weren't
  recorded, so this isn't pinned down.
- **Ghostty's "other" (about 0.75%):** this appears in every round, though only 0.42% is Ghostty's own engine
  time. It's most likely the GPU staying out of rc6 for a short while after each blink frame. That counts as
  busy without being charged to any process.

**Re-measured:** [2026-11-09-terminal-power-idle-rerun.md](2026-11-09-terminal-power-idle-rerun.md). With
nothing else drawing, Terminator idle equals baseline. The extra activity was the user's own Terminator
window running the zellij session `prbot` (Claude Code ×4, yazi).

**Per-run, in run order** (package, iGPU, desktop CPU, whole-iGPU busy). Package power drifts by about
0.6 W over the half hour, whatever is on screen:

```
0 baseline             pkg 6.285 unc 0.109 desk 5.1  busy 7.14
0 terminator:idle      pkg 6.198 unc 0.116 desk 8.4  busy 7.72
0 terminator+tb:idle   pkg 5.359 unc 0.024 desk 2.4  busy 1.79
0 ghostty:idle         pkg 5.367 unc 0.011 desk 0.9  busy 1.39
0 ghostty+tb:idle      pkg 5.399 unc 0.015 desk 1.1  busy 1.96
0 terminator:log       pkg 6.598 unc 0.328 desk 18.8 busy 30.66
0 terminator+tb:log    pkg 6.429 unc 0.306 desk 18.7 busy 30.2
0 ghostty:log          pkg 6.797 unc 0.399 desk 24.9 busy 56.03
0 ghostty+tb:log       pkg 6.316 unc 0.418 desk 19.8 busy 61.51
1 ghostty:idle         pkg 5.37  unc 0.011 desk 0.8  busy 1.26
1 ghostty+tb:idle      pkg 5.422 unc 0.011 desk 0.8  busy 1.32
1 terminator:log       pkg 6.262 unc 0.307 desk 17.4 busy 29.52
1 terminator+tb:log    pkg 6.408 unc 0.301 desk 18.5 busy 30.3
1 ghostty:log          pkg 6.234 unc 0.337 desk 24.2 busy 51.4
1 ghostty+tb:log       pkg 6.101 unc 0.364 desk 18.5 busy 54.85
1 baseline             pkg 5.473 unc 0.004 desk 0.4  busy 0.36
1 terminator:idle      pkg 5.332 unc 0.002 desk 0.2  busy 0.09
1 terminator+tb:idle   pkg 5.954 unc 0.001 desk 0.2  busy 0.11
2 terminator+tb:log    pkg 6.96  unc 0.301 desk 18.6 busy 29.71
2 ghostty:log          pkg 6.75  unc 0.345 desk 24.1 busy 51.91
2 ghostty+tb:log       pkg 6.73  unc 0.365 desk 18.7 busy 57.55
2 baseline             pkg 5.985 unc 0.003 desk 0.3  busy 0.34
2 terminator:idle      pkg 5.945 unc 0.0   desk 0.2  busy 0.03
2 terminator+tb:idle   pkg 5.958 unc 0.0   desk 0.2  busy 0.03
2 ghostty:idle         pkg 6.052 unc 0.012 desk 0.8  busy 1.36
2 ghostty+tb:idle      pkg 6.053 unc 0.012 desk 0.8  busy 1.37
2 terminator:log       pkg 6.836 unc 0.3   desk 17.2 busy 29.73
```

**tb client vs server, and Xorg vs gnome-shell** (CPU %, rounds 0, 1, 2):

```
terminator+tb:idle   client [0.11, 0.13, 0.09] server [0.16, 0.2, 0.2]   xorg [1.53, 0.02, 0.0]     gshell [0.89, 0.18, 0.22]
ghostty+tb:idle      client [0.16, 0.11, 0.11] server [0.18, 0.16, 0.13] xorg [0.44, 0.29, 0.27]    gshell [0.71, 0.49, 0.56]
terminator+tb:log    client [0.47, 0.47, 0.44] server [4.69, 4.58, 4.55] xorg [10.73, 10.6, 10.64]  gshell [7.95, 7.91, 7.95]
ghostty+tb:log       client [0.51, 0.51, 0.51] server [4.49, 4.55, 4.49] xorg [8.42, 7.57, 7.57]    gshell [11.4, 10.97, 11.13]
terminator:log                                                           xorg [10.04, 8.55, 8.51]   gshell [8.8, 8.84, 8.73]
ghostty:log                                                              xorg [12.17, 10.89, 10.88] gshell [12.75, 13.31, 13.24]
```

The log generator itself used 0.6–0.7% in every setup.

```
$ gsettings get org.gnome.desktop.interface cursor-blink-timeout
10
```

## Conclusions

**Worked example:** one line of the log in Ghostty + tb, with each hop's cost at 50 lines a second:

```
 python gen.py
   │ 1. writes "00001234 21:14:05 INFO worker-3 …\n" (0.7% of a core)
   ▼
 pty ──▶ tb server ─────────────── parses it into the pane's grid, builds a frame        4.5% CPU
   │ 2. frame diff over the unix socket
   ▼
 tb client ─────────────────────── copies it to its terminal                            0.5% CPU
   │ 3. escape sequences through the outer pty
   ▼
 Ghostty ───────────────────────── parses, then draws with OpenGL                      17.4% CPU, 16.6% iGPU
   │ 4. a new window frame
   ▼
 Xorg (7.6%) ──▶ gnome-shell composites (11%) ──▶ panel                                 18.2% CPU, 4.9% iGPU (Δ)
                                                              package power: +0.63 W over baseline
```

**Idle costs almost nothing in all four setups.**

- **Total:** at most 1.2% of one core, and about 1% of the iGPU's time.
- **Watts:** too small to see, because package power drifts by about 0.6 W. Idle runs taken next to each
  other are within about 0.1 W. The one jump, 5.33 → 5.95 W in round 1, came between two idle Terminator
  runs and held for the rest of the run whatever was on screen, so it's drift and not tb.
- **Battery:** under 0.25%/h (**estimate**).
- **Terminator:** it read exactly 0% because GTK stops the cursor blinking 10 s after the last input
  (`cursor-blink-timeout`). The settle took longer than that.
- **Ghostty:** it keeps blinking, which costs 0.5% CPU and 0.4% iGPU (about 0.01 W, from `uncore`).

**tb's idle wake-ups cost about 0.3% of a core,** about 0.1% for the client's 50 ms poll and 0.2% for the
server. It's the same in both terminals and doesn't show in watts, so battery life gives no reason to change
them.

**Steady output costs about +0.6 to 1.0 W in every setup** (1.5–2.4% of the battery an hour,
**estimate**). The four setups differ by about 0.3 W at most, which is about the drift. So battery life
gives no reason to prefer one terminal over the other.

**The two terminals split the work differently:**

- **Terminator** draws on the CPU (VTE with Cairo) and does no GPU work itself. The compositor's GPU work
  still keeps the iGPU about 29% busy.
- **Ghostty** draws on the iGPU (14% of its time) and pushes the iGPU to about 51% busy. It also costs more
  Xorg + gnome-shell CPU (24% vs 17%). The iGPU's extra power is small, 0.33 W vs 0.30 W.
- **Xorg and gnome-shell** cost as much CPU as the terminal itself in every setup. That's the X11 compositor
  path, not the terminal.

**Under output, tb costs about 5% of a core** (server 4.5%, client 0.5%). Its effect on the outer terminal
depends on which terminal it is:

- **Terminator + tb:** VTE does more work, up from 15% to 21%, for about 11% of a core more in total.
- **Ghostty + tb:** Ghostty does less CPU work (21% → 17%) and the desktop does less (24% → 18%). That's
  about 5% of a core *less* in total, though the iGPU is slightly busier.

**Terminator + tb vs Ghostty + tb under output:** the table's +0.94 vs +0.63 W gap (0.31 W) is mostly baseline
drift. Comparing the two runs within each round, split by RAPL domain (*rest* = package − cores − iGPU:
memory controller, cache, display engine):

```
terminator+tb:log minus ghostty+tb:log
  round 0 (run #6 vs #8):   pkg +0.113  cores +0.060  igpu -0.112  rest +0.165
  round 1 (run #12 vs #14): pkg +0.307  cores +0.145  igpu -0.063  rest +0.225
  round 2 (run #18 vs #20): pkg +0.230  cores +0.076  igpu -0.064  rest +0.218
  median:                   pkg +0.230  cores +0.076  igpu -0.064  rest +0.218
terminator:log minus ghostty:log
  median:                   pkg +0.028  cores -0.024  igpu -0.045  rest +0.072
```

- **Size:** the real gap is about 0.23 W.
- **Cores (+0.08 W):** Terminator + tb uses about 3% more of a core in total. VTE goes from 15% to 21% under
  tb, while Ghostty goes from 21% to 17%.
- **iGPU (−0.06 W):** the iGPU goes the other way, because Ghostty draws on it.
- **Rest of the chip (+0.22 W):** most of the gap, and already +0.07 W without tb. The likely reason is that
  VTE draws glyph pixels on the CPU and Xorg copies the images to the GPU, while Ghostty sends cell data and
  draws from a glyph atlas on the GPU. This explanation is **not measured**: memory-traffic and package
  C-state counters need root.
- **Caveat:** Terminator + tb ran about 2 minutes before Ghostty + tb in every round, because the rotation
  kept their order. The gap had the same sign in all 3 rounds, but drift isn't ruled out. A re-run should
  alternate the two.

**Unknown:**

- **Why tb changes the outer terminal's cost.** It's probably the shape of what tb writes per frame (whole
  rows redrawn instead of a plain newline scroll), but bytes per frame weren't measured.
- **Real battery drain.** The laptop was on AC, and the power profile and EPP may differ on battery.
- **Other workloads:** typing, a full-screen TUI redrawing (htop, Claude Code's spinner), and moving
  through the finder.
- **Wayland.**
