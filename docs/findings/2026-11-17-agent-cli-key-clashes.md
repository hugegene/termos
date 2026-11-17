# Finding: keys that clash between tos, Ghostty and agent CLIs (2026-11-17)

## Question

The user found that Ctrl+Enter "in tos" turns on fullscreen and clashes with Claude Code, and asked to remove it.
They also asked which other keys may clash with Claude Code, Codex CLI ("ChatGPT CLI"), Cursor CLI and `agy`.

Two decisions depend on the answer:
- where Ctrl+Enter has to be removed: `tos`, or the terminal around it;
- which of `tos`'s own keys to drop or move. That is a later plan; this finding only lists the clashes.

## Method

Dev machine: Ubuntu 24.04, Linux 6.8. Software:
- Ghostty 1.3.1 (`1.3.1~ppa2-noble1`), with `~/.config/ghostty/config.ghostty` empty and `TERM_PROGRAM=ghostty`
- `tos` at `1fc0500` (0.4.4)
- Claude Code 2.1.326, `codex-cli 0.160.1` and `agy` 1.5.9, all in `~/.local/bin`
- Cursor CLI isn't installed, so it was checked from its docs only

**Where Ctrl+Enter is bound:**

```
grep -rn -i "ctrl+enter\|Ctrl+Enter\|KeyCode::Enter" crates/tos/src
ghostty +list-keybinds --default
```

**Which keys `tos` takes, and what it sends to programs:** the code, read directly.
- `crates/tos/src/server/keys.rs`, `global_key`: the keys taken in every mode but locked.
- `crates/tos/src/server/input.rs`, `encode_key`: the bytes a program receives.
- `crates/tos/src/client.rs`: what `tos attach` asks of the outer terminal.

