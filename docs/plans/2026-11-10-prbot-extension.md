# Plan: prbot, the GitHub poller, as its own repo that runs on `tos` or zellij

**Status:** built and published (https://github.com/hugegene/prbot); the check on your Mac (Y3) is next · **Date:** 2026-11-10 · **Answers:** the [rebuild plan](2026-11-03-rebuild-plan.md)'s open
question 2, for the poller · **Builds on:** [the CLI as the extension API](2026-11-05-cli-extensions.md) (step 6) ·
**Findings:** [prbot on `tb`](../findings/2026-11-05-prbot-on-tb.md),
[prbot on `tos` and zellij](../findings/2026-11-10-prbot-on-tos-and-zellij.md)

## Decision

- **The poller is prbot**, today in `~/claudehook`.
  - It is a Python script, standard library only, that searches GitHub every 60 s.
  - For each PR that needs you, it opens a tab running Claude.
  - That folder isn't a git repo yet.
- **prbot gets its own public repo, `hugegene/prbot`,** and its folder moves from `~/claudehook` to `~/prbot`.
  It isn't added to termos, and nothing is put on `PATH`.
  - You use it as you do today: `cd ~/prbot` and run `./bin/start`.
  - Users who want to change it fork the repo.
- **It works on both multiplexers.** A setting in `config.env`, `MUX=tos` or `MUX=zellij`, picks which one it
  drives.
  - New users' example config says `tos`.
  - Your own `config.env` says `zellij` until you choose to switch, so the poller you run today behaves the same.
- **termos links to it** as its full-size example extension. `docs/examples/exit-watcher.py` stays in termos as
  the small example, because it teaches the API and changes along with it.
- **It works on macOS too,** as `tos` does. Today two things in it only work on Linux (see "macOS" below).
- **Not covered here:** the rebuild plan also put a webhook listener and the phone API into its daemon. Those stay
  open. The poller needs none of the daemon's crates (`axum`, `reqwest`, `rustls`, `rusqlite`).

```
 GitHub: hugegene/termos (public, MIT)         GitHub: hugegene/prbot (public, MIT)
 ┌───────────────────────────────────┐         ┌───────────────────────────────────┐
 │ crates/tos …                      │  link   │ prbot.py, bin/start, bin/stop     │
 │ docs/cli.md: "Example extension" ─┼────────▶│ config.example.env  (MUX=tos)     │
 │ docs/examples/exit-watcher.py     │         │ README: needs tos (API 2+) or     │
 └───────────────────────────────────┘         │         zellij, plus gh and claude│
                                               └───────────────────────────────────┘
```

## Why its own repo, and not a folder in termos

1. **Users don't clone termos.** The one-line installer downloads only the `tos` binary. A folder in termos would
   make them clone the whole Rust source, alacritty fork included, to get one Python script.
2. **Customizing means forking.** A fork of prbot keeps the user's changes as commits and can still pull yours.
   A fork of termos would carry the whole terminal along.
3. **Same rule as `~/tfinder`.** The zellij prototype moved out on 2026-11-10 so that termos holds only the Rust
   workspace ([move plan](2026-11-10-move-prototype-out.md)).
4. **It's the better demo.** The point to show is that an outside program drives `tos` through its CLI. A
   separate repo shows exactly that.

## Why both multiplexers can work

`tos` copied zellij's command names, flags and output on purpose ([step 6](2026-11-05-cli-extensions.md)), so
prbot's command lines are identical on both. Only three things differ:

| | zellij | `tos` |
|---|---|---|
| Program | `zellij` | `tos` |
| The pane a program runs in | `ZELLIJ_PANE_ID` | `TOS_PANE_ID` |
| The session a program runs in | `ZELLIJ_SESSION_NAME` | `TOS_SESSION` |
| Detach key | `Ctrl+o d` | `Ctrl+o d` |

These are the commands prbot runs, each with `-s SESSION` before `action`. All of them exist in `tos`:

| Where | Commands |
|---|---|
| `prbot.py` | `list-sessions -n`, `delete-session`, `attach --create-background`, `action list-tabs -j -s`, `action list-panes -j -t`, `action new-tab -n NAME -c DIR -- CMD…`, `action go-to-tab-by-id` |
| `bin/start` | the same three session commands, then `action query-tab-names`, `action rename-tab-by-id 0 home`, `action new-tab -n poller …` |
| `bin/stop` | `action list-tabs -j`, `action close-tab-by-id` |

- **Already tested:** the [2026-11-05 finding](../findings/2026-11-05-prbot-on-tb.md) ran `prbot.py`'s
  functions against `tb` with only the program and variable names changed. They passed after one fix to the
  session list's format, which `tos ls` still follows (`docs/cli.md`).
- **Tested since, on both:** the `bin/start` and `bin/stop` commands too, after the rename from `tb` to `tos`
  ([finding](../findings/2026-11-10-prbot-on-tos-and-zellij.md)). zellij's first tab has id 0 and `tos`'s has
  id 1, so `start` now names whichever tab is listed first, rather than tab 0.
- **Size:** about 40–60 changed lines in prbot (**estimate**). Today `prbot.py` names the multiplexer in 4 commands,
  3 reads of its variables and 3 messages, and `bin/start` and `bin/stop` call it 9 times.

### Example: a review request for PR #487, with `MUX=tos`

```
 ┌────────┐ 1. search: review-requested:@me  ┌────────────────────────┐
 │ GitHub │ ◀─────────────────────────────── │ poller tab:            │
 └────────┘                                  │ python3 prbot.py poll  │
                                             └───────────┬────────────┘
           2. tos -s prbot action new-tab -n rev-487 -c ~/src/demo -- bin/run-claude review demo 487
                                                         ▼
                                  ┌──────────────────────────────────────┐
                                  │ tos session "prbot"                  │
                                  │  tab rev-487: prbot.py run → claude  │
                                  └──────────────────┬───────────────────┘
           3. you say "close the tab": bin/prbot done reads TOS_PANE_ID and TOS_SESSION,
              4. tos -s prbot action list-panes -j -t  →  its own tab id  →  rename to "✓ rev-487", close
```

With `MUX=zellij`, every arrow is the same command with `zellij` in place of `tos`. Step 3 reads
`ZELLIJ_PANE_ID` and `ZELLIJ_SESSION_NAME` instead.

## How the switch works in prbot

- **One place:** `prbot.py` gets one constant for the program and a two-row table of variable names. `zj()`,
  `session_running()`, `ensure_session()`, `own_tab_id()` and the close-later path use them.
- **Settings:**
  - `MUX`: `tos` or `zellij`. If it's missing, the default is `tos`.
  - `SESSION` replaces `ZELLIJ_SESSION`. The old name is still read, so existing configs keep working.
- **Starting and stopping move into `prbot.py`,** as `prbot.py start` and `prbot.py stop`.
  - `bin/start` and `bin/stop` become three-line wrappers, like `bin/new-job` is today.
  - The program is then chosen in one place instead of three.
  - The list of Claude variables to drop (`LEAKY_ENV`), now copied in `bin/start`, is kept only once.
- **Messages name the right program,** for example `attach with: tos attach prbot`.
- **Prompts users can change.** The first message prbot types into each Claude tab is in `prbot.py`
  (`MESSAGES`: review, rereview, fix, reply).
  - `config.env` can replace any of them: `MESSAGE_REVIEW`, `MESSAGE_REREVIEW`, `MESSAGE_FIX` and
    `MESSAGE_REPLY`. The defaults stay in `prbot.py`.
  - `config.example.env` lists the defaults and their placeholders: `{n}`, `{url}`, `{sha}`, `{intro}`, `{note}`
    and `{prbot}`.
  - **The "close the tab" sentence is always added** and can't be replaced, because closing the tab depends on
    Claude running `bin/prbot done`.
  - A message with an unknown placeholder is reported in the tab, which waits for Enter as it does for other
    problems. Claude isn't started with a broken message.
  - **Why in `config.env`:** git ignores it, so a user's own prompts never conflict when they pull updates.
- **Version check:** with `MUX=tos`, `prbot.py start` reads `tos --version`. If the extension API is older than
  2, it stops and says to run `tos update`. There's no check for zellij.

## macOS

`tos` runs on macOS ([macOS plan](2026-11-09-macos-support.md)), so prbot should too. Reading `prbot.py` turned
up two things that only work on Linux:

| What | Today | On macOS today | The fix |
|---|---|---|---|
| Desktop notifications (`notify()`) | `notify-send` | Not installed, so it fails silently: no desktop notifications. ntfy still works | `osascript`, which comes with macOS, run as `display notification`. The title and text go in as arguments, not pasted into the script, so quotes in a PR title can't break it |
| "Is anything running inside this worktree?" (`processes_inside()`), checked before cleanup removes a worktree | Reads `/proc/*/cwd` | There's no `/proc`. Each poll's cleanup fails with an error in the log, and `./bin/prbot cleanup` stops with a traceback | Without `/proc`: `lsof -a -u <you> -d cwd -Fn`, which lists the working folder of each of your processes. `lsof` comes with macOS |

- **If neither works** (no `/proc` and no `lsof`), cleanup keeps the worktree. It says why once, as it already
  does for other reasons to keep one, rather than guessing that nothing is running.
- **Python:** prbot only uses the standard library. It must keep running on Python 3.9, the `python3` that
  macOS's command-line tools install. That version is from memory, not checked on your Mac (**estimate**).
  `prbot.py` compiles with 3.9 today, and the tests in P5 run with 3.9.
- **`bin/start` and `bin/stop`** are bash, and macOS has bash 3.2. After P3 they're three-line wrappers, so
  that doesn't matter.
- **A check command, `./bin/prbot check`,** tells a new user, on either system, what's missing:
  - Python's version.
  - `gh`: installed and logged in, and as whom.
  - `claude`: installed.
  - The `MUX` program: installed, and for `tos`, extension API 2 or later.
  - Each `REPO_DIRS` entry: whether it's a git clone.
  - The process check: which way it works (`/proc` or `lsof`).
  - It also sends one test notification.

  On your Mac it's the whole macOS test in one command (step Y3).

## Options considered

| Topic | Option | Verdict |
|---|---|---|
| Where it ships | **Its own repo** | ✅ Users get only prbot. Forks work. It demos the extension API from outside |
| | A folder in termos (`extensions/prbot`) | ❌ Users would clone the Rust source to get a script, and forks would carry the terminal |
| | Built into the `tos` binary, in Rust | ❌ A rewrite of about 900 lines. It adds HTTP and TLS crates to a terminal that most users run without prbot, and couples prbot to `tos` releases |
| How it starts | **`cd` into the repo and run `./bin/start`** | ✅ Your choice |
| | `tos prbot` running a `tos-prbot` found on `PATH` | ❌ Not wanted |
| Multiplexer | **Both, chosen by `MUX`** | ✅ The commands are identical, and you can switch when you're ready |
| | `tos` only | ❌ Would force your switch the day it lands, and drops users who are on zellij |
| Notifications on macOS | **`osascript`** | ✅ Comes with macOS |
| | `terminal-notifier` | ❌ Another install, from Homebrew |
| Process check on macOS | **`lsof`** | ✅ Comes with macOS, and also works on Linux, so the Linux side can test it |
| | `ps` | ❌ It can't show a process's working folder |
| Choosing it | **A setting** | ✅ Predictable |
| | Whichever one is installed | ❌ Ambiguous when both are, as on your machine |

## Steps

### In `~/claudehook` (becomes the prbot repo)

The poller is running from this folder. Every new job tab runs `prbot.py run` from it, and so do `done` and
Claude's hooks. **A half-edited `prbot.py` would break new tabs,** so the work happens in a separate worktree
and lands in one step.

- [x] **P1. Git, without anything private.**
  - `.gitignore`: `config.env`, `prbot.log`, `state.json`, `worktrees.json`, `worktrees.json.lock` and
    `__pycache__/`. `~/.cache/prbot` is already outside the folder.
  - `config.example.env`: every setting with its comment and no personal values. `GH_USER`, `REPO_DIRS` and
    `NTFY_TOPIC` are left empty. Anyone who knows an ntfy topic can read its messages.
  - `README.md`: the work org and repo (2 places) become a made-up `acme/widgets`, and your work GitHub login
    in the search diagram becomes "you (GH_USER)".
  - The three old plans in `docs/` stay on disk but out of git. They're full of the work repo's branch names,
    PR numbers and paths, and users don't need them.
  - `LICENSE`: MIT, as termos.
  - `git init`, then a first commit of today's code as it is. **You look at `git show --stat` before anything
    else happens.**
  - Then a worktree on a branch, beside the folder, for steps P2–P5. It gets its own `config.env` with
    `SESSION=prbot-dev`, so it never touches your `prbot` session.
- [x] **P2. The multiplexer switch:** as in "How the switch works" above.
- [x] **P3. `start` and `stop` in `prbot.py`,** with `bin/start` and `bin/stop` as wrappers.
- [x] **P4. The `tos` version check, and the prompts from `config.env`.**
- [x] **P4b. macOS:** notifications with `osascript`, the process check with `lsof`, and `./bin/prbot check`.
- [x] **P5. Test on both, and record a finding** (`docs/findings/2026-11-10-prbot-on-tos-and-zellij.md` in
  termos):
  - Re-run the 2026-11-05 harness: `ensure_session`, `spawn`, `find_tab`, `own_tab_id`, then `done` and
    `close-later`. `run-claude` is a stub, and nothing calls GitHub.
  - Run `bin/start` twice (the second run must say "already running") and then `bin/stop`.
  - **On `tos`:** a throwaway `TOS_SOCKET_DIR` under `/tmp`. Socket paths must stay under about 100 bytes, so
    not in the scratchpad.
  - **On zellij:** a session named `prbot-test`, deleted afterwards. Your `prbot` session isn't touched.
  - **The `lsof` check, on Linux:** forced on, it must find a process whose working folder is inside a
    temporary folder, and must not find one outside it.
  - **Python 3.9:** the harness runs with `python3.9`, and `prbot.py` compiles with it.
- [x] **P6. prbot's README:** what it is, what it needs (`gh`, `claude`, and `tos` with API 2 or later, or
  zellij), the setup (`cp config.example.env config.env`, then `./bin/start`), the `MUX` choice, and how to
  customize it.
