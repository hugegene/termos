# Plan: phone interface

**Status:** decided · **Date:** 2026-11-03, revised 2026-11-21 · **Part of:** [rebuild plan](2026-11-03-rebuild-plan.md) ·
**Findings:** [prototype at phone sizes](../findings/2026-11-03-phone-terminal-measurements.md) ·
[phone options research](../findings/2026-11-03-phone-interface-research.md)

## Decision

**termos's phone app is developed separately, outside this repository.** termos itself supports phones in
two ways.

1. **Any SSH terminal app, with Mosh.**
   - Termius or Blink on iOS, Termux or ConnectBot on Android.
   - You run `tos attach`.
   - Below about 80 columns, `tos` needs a single-pane layout (rebuild step 7). Two panes were unusable at
     40–50 columns and fine at 100 ([measurements](../findings/2026-11-03-phone-terminal-measurements.md)).
2. **The `tos` command line, for phone apps.** A phone app's server runs on the dev box next to `tos`. It uses
   the same command line any extension uses ([cli.md](../cli.md)):
   - `list-tabs -j` and `list-panes -j` for the current state
   - `subscribe` for events and pane text
   - `dump-screen`, `write-chars` and `send-keys` to read and type into panes
   - `new-tab` to start agents, and the agents' hooks (`tos claude` and the others)

## Planned in termos (not built yet)

**Agent state events in `tos subscribe`.** They report when an agent is waiting for you (a permission prompt
or a question) and when it has finished.
- **Where they come from:** the agents' `Stop`, `Notification` and `PermissionRequest` hooks. These are added
  to the hook file that `tos claude` and the other agent commands already write at launch.
- **Who uses them:** phone apps, and also the status bar, for example `claude waiting`.
- **Before building:** this gets its own plan.

## Change log

- **2026-11-03:** first version. A PWA over a private phone API inside the daemon, with SSH apps as the
  baseline.
- **2026-11-05:** the narrow layout is now rebuild **step 7**, because a new step 6 (CLI and extensions) comes
  first ([rebuild plan](2026-11-03-rebuild-plan.md) change log).
- **2026-11-21:** **the phone app moved out of termos** and is developed separately. This plan now keeps only
  what termos provides for phones: the SSH path, the command line, and the planned agent state events. The
  phone API is no longer part of termos's daemon. The earlier version is in this file's git history.
