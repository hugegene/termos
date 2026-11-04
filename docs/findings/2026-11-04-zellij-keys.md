# Finding: the user's zellij keys and pane frames (2026-11-04)

## Question

The user asked for `tb` to use "the zellij commands" and to draw a box around panes like zellij. Which key
bindings and frame settings does the user's zellij actually use? The key map in
[../plans/2026-11-04-zellij-keys-and-frames.md](../plans/2026-11-04-zellij-keys-and-frames.md) depends on it.

## Method

On the dev machine (Ubuntu, Linux 6.8), on 2026-11-04:

```sh
/snap/bin/zellij --version
grep -v '^\s*//' ~/.config/zellij/config.kdl | grep -v '^\s*$'
```

`zellij setup --dump-config` printed nothing from Claude's shell (likely the snap sandbox), so the defaults
weren't compared line by line. The user's config starts with `keybinds clear-defaults=true` and lists every
binding, so it is the complete key map.

## Results

- **Version:** `zellij 0.44.1`.
- **Other settings:** `theme "catppuccin-latte"` (a light theme), `default_mode "normal"`,
  `show_startup_tips false`.
  - The config sets no `pane_frames`, `rounded_corners`, `default_layout` or `simplified_ui`, so zellij's
    defaults apply: frames on, square corners, the default layout.
- **Key bindings, trimmed** (each `SwitchToMode "normal"` written as "→ normal"):

```
locked:   Ctrl g → normal
pane:     ←↓↑→ / h j k l MoveFocus · c rename pane (cleared) · d NewPane down → normal · e TogglePaneEmbedOrFloating
          · f ToggleFocusFullscreen · i TogglePanePinned · n NewPane · p SwitchFocus · r NewPane right
          · s NewPane stacked · w ToggleFloatingPanes · x CloseFocus · z TogglePaneFrames · Ctrl p → normal
tab:      ← ↑ h k previous tab · → ↓ l j next tab · 1-9 GoToTab → normal · [ BreakPaneLeft · ] BreakPaneRight
          · b BreakPane · n NewTab · r rename tab (cleared) · s ToggleActiveSyncTab · x CloseTab · Tab ToggleTab
          · Ctrl t → normal
resize:   ←↓↑→ / h j k l Increase <dir> · H J K L Decrease <dir> · + = Increase · - Decrease · Ctrl n → normal
move:     ←↓↑→ / h j k l MovePane <dir> · n Tab MovePane · p MovePaneBackwards · Ctrl h → normal
scroll:   e EditScrollback → normal · s → entersearch · Alt ←↓↑→ / Alt h j k l MoveFocus(OrTab) → normal
search:   c case sensitivity · n Search down · o whole word · p Search up · w wrap
scroll+search: PageDown → l Ctrl f page down · PageUp ← h Ctrl b page up · ↓ j / ↑ k line · d / u half page
          · Ctrl c ScrollToBottom → normal · Ctrl s → normal
entersearch: Enter → search · Esc, Ctrl c → scroll
session:  d Detach · w session manager · c configuration · p plugin manager · a about · s share · l layouts
          · Ctrl o → normal
tmux:     ←↓↑→ / h j k l MoveFocus → normal · space NextSwapLayout · " NewPane down · % NewPane right
          · , rename tab · [ → scroll · Ctrl b Write 2 · c NewTab · n next tab · o FocusNextPane · p previous tab
          · z fullscreen · d Detach · x CloseFocus
all but locked: Alt + = - resize · Alt [ ] swap layout · Alt f floating · Alt i / Alt o MoveTab · Alt n NewPane
          · Alt p / Alt Shift p pane groups · Ctrl g → locked · Ctrl q Quit
          · Ctrl p/t/n/h/s/o/b → pane/tab/resize/move/scroll/session/tmux mode (Ctrl s and Ctrl b differ in
            scroll and search; Ctrl b differs in tmux)
          · Alt ←→ / Alt h l MoveFocusOrTab, Alt ↓↑ / Alt j k MoveFocus (outside scroll)
all modes but normal/locked: Enter → normal; Esc → normal (rename modes: Esc undoes the rename)
```

## Conclusions

- `tb` copies this key map. Features `tb` doesn't have yet are listed as gaps in the plan: floating, stacked and
  pinned panes, swap layouts, pane groups, and the session-mode plugins.
- Frames: draw a square-cornered box around every pane, with the title in the top border.
- Unknown, not measured: zellij's exact frame colours per mode. The plan uses green for the focused pane and
  yellow while a mode is active. Those colours are an **estimate** of zellij's look.

## Addendum: default tab and pane names (2026-11-04)

**Question:** how does zellij name tabs and panes when the user hasn't named them? This decides whether `tb`'s
automatic names should copy zellij's.

**Method:** run in a throwaway background session, then delete it:

```sh
zellij attach --create-background tftest-names
zellij -s tftest-names action new-tab
zellij -s tftest-names action query-tab-names
zellij -s tftest-names action dump-layout
zellij delete-session tftest-names --force
```

**Results:** `query-tab-names` printed `Tab #1` and `Tab #2`. `dump-layout` showed `tab name="Tab #1"` and
`tab name="Tab #2"`, and each content pane as a plain `pane` with no name.

**Conclusions:**
- zellij numbers unnamed tabs `Tab #N`.
- Panes have no stored name until they are renamed. The frame title then comes from the program's title, or
  falls back to `Pane #N`. This fallback is zellij's documented behaviour; it wasn't checked here, because the
  frames weren't captured.
