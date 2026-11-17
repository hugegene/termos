# Plan: Ctrl+B goes to the program; tmux's keys are opt-in

**Status:** built 2026-11-17 · **Date:** 2026-11-17 · **Decided by:** the user asked "can we remove control+b
for tmux in tos?" (2026-11-17) · **Changes:** the Tmux row of [zellij keys and frames](2026-11-04-zellij-keys-and-frames.md)
· **Finding:** [the user's zellij keys](../findings/2026-11-04-zellij-keys.md) (where Ctrl+B is bound)

## Decision: A, drop the default key and keep the mode

- **Ctrl+B no longer enters tmux mode.** In normal mode `tos` doesn't bind it, so it goes to the program like
  any other unbound key. A shell moves back one character and Vim pages up. Before, the program got Ctrl+B only
  if you pressed it twice.
- **Scroll and search mode are unchanged:** Ctrl+B still pages up there, as in zellij.
- **Tmux mode stays, off by default.** Its keys are unchanged (`c` `%` `"` `,` `[` `o` `n` `p` `z` `x` `d` `f`,
  and Ctrl+B to send a real Ctrl+B). It's reached by zellij's own binding in `~/.config/termos/config.kdl`:

  ```kdl
  keybinds {
      shared_except "locked" "scroll" "search" "tmux" {
          bind "Ctrl b" { SwitchToMode "tmux"; }
      }
  }
  ```

  or by `tos action switch-mode tmux`.

Nothing is lost without the binding: every tmux-mode key has a twin in pane or tab mode (`%` is Ctrl+P `r`, `c`
is Ctrl+T `n`, and so on).

**Example: the cursor is after `echo hello` in a shell, and you press Ctrl+B.**

```
 before (2026-11-04 to 2026-11-17)

 ┌──────────┐ 1. key Ctrl+B  ┌─────────────────────────────┐            ┌───────┐
 │ client   │───────────────>│ server: global_key          │  nothing   │ shell │
 │ (tos     │                │ Ctrl+B → mode TMUX          │ ─ ─ ─ ─ ─> │       │
 │  attach) │                │ status bar shows " TMUX "   │            │       │
 └──────────┘                └─────────────────────────────┘            └───────┘

 after

 ┌──────────┐ 1. key Ctrl+B  ┌─────────────────────────────┐ 2. byte    ┌───────┐
 │ client   │───────────────>│ server: no config binding,  │   0x02     │ shell │
 │          │                │ not a mode key → send_key   │───────────>│ cursor│
 │          │                │ mode stays NORMAL           │            │ ← one │
 └──────────┘                └─────────────────────────────┘            └───────┘
```

## Options considered

| Option | Verdict |
|---|---|
| **A: drop Ctrl+B's default binding; keep tmux mode for `config.kdl`** | ✅ Programs get Ctrl+B. tmux users get the keys back by copying zellij's binding, and a zellij config with a `tmux` block still loads |
| B: remove tmux mode entirely | ❌ Saves about 40 lines, but a `tmux { … }` block in a copied zellij config would be skipped with a warning, and tmux users would have no way back |
| C: move tmux mode to another key | ❌ Every Ctrl key takes something from programs; whoever wants one can bind it in `config.kdl` |

No measurement: the user decided, and where Ctrl+B is bound is already in the finding and in `keys.rs`.

## Steps

- [x] `server/keys.rs`: remove the Ctrl+B branch from `global_key`, and the `Ctrl+b` row from the help screen.
      Keep `Modal::Tmux`, `tmux_key`, its hints and the mode's name in `config.rs` and `actions.rs`.
- [x] End-to-end tests:
  - `zellij_modes`: Ctrl+B reaches the shell. `echo a`, Ctrl+B, `b`, Enter prints `ab`. Before, `tos` took
    Ctrl+B and `b`, and Enter only left tmux mode.
  - `config_key_bindings`: the opt-in above brings tmux mode back (Ctrl+B, then `c` opens a tab).
  - `tabs_by_key_and_click`: drop the Ctrl+B `c` step. Ctrl+T `n` already opens a tab there.
- [x] Docs: the README's keys table and "Instead of" row, the Pages site's two mentions, and the config section
      of `docs/cli.md` (the opt-in).
- [x] Note the change in [zellij keys and frames](2026-11-04-zellij-keys-and-frames.md) and in
      [build steps 0–3](2026-11-03-build-steps-0-3.md).

## Change log

- **2026-11-17:** written.
- **2026-11-17:** built. The new `zellij_modes` step fails against the old `keys.rs` (it times out
  waiting for `ab`), so it tests the change. The whole suite, clippy and `cargo fmt --check` pass.
  - The Ctrl+B step comes before the locked-mode step, because that step leaves a `^P` on the shell's line.