**Claude Code's keys:** the default binding table in Claude Code's keybindings reference
(<https://code.claude.com/docs/en/keybindings>, as shipped with 2.1.326).

**Codex, Cursor CLI and `agy`:** read from their source, packages, help and docs. None was run in a terminal.
- **Codex 0.160.1:** the source at tag `rust-v0.160.1` from github.com/openai/codex. The defaults are in
  `codex-rs/tui/src/keymap.rs`, `built_in_defaults()` (lines 1643–1943). Terminal modes are in `tui/keyboard_modes.rs`.
- **Cursor CLI:** the package `2026.11.13-eacade1`, the version `downloads.cursor.com`'s install script pointed
  to. The minified `dist-package/1212.index.js` and `9494.index.js` were searched for key names and help text.
  Also its docs: <https://cursor.com/docs/cli/using>, `…/cli/reference/terminal-setup` and `…/cli/changelog`.
- **`agy` 1.5.9:** `agy help`, `agy changelog` and `strings ~/.local/bin/agy`. Also its docs:
  <https://antigravity.google/docs/cli/using>, `…/cli/prompting`, `…/cli/vim-editor-mode` and `…/cli/modes`.
- **Spot-checked by hand afterwards:**
  - Codex: Ctrl+T, G, O and S in `keymap.rs`.
  - Cursor: `{key:"Ctrl+G",description:"open prompt in $EDITOR"}` and "ctrl+o to expand" in `1212.index.js`.
  - `agy`: Ctrl+G, Ctrl+O, Alt+J and the Vim-mode Ctrl+S / Ctrl+Enter in its changelog.

## Results

### Ctrl+Enter is Ghostty's, not tos's

`tos` has no Ctrl+Enter binding: the grep finds `KeyCode::Enter` only in plain-Enter handlers. Ghostty's Linux
defaults (trimmed to the keys that matter here):

```
keybind = ctrl+enter=toggle_fullscreen
keybind = ctrl+shift+enter=toggle_split_zoom
keybind = ctrl+-=decrease_font_size:1
keybind = ctrl+==increase_font_size:1
keybind = ctrl++=increase_font_size:1
keybind = ctrl+0=reset_font_size
keybind = ctrl+,=open_config
keybind = ctrl+tab=next_tab
keybind = ctrl+shift+tab=previous_tab
keybind = ctrl+page_down=next_tab
keybind = ctrl+page_up=previous_tab
keybind = ctrl+alt+arrow_down=goto_split:down        (and left, right, up)
keybind = ctrl+shift+a / c / e / f / i / j / n / o / p / q / t / v / w   (select all, copy, splits, search, …)
keybind = alt+1 … alt+8=goto_tab:N,  alt+9=last_tab
keybind = shift+arrow_up / down / left / right=adjust_selection:…
keybind = shift+home=scroll_to_top,  shift+end=scroll_to_bottom
keybind = shift+page_up=scroll_page_up,  shift+page_down=scroll_page_down
```

`ghostty +list-actions --docs` says that `adjust_selection` "does nothing when there currently isn't one"
(a selection). Whether Ghostty then passes Shift+arrow on to the program is **unconfirmed**.

### tos turns every modified Enter into a plain Enter

`tos attach` asks Ghostty for the kitty keyboard protocol (`client.rs` pushes `DISAMBIGUATE_ESCAPE_CODES`). So
`tos` can tell Ctrl+Enter and Shift+Enter apart from Enter. It doesn't pass the difference on:

```
input.rs:73     KeyCode::Enter => with_alt(alt, b"\r".to_vec()),
input.rs:1-5    "This is xterm's legacy encoding … The kitty keyboard protocol comes later; until then our
                 terminal engine tells programs it isn't available."
```

So inside `tos`, Ctrl+Enter and Shift+Enter both reach the program as `\r`, which is Enter. With Ctrl and a letter, Shift
is dropped too: Ctrl+Shift+B arrives as Ctrl+B (`0x02`).

### tos's own keys (normal mode)

`global_key` takes these in every mode but locked:
- **Ctrl:** Ctrl+G (lock), Ctrl+P (pane), Ctrl+T (tab), Ctrl+N (resize), Ctrl+H (move), Ctrl+O (session),
  Ctrl+S (scroll) and Ctrl+Q (quit).
- **Alt:** Alt+N (new pane), Alt+←↓↑→ and Alt+H/J/K/L (focus), Alt+= Alt++ Alt+- (resize), Alt+I and Alt+O
  (move the tab).

Exceptions:
- Ctrl+S goes to the program in an editor `tos` started.
- Ctrl+B goes to the program since `0e16ee0`.
- Alt+F, Alt+[ ], Alt+P and Alt+T aren't bound.

### Claude Code 2.1.326: defaults that meet a tos or Ghostty key

| Key | Claude Code action (context) | Taken by |
|---|---|---|
| Ctrl+Enter | `chat:sendNow` (Chat) | Ghostty (fullscreen). Inside `tos`, also arrives as Enter |
| Shift+Enter | newline, when the terminal speaks the kitty protocol | `tos` sends `\r` = `chat:submit` |
| Ctrl+X Ctrl+S | `chat:sendNow`, the other key for it | `tos` (Ctrl+S → scroll mode) |
| Ctrl+G | `chat:externalEditor` (also Ctrl+X Ctrl+E) | `tos` (lock mode) |
| Ctrl+T | `app:toggleTodos` (Global); `theme:toggleSyntaxHighlighting`; `agents:togglePin` | `tos` (tab mode) |
| Ctrl+O | `app:toggleTranscript` (Global) | `tos` (session mode) |
| Ctrl+S | `chat:stash`; `historySearch:cycleScope`; `plugin:cycleMarketplace`; `agents:switchView` | `tos` (scroll mode) |
| Ctrl+P / Ctrl+N | `footer:up/down`, `select:previous/next`, `scroll:lineUp/lineDown` (↑ ↓ also work) | `tos` (pane / resize mode) |
| Ctrl+H | Backspace (Claude Code treats them as one key) | `tos` (move mode) |
| Alt+O | `chat:fastMode` | `tos` (move tab right) |
| Alt+↑ / Alt+↓ | `app:diffFileListUp/Down`, `agents:previous/nextGroup` (Ctrl+↑ ↓ also work) | `tos` (focus) |
| Ctrl+Shift+B | `app:toggleBrief` | `tos` sends Ctrl+B = `task:background` |
| Ctrl+- | `chat:undo` (Ctrl+_ also works) | Ghostty (font size) |
| Ctrl+X Ctrl+= / Ctrl+X Ctrl+- | `chat:increaseEffort` / `decreaseEffort` (Ctrl+X = / -, Shift+↑ ↓ also work) | Ghostty (font size) |

No clash: Ctrl+B (`task:background`, free since `0e16ee0`), Ctrl+Q, Ctrl+R, Ctrl+L, Ctrl+V, Ctrl+Y, Ctrl+J,
Ctrl+], Ctrl+X chords other than Ctrl+X Ctrl+S, Alt+P, Alt+T and Alt+W, Shift+Tab, Esc.

### Codex CLI 0.160.1: defaults that meet a tos or Ghostty key

