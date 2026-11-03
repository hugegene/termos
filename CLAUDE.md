# CLAUDE.md

## Project

This repo holds a terminal workspace in two stages:

- **The prototype (current code):** a two-pane file finder built from zellij, yazi and our own `preview.py`.
- **The planned rebuild:** a Rust product with our own multiplexer, finder and poller/webhook daemon, built
  around a fork of `alacritty_terminal`. See [docs/plans/2026-11-03-rebuild-plan.md](docs/plans/2026-11-03-rebuild-plan.md).

## Docs workflow (always follow)

### 1. Measure before planning

Before writing or changing a plan, **measure what the plan depends on instead of assuming it**. That
includes:
- timings and sizes
- versions, licenses and dependency lists
- how a tool actually behaves
- what the current code does

Skip measuring only when no decision depends on a fact that can be checked.

Record every measurement in **`docs/findings/YYYY-MM-DD-<topic>.md`** with these sections:

- **Question:** what we needed to know, and which decision depends on it.
- **Method:** the exact commands or scripts, plus versions and machine, so it can be re-run.
- **Results:** raw numbers and output, trimmed but not paraphrased.
- **Conclusions:** what it means for the plan, and what is still unknown.

Findings are a record. Don't edit old results; when re-measuring, add a new dated finding.

### 2. Write every plan to docs/plans

Write each plan to **`docs/plans/YYYY-MM-DD-<topic>.md`** before implementing it, not only in chat.

- **Order:** the decision or summary first, then the options considered, then the steps.
- **Sources:** link the findings it relies on (`../findings/...`). Label anything not measured as an **estimate**.
- **Keep it current:** when the plan changes, update the same file and note what changed and when. Don't start a duplicate.

## Prototype

- **Files:**
  - `finder`: the launcher; it writes a zellij layout.
  - `bin/files`: runs yazi in the left pane.
  - `bin/send`: sends commands to the right pane through a fifo.
  - `preview.py`: the preview and edit pane.
  - `yazi/`, `nvim/`, `visidata/`: configs.
  - `install.sh`: one-time setup.
- **Testing:** test in throwaway zellij sessions (`tftest-*`), never in the user's own session. The user's
  zellij config binds **Ctrl+Q to quit all of zellij**.
