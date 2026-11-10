# Rename: the product is termos, the command is `tos`

## Decision

The user picked the names on 2026-11-10: the product is **termos** (as the repo already is) and the command is
**`tos`** instead of the working name `tb`.

| What | Before | After |
|---|---|---|
| Command, crate and binary | `tb`, `crates/tb` | `tos`, `crates/tos` |
| Environment variables | `TB_SESSION`, `TB_TRASH`, … | `TOS_SESSION`, `TOS_TRASH`, … (no fallback) |
| Folders | `…/tb` | `…/termos`: `$XDG_RUNTIME_DIR/termos`, `/tmp/termos-<uid>`, `~/.local/state/termos`, `~/.local/share/termos`, `~/.config/termos/config.kdl` |
| Release files | `tb-<target>.tar.gz` | `tos-<target>.tar.gz`, holding `tos-<version>-<target>/tos` |
| Version | `tb 0.1.1 (extension API 1)` | `tos 0.2.0 (extension API 2)` |

- **Folders take the product name.** "tos" alone is too generic for a folder in `~/.config`, and Helix (`hx`
  and `~/.config/helix`) does the same.
- **Env vars have no fallback to `TB_*`.** The only release is a test release from today, and none of the
  user's shell files set a `TB_` variable (checked: `~/.bashrc`, `~/.profile`, `~/.bash_aliases`, `~/.zshrc`).
- **Saved state moves by itself.** On its first run, `tos` renames `~/.local/state/tb`, `~/.local/share/tb`
  and `~/.config/tb` to their `termos` names, if the new folder doesn't exist yet. Each move is printed once.
- **Extension API 2.** [docs/cli.md](../cli.md) promises that names don't change within an API version, and
  extensions call `tb` and read `TB_SESSION`/`TB_PANE_ID`. So the API version goes up, and the version goes
  to 0.2.0.
- **History stays as it was.** Findings and finished plans keep saying `tb`, since that's what it was called
  then. The [rebuild plan](2026-11-03-rebuild-plan.md) gets a note. The current docs are updated: `README.md`,
  `CLAUDE.md`, `docs/cli.md`, `docs/cli/` (regenerated), and the open parts of the
  [distribution](2026-11-09-distribution.md) and [macOS](2026-11-09-macos-support.md) plans.
- **No release in this change.** Until `v0.2.0` is tagged, the one-liner still installs `tb 0.1.1`.

*Done 2026-11-10.* Checked:
- `tos --version` prints `tos 0.2.0 (extension API 2)`.
- `cargo test --workspace` passes (33 e2e tests), and clippy is clean for Linux and `aarch64-apple-darwin`.
- **The folder move, with a throwaway `HOME`:** while a fake `tb` socket answered, nothing moved and the note
  named `main2`. After it closed, `~/.local/state/tb` moved and `tos ls` listed the saved session. A third
  run said nothing.
- **`install.sh`** against a local server installed `tos 0.2.0`, with the note about an old `tb` in the same
  folder.
- **The user's own state was left alone:** `~/.local/state/tb` still holds `main.json` and `main2.json`, and
  `main2` keeps running under `target/release/tb`.

### Example: this machine switching over

On 2026-11-10 the user has session `main2` running under `target/release/tb`, with saved layouts
`main.json` and `main2.json` in `~/.local/state/tb/sessions/`.

```
 before                                     after `cargo build --release`
┌──────────────────────────────┐           ┌──────────────────────────────────────┐
│ target/release/tb  (old)     │           │ target/release/tos  (new)            │
│  server main2 ── socket ──▶  │           │  sockets in $XDG_RUNTIME_DIR/termos/ │
│  $XDG_RUNTIME_DIR/tb/        │           │  state in ~/.local/state/termos/     │
│  state ~/.local/state/tb/    │           └──────────────────────────────────────┘
└──────────────────────────────┘
```

1. `tb kill-all-sessions -y` (the old binary) ends `main2` and saves its layout to `~/.local/state/tb`.
2. The first `tos` command renames `~/.local/state/tb` to `~/.local/state/termos`, and `~/.local/share/tb`
   likewise. It prints `tos: moved ~/.local/state/tb to ~/.local/state/termos (tb is now called tos)`.
3. `tos attach main2` restores the session from `~/.local/state/termos/sessions/main2.json`.

If step 1 is skipped, the old server keeps running from its own socket folder, which `tos` never looks in.
It also keeps saving into `~/.local/state/tb`, which would appear again after the move. That's why its
sessions should be ended first.

## Options considered

- **Folders named `tos`:** this matches the command, but the name is generic and could clash with other
  tools. Not chosen.
- **Reading `TB_*` as a fallback:** that would only help scripts written in the past few hours. It adds code
  to remove later. Not chosen.
- **No migration (start fresh):** this would lose the user's saved layouts. Not chosen.

## Steps

- [x] `git mv crates/tb crates/tos`. Rename the package and binary to `tos` and update the workspace members.
- [x] Code: `tb` becomes `tos` in messages, help text and examples, and `TB_*` becomes `TOS_*`. Folders become
  `termos` (`paths.rs`, `server/config.rs`). Bump to 0.2.0 and extension API 2. The tests' socket prefix
  becomes `/tmp/tost-`.
  - Leave alone: `finder-core/src/lib.rs`'s `tb` variable and `"TB"` unit, and `grid.rs`'s `"a\tb"` test
    data.
- [x] `paths::migrate_from_tb()`, called first in `main`: it moves the old state, data and config folders, with
  a unit test.
- [x] Scripts and CI:
  - `install.sh` (`TOS_INSTALL_DIR`, `TOS_VERSION`, `TOS_DOWNLOAD_URL`, installs `tos`, and says when an old
    `tb` sits next to it)
  - `package.sh`, `release-README.md`, `release.yml` and `ci.yml` (`-p tos`)
- [x] Docs: `README.md`, `CLAUDE.md`, `docs/cli.md`, and `docs/cli/` regenerated with
  `TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs`. Add notes to the rebuild, distribution and macOS
  plans.
- [x] Check: `cargo build`, `cargo test --workspace`, clippy for Linux and `aarch64-apple-darwin`,
  `cargo deny check licenses`, and `install.sh` against a local server. Then CI on macOS after the push.
