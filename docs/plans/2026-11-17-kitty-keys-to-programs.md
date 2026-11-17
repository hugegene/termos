# Plan: pass Ctrl and Shift on to programs (the kitty keyboard protocol in panes)

**Status:** built 2026-11-17 · **Date:** 2026-11-17 · **Decided by:** the user asked to "fix the tos passing of control and
shift" (2026-11-17) · **Finding:** [keys that clash between tos, Ghostty and agent CLIs](../findings/2026-11-17-agent-cli-key-clashes.md)

## Decision: A, speak the kitty keyboard protocol to each program that asks for it

**What changes:**
- **Panes answer the protocol.** Each pane's terminal turns on the fork's `kitty_keyboard` setting. A program
  that asks (`CSI ? u`) gets an answer, and one that pushes flags (`CSI > 1 u`) has them recorded. The fork already
  keeps a stack of flags for each screen; `tos` just never turned it on.
- **`encode_key` follows the pane's flags.** With no flags, keys are encoded the classic way, exactly as today,
  so bash, dash and vim see no change. With flags, keys follow the kitty spec
  (<https://sw.kovidgoyal.net/kitty/keyboard-protocol/>):
  - **Flag 1 (disambiguate):**
    - Esc becomes `CSI 27 u`.
    - A key with Ctrl or Alt becomes `CSI <key> ; <mods> u`, with the key as its unshifted lowercase code point.
    - Enter, Tab and Backspace with a modifier become `CSI 13 / 9 / 127 ; <mods> u`.
    - Unchanged: Enter, Tab and Backspace without a modifier, Shift+Tab (`CSI Z`), and text typed with or without
      Shift. Arrows, Home/End, PgUp/PgDn, Insert/Delete and the F keys keep their CSI forms, which already carry
      modifiers.
  - **Flag 8 (all keys as escape codes):** every key becomes an escape code, plain letters and unmodified Enter
    included.
  - **Flag 4 (alternate keys):** a Shift+letter also reports the shifted letter (`CSI 97:65;2u`).
  - **Flag 16 (associated text):** with flag 8, a key that types text also reports that text.
  - **Flag 2 (event types):** presses only. `tos` asks the outer terminal for flag 1 only, so no repeats or
    releases reach it.
- **The outer terminal is unchanged.** `tos attach` keeps asking Ghostty (or kitty, WezTerm, …) for flag 1. That
  is enough for every row in the finding but Shift+Space, which the user dropped.
- **tmux mode's Ctrl+B Ctrl+B** goes through the encoder instead of writing `0x02` itself.
- **A fork fix the change makes reachable:**
  - In `push_keyboard_mode`, a full stack (4096 entries) trims `title_stack` instead of `keyboard_mode_stack`.
    With no titles pushed, `title_stack.remove(0)` panics. Before this change the code was unreachable, because
    `kitty_keyboard` was off.
  - The fix trims the right stack. It is recorded in the fork's `UPSTREAM.md`.

**Example: Shift+Enter, with Ghostty in front.**

```
 Claude Code's pane: at start, it pushed flag 1 (CSI > 1 u); tos's pane terminal recorded it

 ┌─────────┐ 1. ESC[13;2u  ┌─────────────────────────────────────┐ 2. ESC[13;2u ┌─────────────┐
 │ Ghostty │──────────────>│ tos server                          │─────────────>│ Claude Code │
 │         │               │ key: Enter + Shift                  │              │ → newline   │
 │         │               │ pane mode has DISAMBIGUATE_ESC_CODES│              │             │
 └─────────┘               └─────────────────────────────────────┘              └─────────────┘

 bash's pane: no flags pushed

 ┌─────────┐ 1. ESC[13;2u  ┌─────────────────────────────────────┐ 2. "\r"      ┌─────────────┐
 │ Ghostty │──────────────>│ tos server: no kitty flags          │─────────────>│ bash: Enter │
 │         │               │ → classic encoding, as before       │              │             │
 └─────────┘               └─────────────────────────────────────┘              └─────────────┘
```

**What it fixes** (encodings from the spec, not captured from Ghostty):

| You press | Before | After, in a pane that asked |
|---|---|---|
| Shift+Enter | `\r` | `ESC[13;2u` |
| Ctrl+Enter (once Ghostty's own binding is removed) | `\r` | `ESC[13;5u` |
| Ctrl+Shift+Z | `0x1a` (Ctrl+Z) | `ESC[122;6u` |
| Ctrl+Shift+B | `0x02` (Ctrl+B) | `ESC[98;6u` |
| Ctrl+Backspace | `0x08` | `ESC[127;5u` |
| Ctrl+M | `\r` | `ESC[109;5u` |
| Ctrl+= (once Ghostty's own binding is removed) | `=` | `ESC[61;5u` |

**Not in this plan:**
- Ctrl+G, which locked mode keeps for itself. **Decided 2026-11-17: Ctrl+G stays.** Alt+G was the only key free in
  all four CLIs, Ghostty and `tos`. The user turned it down, because Alt keys on macOS need Option set to act as
  Alt.
- `tos`'s own mode keys (Ctrl+T, O, S, …), which are a separate decision too.
- Ghostty's Ctrl+Enter, which is fixed in Ghostty's config (`keybind = ctrl+enter=unbind`).

## Options considered

| Option | Verdict |
|---|---|
| **A: kitty protocol per pane, as each program asks** | ✅ It's what Claude Code, Codex, Cursor and agy ask for, and the fork already tracks the flags |
| B: xterm's modifyOtherKeys (`CSI 27;<mods>;<key>~`) when a program sets `CSI > 4;2 m` | ❌ Of the four, only Codex asks for it, and only inside tmux. The kitty protocol covers them all |
| C: always send `CSI u` for keys with modifiers | ❌ Programs that didn't ask would get junk: Shift+Enter in bash would print `[13;2u` |
| D: also ask the outer terminal for flag 8, to fix Shift+Space | ❌ The user dropped Shift+Space. And every key, plain letters included, would then arrive as an escape code |

No measurement: the decision rests on the finding and on the code (`term/mod.rs` in the fork, `input.rs`). The byte
encodings come from the kitty spec. The end-to-end test checks the bytes a program actually receives.

## Steps

- [x] Fork: trim `keyboard_mode_stack` in `push_keyboard_mode`. Add the header note to `term/mod.rs` and list the
      change in `UPSTREAM.md`.
- [x] `server/pane.rs`: create each pane's `Term` with `kitty_keyboard: true` (`term_config`). A unit test pushes
      flag 1, reads the answer, and pushes 4100 times.
- [x] `server/input.rs`: the kitty encoder, used when the pane has any kitty flag. Unit tests for the rows above,
      Esc, plain text, flags 8, 4 and 16, and no change without flags.
- [x] `server/keys.rs`: tmux mode's Ctrl+B Ctrl+B through the encoder.
- [x] End-to-end test (`kitty_keys_reach_programs_that_ask`): the test terminal speaks the kitty protocol, as
      Ghostty does.
  - A pane program pushes flag 1 and asks for the flags. It reads `ESC[?1u`, then receives Shift+Enter as
    `ESC[13;2u`.
  - After it pops the flags, Shift+Enter arrives as `\r`.
- [x] Docs: `input.rs`'s header comment, the "no kitty keyboard protocol" gap in
      [build steps 0–3](2026-11-03-build-steps-0-3.md), and a paragraph in the README's keys section (with
      Ghostty's Ctrl+Enter).
- [x] Build, the whole test suite, clippy and `cargo fmt`.

## Change log

- **2026-11-17:** written.
- **2026-11-17:** built. Both new tests fail without the change:
  - The pane test panics in the fork's `push_keyboard_mode` against the upstream code.
  - With `kitty_keyboard: false`, the end-to-end test times out: the program reads `\r` (`got:^M`).
  - The whole suite, clippy and `cargo fmt` pass.
  - The end-to-end tests must run outside a `tos` session: `Screen::spawn` passes on `TOS_SESSION`, and
    `tos attach` then refuses to attach ("already inside session").
