# Finding: prbot on `tos` and on zellij, from one script (2026-11-10)

## Question

The [prbot plan](../plans/2026-11-10-prbot-extension.md) keeps prbot, the GitHub poller, working on both `tos`
and zellij, chosen by `MUX` in its `config.env`. Two things depend on that:

1. **Can one script drive both?** It would send the same commands to each, with only the program name and the
   pane variables changed. The [2026-11-05 finding](2026-11-05-prbot-on-tb.md) showed this for `prbot.py`'s
   functions on `tb`. It didn't cover `bin/start` and `bin/stop`, zellij's wording for an exited session, or
   `tos` after its rename from `tb`.
2. **Is the macOS fix for the process check sound?** On macOS, cleanup checks that nothing is running in a
   worktree with `lsof` instead of `/proc`. Does `lsof` find the same processes, and how long does it take?

## Method

On the dev machine on 2026-11-10:

- **Machine:** Ubuntu 24.04.5 LTS, Linux 6.8.0-149-generic.
- **`tos`:** 0.3.1 (extension API 2), built from termos `f73070f`.
- **zellij:** 0.44.1 (snap, classic confinement).
- **Other tools:** lsof 4.95.0, and Python 3.12.3 and 3.9.25.

The prbot branch `tos-and-zellij` (`38d5348`, in `~/claudehook`) adds `tests/test_prbot.py`. It ran with both
Pythons:

```bash
python3 -m unittest discover -s tests -v
~/.local/bin/python3.9 -m unittest discover -s tests -v
```

The tests work on a copy of prbot in a temporary folder:

- **Isolated from your sessions and files:**
  - `HOME` is a temporary folder.
  - `tos` uses its own `TOS_SOCKET_DIR` and `TOS_STATE_DIR`; zellij uses its own `ZELLIJ_SOCKET_DIR`. The socket
    folders are under `/tmp`, to keep their paths short.
  - Each session is named `prbot-test-<pid>`.
- **Stand-ins:**
  - `gh` prints a fake token.
  - The GitHub API address points at a closed local port, so the poller keeps retrying instead of exiting.
  - `bin/run-claude` writes its pane's environment to a file and sleeps.

For each multiplexer, one test runs the round prbot does:

1. `prbot.py start` twice: the session starts with its first tab named `home` and a `poller` tab. The second run
   must say it's already running.
2. `spawn()` a review job. Tab `rev-487` opens, and the active tab stays what it was.
3. In that tab, `own_tab_id()` reads the pane's variables and must find `rev-487`.
4. `prbot.py done` renames the tab `✓ rev-487`, and it closes after `CLOSE_GRACE_SECS=1`.
5. `bin/stop` closes the poller's tab. A second `stop` says it isn't running.

The process check ran both ways, `/proc` and `lsof`, on a `sleep` started inside a temporary folder. It must be
found inside that folder and not outside it. Each way was also timed on the whole machine.

Isolation was checked separately. A zellij session started with `ZELLIJ_SOCKET_DIR=/tmp/prbt-iso` and a
temporary `HOME` must not show up in your normal `zellij list-sessions`.

## Results

Both runs:

```
== python 3.12
Ran 19 tests in 9.329s

OK
== python 3.9
Ran 19 tests in 9.540s

OK
```

The first run had two failures. Both were mistakes in the tests, not in prbot:

- **`tos`:** the stand-in quoted its output file as `'…/run-$3.env'`, so the shell never filled in `$3`. The
  tab had run.
- **zellij:** the test expected one tab to be active. Here is what each one's `list-tabs -j` says, with nobody
  attached:

  ```
  tos:    [(1, 'home', False), (2, 'poller', True),  (3, 'rev-487', False)]   (tab_id, name, active)
  zellij: [(0, 'home', False), (1, 'poller', False), (2, 'rev-487', False)]
  ```

Other differences seen:

- **The first tab's id:** 0 on zellij, 1 on `tos`. The old `bin/start` named the first tab with
  `rename-tab-by-id 0 home`, which would miss on `tos`. `prbot.py start` now names the first tab
  `list-tabs -j` returns.
- **zellij's wording for an exited session:** `NAME [Created 6days 3h 15m 2s ago] (EXITED - attach to
  resurrect)`. prbot only looks for the word `EXITED`, and `tos ls` prints `NAME (EXITED - attach to restore)`.

The isolation check:

```
in the test folder:      1
in your normal list:     0
```

The process check, on the whole machine:

```
proc: 134 processes in 0.00 s
lsof: 145 processes in 0.28 s
```

`lsof` sees a few more processes, because it runs as a program of its own and lists its parent shell. In the
test, both ways found the `sleep` inside the folder, and neither found anything outside it.

## Conclusions

- **One script drives both multiplexers.** Every command prbot runs behaves the same on `tos` 0.3.1 and zellij
  0.44.1. The differences are in the values: which tab is active, and which id the first tab has. prbot no
  longer depends on either.
- **The `lsof` way is sound on Linux,** and takes about 0.3 s. Cleanup runs it once for each worktree it's
  about to remove, so that cost is fine.
- **Still unknown:**
  - **macOS itself.** Nothing here ran on a Mac. The `osascript` notification, `lsof`'s output there, and
    macOS's `python3` (3.9 from the command-line tools, an **estimate**) are checked by hand on your Mac (plan
    step Y3, `./bin/prbot check`).
  - **Claude itself.** No test started Claude or reached GitHub; those parts didn't change.
