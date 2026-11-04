# Plan: zellij keys and pane frames for `tb`

**Status:** built (2026-11-04) · **Date:** 2026-11-04 · **Changes:** the "Our keys" and "Mouse" rows of
[build steps 0–3](2026-11-03-build-steps-0-3.md) · **Finding:** [the user's zellij keys](../findings/2026-11-04-zellij-keys.md)

## Decision

The user asked for `tb` to "use the zellij commands" and to "draw a box on the outer edge like zellij".

1. **Keys:** replace the Ctrl+B prefix with **zellij's modes, using the user's own zellij 0.44.1 bindings**.
   - Ctrl+B still works, because zellij's "tmux" mode is part of those bindings.
   - Keys for features `tb` doesn't have yet are listed under **Not supported yet**.
2. **Frames:** draw a **box around every pane**, with the title in the top border, as zellij does.
   - The focused pane's box is green, and turns yellow while a mode (pane, tab, …) is active.
   - **Ctrl+P `z`** turns frames off and brings back the 1-cell separators.
   - The finder draws **two boxes** (the list and the preview), like the prototype's two zellij panes.
3. **CLI:** add zellij's names as aliases: `list-sessions`, `kill-session`, `action move-focus`,
   `action close-pane`, `action toggle-fullscreen` and `action write-chars`.

### Keys

| Mode (enter with) | Keys |
|---|---|
| **Normal** | Unbound keys go to the program. Alt+N new pane · Alt+←↓↑→ or Alt+H/J/K/L focus (Alt+←/→ go to the next tab at the edge) · Alt+= / Alt++ / Alt+- resize · Alt+I / Alt+O move the tab · Ctrl+Q quit |
| **Locked** (Ctrl+G) | Everything goes to the program except Ctrl+G |
| **Pane** (Ctrl+P) | ←↓↑→ / h j k l focus · p next pane · n new · d new below · r new right · x close · f fullscreen · z frames on/off · c rename pane |
| **Tab** (Ctrl+T) | ← ↑ h k previous · → ↓ l j next · 1–9 go to · n new · x close tab · r rename · s sync input to all panes · b break the pane out to a new tab · [ / ] move the pane to the tab on the left / right · Tab last tab · **f finder** (ours) |
| **Resize** (Ctrl+N) | ←↓↑→ / h j k l grow that side · H J K L shrink that side · + = grow · - shrink |
| **Move** (Ctrl+H) | ←↓↑→ / h j k l swap with the neighbour · n / Tab next · p previous |
| **Scroll** (Ctrl+S) | ↓ ↑ j k line · d u half page · PgDn PgUp l h → ← Ctrl+F Ctrl+B page · Ctrl+C bottom · s search · e edit scrollback in the editor · **v select with vi keys** (ours: the step 3 copy mode) |
| **Search** (Scroll, then `s`, type, Enter) | n down · p up · c case-sensitive · w wrap · o whole word, plus the scroll keys |
| **Session** (Ctrl+O) | d detach |
| **Tmux** (Ctrl+B) | c new tab · % split right · " split down · ←↓↑→ / h j k l focus · o next pane · n / p tab · z fullscreen · x close · [ scroll · , rename tab · d detach · Ctrl+B send Ctrl+B · **f finder**, **? help** (ours) |
| **All modes but Normal and Locked** | Esc or Enter back to Normal · Ctrl+<mode key> again leaves the mode · **? help** (ours) |

**Differences from zellij, on purpose:**
- **Ctrl+Q asks y/n before ending the session.** Without session restore (step 4), a stray Ctrl+Q would lose
  everything.
- **Unsupported Alt keys go to the program instead of being swallowed:** Alt+F (floating), Alt+[ / ] (swap
  layouts), Alt+P (groups). Alt+F is "forward word" in shells.
- **Search defaults:** case-insensitive, wrap on, whole word off. These defaults are an **estimate** of what is
  convenient; zellij's own defaults weren't checked.

**Not supported yet:**
- floating, stacked and pinned panes (pane `w` `e` `s` `i`)
- swap layouts (Alt+[ ], tmux space)
- pane groups (Alt+P)
- session-mode plugins (`w` `c` `p` `a` `s` `l`): `tb ls` lists sessions instead

### Frames

- Each pane gets its own box: `┌ title ───┐`. Neighbouring boxes touch, so two lines sit side by side, as in
  zellij.
- **Title:** the pane's name if it was renamed (Ctrl+P `c`), otherwise its title or program.
- **Scrolling:** a scrolled pane shows `SCROLL: n/total` at the right of its top border.
- **Resizing with the mouse:** dragging the two lines between boxes resizes the panes.
- **Minimum size:** a pane needs 3×3 cells with frames. A split that would leave less shows "not enough room".
- **Status bar:** stays one line.
  - In Normal mode it lists the modes (`Ctrl + <g> LOCK <p> PANE …`), shortened to fit.
  - In other modes it shows the mode and its keys.

## Options considered

| Topic | Option | Verdict |
|---|---|---|
| Keys | Keep Ctrl+B and add zellij keys beside it | ❌ The user asked for zellij's keys "instead"; zellij's tmux mode keeps Ctrl+B anyway |
| | **zellij's modes with the user's bindings** | ✅ Matches their habits |
| Status bar | Two lines like zellij's default layout | ❌ Costs a row on every screen (the phone later); one line can show the same, one mode at a time |
| | **One line** | ✅ |
| Frames | One box around the whole screen | ❌ zellij frames each pane, and the user's screens look like that |
| | **A box per pane, toggleable** | ✅ |
| Finder frame | One box around the whole finder | ❌ Less like the prototype |
| | **Two boxes (list, preview)** | ✅ |
| Copy mode | Drop it (zellij has no keyboard selection) | ❌ It works and is tested; `v` in scroll mode is a free key |

## Steps

- [x] `layout.rs`: frames mode (no gap, 3-cell minimum, 2-cell drag zones). Add swapping panes, resizing one
      side, and replacing a pane in place.
- [x] `render.rs`: draw frames; the scroll position goes into the frame.
- [x] Server modes and key handling as in the table above, plus sync input, break pane, move tab, last tab,
      rename pane and edit scrollback.
- [x] Scroll and search modes; the step 3 copy mode becomes "select" (`v` in scroll mode).
- [x] Finder: two boxes when frames are on.
- [x] Status bar and help for the modes.
- [x] CLI aliases.
- [x] Update the end-to-end tests to the new keys; add tests for modes, frames and search.

## Change log

- **2026-11-04:** written.
- **2026-11-04:** built.
  - Code: keys and the status bar are in `server/keys.rs`, scroll and search in `server/scroll.rs`, and frames
    in `layout.rs`, `render.rs` and the finder.
  - New end-to-end tests: `zellij_modes`, `scroll_mode_search` and `sync_move_tab_and_edit_scrollback`.
    The others moved to the new keys. With 20 unit tests in `tb` and 11 end-to-end tests, the suite passed 5 runs in a row,
    and so did the real Neovim test.
  - Two details found while building:
    - In Normal mode a status message briefly replaces the mode list, which otherwise fills the line.
    - At 80 columns the mode list uses a short form (`^ g lock p pane …`).
