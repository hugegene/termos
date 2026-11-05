# Finding: `prbot` on `tb` (2026-11-05)

## Question

Step 6's acceptance test ([CLI extension plan](../plans/2026-11-05-cli-extensions.md)): does a copy of `prbot`
(`~/claudehook/prbot.py`) work on `tb`, with only `zellij` changed to `tb` and the variable names renamed?
Its tab functions must open, find, rename and close its tabs.

## Method

On the dev machine, on 2026-11-05, with `tb` built from the step 6 work:

1. **The copy:** `~/claudehook/prbot.py` was copied to the scratchpad, with three `sed` replacements:
   - `"zellij"` → `"tb"`
   - `ZELLIJ_PANE_ID` → `TB_PANE_ID`
   - `ZELLIJ_SESSION_NAME` → `TB_SESSION`

   `diff` was used to count the changed lines.
2. **Its config** (`config.env`, beside the copy): `REPO_DIRS=demo=<a scratch folder with .git>` and
   `CLOSE_GRACE_SECS=1`.
3. **No Claude:** `bin/run-claude` became a stub that writes its arguments to a file and sleeps.
4. **The harness** was a Python script that imports the copy and calls its own functions:
   - `ensure_session()`
   - `spawn({"kind": "review", "repo": "demo", "number": 487, "extra": []})`
   - `find_tab`, `list_tabs`
   - `own_tab_id()`, with `TB_PANE_ID`/`TB_SESSION` set as in that tab's pane
   - `cmd_done()`, which renames the tab and starts `close-later`

   It ran with `PATH` holding only `tb` (no `zellij`), `HOME` in the scratchpad, and a throwaway socket folder
   (`TB_SOCKET_DIR=/tmp/tbt-prbot`). Nothing called GitHub.
5. **zellij's session-list format**, read before the run: `zellij list-sessions -n`, with names hidden.

## Results

The copy differs from the original in 7 lines:

```
return subprocess.run(["tb", "-s", session or SESSION, "action", *args], capture_output=True, text=True)
r = subprocess.run(["tb", "list-sessions", "-n"], capture_output=True, text=True)
subprocess.run(["tb", "delete-session", SESSION], capture_output=True)
subprocess.run(["tb", "attach", "--create-background", SESSION], capture_output=True, env=clean_env())
pane = os.environ.get("TB_PANE_ID")
if not pane or os.environ.get("TB_SESSION") != SESSION:
return os.path.join(CLOSING, re.sub(r"\D", "", os.environ.get("TB_PANE_ID", "")) or "none")
```

zellij's session list:

```
NAME [Created 3days 4h 51m 2s ago] (current)
NAME [Created 18h 2m 39s ago]
```

The first run found an incompatibility.
- **The problem:** `prbot`'s `session_running()` takes the first word of each line as the name and looks
  for `EXITED`, but `tb ls` printed `prbot: 1 tabs, 0 attached` (the name with a colon) and
  `(exited, attach to restore)`.
- **The fix:** `tb ls` now prints `NAME [1 tabs, 0 attached]` and `NAME (EXITED - attach to restore)`.

Then the harness printed:

```
ensure_session: session prbot running
spawn: opened tab rev-487 id 2
spawn: focus went back to tab 1
the tab runs: run-claude review demo 487 in <scratch>/prbot/repo
own_tab_id: tab 2 from pane 2
Closing this tab in 1s. Type anything to keep it open.
cmd_done: renamed to ✓ rev-487
close-later: tab closed
prbot_tabs now: []
ACCEPTANCE OK
```

## Conclusions

- **Step 6's acceptance test passes.** `prbot` drives `tb` with only the program and variable names changed.
- **zellij's text output is part of the API,** not just its JSON. Scripts parse `list-sessions` lines, so
  `tb ls` follows zellij's shape.
- **Not tested here:** `prbot`'s polling of GitHub and its Claude tabs, which don't depend on the
  multiplexer, and zellij's exact `EXITED` wording, since no exited zellij session existed to look at.
