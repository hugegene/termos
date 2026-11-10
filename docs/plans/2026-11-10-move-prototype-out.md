# Moving the zellij prototype out of the termos repo, to ~/tfinder

## Decision

The user asked on 2026-11-10 for the repo to hold only termos (`tos` and its built-in finder). The zellij
prototype moves to **`~/tfinder`**, a folder of its own with its own local git repo.

- **The name:** "tfinder" is what the prototype already calls itself (`tfinder-nvim`, `TFINDER_IMAGES`,
  `tfinder.yazi`, `$XDG_RUNTIME_DIR/tfinder`). Its files (`bin/`, `yazi/`, …) would clutter `~/` if loose.
- **What moves:**
  - `finder`, `bin/files`, `bin/send`, `preview.py`, `install.sh`
  - `yazi/`, `visidata/` and `nvim/init.lua`
  - the prototype's parts of `README.md`, and the "Prototype" section of `CLAUDE.md`
  - `~/.local/bin/finder` now links to `~/tfinder/finder`.
- **What `tos` keeps:** `tos` builds in `nvim/init.lua` (`crates/tos/src/server/editor.rs`, `include_str!`),
  so that file moves to **`crates/tos/nvim/init.lua`**, and `~/tfinder` gets its own copy. The two can then
  change separately.
- **Unchanged for now:** `tos` keeps using the Neovim profile `tfinder-nvim`, whose plugin folder
  (`render-markdown.nvim`) the prototype's `install.sh` fills. Renaming it would lose the plugin on this
  machine. A `tos`-owned profile, which a new Mac wouldn't have either, is a later item.
- **History:** the prototype's history stays in termos's git log. `~/tfinder` starts with one commit that
  names the termos commit it came from. It isn't pushed anywhere unless the user asks.
- The `install.sh` name clash noted in the [distribution plan](2026-11-09-distribution.md), between the
  prototype's `install.sh` and `scripts/install.sh`, goes away.

### Example: typing `finder ~/docs` after the move

```
 shell                    ~/.local/bin/finder ──link──▶ ~/tfinder/finder
   1 finder ~/docs  ──────────────────────────────────▶ HERE=~/tfinder; writes a zellij layout
                                                         2 left pane:  ~/tfinder/bin/files (yazi, ~/tfinder/yazi/)
                                                         3 right pane: python3 ~/tfinder/preview.py
   4 Enter in yazi ──▶ ~/tfinder/yazi/plugins/tfinder.yazi ──▶ ~/tfinder/bin/send ──fifo──▶ preview.py opens nvim
```

`finder` finds its files through `readlink -f "$0"`, so the move needs no edits to the scripts.

## Options considered

- **Loose in `~/`:** that's what the user said, but it puts `bin/`, `yazi/`, `nvim/` and others into the home
  folder. Not chosen; a single folder is the same idea without the clutter.
- **A new GitHub repo for the prototype:** possible later. The user didn't ask, and the prototype is only
  for this machine.
- **Keeping its full history** (`git subtree split` or `filter-repo`): more work for a prototype whose history
  stays readable in termos. Not chosen.

## Steps

- [x] Copy the prototype files to `~/tfinder`, with a `README.md`, a `CLAUDE.md` holding the prototype rules,
  `LICENSE` (MIT) and a `.gitignore`. Then `git init` and one commit.
- [x] Point `~/.local/bin/finder` at `~/tfinder/finder`.
- [x] Smoke-test in a throwaway zellij session (`tftest-move`): `finder` opens its tab, and both panes run
  from `~/tfinder`. Then end the session.
- [x] In termos:
  - `git mv nvim/init.lua crates/tos/nvim/init.lua` and fix the `include_str!` path
  - `git rm` the rest of the prototype
  - remove the leftover `__pycache__/`
- [x] `README.md` becomes termos only. The Development part moves to the end. `CLAUDE.md` says where the
  prototype went, and the rebuild plan gets a note.
- [x] Check: `cargo build`, `cargo test --workspace` and clippy for both targets.

*Done 2026-11-10.* `~/tfinder` is at its first commit, and `~/.local/bin/finder` points there. In the throwaway
session `tftest-move`, `finder` opened its tab with `~/tfinder/bin/files` and `~/tfinder/preview.py`
running, and the session was then deleted. Two finder tabs the user had opened earlier (in `prbot` and
`outstanding-duck`) still run from `~/termos/bin/…`. They can't open files for editing until they're
reopened with `finder`.

**After renaming the repo folder,** the first `cargo test` failed in `alacritty_terminal` (45 tests) and
`finder-core` (4 PDF tests). Their test binaries were unchanged since before the folder rename, so cargo
didn't rebuild them, and they still read their test data through `env!("CARGO_MANIFEST_DIR")`, which was
`~/terminal-browser/…`. `cargo clean -p alacritty_terminal -p finder-core -p tos` fixed it, and every test
passes. Moving the folder again needs the same clean.
