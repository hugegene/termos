# termos

termos is a terminal workspace: our own multiplexer with a file finder built in. It has persistent sessions,
tabs, split panes, scrolling and search, and mouse support, all in one program, run as `tos`. It works on
Linux and on Macs with Apple Silicon.

```
 your terminal window              background (keeps running)
┌───────────────────┐  1 keys    ┌───────────────────────────────┐
│ tos (client)      │──────────▶ │ tos server, session "main"     │
│                   │ ◀──────────│  tab 1: shell │ shell          │
└───────────────────┘  2 screen  │  tab 2: finder                 │
                                 └───────────────────────────────┘
 close the window or detach: the server keeps your shells running; `tos` reconnects
```

The zellij prototype that termos grew from now lives in its own folder, `~/tfinder`.

## Install

```bash
curl -fsSL https://github.com/hugegene/termos/releases/latest/download/install.sh | sh
```

- **Where it runs:** Linux on x86_64 or arm64 with glibc 2.34 or newer (Ubuntu 22.04+, Debian 12+, RHEL 9+),
  and Macs with Apple Silicon.
- **Where it goes:** `~/.local/bin/tos`, without sudo. `TOS_INSTALL_DIR=<folder>` picks another folder, and
  `TOS_VERSION=0.2.0` a particular version. Run it again to upgrade.
- **PATH:** if that folder isn't on your `PATH`, it adds it to your shell's startup file, once. That's
  `~/.zshrc`, `~/.bashrc` (`~/.bash_profile` on macOS), fish's `conf.d/termos.fish`, else `~/.profile`.
  `TOS_NO_MODIFY_PATH=1` skips this and prints the line instead.
- **What it checks:** the download's SHA-256, and that `tos` runs, before it replaces anything.
- **Upgrading:** `tos update`. Each version lives in `~/.local/share/termos/versions/`, `~/.local/bin/tos`
  links to the current one, and the one before is kept. Sessions show a note in the status bar when a newer
  release is out (at most one check a day; `TOS_NO_UPDATE_CHECK=1` turns it off).
- **Optional programs:** Neovim, poppler (PDF), ffmpeg (video previews), mpv (playing videos) and gio or trash.
  `tos` names the package when one is missing.

```bash
tos               # attach to session "main", starting it if needed
tos finder ~/docs # open a finder tab
tos ls            # list sessions;  tos kill  ends one (attach again to restore it)
```

## Sessions

Session commands are **zellij's**, with the same names, aliases and flags:

```bash
tos attach work           # or: tos a work, tos -s work; starts "work" if needed (-c is accepted)
tos attach -b work        # start it in the background without attaching
tos attach --forget work  # forget its saved layout and start fresh
tos attach -f work        # when restoring, run the commands that were running
tos list-sessions         # or: tos ls; -s names only, -r reversed
tos kill-session work     # or: tos k work; keeps the layout for next time
tos delete-session work   # or: tos d work; forgets the saved layout (-f also ends a running session)
tos kill-all-sessions     # or: tos ka; -y skips the question
tos delete-all-sessions   # or: tos da; -y skips the question, -f also ends and forgets running ones
```

Unlike zellij, plain `tos` returns to "main" instead of starting a new session with a random name.

**Sessions come back.** `tos kill`, Ctrl+q, a crash or a reboot keeps the session's layout: tabs, splits, names,
folders and finder state. Attaching again restores it. A command that was running is typed at the prompt
without Enter. `tos ls` shows saved sessions, and `tos delete NAME` forgets one.

## Keys and mouse

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

`Esc` or `Enter` leaves a mode; `?` in any mode shows all keys. Not in `tos` yet: floating, stacked and pinned
panes, swap layouts, pane groups and zellij's plugins.

- **Mouse:** click to focus, click tabs, drag the lines between panes to resize, wheel to scroll back, drag to
  select and copy (OSC 52).

## Finder

- **Moving:** the arrows move; `←` and `→` move focus between the file list and the preview, and up and into
  folders. `Enter` in the list does what `→` does. `[` `]` page through a preview. `/` filters, `.` hides hidden
  files (shown by default).
