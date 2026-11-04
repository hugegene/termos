# Terminal Finder

A two-pane file finder in one zellij tab: **yazi** on the left for moving around, a
**preview/edit pane** on the right. Built for markdown, text, CSV and images.

```
┌ files (yazi) ──────┬ preview ─────────────────────────────────┐
│ ↑↓ move            │ follows the cursor: glow / bat / table /  │
│ Enter / e  edit ───┼→ editor opens here; quit → back to files  │
└────────────────────┴───────────────────────────────────────────┘
```

## Install & run

```bash
~/terminal-browser/install.sh     # once: chafa, bat, visidata, html2text, w3m, Neovim + render-markdown, `finder` command
finder [folder]                   # inside zellij → new tab; outside → new zellij session
```

## Keys

| Where | Key | Does |
|---|---|---|
| files | `↑↓` `j k` | move — preview follows |
| files | `→ ←` `l h` | open folder / go up |
| files | `Enter` `e` | edit file in the right pane (on a folder: open it) |
| files | `o` | open with desktop app (html → your browser) |
| files / preview | `i` (preview also `s`) | rendered ⇄ source view (markdown, html, csv) — sticks while you browse |
| files | `q` | close the finder |
| preview | `↑↓` `j k`, `space` `b`, `g` `G` | scroll, page, top/bottom |
| preview | `e` / `o` / `h` `Esc` | edit / open / back to files |
| anywhere | `Alt+h` `Alt+l` | switch panes (zellij) |
| mouse | click folder / file | open folder / preview file |
| mouse | double-click or right-click file | edit in the right pane |
| mouse | click empty space in the parent column | go up |
| mouse | wheel | move cursor (files) / scroll (preview) |

| File | Preview | Edit |
|---|---|---|
| `.md` | glow | Neovim + render-markdown — stays rendered while you type, cursor line shows raw markdown |
| text / code | bat | Neovim |
| `.html` `.htm` | converted to markdown (html2text) and rendered with glow — same look as `.md`; w3m as fallback | Neovim (source) |
| `.csv` `.tsv` | table (numbers right-aligned) | VisiData — `e` edit cell, `Alt+s` save, `q` close |
| images | chafa | desktop app (Pinta/GIMP/Krita if installed, else default viewer) |

**Neovim here is set up like a notepad:** opens ready to type, `Alt+s` save, `Alt+q` close,
`Ctrl+z` undo. (`Ctrl+S`/`Ctrl+Q` are taken by zellij — and **Ctrl+Q quits all of zellij** in this setup.)


## tb: the Rust rebuild (work in progress)

`tb` is a working name. It's our own multiplexer with the finder built in: persistent sessions, tabs, split
panes, scrolling and search, and mouse support, all in one binary. See `docs/plans/2026-11-03-build-steps-0-3.md`
and `docs/plans/2026-11-04-zellij-keys-and-frames.md`.

```bash
cargo build --release
./target/release/tb               # attach to session "main", starting it if needed
./target/release/tb finder ~/docs # open a finder tab
./target/release/tb ls            # list sessions;  tb kill  ends one
```

Inside a session the keys are **zellij's** (your `~/.config/zellij/config.kdl` bindings), and each pane has a
frame like zellij's:

| Keys | Mode and what it does |
|---|---|
| `Ctrl+p` | **pane:** arrows/`hjkl` focus, `n` new, `d` down, `r` right, `x` close, `f` fullscreen, `z` frames, `c` rename |
| `Ctrl+t` | **tab:** arrows switch, `n` new, `x` close, `r` rename, `1`–`9`, `Tab` last, `s` sync, `b` break out, `f` finder |
| `Ctrl+n` | **resize:** arrows/`hjkl` grow that side, `HJKL` shrink it, `+` `-` |
| `Ctrl+h` | **move:** arrows/`hjkl` swap the pane with its neighbour, `n` `p` |
| `Ctrl+s` | **scroll:** `↓↑` `d` `u` `PgDn` `PgUp`, `s` search (`n` `p` `c` `w` `o`), `e` edit in the editor, `v` select with vi keys |
| `Ctrl+o` | **session:** `d` detach |
| `Ctrl+b` | **tmux keys:** `c` `%` `"` `,` `[` `o` `n` `p` `z` `x` `d`, plus `f` finder |
| `Ctrl+g` | **lock:** all keys go to the program until `Ctrl+g` again |
| `Ctrl+q` | quit: ends the session (asks first) |
| `Alt+n`, `Alt+arrows`, `Alt+=`/`Alt+-`, `Alt+i`/`Alt+o` | new pane, focus, resize, move the tab |

`Esc` or `Enter` leaves a mode; `?` in any mode shows all keys. Not in `tb` yet: floating, stacked and pinned
panes, swap layouts, pane groups and zellij's plugins.

- **Mouse:** click to focus, click tabs, drag the lines between panes to resize, wheel to scroll back, drag to
  select and copy (OSC 52).
- **Finder:** `j`/`k` move, `Enter` edits in Neovim (`Alt+s` save, `Alt+q` close), `/` filters, `.` shows
  hidden files.

## Images

zellij only passes through **sixel** images. Ghostty and Terminator don't do sixel, so images are
drawn with colored blocks (recognisable, not sharp); press `o` for the real thing. In a sixel
terminal (WezTerm, Konsole, xterm -ti vt340) it switches to real images automatically when zellij
was started from it, or force it:

```bash
TFINDER_IMAGES=sixel finder     # or =symbols
```

## How it works

- `finder` writes a one-off zellij layout + a run dir (`$XDG_RUNTIME_DIR/tfinder/<id>/`) holding a fifo.
- Left pane `bin/files` runs yazi with this repo's `yazi/` config (your `~/.config/yazi` is untouched)
  and a fixed `--client-id`. `yazi/init.lua` replaces yazi's click handler with the mouse behaviour above.
- Right pane `preview.py` listens to `ya sub hover,cd` (filtered to that client id) and renders.
- `Enter`/`e`/`o` in yazi → `yazi/plugins/tfinder.yazi` → `bin/send` → fifo → `preview.py` runs the
  editor in its own pane, then focuses yazi again via `zellij action focus-pane-id`.

Files: `finder` · `bin/files` · `bin/send` · `preview.py` · `yazi/` · `nvim/init.lua` · `visidata/visidatarc` · `install.sh`
