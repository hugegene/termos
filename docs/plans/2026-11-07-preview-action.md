# Plan: `tb action preview FILE`, so other file browsers can drive the preview

**Status:** proposed · **Date:** 2026-11-07 · **Related:** [the CLI as the extension API](2026-11-05-cli-extensions.md),
[the finder as two panes](2026-11-05-finder-divider.md), [Markdown links](2026-11-07-markdown-links.md) (adds the
finder's `show(path)`), the prototype's yazi glue (`preview.py`, `yazi/`) · **Findings:**
[one `tb action` call](../findings/2026-11-07-tb-action-latency.md)

## Decision

**A new action, `tb action preview [-p ID] [--edit] PATH`, shows PATH in a preview pane.** Any program can call
it, so yazi, lf or a script can be the file list while `tb` draws the preview: text, Markdown, CSV, images, PDFs,
and later videos and Office files.

- **What it shows:** PATH, a file or a folder, exactly as the finder's preview would, with the same keys once you
  focus the pane.
  - A relative PATH is resolved against the caller's working folder. The client does this before sending, since
    the server's folder is different.
  - A missing PATH exits with status 1 and `no such file: PATH`.
- **Which pane:**
  - **`-p ID`:** a finder (its list or its preview pane), or a lone preview pane (below).
  - **Without `-p`:** the first finder or lone preview in the caller's tab. The caller's tab is the one holding
    `TB_PANE_ID`, or the active tab when it's called from outside `tb`.
  - **If there's none, it opens a lone preview pane** to the right of the caller's pane, taking 65% of its width
    as a finder's preview does.
  - **Output:** it always prints the pane's id (`preview_7`), so a script can pass `-p` after the first call.
  - **Focus stays with the caller.** yazi keeps the keyboard.
- **On a finder,** the file list moves to PATH's folder and selects it, using `show(path)` from the Markdown
  links work. List and preview stay in step.
- **A lone preview pane** is a finder with no file list: one pane that only previews.
  - **Keys:** the preview's keys work in it: scrolling, `t` and zoom on PDFs, links and `Backspace`, and Enter to
    edit in Neovim inside the pane.
  - **Leaving it:** `h`, `Esc`, `q` and `Tab`, which go back to the list in a finder, focus the pane that last
    called `preview` (yazi), if it's still open.
  - **Saved with the session:** it comes back showing the same file.
  - **In listings:** `list-panes` calls it `preview`, and its id prints as `preview_7`. `tb` already accepts that
    prefix.
- **`--edit`** opens PATH in the pane's Neovim, as Enter does in the finder. This is the prototype's "edit in the
  right pane", which yazi uses on double-click.
- **One call per hover is fast enough:** a `tb action` call takes about 3 ms (finding), so there's no stdin
  mode.
  - Callers must send calls one after another, not as background processes. Calls started together can arrive
    out of order and leave the preview on an older file.
- **yazi glue, as an example rather than a built-in:** `docs/examples/yazi-preview.py`, which you run in a `tb`
  pane, or bind to a key with `Run`. It:
  1. starts yazi with its own `--client-id`
  2. opens the lone preview with `tb action preview DIR`, and keeps the id
  3. follows yazi with `ya sub hover,cd`, keeping only its own yazi's events (as the prototype's `preview.py`
     does), retrying until yazi is up
  4. calls `tb action preview -p ID URL` for each hover, one at a time
  5. closes the preview pane when yazi exits

  The docs suggest hiding yazi's own preview column (`[mgr] ratio = [1, 4, 0]`), as the prototype's config does.
- **Not zellij's:** zellij has no such action, so this is `tb`'s own, like `set-status`.
- **Protocol:** a new command means a new protocol version (4). If the
  [video plan](2026-11-07-video-and-office-previews.md) or the [PDF scrolling plan](2026-11-07-pdf-scroll-source-rect.md)
  is built at the same time, they share the change.
- **Size:** about 400–600 lines plus tests, and about 100 lines of example (**estimate**).

