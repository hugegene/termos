# Plan: save and close keys that work on Mac and Linux, and editing without Neovim

**Status:** built 2026-11-13 (see **Built**, at the end) · **Date:** 2026-11-13 · **Decided by:** the user chose S, `Ctrl+s` and `Ctrl+e`
(2026-11-13), over A (`Ctrl+k`) and over `Ctrl+s` with `Ctrl+q` · **Finding:**
[keys for saving and closing in the finder's editor](../findings/2026-11-13-editor-keys.md) · **Related:**
[macOS support](2026-11-09-macos-support.md) (its open item "the Option key as Alt")

## Decision: S

**S: `Ctrl+s` saves and `Ctrl+e` closes**, in `tos`'s Neovim and in the CSV grid. `Alt+s` and `Alt+q` keep working.
While the finder is editing, `tos` hands `Ctrl+s` to the editor; everywhere else `Ctrl+s` still enters scroll
mode, as in zellij.

**Why S over A** (`Ctrl+k` to save, the pick before `Ctrl+s` was looked at again):
- **The most familiar save key:** Ctrl+S on Linux and Windows, close to Cmd+S on a Mac. Editors people might set
  in `$TOS_EDITOR` use it too (an **estimate**, unchecked: micro, nano).
- **It hides nothing in `tos`'s editor.** Neovim's only default for it is LSP signature help, and the editor has
  no LSP ([finding](../findings/2026-11-13-editor-keys.md)). `Ctrl+k` would hide digraphs.
- **The cost is one exception in `tos`:** while editing, `Ctrl+s` saves instead of entering scroll mode.
  - **Nothing lost:** scroll mode is for a shell's output, and an editor redraws its whole pane and scrolls
    itself.
  - **The exception is small:** the editor and the grid already get keys through `send_key`
    (`mod.rs:1584`). Only `global_key`'s check for `Ctrl+s` (`keys.rs:131`) comes first.

Both S and A work on Mac and Linux without any terminal setting, as Ctrl is a plain key in every terminal.

**How `Ctrl+s` is routed under S:**

```
Ctrl+s pressed
  │
  ├─ 1. config.kdl binds Ctrl+s in this mode?             → that binding                 (unchanged)
  ├─ 2. normal mode, and the focused finder is editing
  │     (Neovim or the grid open)?                         → send_key → the editor: saves  (new)
  └─ 3. otherwise                                          → scroll mode, as in zellij     (unchanged)
```

**And without Neovim**, `Enter` on a text file opens the first of these (`editor.rs`):

```
$TOS_EDITOR ──▶ nvim with tos's config (Ctrl+s, Ctrl+e) ──▶ $VISUAL ──▶ $EDITOR ──▶ vi
```

So the hints must name the keys of the editor that actually opened.

## Example: editing `notes.md` under S

1. **`Enter` on `notes.md`:** `tos` starts `nvim -u ~/.local/share/termos/nvim/init.lua notes.md` in the
   preview pane. It's ready to type.
2. **Type, then `Ctrl+s`:**
   - The key isn't bound in `config.kdl`, the mode is normal, and the focused finder is editing.
   - So `send_key` types byte `0x13` into Neovim's pane.
   - `init.lua` maps `<C-s>` to `:write`, and the file is saved.
3. **`Ctrl+e`:** `tos` doesn't take it, so it reaches Neovim as `0x05`. `<C-e>` runs `:confirm quitall`, which
   asks first if anything is unsaved. Neovim closes, and the preview shows the saved file.
4. **`Ctrl+s` in a shell pane:** the finder isn't editing there, so it enters scroll mode, as before.
5. **In the CSV grid:** `Ctrl+s` saves (keeping a cell being edited first), and `Ctrl+e` closes, asking first if
   anything is unsaved, as `q` does.

## Options considered

| | Save | Close | For | Against |
|---|---|---|---|---|
| Now | `Alt+s` | `Alt+q` | no conflicts | on a Mac, needs the terminal's Option-as-Alt setting; otherwise types `ß` |
| — | `Ctrl+w` | `Ctrl+e` | free in `tos` | hides Neovim's delete-word while typing, a common habit |
| **A** | `Ctrl+k` (keep) | `Ctrl+e` | free in `tos`; hides only digraphs | unfamiliar |
| B | `Ctrl+y` | `Ctrl+e` | free in `tos` | takes `tos`'s redo (`Ctrl+z`/`Ctrl+y`); a Windows habit would save instead of redoing |
| **S (chosen)** | `Ctrl+s` | `Ctrl+e` | the familiar save key; hides nothing in the editor | needs the exception above |
| — | `Ctrl+s` | `Ctrl+q` | the most familiar pair (micro uses it); in Neovim, `Ctrl+q` only repeats `Ctrl+v` | `Ctrl+q` ends the session everywhere else, so it needs a second exception; pressing `Ctrl+q` twice, then `y` (as for Neovim's "save changes?"), would end the session and stop running programs. That's safe only with a guard against a quick second `Ctrl+q` and a clearer quit prompt |
| — | F2 | F10 | | need Fn on most Mac keyboards |
| — | Cmd+S | | | terminals don't pass Cmd to programs unless each is set to |

**`Ctrl+e` closes.** In Neovim it hides only "copy the character below" and "scroll down a line". It's free in
`tos` and the grid, it reads as **e**xit, and unlike `Ctrl+q` it can never reach `tos`'s "end the session?"
prompt.

## Without Neovim

Neovim isn't shipped: `scripts/install.sh` doesn't install it, and the README lists it as optional.

**The gaps** ([finding](../findings/2026-11-13-editor-keys.md)):
1. **The hints say `Alt+s` and `Alt+q` whichever editor opened** (`finder/mod.rs:1224`, `1324`). In `vi`,
   `Alt+s` replaces a character.
2. **Nothing says Neovim is missing,** although the README says "`tos` names the package when one is missing".
3. **The grid's messages say "E opens it in Neovim"** (`grid.rs:118`, `121`, `132`), even when it would be `vi`.

**The fixes:**
- **Hints that follow the editor:**
  - `tos`'s Neovim: "Ctrl+s save · Ctrl+e close".
  - Neovim isn't installed and it fell back to `vi`, `$VISUAL` or `$EDITOR`: name it and its usual keys, for
    example "editing in vi · Esc :wq saves and closes".
  - `$TOS_EDITOR`: "editing in <name> · its own keys".
- **A one-time tip** when Neovim is missing, as for poppler and ffmpeg: "install Neovim for Ctrl+s save and
  Ctrl+e close: `sudo apt install neovim` (`brew install neovim` on a Mac)".
- **The grid's messages:** "E opens it in your editor".
- **README:** the notepad-style editing needs Neovim; without it you get `$EDITOR` or `vi`.

## Steps

1. **`keys.rs`:** in normal mode, before `global_key`, send `Ctrl+s` to the focused finder when it's editing
   (`editor` or `grid` open). User bindings from `config.kdl` still come first. A test for each case.
2. **`crates/tos/nvim/init.lua`:**
   - map `<C-s>` to `:write` and `<C-e>` to `:confirm quitall`, in normal, insert and visual mode
   - keep `<M-s>` and `<M-q>`
   - the status line: "Ctrl+s save · Ctrl+e close · Ctrl+z undo"
3. **`grid.rs`:**
   - `Ctrl+s` saves; while a cell is being edited, it keeps the cell first
   - `Ctrl+e` closes, as `q` does, asking first if anything is unsaved
   - keep `Alt+s` and `Alt+q`
   - tests
4. **Hints:**
   - `finder/mod.rs:1224` and `1324`, `grid.rs:436` and `714`, following the editor actually running
   - `editor.rs` reports which kind it started
5. **The missing-Neovim tip,** shown once per session, through the same path as the poppler and ffmpeg
   messages.
6. **Docs:**
   - the README (the "Instead of…" row, **Finder**, **Editing**, the optional programs)
   - `docs/cli.md` and `docs/cli/` if any help text names the keys
   - `TOS_UPDATE_DOCS=1 cargo test -p tos --bin tos cli_docs`
7. **Checks:** `cargo test --workspace`, clippy, `cargo fmt -p tos -p finder-core`.
8. **The demo:**
   - change `alt+s` and `alt+q` in `scripts/demo/record.py`, and its captions, to `ctrl+s` and `ctrl+e`
   - re-record with the user's media (one Claude request)
9. **Still to check on a real Mac:** `Ctrl+s` and `Ctrl+e` in Terminal.app, iTerm2 and Ghostty. This is from the
   macOS plan's by-hand list.

## Built (2026-11-13)

- **Routing:** `keys.rs` sends `Ctrl+s` in normal mode to the focused pane when `Server::focused_is_editor` says
  it's an editor `tos` started, or a finder with its editor or grid open.
  - `TermPane::editor` marks the editors. `Server::mark_editor` sets it for the finder's editor, `tos edit` and
    `e` in scroll mode.
  - This covers more than step 1 said: those last two start `tos`'s Neovim too, whose status line says
    `Ctrl+s save`.
- **`editor::command`:** returns which editor it picked (`Ours`, `Chosen`, `Fallback`).
  - The finder's caption and the editor's frame use `Editor::keys`.
  - The first fallback in a session flashes `editor::missing_neovim`.
- **The grid:**
  - `Ctrl+s` and `Alt+s` save; `Ctrl+e`, `Alt+q` and `q` close.
  - Both keep a cell being edited first.
  - In a spreadsheet, `Ctrl+s` says it's read-only.
- **Checks:**
  - `cargo test --workspace`: all pass, including three new grid tests, the end-to-end CSV test (now
    `Ctrl+s`, `Ctrl+e`), and a new end-to-end `editing_without_neovim_names_the_editor`. That test is
    skipped where Neovim is in `/usr/bin` or `/bin`.
  - The ignored `real_neovim` test passes with Neovim 0.12.6: `Ctrl+s` saved, and `Ctrl+e` asked and closed.
  - Clippy and `cargo fmt --check` are clean.
  - Run the tests with `TOS_SESSION` and `TOS_PANE_ID` unset when inside a `tos` session. Otherwise every
    end-to-end test fails with "already inside session".
- **The demo:** re-recorded with the new keys and captions.
  - `finder-demo.gif`: 1.1 MB, 94 s
  - `finder-demo.webp`: 1.4 MB
  - `demo-short.gif`: 0.8 MB, 29 s
- **Not checked yet:** a real Mac (Terminal.app, iTerm2, Ghostty).