- [x] **P7. Land it and move the folder to `~/prbot`.**
  1. Merge the branch into `main` in `~/claudehook`, and remove the P1 worktree. A worktree's link to its repo
     is an absolute path, so it goes before the move.
  2. `./bin/stop`, then `mv ~/claudehook ~/prbot`.
  3. `ln -s ~/prbot ~/claudehook`. Job tabs that are still open were told to run
     `~/claudehook/bin/prbot done`, and their Claude hooks point there too
     (`~/.cache/prbot/claude-settings.json`). The link keeps those working. Nothing else refers to the old
     path: `state.json`, `worktrees.json`, crontab and systemd don't mention it.
  4. Your `config.env` gets `MUX=zellij`, `SESSION=prbot` (instead of `ZELLIJ_SESSION`), and `MESSAGE_REVIEW`
     with today's review message, so nothing about your tabs changes. Then `cd ~/prbot && ./bin/start`.
  5. Remove the `~/claudehook` link once the old tabs have closed.

### In termos

- [x] **T1.** `docs/cli.md`, under "Example extension": a paragraph linking prbot as the full-size example.
  README: one line under the finder and extension notes.
- [x] **T2.** Rebuild plan: open question 2 answered for the poller, linking here, plus a change-log line.

### You

- [x] **Y1. Publish:** create `hugegene/prbot` on GitHub (public, MIT) and push. Claude doesn't create or push
  without your go-ahead.