```
keymap.rs (rust-v0.160.1), trimmed:
  open_transcript:      ctrl('t')            open_external_editor: ctrl('g')
  copy:                 ctrl('o')            history_search_next:  ctrl('s')
  move_up / move_down:  Up, ctrl('p') / Down, ctrl('n')
  edit_queued_message:  shift(Left), alt(Up) prompt_stack_back:    shift(Right), alt(Down)
  newline:              ctrl('j'), ctrl('m'), shift(Enter), alt(Enter)
  reasoning down / up:  alt(','), shift(Down) / alt('.'), shift(Up)
```

| Key | Codex action | Taken by |
|---|---|---|
| Ctrl+G | open the draft in `$EDITOR` | `tos` (lock) |
| Ctrl+T | transcript overlay | `tos` (tab mode) |
| Ctrl+O | copy the last agent response | `tos` (session mode) |
| Ctrl+S | history search, next | `tos` (scroll mode) |
| Ctrl+P / Ctrl+N | cursor up / down, list navigation (↑ ↓ also work) | `tos` (pane / resize mode) |
| Ctrl+H | delete a character back (Backspace also works) | `tos` (move mode) |
| Alt+↑ / Alt+↓ | edit queued message / prompt stack back (Shift+← → also work) | `tos` (focus) |
| Alt+← / Alt+→ | word left / right; with an empty composer, previous / next agent (Ctrl+← → also work) | `tos` (focus / tab) |
| Shift+Enter | newline | `tos` sends `\r`, which submits (Ctrl+J and Alt+Enter also work) |
| Ctrl+Backspace | delete a word back | `tos` sends `0x08`, which deletes one character |
| Ctrl+Shift+A | fullscreen, in approvals (Ctrl+A also works) | Ghostty (select all) |
| Shift+↑ / ↓ / ← / → | reasoning effort; queued message / prompt stack | Ghostty `adjust_selection`, only with a selection (**unconfirmed**) |

