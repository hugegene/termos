# Finding: step 0 toolchain, fork and `alacritty_terminal` API

**Date:** 2026-11-04 · **Machine:** the Ubuntu 24.04 dev box (16 cores, 31 GB RAM) ·
**Used by:** [build plan for steps 0–3](../plans/2026-11-03-build-steps-0-3.md)

## Question

Before writing steps 1–3 we needed to know:
1. Do the toolchain and the dependencies install and build, offline from my sandboxed shell?
2. Is the vendored fork identical to the published 0.26.0, and do its own tests pass?
3. Do all dependency licences pass the policy in the rebuild plan §7?
4. Which parts of the `alacritty_terminal` API will the session server use, and how do they behave?

## Method

- **Install (run by the user):**
  ```
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal -c clippy,rustfmt
  cargo fetch && cargo install --locked cargo-deny cargo-about
  ```
- **Fork:** copied `~/.cargo/registry/src/*/alacritty_terminal-0.26.0` into `crates/alacritty_terminal`, removed
  cargo's `.cargo-ok` marker, and checked the copy with `diff -r`. It replaces the registry crate through
  `[patch.crates-io]`.
- **Checks (offline):**
  - `cargo build --offline`
  - `cargo test --offline --workspace`
  - `cargo tree -i alacritty_terminal`
  - `cargo deny --offline check licenses`
  - `cargo deny --offline list -l license`
- **API:** read the vendored source with `grep` and `sed`:
  - `event.rs`, `event_loop.rs`
  - `tty/mod.rs`, `tty/unix.rs`
  - `term/mod.rs`, `term/cell.rs`, `term/search.rs`
  - `grid/mod.rs`, `selection.rs`, `vi_mode.rs`
  - plus `ratatui-core` 0.1.2 and `ratatui-crossterm` 0.1.2

## Results

### Toolchain and build

| Item | Result |
|---|---|
| rustc / cargo | 1.99.1 (2026-10-13) / 1.99.1 |
| cargo-deny | 0.20.2 installed |
| cargo-about | **Not installed:** "none of the package's binaries are available for install using the selected features" (needs `--features cli`) |
| Network from my shell | Blocked (`Could not resolve host: crates.io`). Offline builds from the fetched registry work |
| First `cargo build --offline` | 13.5 s wall time (2 m 15 s CPU), all dependencies |
| Dependencies of `tb` (normal) | 204 crates |

### Fork

| Item | Result |
|---|---|
| Published crate | 0.26.0, edition 2024, `rust-version = "1.85.0"`, licence Apache-2.0, `LICENSE-APACHE` included |
| Upstream commit | `94e7c8874e526b1e67b349d9ba30ddf81669119e`, path `alacritty_terminal` |
| Size | 211 files; 11,739 lines of Rust in `src/` |
| `diff -r` against the registry copy | Identical apart from `.cargo-ok` |
| Its tests | 132 + 45 + 1 passed, 0 failed |
| `cargo tree -i` | `alacritty_terminal v0.26.0 (…/crates/alacritty_terminal)`, so the fork is the copy in use |
| Warning | "resolver for the non root package will be ignored". It comes from the published `Cargo.toml`, which we leave unmodified |

### Licences

- **First run failed:** `option-ext` 0.2.0 is **MPL-2.0**. It came in through `dirs` → `dirs-sys`. We removed
  `dirs`; the server reads `XDG_RUNTIME_DIR`, `XDG_STATE_HOME` and `HOME` itself.
- **After that:** `licenses ok`.
- **Licences found** (crate counts; dual-licensed crates count under each):

  | Licence | Crates |
  |---|---|
  | MIT | 197 |
  | Apache-2.0 | 152 |
  | Unlicense | 9 |
  | Zlib | 9 |
  | Apache-2.0 WITH LLVM-exception | 4 |
  | BSD-3-Clause | 2 |
  | ISC | 2 |
  | 0BSD, BSL-1.0, CC0-1.0, Unicode-3.0 | 1 each |

  0BSD (`adler2`) and BSL-1.0 (`ryu`) pass because each crate also offers an allowed licence.

### `alacritty_terminal` 0.26.0 API we rely on

**Creating a pane:**
- `tty::new(&Options, WindowSize, window_id) -> Pty`.
  - `Options` has `shell: Option<Shell>`, `working_directory`, `drain_on_exit` and `env: HashMap`.
  - Without a shell it runs `$SHELL` (Linux).
  - It always sets `ALACRITTY_WINDOW_ID`, `WINDOWID`, `USER` and `HOME` in the child, and removes
    `XDG_ACTIVATION_TOKEN` and `DESKTOP_STARTUP_ID`.
