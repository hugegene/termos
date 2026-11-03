# Finding: iTerm2 image support in our image crate

**Date:** 2026-11-03 · **Used by:** [rebuild plan](../plans/2026-11-03-rebuild-plan.md)

## Question

We are adding iTerm2's inline image protocol to the rebuild, next to kitty and sixel. Does `ratatui-image`
(already in the plan, 11.1.0, MIT) support it, or do we need another crate or our own encoder?

## Method

- Read the crate docs at `https://docs.rs/ratatui-image/11.1.0/ratatui_image/` on 2026-11-03, through a web
  fetch. A direct `curl` to crates.io failed from the dev box (`Could not resolve host: crates.io`).
- Which phone terminals show iTerm2 images comes from the earlier
  [phone interface research](2026-11-03-phone-interface-research.md) §1. It was not re-checked.

## Results

From the `ratatui-image` 11.1.0 docs:
- **Protocols:** it unifies rendering "across Sixels, Kitty, and iTerm2 protocols".
- **Fallback:** "halfblocks", which "uses some unicode half-block characters with fore- and background colors".
- **Detection:** it first tries to "Guess by env vars". If that fails, it queries the terminal with control
  sequences, and falls back to halfblocks if no protocol is found. It also queries the font size in pixels.
  The quick start recommends `Picker::from_query_stdio()`.
- **Not mentioned:** tmux or other multiplexers, WezTerm, Ghostty.

From the phone research §1:
- **Blink (iOS):** iTerm2 images only, and only over plain SSH, not through tmux or Mosh.
- **ConnectBot (Android):** iTerm2 and kitty images.
- **Termux:** sixel and iTerm2 support merged in Sep 2026 but not yet released.

## Conclusions

1. **No new crate is needed.** `ratatui-image` already draws iTerm2 images for the finder, and detects
   support on its own.
2. **iTerm2 support also helps on phones.** Blink and ConnectBot can show real images instead of blocks.
   With Blink this works over plain SSH only.
3. **Not checked:**
   - whether `ratatui-image` lets us choose the order when a terminal supports several protocols
   - which desktop terminals besides iTerm2 accept iTerm2 images (WezTerm is believed to)
   - how `alacritty_terminal` handles iTerm2 image sequences (OSC 1337) printed by programs in a pane

   Check all three before roadmap step 5.
