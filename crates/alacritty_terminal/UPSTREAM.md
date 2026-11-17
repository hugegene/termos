# Upstream: alacritty_terminal

This folder is our fork of `alacritty_terminal`, the terminal-emulation library from the Alacritty project.

| | |
|---|---|
| Upstream repo | https://github.com/alacritty/alacritty (folder `alacritty_terminal/`) |
| Version | 0.26.0, as published on crates.io |
| Upstream commit | `94e7c8874e526b1e67b349d9ba30ddf81669119e` (from the crate's `.cargo_vcs_info.json`) |
| License | Apache-2.0 (`LICENSE-APACHE`, unchanged) |
| Imported | 2026-11-04, unmodified apart from adding this file |

## Rules for changes (rebuild plan §3 and §7)

- Keep changes small and clearly marked.
- Every file we change gets a note at the top: `// Modified by <us> (<date>): <what changed>`.
- List each change below, so merges from upstream stay easy.
- Send generic fixes upstream.

## Changes from upstream

| Date | File | Change | Why |
|---|---|---|---|
| 2026-11-17 | `src/term/mod.rs` | `push_keyboard_mode` removes the oldest entry of `keyboard_mode_stack` when the stack is full; upstream removed from `title_stack` | With no title pushed, `title_stack.remove(0)` panics. A program that pushes 4096 keyboard modes would kill its pane's thread. `tos` turns on `kitty_keyboard` from this date, which makes the code reachable. Plan: `docs/plans/2026-11-17-kitty-keys-to-programs.md`. Generic: send upstream |
