# Finding: how long one `tb action` call takes (2026-11-07)

## Question

The [preview action plan](../plans/2026-11-07-preview-action.md) lets yazi drive `tb`'s preview by calling
`tb action preview FILE` each time the hovered file changes. Holding `j` in yazi changes it about 30 times a
second (**estimate**, from the usual key-repeat rate). We needed to know whether starting one `tb` process per
hover keeps up, or whether `tb` needs a long-running mode that reads paths from stdin.

## Method

Dev machine: 12th Gen Intel Core i7-12650H (16 CPUs), Ubuntu, Linux 6.8.0-147, governor `powersave`. Load
average about 2.0. `target/release/tb` built on 2026-11-05 (`tb 0.1.0 (extension API 1)`).

This ran in a throwaway session, with its own socket and state folders:

```sh
D=$(mktemp -d /tmp/tbm.XXXX); export TB_SOCKET_DIR=$D/s TB_STATE_DIR=$D/state; mkdir -p $D/s
unset TB_SESSION TB_PANE_ID
tb start tftest-latency
python3 -I - "$(command -v tb)" <<'EOF'
import subprocess, sys, time, statistics
tb = sys.argv[1]
for args in (["--version"], ["action", "-s", "tftest-latency", "current-tab-info"],
             ["action", "-s", "tftest-latency", "rename-tab", "x"]):
    t = []
    for _ in range(50):
        s = time.perf_counter(); r = subprocess.run([tb, *args], capture_output=True)
        t.append((time.perf_counter() - s) * 1000)
    t.sort()
    print(f"{' '.join(args)}: median {statistics.median(t):.2f} ms, p90 {t[44]:.2f} ms, max {t[-1]:.2f} ms")
EOF
tb kill tftest-latency; rm -rf $D
```

## Results

```
--version: median 2.15 ms, p90 2.82 ms, max 3.01 ms, exit 0
action -s tftest-latency current-tab-info: median 3.08 ms, p90 3.89 ms, max 13.12 ms, exit 0
action -s tftest-latency rename-tab x: median 2.67 ms, p90 3.03 ms, max 3.45 ms, exit 0
```

## Conclusions

- **A `tb action` call takes about 3 ms,** start to finish, and 2 ms of that is starting the program. At 30
  hovers a second, that's under 10% of one core.
- **One call per hover is enough.** `tb action preview` doesn't need a stdin mode.
- **Calls must run one after another,** not as separate background processes. Two calls started together can
  arrive in either order, and the preview would end up on the older file. The yazi glue reads hover events in a
  loop and calls `tb` for each in turn.
- **Not measured:** the time for the server to load the preview after the call returns. That's the finder's
  existing preview path, and the same as moving through the finder's own list.
