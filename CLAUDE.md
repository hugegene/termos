# CLAUDE.md

## Project

This repo is **termos**, a terminal workspace in Rust. The command is `tos`; it was `tb` until 2026-11-10,
and older docs say `tb`. It has our own multiplexer, finder and poller/webhook daemon, built around a fork of
`alacritty_terminal`. See [docs/plans/2026-11-03-rebuild-plan.md](docs/plans/2026-11-03-rebuild-plan.md).

The zellij prototype it grew from (`finder`, yazi and `preview.py`) moved to **`~/tfinder`** on 2026-11-10.
That folder has its own git repo and `CLAUDE.md`. `tos` keeps its own copy of the prototype's Neovim config
in `crates/tos/nvim/init.lua`. See [docs/plans/2026-11-10-move-prototype-out.md](docs/plans/2026-11-10-move-prototype-out.md).

## Docs workflow (always follow)

### 1. Measure only when necessary

Measure only when **a plan decision depends on a fact you can check and don't already know**. Such facts
include:
- timings and sizes
- versions, licenses and dependency lists
- how a tool actually behaves
- what the current code does

**Don't measure** when nothing depends on it, for example when:
- the user has already decided (scope, priorities, preferences)
- the fact is already recorded in `docs/findings/` and still current
- the change is wording, structure or a small edit
- a rough figure is good enough; label it as an **estimate** instead

When you do measure, record it in **`docs/findings/YYYY-MM-DD-<topic>.md`** with these sections:

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

## Explaining (always follow)

This applies to answers in chat and to everything in `docs/`.

- **Walk through an example.** Whenever possible, explain by following one concrete case step by step, such as
  a real file, key press, command or request, rather than describing only the general rule.
- **Draw a diagram in a code block.** Whenever parts are separated or a request travels between them, draw a
  plain-text diagram in a fenced code block.
  - **Logical or physical separation:** boxes for each part, such as processes, threads, crates, sockets or machines.
  - **Request or network flow:** numbered arrows for each hop, labelled with what travels, such as keys, IPC
    messages, HTTP calls or webhooks.

## Rust workspace (termos, command `tos`)

- **Layout:**
  - `Cargo.toml` (workspace)
  - `crates/tos`: client, server, finder UI and CLI
  - `crates/finder-core`: listing, file types, preview data
  - `crates/alacritty_terminal`: our fork, unmodified so far; see its `UPSTREAM.md` and record every change there
  - `scripts/`: `install.sh` (the one-line installer) and `package.sh`, used by `.github/workflows/release.yml`.
    Pushing a tag `v<version>` publishes a release on GitHub (`hugegene/termos`, public, MIT).
  - `scripts/demo/`: `record.py` records the README's demos: `docs/images/demo-short.gif` (the top),
    `finder-demo.gif` (the full tour) and `finder-demo.webp`: attach, the finder, then Claude Code in a new tab.
    Re-run it after changing what the finder shows or its keys, with the user's media:
    `python3 scripts/demo/record.py --video ~/ghibli-video/output/full_v5_depth8/babyplayful_ghibli.mp4
    --image ~/ghibli-video/output/t2v/flying_baby.mp4`.
- **Build and test:** `cargo build`, `cargo test --workspace`,
  `cargo clippy --no-deps -p tos -p finder-core --all-targets -- -D warnings`, `cargo deny check licenses`.
  - Format with `cargo fmt -p tos -p finder-core`, never `cargo fmt --all`: that rewraps the
    `alacritty_terminal` fork to our `rustfmt.toml`.
  - Rust lives in `~/.cargo/bin`.
  - Network access from Claude's shell is sandboxed, so build with `--offline`. A new dependency needs the user to
    run `cargo fetch`.
- **Command docs:** `docs/cli/` has a page per command, generated from the clap definitions, so don't edit it
  by hand.
  - After changing a command, its help text or its examples (`after_long_help`), run
    `TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs`.
  - The test `cli_docs_are_current` fails until you do.
  - New concepts that span commands (ids, JSON fields, events, config) go in `docs/cli.md`.
- **End-to-end tests:** `crates/tos/tests/e2e.rs` runs `tos attach` in a pseudo-terminal and reads the screen back.
  - Tests use their own socket and state folders (`TOS_SOCKET_DIR`, `TOS_STATE_DIR`), never the user's sessions.
  - Socket paths must stay under about 100 bytes, so don't put sockets in the scratchpad.