Codex pushes kitty flags (`CSI >5u` on Ghostty) when the terminal supports them, and falls back otherwise. The
current docs describe 0.169.0. From 0.163.0, Ctrl+X becomes a leader key (`tos` doesn't take Ctrl+X), and
0.166–0.168 add pane and zoom keys under it. None of that is in 0.160.1.

### Cursor CLI 2026.11.13: defaults that meet a tos or Ghostty key

```
1212.index.js:  {key:"Ctrl+G",description:"open prompt in $EDITOR"}  ·  "… lines hidden · ctrl+o to expand"
                ctrl_o toggle_compact · ctrl_r request_review · ctrl_y (resume) · ctrl_slash cycle_model
```

| Key | Cursor action | Taken by |
|---|---|---|
| Ctrl+G | open the prompt in `$EDITOR`; dismiss palettes | `tos` (lock) |
| Ctrl+O | compact / expanded output | `tos` (session mode) |
| Ctrl+T | transpose characters | `tos` (tab mode) |
| Ctrl+P / Ctrl+N | line up / down, prompt history (↑ ↓ also work) | `tos` (pane / resize mode) |
| Ctrl+H | delete a word back (Ctrl+W also works) | `tos` (move mode) |
| Alt+← / Alt+→ | word left / right (Ctrl+← → also work) | `tos` (focus / tab) |
| Shift+Enter | newline | `tos` sends `\r`, which submits (Ctrl+J, `\` then Enter, and Alt+Enter also work) |
| Ctrl+Backspace | delete to the start of the line (`CSI 127;5u`) | `tos` sends `0x08`, which deletes a word |

No clash: Ctrl+B twice (background the shell), Ctrl+R, Ctrl+Y, Ctrl+L, Ctrl+V and Ctrl+/. Cursor pushes kitty
flag 1 (`CSI >1u`).

### agy 1.5.9: defaults that meet a tos or Ghostty key

```
agy changelog, trimmed:
  "`ctrl+s` and `ctrl+enter` keys that submit from Normal mode" (Vim editor mode)
  "your external editor (`Ctrl+G`)"  ·  "tool groups … expanded with `Ctrl+O`"  ·  "`alt+j`, `ctrl+k`"
```

| Key | agy action | Taken by |
|---|---|---|
| Ctrl+G | open the prompt in the external editor; full-screen command editor in approvals | `tos` (lock) |
| Ctrl+O | expand / collapse tool details; fold hunks in `/diff` | `tos` (session mode) |
| Alt+J | next subagent waiting for confirmation | `tos` (focus down) |
| Ctrl+S | submit, in Vim mode | `tos` (scroll mode) |
| Ctrl+Enter | submit, in Vim mode | Ghostty (fullscreen). Inside `tos` it arrives as Enter |
| Shift+Enter | newline | `tos` sends `\r`, which submits (Ctrl+J, Alt+Enter, and `\` then Enter also work) |
| Ctrl+Shift+Z | redo | `tos` sends Ctrl+Z, which is suspend (or undo, per changelog 1.4.4: **unconfirmed**) |
| Ctrl+= / Ctrl+- | zoom, in the artifact viewer | Ghostty (font size) |
| Shift+↑ / Shift+↓ | half page | Ghostty `adjust_selection`, only with a selection (**unconfirmed**) |

No clash: Ctrl+B, Ctrl+K, Ctrl+R, Ctrl+L, Alt+M, Alt+S, Alt+X, Alt+V and Shift+Tab. `agy` is built on Bubble Tea v2,
which asks for the kitty protocol (`CSI ?u`). Its exact default key table can't be read from the stripped binary.

## Conclusions

- **Ctrl+Enter → fullscreen comes from Ghostty**, so the fix belongs in Ghostty's config:
  `keybind = ctrl+enter=unbind` in `~/.config/ghostty/config.ghostty`. Nothing in `tos` binds it.
- **Even then, Ctrl+Enter inside `tos` acts as Enter.** The same goes for Shift+Enter, because `tos` doesn't
  forward the kitty keyboard protocol yet. That's a `tos` gap, separate from its key bindings. All four CLIs use
  Shift+Enter for a newline and ask for the kitty protocol.
- **`tos`'s legacy encoding also drops Shift from Ctrl+Shift+letter, and turns Ctrl+Backspace into `0x08`.** The worst
  case is `agy`'s redo, Ctrl+Shift+Z, which arrives as Ctrl+Z.
- **Two `tos` keys clash with all four CLIs:**
  - Ctrl+G (`tos`'s lock) is every CLI's "open the prompt in an editor" key.
  - Ctrl+O (`tos`'s session mode) is a view toggle in Claude Code, Cursor and `agy`, and "copy" in Codex.
- **Other `tos` keys that clash:**
  - Ctrl+T: Claude Code, Codex and Cursor.
  - Ctrl+S: Claude Code, Codex and `agy`.
  - Ctrl+P / N and Ctrl+H: lists and editing; each CLI also has ↑ ↓ or Backspace.
  - Alt+arrows: Claude Code, Codex and Cursor. Alt+J: `agy`. Alt+O: Claude Code.
- **No clash found:** Ctrl+Q, Alt+N, Alt+I, Alt+= and Alt+-. Ctrl+B has been free since `0e16ee0`.
- **What's left after Ghostty is configured and the pane is locked (Ctrl+G).** Lock mode passes every key but
  Ctrl+G through `encode_key` (`keys.rs:59`, `input.rs`), so these remain:
  - **Ctrl+G** unlocks. Only Claude Code has another key for its editor (Ctrl+X Ctrl+E).
  - **The encoder** changes these keys:
    - Shift+Enter → Enter: all four submit.
    - Ctrl+Enter → Enter: Claude Code, `agy`.
    - Ctrl+M → Enter: Codex.
    - Ctrl+Shift+Z → Ctrl+Z: suspends `agy`.
    - Ctrl+Shift+B → Ctrl+B: Claude Code backgrounds the task.
    - Ctrl+Backspace → `0x08`: Codex, Cursor.
    - Ctrl+= → `=`, and Ctrl+- → `0x1f`: `agy`'s artifact zoom.
    - Shift+Space → Space: pages down instead of up in Claude Code's and Codex's transcripts.
      - **Corrected the same day:** this is probably not `tos`'s doing. Under the kitty protocol's flag 1, a key with
        only Shift sends its text, so Ghostty sends a plain space too. Only flag 8 ("report all keys") tells
        Shift+Space apart, and Codex asks Ghostty for flags 1 and 4. This is from the spec, not tested.
  - Forwarding the kitty protocol fixes every other encoder row. Ctrl+G needs its own change.
- **The engine already supports the protocol.** The `alacritty_terminal` fork has `Config::kitty_keyboard` (off by
  default; `tos` never sets it), the `TermMode` flags from `DISAMBIGUATE_ESC_CODES` to `REPORT_ALL_KEYS_AS_ESC`, and
  `report_keyboard_mode` (`term/mod.rs:75–104`, `:350`, `:1275`). What `tos` lacks is an encoder in `input.rs` that
  writes CSI u codes when a pane's flags ask for them.
- **Still unknown:**
  - whether Ghostty passes Shift+arrows through when nothing is selected;
  - `agy`'s full default table, which `/keybindings` in its TUI would write out;
  - bytes captured live: everything here comes from code, packages and docs.
