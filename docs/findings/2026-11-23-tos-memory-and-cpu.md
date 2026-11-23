# Does tos 0.4.9 leak memory or cause CPU bursts?

## Question

After upgrading to tos 0.4.9, the laptop seemed to "run strong every few minutes". Does tos leak memory, or
burn CPU on a timer? Whether we need a fix in tos depends on the answer.

## Method

Machine: Linux 6.8.0-149, 16 cores, 32 GB RAM, tos 0.4.9 release build (`target/release/tos`). At the time it also ran
5 Kubernetes clusters, LLM inference simulators and Chrome.

Where the work can happen:

```
  ┌─────────────┐  1. screen updates   ┌───────────────┐  2. frames (ANSI)   ┌──────────────┐
  │ programs in │ ───────────────────▶ │ tos __server  │ ──────────────────▶ │ tos attach   │
  │ panes       │      (PTY)           │ main          │    (Unix socket)    │              │
  │ claude,     │                      │ scrollback,   │                     └──────┬───────┘
  │ prbot poll, │                      │ autosave 2s   │                            │ 3. bytes to stdout
  │ shells      │                      └───────────────┘                            ▼
  └─────────────┘                                                            ┌──────────────┐
                                                                             │ ghostty      │
  ┌──────────────────────────────────────────────┐                           │ (draws them) │
  │ unrelated: kube-apiserver ×5, etcd, kubelet,  │                           └──────────────┘
  │ llm-d-inference-sim, epp, containerd, Chrome  │
  └──────────────────────────────────────────────┘
```

1. **The live server, 10 minutes:** every 2 s, read `/proc/<server pid>/status` (VmRSS, Threads), count
   `/proc/<pid>/fd`, and read utime+stime from `/proc/<pid>/stat`.
2. **The whole machine, 10 minutes:** every 5 s, read `/proc/stat` for the share of CPU that's busy, and every
   `/proc/*/stat` for the top 6 processes by CPU used in that 5 s. This catches bursts and shows which process caused them.
3. **Leak test**, on a throwaway session with its own socket and state folders:

   ```sh
   export TOS_SOCKET_DIR=/tmp/claude-1000/tl TOS_STATE_DIR=<scratch>/tos-state
   T="target/release/tos -s leak"; $T start
   # phase A and C: 100 rounds each of
   id=$($T action new-tab -- sh -c 'seq 1 200000; sleep 60'); sleep 1.2; $T action close-tab -t "$id"
   # phase B: one tab printing nonstop for 60 s
   $T action new-tab -- sh -c 'while :; do seq 1 50000; sleep 0.5; done'
   # then 20 s idle; VmRSS / Threads / fd count / CPU ticks read every 20 rounds or 5 s
   ```
4. **Terminal output**, 45 s: every second, the bytes `tos attach` wrote (`wchar` in `/proc/<pid>/io`), next to
   ghostty's and the server's CPU.
5. **CPU by program**, 20 s: the CPU each process used, summed by process name.

## Results

**1. The live `main` server** (up 1 h 34 min at the start, 6 tabs, one running Claude Code and one `prbot poll`):

```
samples=294 avg_cpu=0.6% max_cpu=2% samples>=10%=0 rss_min=42484 rss_max=42948 kB
threads 13 → 13, fds 45 → 45
```

**2. The whole machine:**

```
avg busy=10.5% max=11.9% samples>30%=0 of 120
21:41:23 busy= 11.4%  ghostty[15246]=85%  claude[1405792]=12%  gnome-shell=3%  llm-d-inference=3% ...
ghostty: in top-6 in 107 of 120 samples, avg when listed 41% of a core, >=30% in 76 samples
```

**3. Leak test:**

```
server start: rss=9464kB threads=5 fds=12
phase A  round 20: rss=29640kB  round 60: 66436kB  round 100: 65440kB   threads=5 fds=12
phase B  t=5s..60s: rss=66452kB (flat)  threads=6;  after closing: 65440kB threads=5 fds=12
phase C  round 20..100: rss=65440–66452kB  threads=5 fds=12
idle 20s: 0 CPU ticks
```

**4. Terminal output:** 0 bytes/s from `tos attach` for all 45 s (the visible tab didn't change), ghostty at
2–8% of a core. In total, `attach` had written 16,167,408 bytes in about 1 h 45 min (about 2.5 kB/s).

**5. CPU by program** (177% of one core in total, out of 16 cores):

```
 29.1% llm-d-inference   26.4% kube-apiserver   21.8% kubelet   19.1% epp   15.6% etcd
 14.8% containerd-shim    9.7% kube-controller   9.4% containerd  6.8% python   5.3% claude
  5.0% kube-scheduler     4.5% ghostty           2.1% coredns     1.4% tos
```

## Conclusions

- **No leak in tos.** Memory rises to an allocator high-water mark (about 65 MB) and stays there. Closing
  tabs frees what they used, and scrollback stops at alacritty's 10,000 lines per pane. Threads and file handles
  come back to where they started.
- **No CPU timer in tos.** An idle server uses no CPU, and the live one averaged 0.6%. Autosave writes only when
  the session changed.
- **The constant load is the Kubernetes and inference stack**, about 160% of one core around the clock (≈90% of all
  CPU use). tos is about 1.4%.
- **Ghostty's spikes (50–85% of a core) come from drawing busy output**, for example Claude Code streaming in the
  visible tab. When nothing on the visible tab changes, tos sends ghostty nothing.
- **Still unknown:** no burst above 12% busy happened in the 10-minute window, so whatever the user noticed
  "every few minutes" wasn't caught. A longer machine-wide watch would catch it.
- The 0.4.9 change (control characters drawn as blanks) adds no extra output, so it can't be the cause.
