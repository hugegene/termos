# Finding: keys for saving and closing in the finder's editor (2026-11-13)

## Question

Mac terminals don't send Option as Alt unless set to, so the finder's `Alt+s` (save) and `Alt+q` (close) don't
work out of the box there. Which Ctrl keys could replace them on both Mac and Linux? Three things decide it:
- what each candidate already does in Neovim
- whether `tos` takes it for itself
- what happens when Neovim isn't installed

The [plan](../plans/2026-11-13-editor-save-keys.md) depends on it.

## Method

Dev machine (Ubuntu 24.04), Neovim v0.12.6, `tos` source at `0f15460`.

- **Neovim's own behaviour:** `nvim --clean --headless -S <script>`, with no config loaded. Each case set the
  buffer to `['abcdef', 'XYZ']`, fed keys with `feedkeys(…, 'xt')`, then recorded the lines and
  `reg_recording()`. Also `maparg(key, mode, 0, 1)` for the built-in mappings, and `:help` for `CTRL-Z`.
- **`tos`'s keys:** read `crates/tos/src/server/keys.rs` (which keys are caught before the focused pane),
  `crates/tos/src/server/mod.rs` (`send_key`), `crates/tos/src/server/finder/grid.rs` (the grid's keys) and
  `crates/tos/nvim/init.lua` (the editor's mappings). Then `grep` for `ctrl(key, 'w'|'e')` and for the hint
  texts.
- **Without Neovim:** read `crates/tos/src/server/editor.rs`.

## Results

**Fed keys, in plain Neovim:**

```
insert: Ctrl+y              lines=['abcdef', 'XYZd']    copies the character above
insert: Ctrl+e              lines=['abcdef', 'XYZ']     (no character below at that column)
insert: Ctrl+z              lines=['abcdef', 'XYZ']     nothing
insert: Ctrl+k e '          lines=['abcdef', 'XYZé']    a digraph
insert: Alt+s then Q        lines=['abcdeQ', 'XYZ']     = Esc, then s (replace the character)
normal: Alt+s then Q        lines=['Qbcdef', 'XYZ']     = s
normal: Alt+q then a        recording=[a]               = Esc, then q (records a macro)
insert: Ctrl+s then Q       lines=['abcdef', 'XYZQ']    nothing
normal: Ctrl+s              lines unchanged             nothing
normal-mode Ctrl+z          built-in: suspend Neovim (:help CTRL-Z)
```

**Built-in mappings** (`maparg`):

```
i <C-s>  vim.lsp.buf.signature_help()   (LSP only; tos's editor has no LSP)
i <C-w>  :help i_CTRL-W-default         (delete the word before the cursor)
n <C-s>, i/n <C-k>, i/n <C-e>, n <C-w>   none (built-in behaviour only)
```

**`tos`:**
- **Keys caught before the focused pane, in every mode but Locked** (`keys.rs:116`):
  - `Ctrl+g`, `p`, `t`, `n`, `h`, `o`, `s` (scroll mode), `b` and `q` (quit)
  - the `Alt` shortcuts (`n`, the arrows, `hjkl`, `=` `+` `-`, `i`, `o`)
- **Not caught:** `Ctrl+w`, `Ctrl+e`, `Ctrl+k` and `Ctrl+y`.
- **Bindings from `config.kdl`** are checked before all of these (`keys.rs:51`).
- **How keys reach the finder:** in normal mode, keys go to `send_key` (`mod.rs:1584`).
  - While a file is open in the editor, it types them into the editor's pane.
  - Otherwise it calls the finder's `handle_key`, which hands them to the grid when the grid is open.
- **The grid ignores every Ctrl and Alt key but `Alt+s` and `Alt+q`** (`grid.rs:463`, `467`, `512`, `518`).
- **The editor's config** maps `Alt+s` (`:write`), `Alt+q` (`:confirm quitall`), `Ctrl+z` (undo) and `Ctrl+y` (redo)
  (`init.lua:23`–`30`).
- **The hints that name `Alt+s`:**
  - `finder/mod.rs:1224` ("editing · Alt+s save · Alt+q close")
  - `finder/mod.rs:1324` (the editor's frame)
  - `grid.rs:436` and `grid.rs:714`
  - `init.lua`'s status line
  - the README

**Without Neovim** (`editor.rs:38`–`60`):
- **The order:** `$TOS_EDITOR` if set; else `nvim -u <tos's config>` with `NVIM_APPNAME=tfinder-nvim`; else
  `$VISUAL`, `$EDITOR` or `vi`.
- **No message** says Neovim is missing.
- **The hints still say `Alt+s` and `Alt+q`,** whichever editor opened.
- **The grid's messages say "E opens it in Neovim"** (`grid.rs:118`, `121`, `132`).
- **Neovim isn't installed by `scripts/install.sh`.** The README lists it under optional programs.

## Conclusions

- **`Alt+s` and `Alt+q` aren't Neovim commands,** only `Esc` plus a letter. So mapping them hid nothing, and
  unmapped they do surprising things.
- **What each Ctrl key would hide in `tos`'s editor:**

  | Key | What it would hide |
  |---|---|
  | `Ctrl+s` | nothing (no LSP), but `tos` takes it for scroll mode, so it needs an exception while editing |
  | `Ctrl+k` | digraphs only |
  | `Ctrl+e` | "character below" and "scroll down a line" |
  | `Ctrl+y` | `tos`'s redo |
  | `Ctrl+w` | delete-word, a common habit |

- **An exception for `Ctrl+s` is a check before `global_key`.** The editor and the grid already get their keys
  through `send_key`.
- **Without Neovim, editing works in `vi` on most machines, but the hints are wrong.** On minimal systems with no
  `vi`, text can't be edited at all.
- **Not checked:** a real Mac; and editors set by `$TOS_EDITOR` (micro and nano also use `Ctrl+s` to save).

## Added later the same day: `Ctrl+q`

**Question:** could `Ctrl+q` close instead of `Ctrl+e`?

**Method:** the same headless Neovim script (`nvim --clean`), with `maparg('<C-q>', 'i'|'n', 0, 1)` and fed keys;
and `crates/tos/src/server/keys.rs` for `tos`'s quit prompt.

**Results:**

```
i <C-q> mapping: none (built-in behaviour only)
n <C-q> mapping: none (built-in behaviour only)
normal: Ctrl+q j d     lines=['bcdef', 'YZ']   a block selection, as Ctrl+v makes
```

- **In `tos`:** `Ctrl+q` opens the "end the session?" prompt in every mode but Locked (`keys.rs:142`).
- **In that prompt:** only `y` or `Y` ends the session, and any other key cancels it (`keys.rs:73`).
- **Neovim while typing:** the test didn't show what `Ctrl+q` does there, as the inserted character is
  invisible. Neovim documents it as inserting the next key literally, like `Ctrl+v`.

**Conclusions:**
- **In Neovim,** `Ctrl+q` only repeats `Ctrl+v`, so it would hide nothing unique.
- **In `tos`,** it would need a second exception.
- **The risk:** a double press could land on `tos`'s quit prompt. A `y` meant for Neovim's "save changes?" would
  then end the session.
- **The decision:** `Ctrl+e` closes instead ([plan](../plans/2026-11-13-editor-save-keys.md)).