- [ ] **Y2. Switch your own poller to `tos`** whenever you like: set `MUX=tos` in `config.env`, then run
  `./bin/stop` and `./bin/start`. Tabs already open stay in the zellij session until they close.
- [ ] **Y3. On your Mac:** clone prbot, fill in `config.env`, run `./bin/prbot check` (a notification must
  appear), then `./bin/start` with `MUX=tos` and `./bin/prbot cleanup --dry-run`.

## Open questions

1. ~~**The repo's name**~~ **Answered 2026-11-10:** `prbot`.
2. ~~**The local folder**~~ **Answered 2026-11-10:** `~/prbot` (step P7).
3. ~~**The prompts**~~ **Answered 2026-11-10:** users can replace them from `config.env`, in this plan (P4).

## Change log

- **2026-11-10:** written. You chose a separate repo started with `./bin/start` (no `tos-prbot` on `PATH`), and
  asked to keep zellij working alongside `tos`.
- **2026-11-10:** the repo is `prbot`, and the folder moves to `~/prbot`, with a link from `~/claudehook` for
  tabs that are still open (P7).
- **2026-11-10:** the prompts can be replaced from `config.env` (P4).
- **2026-11-10:** P1's first commit made (`99f47a3` in `~/claudehook`, 11 files). The old plans are kept out of
  git instead of edited, and the README also had your work GitHub login, now removed.