- `EventLoop::new(Arc<FairMutex<Term>>, listener, pty, drain_on_exit, ref_test)`, then `.channel()` and
  `.spawn()`. That's **one thread per pane**.
- We send it `Msg::Input(bytes)`, `Msg::Resize(WindowSize)` and `Msg::Shutdown`.

**Events we receive** (`event::Event`):
- `Wakeup`, `Title`, `ResetTitle`, `Bell`
- `PtyWrite(String)`: the answer to a terminal query, which we must write back to the PTY
- `ClipboardStore` / `ClipboardLoad`
- `ColorRequest`, `TextAreaSizeRequest`
- `ChildExit(ExitStatus)`, `Exit`
- `CursorBlinkingChange`, `MouseCursorDirty`

**When the program in a pane exits:**
- The loop sends `ChildExit` (if a status is known), calls `term.exit()` (which sends `Exit`), sends `Wakeup`,
  then stops.
- When the `Pty` is dropped it sends `SIGHUP` to the child and **waits** for it. So we must drop it on the
  pane's own thread (detached), never on the server's main loop.

**Terminal state** (`Term`):
- Lifecycle: `Term::new(Config, &impl Dimensions, listener)` and `resize(impl Dimensions)`.
- Reading the screen:
  - `renderable_content()` gives `display_iter`, `selection`, `cursor` (shape `Hidden` when the program hid
    it; the vi cursor while in vi mode), `display_offset`, `colors` and `mode`.
  - `mode()` returns `TermMode`.
- Scrolling and copy mode: `scroll_display(Scroll::Delta/PageUp/PageDown/Top/Bottom)`, `toggle_vi_mode`,
  `vi_motion(ViMotion)`, `vi_goto_point`.
- Selection: `selection_to_string`, `bounds_to_string`.
- Search: `search_next(&mut RegexSearch, origin, Direction, Side, max_lines)`.
- Public fields: `selection: Option<Selection>`, `vi_mode_cursor` and `is_focused`.
- `Config` has these defaults:
  - `scrolling_history` 10000
  - `osc52`: `OnlyCopy`
  - `kitty_keyboard`: false
- `TermMode` covers:
  - cursor and keypad: `SHOW_CURSOR`, `APP_CURSOR`, `APP_KEYPAD`
  - mouse: `MOUSE_REPORT_CLICK`, `MOUSE_DRAG`, `MOUSE_MOTION`, `SGR_MOUSE`, `UTF8_MOUSE`, `ALTERNATE_SCROLL`
  - input: `BRACKETED_PASTE`, `FOCUS_IN_OUT`
  - screen: `ALT_SCREEN`, `VI`
  - the kitty keyboard flags

**Cells** (`Cell`):
- Fields: `c`, `fg`, `bg` (`vte::ansi::Color`: `Named`, `Spec(Rgb)` or `Indexed`), `flags` and
  `zerowidth()`.
- Flags:
  - style: `BOLD`, `DIM`, `ITALIC`, `INVERSE`, `HIDDEN`, `STRIKEOUT`
  - underlines: `UNDERLINE`, `DOUBLE_UNDERLINE`, `UNDERCURL`, `DOTTED_UNDERLINE`, `DASHED_UNDERLINE`
  - wide characters: `WIDE_CHAR`, `WIDE_CHAR_SPACER`, `LEADING_WIDE_CHAR_SPACER`

**Not in the library** (as expected from the rebuild plan §4):
- encoding keys and mouse events for a pane
- drawing to a real terminal

**`ratatui` 0.30.2:**
- `CrosstermBackend<W>` takes any writer and has `writer_mut()`.
- `Backend::draw(iter of (x, y, &Cell))` writes cell updates as terminal output.
- `Buffer::diff` returns the changed cells and skips the second half of wide characters.

## Conclusions

1. **Step 0 works offline.** The user fetches once; I build offline. A new dependency needs another
   `cargo fetch` from the user.
2. **The fork is intact** and is the copy in use. `UPSTREAM.md` records the version and commit.
3. **Licences pass** after dropping `dirs`. The MPL check caught the problem as designed.
4. **The plan's design holds.**
   - Each pane is a `Term` + `Pty` + `EventLoop` thread.
   - The server writes `PtyWrite` answers back to the PTY.
   - It drops the PTY away from the main loop.
   - It draws through `ratatui` into a byte buffer per client.
5. **To fix in the fork later (not urgent):** child processes see `ALACRITTY_WINDOW_ID`, which could make
   programs think they run in Alacritty. Record any change in `UPSTREAM.md`.
6. **Still open:** installing `cargo-about` with `--features cli`. CI can't run until the repo has a remote.
