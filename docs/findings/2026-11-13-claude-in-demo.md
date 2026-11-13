# Finding: Claude Code in the demo GIF (2026-11-13)

## Question

The user wanted the demo to start with `tos attach`, go through the finder, and then open a tab where Claude Code
writes a simple server. These decisions depended on facts that could be checked:
- **Sign-in:** how Claude Code signs in under the demo's own HOME.
- **Privacy:** what its start screen shows. The GIF is public, so a name, an email or notices would matter.
- **Timing:** how long a server takes. This sets the speed-up, and whether a fixed wait could work.
- **Questions:** whether Claude asks anything that would stall the script.
- **Your login:** whether a copy of it is safe for the real one.
- **Keys:** which keys get back to the finder tab.

## Method

Dev machine (Ubuntu, Linux 6.8), Claude Code 2.1.323 signed in with a Claude account (`~/.claude/.credentials.json`;
no `ANTHROPIC_API_KEY`), `tos` built from `c5a1a39`, xterm 390 on Xvfb `:88`, at 104×34 cells.

- **Network:** `curl https://api.anthropic.com/v1/messages` from Claude's sandboxed shell.
- **A config of its own:**
  - `CLAUDE_CONFIG_DIR` was a new folder under `/tmp/tosdemo.*`, made with mode 700, holding a copy of
    `.credentials.json` (mode 600).
  - Its `.claude.json` held `hasCompletedOnboarding`, `theme` and the sample folder marked as trusted.
  - Then a second version: the same, plus the user's `~/.claude.json` without `oauthAccount`, `userID`,
    `machineID`, `projects` and other per-user keys (82 keys left).
  - `IS_DEMO=1` was tried as well.
- **The start screen:** `claude` was run, and the screen captured with `import -window root`.
- **One request, inside tos:**
  - `tos attach`, `tos finder`, `Ctrl+t` `n`, `claude`, then
    `Write a simple Python web server in server.py that answers "Hello from Brightwater Bakery!"`.
  - Polled `tos dump-screen` every second until `server.py` existed and "esc to interrupt" was gone.
  - Afterwards: processes whose working folder was under the test folder, and `ss -ltnp` for port 8000.
- **The login copy:** after the session, `cmp` against the original. `expiresAt` was read from the original.
- **The dialog afterwards:** chose "Don't show again", then compared `.claude.json` before and after.
- **Tabs:** tried `Ctrl+t` `1`, `Ctrl+t` `f`, and `Ctrl+t` `Tab` `Esc`.
- **The recording:** `python3 scripts/demo/record.py --out <gif> --keep`, then frames pulled from the GIF.

## Results

**Network:** `http 405` (reachable; GET isn't allowed on that endpoint).

**Start screen, new config with only the login:**

```
Claude Code v2.1.323
Opus 5.5 · Claude Max
~/bakery
Auto mode is now Claude Code's default permission mode. …   (notice)
2 more notices hidden
Fast mode is now available · /fast to turn on                (notice, in red)
```

- **With the user's other settings copied:** the same three lines, and no notices.
- **`IS_DEMO=1`:** no visible change; it still shows "Claude Max".
- **Name and email:** neither shows in any version.

**The request:**

```
done after 12.1 s          (Claude: "Worked for 11s")
server.py written; Claude started it, fetched a page with curl and stopped it
afterwards: no process under the test folder but tos's own; nothing listening on 8000
then a dialog: "Teach auto mode about your environment?  1. Yes  2. Not now  3. Don't show again"
"Don't show again" changed: autoModeEnvSetup {"denials": 5} -> {"denials": 5, "dismissed": true}
```

Auto mode was on, which is the default here. Claude asked nothing during the task.

**Login copy:** unchanged after the session (no token refresh). The token then had 4.6 h left.

**Tabs:**
- **`Ctrl+t` `n`:** a new tab ("2 bakery"), back in normal mode.
- **`Ctrl+t` `1`:** "1 bakery". The finder tab has no number.
- **`Ctrl+t` `f`:** a second finder tab.
- **`Ctrl+t` `Tab` `Esc`:** back to the last tab, with the file list focused.

**The recording** (prompt `Write a simple web server in server.py that says "Hello from the bakery!"`):

```
recorded 100 s; Claude worked 76.1–88.2 s (12.1 s), shown at 2×
GIF: 1.41 MB (1 414 105 bytes), 93.2 s, 317 frames, 832x618
Claude's reply: wrote server.py, ran it on port 8000 with curl, "Worked for 8s"
whole run: 1 min 52 s; afterwards nothing left running and nothing on port 8000
```

## Conclusions

- **The demo signs in with a copy of the user's login** in a config folder of its own, without the per-user keys
  of `~/.claude.json`. Claude then starts with no notices, no name and no email.
  - **What the GIF does show:** the model and plan ("Opus 5.5 · Claude Max"). The user should know this, as the
    GIF is public.
- **The copy must not refresh the token,** because a refresh could sign out the original. The script refuses to
  start when less than 30 minutes are left.
- **Claude's time varies, so the script watches the screen,** not a clock. The time is about 12 s here; it's an
  **estimate** for other runs. The wait is sped up to show for at most about 6 s, and the caption says by how
  much.
- **Seed `autoModeEnvSetup.dismissed`;** otherwise a dialog follows the task.
- **Claude may start the server itself.** It used port 8000 for a moment and stopped it. The script also ends
  any process left in its folder.
- **Back to the finder is `Ctrl+t` `Tab` `Esc`.**
- **Unknown:** how other Claude Code versions word "esc to interrupt". If that changes, the wait still ends,
  because it also needs the screen to stay the same for 1.5 s.