- **2026-11-10:** macOS added: notifications through `osascript`, the process check through `lsof`, and
  `./bin/prbot check` (P4b, Y3).
- **2026-11-10:** P2–P6 built on the branch `tos-and-zellij` (`38d5348`), with 19 tests that pass on Python
  3.12 and 3.9, including a full round on `tos` and on zellij. Changes from the plan:
  - The default review message dropped "Skip reading in /review", which only your repo's `/review` has. P7 puts
    your current text in your `config.env` as `MESSAGE_REVIEW`, so your tabs don't change.
  - `start` takes the poller's own options (`--dry-run`, `--since`), instead of passing anything through.
  - `config.env`'s values now lose only one pair of surrounding quotes. Before, every quote at either end was
    stripped, which would have cut a message ending in a quote.
- **2026-11-10:** T1 and T2 done in termos, not committed. Their links to `hugegene/prbot` work once Y1
  publishes it, so they go out together.
- **2026-11-10:** P7 done at 23:37. Your `config.env` got `MUX=zellij`, `SESSION=prbot` and your review message
  first. The branch was merged into `main` (`38d5348`), the folder moved to `~/prbot` with a link from
  `~/claudehook`, and the poller restarted there. Its first poll succeeded and carried on from the old poller's
  last one. No job tabs were open. A Claude session and a shell were working in the folder; they kept running.
  Step 5, removing the link, waits until nothing uses the old path.
- **2026-11-10:** the `~/claudehook` link removed (P7 step 5), once nothing used the old path.
- **2026-11-10:** published at your request: https://github.com/hugegene/prbot (public, MIT), pushed as
  `hugegene` through the same credential helper termos uses. The termos changes (T1, T2, this plan and the
  finding) were committed and pushed with it.