## Options considered

| Topic | Option | Verdict |
|---|---|---|
| What yazi drives | Only a full finder: the list follows yazi | ❌ Two file lists side by side |
| | **A lone preview pane, and also finders** | ✅ The lone pane is a finder without its list, so all the preview code is shared |
| | A new kind of pane with its own preview code | ❌ A second copy of the preview code |
| No pane to show in | Exit with an error | ❌ Every script would need a setup step |
| | **Open a lone preview beside the caller** | ✅ The first call sets it up |
| Feeding paths | **One call per path** | ✅ About 3 ms a call (finding) |
| | A long-running `--stdin` mode | ❌ Not needed at that speed; one more mode to keep stable |
| Following yazi | **An example script using `ya sub`** | ✅ The prototype's tested method; it stays outside `tb` |
| | A yazi Lua plugin that calls `tb` on each hover (`ps.sub`) | ❌ yazi's `shell` runs each call as its own process, so calls could arrive out of order |
| | `tb` running `ya sub` itself | ❌ Ties `tb` to one file browser |
| Editing from yazi | **`--edit`** | ✅ Small; matches the prototype |
| | Only `tb action edit`, which opens a new pane | ❌ Not where you're looking |

## Steps

- [ ] **1. CLI** (`cli.rs`): `preview` with `-p/--pane`, `--edit` and `PATH`.
  - The client makes PATH absolute and sends it, with the caller's pane from `TB_PANE_ID`.
  - Help text and examples (`after_long_help`): driving it from a script, and the yazi example.
- [ ] **2. Server** (`server/actions.rs`, `server/finder/`):
  - Find the target pane as described above.
  - On a finder: `show(path)`.
  - The lone preview: a `Finder` flag that leaves out the list, so the whole pane is the preview.
    - It's laid out by splitting the caller's pane to the right, at `1 - LIST_SHARE`.
    - `FocusFiles` from it focuses the last caller.
    - `FinderState` gains `#[serde(default)] lone: bool`.
  - `--edit`: the finder's `OpenEditor`.
  - Bump `PROTOCOL_VERSION` (shared, see above).
- [ ] **3. Example:** `docs/examples/yazi-preview.py`, as above, with a short header on how to run and bind it.
- [ ] **4. Tests:**
  - **End to end** (`crates/tb/tests/e2e.rs`), from a shell pane:
    - `tb action preview notes.md` opens a lone preview showing its heading and prints `preview_N`; focus stays
      on the shell
    - `-p preview_N other.csv` shows the table
    - `-p` with a finder's id moves its list to the file
    - a missing file exits 1
    - `--edit` opens Neovim in the pane (skipped without Neovim, as the editor tests are)
    - Esc in the lone preview focuses the shell
    - kill and restore keep the lone preview and its file
  - **The example script,** with fake `yazi` and `ya` in `PATH`: the fake `ya sub` prints hover lines for two
    client ids, and only this client's files are previewed, in order.
- [ ] **5. Docs:**
  - `TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs`
  - a "Previews driven by other programs" section in `docs/cli.md`, which links the example
- [ ] **6. Check by hand (you):** yazi in a `tb` pane through the example script. Hover over Markdown, a PDF and
  an image; double-click to edit; quit yazi.

Build and test as CLAUDE.md says: `cargo build --offline`, `cargo test --workspace`, clippy,
`cargo fmt -p tb -p finder-core`.

**Order:** after the [Markdown links](2026-11-07-markdown-links.md) work is committed, because it adds `show(path)`
and edits the same files.

## Open questions

1. **Where a new lone preview opens:** to the right of the caller, as the plan says, or in a new tab?
2. **Should yazi's double-click (`--edit`) be part of the example from the start,** or only hover first?

## Change log

- **2026-11-07:** written, from the [`tb action` timing](../findings/2026-11-07-tb-action-latency.md). The user
  asked for `tb action preview FILE`, so another file browser such as yazi could drive the preview pane.