- **Editing:** `Enter` in the preview edits in tos: Neovim for text (`Alt+s` save, `Alt+q` close), the grid for
  CSV and spreadsheets. Where tos can't edit a file, the caption doesn't offer `Enter`.
- **Opening in an app:** `o` opens any file or folder in its default app (`xdg-open`, or `open` on macOS), on the
  screen of the terminal you pressed it in.
- **The menu:** right-click a file, or press `m`, to rename it, copy its name or path, open it in its app, or
  delete it (to the trash; where there's no trash it asks first).

**Images** show for real in terminals that support kitty, iTerm2 or sixel images (Ghostty, kitty,
WezTerm, iTerm2, xterm with sixel), and as coloured blocks elsewhere. Force one with
`TOS_IMAGES=kitty|iterm2|sixel|blocks`. Enter in a `.csv`/`.tsv` preview opens a **grid editor**: arrows move,
`e` edits a cell, `a`/`A` add a row or column, `d`/`D` delete one, `u` undoes, `Alt+s` saves and `q` closes.
`v` (or `V` for whole rows, or a mouse drag) selects a range of cells, and `y` copies it as tab-separated text,
which spreadsheet apps paste as cells. `E` opens any text file in Neovim.

**PDFs** in the finder show their text; `t` shows the pages (`+`/`-` zoom, `0` fit width, `[` `]` page). This
needs poppler (`sudo apt install poppler-utils`, or `brew install poppler` on macOS).

**Videos** show their length, size and a frame from 10% of the way in, which needs ffmpeg
(`sudo apt install ffmpeg`, or `brew install ffmpeg`). `o` plays one in your default video player, as it opens
any file; with no default set, in mpv (`sudo apt install mpv`), or else ffplay. Attached over SSH without X
forwarding, there's nowhere to show it.

**Spreadsheets** (`.xlsx`, `.xlsm`, `.xlsb`, `.xls`, `.ods`) show their first sheet as a table, with formulas'
last values and dates as `2026-11-07`; `[` `]` in the preview change sheet. `Enter` in the preview opens the
sheet in the grid, **read-only** (`tos` can't save these formats), where you can select and copy cells.
**Word** (`.docx`) and **PowerPoint** (`.pptx`) files show as Markdown: headings, lists, tables, bold and italic,
and a section per slide. Old `.doc` and `.ppt` files aren't read.

**Links** in a Markdown preview can be clicked: the finder shows the linked file, and `Backspace` goes back.

## Extending

**Extending `tos`** works through the same command line, from any language; there are no plugins. See
[docs/cli.md](docs/cli.md) for the overview and [docs/cli/](docs/cli/README.md) for a page per command.
[prbot](https://github.com/hugegene/prbot) is a full-size example: it opens a Claude tab for each GitHub PR that
needs you.

```bash
tos action new-pane -d right -- make test    # zellij's actions, names and flags; prints terminal_7
tos action list-tabs -j                      # JSON with zellij's fields
tos subscribe -f json                        # events: tabs and panes opening, closing, exiting, …
tos action set-status --id ci "build ✓"      # text in the status bar; clicks arrive as events
tos run -- htop                              # also: tos edit FILE, tos watch NAME
```

Keys can run commands. Add zellij-style `bind` blocks to `~/.config/termos/config.kdl`, for example
`tab { bind "p" { Run "prbot" "ls"; SwitchToMode "Normal"; } }`. A zellij script such as `prbot` ports by
changing `zellij` to `tos` and `ZELLIJ_*` to `TOS_*`.

## Development

- **Build:** `cargo build --release` gives `target/release/tos`. `CLAUDE.md` lists the tests and checks.
- **Release:** set `version` in `Cargo.toml`, commit, then push a tag `v<version>`.
  `.github/workflows/release.yml` builds each target with `scripts/package.sh` and publishes the release
  with `scripts/install.sh`. See `docs/plans/2026-11-09-distribution.md`.

## License

MIT, see [LICENSE](LICENSE). `crates/alacritty_terminal` is a fork of Alacritty's terminal crate and stays
under its own license, Apache-2.0 ([crates/alacritty_terminal/LICENSE-APACHE](crates/alacritty_terminal/LICENSE-APACHE)).
