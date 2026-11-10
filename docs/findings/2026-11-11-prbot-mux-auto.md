# Finding: starting prbot from inside a pane, for `MUX=auto` (2026-11-11)

## Question

`MUX=auto` ([prbot plan](../plans/2026-11-10-prbot-extension.md), P8) makes `./bin/start` use the multiplexer whose
pane you type it in. The plan depends on two facts that weren't known:

1. **Do `tos` and zellij start a background session and open tabs in it when run from inside one of their own
   panes?** Some multiplexers refuse to start a session from inside a session.
2. **Do the variables of the pane you typed `start` in reach the new session's tabs?** If they do, a job tab in a
   zellij session started from a `tos` pane would look like it's inside both.

## Method

On the dev machine on 2026-11-11, with the same versions as the
[2026-11-10 finding](2026-11-10-prbot-on-tos-and-zellij.md): `tos` 0.3.1 (extension API 2), zellij 0.44.1,
Ubuntu 24.04.5, and Python 3.12.3 and 3.9.25. Claude's shell runs inside a real zellij pane, so it has real
`ZELLIJ`, `ZELLIJ_SESSION_NAME` and `ZELLIJ_PANE_ID`.

1. **From inside a pane,** with throwaway socket folders and a temporary `HOME`:
   ```bash
   # zellij, with this shell's real ZELLIJ_* variables
   ZELLIJ_SOCKET_DIR=/tmp/prbt-nest zellij attach --create-background nest-a
   ZELLIJ_SOCKET_DIR=/tmp/prbt-nest zellij -s nest-a action new-tab -n probe
   # tos, with a tos pane's variables set by hand
   TOS_SESSION=outer TOS_PANE_ID=3 TOS_SOCKET_DIR=/tmp/prbt-nest-t tos attach --create-background nest-b
   TOS_SESSION=outer TOS_PANE_ID=3 TOS_SOCKET_DIR=/tmp/prbt-nest-t tos -s nest-b action new-tab -n probe
   ```
2. **Inheritance:** prbot's test `test_a_round` (prbot `3ad5662`) runs `prbot.py start` with the *other*
   multiplexer's pane variables set. It then reads the poller's environment from `/proc/<pid>/environ`, and none
   of those variables may be there. It ran twice: on prbot as committed, which leaves every pane variable out
   when it runs `tos` or zellij, and on a copy with that line changed back to `env = clean_env()`.
3. **All of prbot's tests,** with both Pythons: `python3 -m unittest discover -s tests`.

## Results

1. Both started from inside a pane:

   ```
   == zellij, started inside a zellij pane (real ZELLIJ vars):
   rc=0
   1                      (nest-a in the list)
   new-tab rc=0
   == tos, started with a tos pane's vars set:
   rc=0
   nest-b [1 tabs, 0 attached]
   2                      (new-tab prints the new tab's id)
   new-tab rc=0
   termos                 (query-tab-names)
   probe
   ```

2. Inheritance, on the copy without the fix:

   ```
   FAIL: test_a_round (tests.test_prbot.Zellij.test_a_round)
   AssertionError: Lists differ: ['TOS_PANE_ID', 'TOS_SESSION'] != []
   Ran 2 tests in 3.715s
   FAILED (failures=1)
   ```

   `Tos.test_a_round` passed on that copy, so `tos`'s tabs didn't get the zellij pane's variables even without
   the fix. On prbot as committed, both pass.

3. All tests:

   ```
   == 3.12
   Ran 26 tests in 14.032s
   OK
   == 3.9
   Ran 26 tests in 14.948s
   OK
   ```

## Conclusions

- **`./bin/start` works typed inside either one.** Neither refuses to start a background session from inside a
  pane, and neither needs the pane's variables, because prbot always names the session with `-s`.
- **zellij passes the caller's environment on to its tabs; `tos` didn't pass zellij's variables.** Without the
  fix, a zellij session started from a `tos` pane gives its tabs `TOS_PANE_ID` and `TOS_SESSION`, so they'd look
  like they're inside both. prbot now leaves every pane variable out when it runs either program.
- **Why `tos` didn't pass them on wasn't looked into.** It doesn't matter to prbot now.
- **macOS is still unchecked,** as in the [2026-11-10 finding](2026-11-10-prbot-on-tos-and-zellij.md).
