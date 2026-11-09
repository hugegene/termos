# Plan: the finder as two panes, files and preview

**Status:** built, waiting for the check by hand (step 7) · **Date:** 2026-11-05 · **Related:** [build steps 4–5](2026-11-04-build-steps-4-5.md)
(the finder), [zellij keys and frames](2026-11-04-zellij-keys-and-frames.md) (dragging pane borders),
[PDF preview](2026-11-05-pdf-preview.md), [phone plan](2026-11-03-phone-interface.md)

## Decision

A finder tab becomes **two real `tb` panes**, as in the zellij version: **files** (35%, focused) and
**preview**, side by side. The user asked for this (2026-11-05) after finding they couldn't drag the line
between the finder's list and its preview.

- **Why the line can't be dragged now:** the finder is one pane that divides itself, with the list fixed at
  35% (`Finder::areas`). `tb`'s pane machinery doesn't know about that line. In the prototype, `finder`
  writes a zellij layout with two panes, `files` (`size="35%"`) and `preview`, so zellij resized them.
- **What two panes bring, from machinery `tb` already has:**
  - dragging the border, and zellij's resize mode (`Ctrl+n`)
  - focus by click, `Ctrl+p` and arrows, or `Alt`+arrows
  - fullscreen for the preview alone (`Ctrl+p f`)
  - frames and pane names
  - `tb action` commands (`resize`, `focus-pane-id`, `list-panes` and so on)
- **The preview pane is a view of the files pane.** One finder state stays with the files pane. The server
  draws the preview pane from it, and the preview pane's keys and mouse go to it.
  - The files pane's `Finder` keeps the preview worker thread, the folder watcher and the editor.
  - The preview pane has no state or thread of its own, only the id of its files pane. Previews stay drawn by the
  server, so each client still gets images in its own terminal's format (kitty, iTerm2, sixel or blocks).
- **Keys stay the same:** `l` (or Enter on a file you can't edit) moves focus to the preview pane, and `h`,
  `Esc` or `q` move it back to the files pane. Today they move focus inside the one pane. Everything else in the
  preview works as it does now: scrolling, `e` to edit, `t` for PDFs, zoom.
- **The editor and the CSV grid open in the preview pane**, as in the prototype, where `preview.py` ran the
  editor in its pane.
- **Linking rules:**
  - **Closing:** closing either pane closes both, because they're one finder.
  - **Moving:** moving either pane to another tab keeps the link. The preview follows the files pane wherever
    it is.
  - **Several finders:** each files pane has its own preview pane.
- **Narrow terminals (under 60 columns):** today the finder shows one side at a time. A finder tab that narrow
  shows only its focused pane, as if zoomed, and `l` and `h` switch which one. That keeps today's behaviour
  until the phone layout (roadmap step 7) does the same for every tab.
- **Names and the CLI:** the panes are named `files` and `preview`.
  - `list-panes` and events already have a `kind` field (`terminal` or `finder`). The files pane stays
    `finder`, and the preview pane is a new kind, `preview`, with ids printed as `preview_8`.
  - Adding a kind changes the documented API ([docs/cli.md](../cli.md)), so the docs list the new kind.
    Scripts that only know `terminal` and `finder` see one more value.
- **Saved sessions:**
  - The layout saves both panes; the preview saves which files pane it belongs to.
  - A session saved with the old one-pane finder comes back as a files and preview pair.
- **Size:** about 500–800 lines changed, mostly the finder's drawing and keys split in two, the linking rules,
  and updates to tests (**estimate**).

## How it fits together

**The two panes and what they own.** The preview pane only points at its files pane; everything else stays
with the files pane's `Finder`.

```
tab "finder"
┌─ files (35%) ────────────────┐┌─ preview (65%) ─────────────────────────┐
│ Pane::Finder(Finder)         ││ Pane::Preview { finder: 7 }             │
│   draws: the folder listing  ││   draws: Finder::render_preview(rect)   │
│                              ││   keys:  → Finder::handle_preview_key   │
└──────────────┬───────────────┘└──────────────────┬──────────────────────┘
               │ owns                              │ points at pane 7
               ▼                                   │
        Finder (one state) ◀───────────────────────┘
          ├─ selection, folder, filter, View / PdfView
          ├─ preview worker thread   (load_preview, PDF jobs → poppler)
          ├─ folder watcher thread   (notify / inotify)
          └─ editor / CSV grid       (drawn in the preview pane)
```

**Threads and channels in the session server.** This plan doesn't change them: a finder still has one
preview worker and one folder watcher, and the preview pane adds no thread. The main loop is an ordinary loop
on one OS thread (not async). It's the only reader of the inbox and the only owner of the state.

```
THREADS THAT SEND IN   (each holds a copy of the inbox's sender, tx: Sender<Ev>)

  accept thread   × 1              ──▶ Ev::Conn(stream)          a client connected to the socket
  client reader   × each client    ──▶ Ev::Client(id, msg)       keys, mouse, resize, CLI commands
                                       Ev::Gone(id)              the client disconnected
  PTY reader      × each terminal  ──▶ Ev::Pane(id, event)       new output, title, program exited
  preview worker  × each finder    ──▶ Ev::Finder(id, View|Pdf)  a preview is ready
  folder watcher  × each finder    ──▶ Ev::Finder(id, Changed)   files in the folder changed
  image encoder   × 1              ──▶ Ev::Image(key, bytes)     a picture is encoded
                                          │
                                          ▼
┌─ tb __server <session>: the main thread ───────────────────────────────────────────────────────┐
│ ┌ inbox ───────────────────────────────────────────────────────────────────────────────────┐   │
│ │ rx: Receiver<Ev>  one unbounded channel; only this thread reads it; events stay in order │   │
│ └──────────────────────────────────────────────────────────────────────────────────────────┘   │
│                                                                                                │
│ main_loop(rx): an ordinary loop on one OS thread, not async                                    │
│   loop {                                                                                       │
│     1  ev = rx.recv_timeout(t)     sleeps here until an event comes or t runs out              │
│                                     t = time until the next frame if dirty, else ≤ 500 ms      │
│     2  handle(ev), then up to 2000 more already queued   (sets dirty if the screen changed)    │
│     3  timers:  status and messages expire · events to subscribers (≤ every 100 ms)            │
│                redraw anyway (every 500 ms) · save the session if changed (every 2 s)          │
│     4  if dirty and ≥ 8 ms since the last frame:  render_all()  → one diff per client          │
│   }                                                                                            │
│                                                                                                │
│ all state lives here: tabs, panes, finders, clients, image cache; no locks needed for it       │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
                                          │
                                          ▼
CHANNELS THE MAIN LOOP SENDS ON

  Finder.requests  Sender<Request> + seq       ──▶ preview worker (one per finder)
  Images.jobs      Sender<Job>                 ──▶ image encoder
  client.tx        SyncSender<ServerMsg>, 256  ──▶ client writer (one per client) ──▶ socket
                   try_send: a client that's full skips frames and is redrawn fully later
  pane.sender      alacritty EventLoopSender   ──▶ PTY thread ──▶ the shell (typed keys)
```

**The calls with two panes.**
- **`j` in the files pane:** the worker builds the preview, then the server draws the list in the files pane
  and the preview in the preview pane, both from the same `Finder`.
- **`l`:** the finder returns a new `Action::FocusPreview`, and the server moves focus to the preview pane.
- **`k` in the preview pane:** the key goes to the `Finder` through the preview pane. `preview_key` stands
  for `Finder::handle_preview_key`.
- **Pictures:** an `ImageSlot` goes through the image encoder as it does today; that's left out here.

```
tb attach        server loop          files: Finder         preview pane             worker
   │ j                │                     │                     │                     │
   ├───── Input ─────▶│                     │                     │                     │
   │                  │ files has focus     │                     │                     │
   │                  │ send_key            │                     │                     │
   │                  ├──── handle_key ────▶│                     │                     │
   │                  │                     │ move_selection      │                     │
   │                  │                     │ request_preview     │                     │
   │                  │                     ├─────────── Request{job, seq} ────────────▶│
   │                  │                     │                     │                     │ load_preview
   │                  │                     │                     │                     │ view::build
   │                  │◀────────────────────── Ev::Finder(View) ────────────────────────┤
   │                  ├───── on_event ─────▶│                     │                     │
   │                  │                     │ view = View         │                     │
   │                  │ render_all          │                     │                     │
   │                  ├── render (list) ───▶│                     │                     │
   │                  ├─────────────── draw_pane ────────────────▶│                     │
   │                  │                     │                     │ finder: 7           │
   │                  │                     │◀─ render_preview ───┤                     │
   │                  │                     │ text or ImageSlot   │                     │
   │◀─── Output ──────┤                     │                     │                     │
   │                  │                     │                     │                     │
   │ l                │                     │                     │                     │
   ├───── Input ─────▶│                     │                     │                     │
   │                  ├──── handle_key ────▶│                     │                     │
   │                  │◀── FocusPreview ────┤                     │                     │
   │                  │ focus_pane          │                     │                     │
   │                  │                     │                     │                     │
   │ k                │                     │                     │                     │
   ├───── Input ─────▶│                     │                     │                     │
   │                  │ preview has focus   │                     │                     │
   │                  │ send_key            │                     │                     │
   │                  ├───────────── Pane::Preview ──────────────▶│                     │
   │                  │                     │◀── preview_key ─────┤                     │
   │                  │                     │ scroll, t, zoom     │                     │
   │                  │ dirty → next frame  │                     │                     │
```

## Options considered

| Option | Verdict |
|---|---|
| **Two panes, the preview a server-drawn view of the files pane** | ✅ The user's choice. Every pane feature works on both halves (drag, resize mode, fullscreen, focus keys, CLI), as in the zellij version |
| One pane with a draggable dividing line, plus `<` `>` keys | ❌ This plan's first version. Smaller (80–120 lines), but it gives only the drag: no resize mode, no fullscreen preview, no CLI |
| Two panes, the preview a terminal running a preview program (like `preview.py`) | ❌ The server draws images for each client's own terminal; a program in a pane would draw one format for all, and `tb` would need a second protocol between the panes |
| Build the finder outside the server, as programs linked through the `tb` CLI (like the zellij version's glue) | ❌ Programs in a `tb` pane can't show images: the `alacritty_terminal` fork has no sixel or kitty graphics support (checked 2026-11-05, nothing in its source). Previews would be text and blocks only, and the state, saving and editor handling would need rebuilding outside |
| Two panes, each with its own finder state | ❌ Selection, folder and filter would have to be synced between them; one state is simpler |

## Steps

- [x] **1. A preview pane kind:**
  - `Pane::Preview { finder }`, drawn with the finder's preview drawing.
  - `Finder` loses `areas` and draws only its list; `Finder::render_preview` draws the preview into any
    rectangle.
- [x] **2. Opening and closing:**
  - `tb finder` and `Ctrl+t f` open a tab split 35/65.
  - Closing either pane closes both.
  - The finder's editor and grid draw in the preview pane.
- [x] **3. Focus and keys:**
  - `l`, `h`, `Esc` and `q` move real pane focus, through new finder actions (`Action::FocusPreview`,
    `Action::FocusFiles`).
  - Keys and mouse in the preview pane go to the finder's preview handling.
  - The status hint follows the focused pane.
- [x] **4. Narrow finder tabs:** under 60 columns, show only the focused pane.
- [x] **5. CLI, events and saving:**
  - the `preview` kind and `preview_N` ids in `list-panes`, events and pane-id parsing, and in
    `docs/cli.md`
  - the link in the saved layout, and old saved sessions converted
  - the generated CLI docs updated if the help text changes
- [x] **6. Tests:**
  - **Existing end-to-end finder tests:** layout, editing, CSV grid, PDF, images, sessions coming back.
  - **New:**
    - dragging the border changes the split
    - `Ctrl+p f` makes the preview fullscreen
    - closing one pane closes both
    - an old one-pane saved session restores as two
- [ ] **7. Check by hand (you):** in Ghostty, drag the border, use resize mode, and fullscreen the preview.

## Change log

- **2026-11-05:** written as "drag the line between the finder's list and preview", with one pane, a
  draggable line and `<` `>` keys.
- **2026-11-05:** changed at the user's request to **two real panes**, as in the zellij version. The draggable
  line and the `<` `>` keys are dropped, because pane borders and resize mode cover them.
- **2026-11-05:** added "How it fits together", with three diagrams:
  - which pane owns what
  - the server's threads and channels, which this plan doesn't change
  - the calls for `j`, `l` and `k` with two panes

  Step 3 now names the new focus actions.
- **2026-11-05:** built (steps 1–6); step 7, the check by hand, is still to do.
  - **Size:** 556 lines added and 205 removed across 10 files, tests included, inside the 500–800 estimate.
  - **Code:**
    - `Pane::Preview { finder }` in `server/mod.rs`. `Finder` draws with `render_list` and `render_preview`,
      and is sized by `resize_list` and `resize_preview`.
    - Keys and the mouse carry which pane they came from, and `l`, `h` and friends return
      `Action::FocusPreview` and `Action::FocusFiles`.
    - Narrow tabs: `Server::shows_one`.
    - Saving: `SavedPane::link` and `PaneKind::Preview`, with `link_finders` on restore.
  - **Tests:**
    - `finder_is_two_panes` covers the 35/65 split, dragging the border, `l`, `Ctrl+p f`, `h`, closing the
      preview (which closes both), and a 50-column terminal showing one pane at a time.
    - `old_one_pane_finder_comes_back_as_two` restores a session file in the old format.
    - `sessions_come_back_after_kill` now expects the finder as two panes.
    - The existing finder, editor, CSV-grid, PDF and image tests pass unchanged.
  - **Decided while building:**
    - **While a file is open in the editor,** typing goes to the editor from either finder pane, and the file
      list ignores clicks.
    - **Focus after editing:** closing the editor or grid returns focus to the pane you opened it from.
    - **Moving a pane out:** both halves keep the tab name "finder" when moved to a tab of their own.
    - **Old saves:** `FinderState::focus_preview` is gone, and old saved files that have it still load,
      because the field is ignored. A converted old session starts with the file list focused.
    - **Minimum widths:** the old 16- and 20-column minimums for the list and preview are gone. Pane
      splits keep their usual minimum instead.
    - **Events:** the editor's output marks the preview pane as changed, since the editor shows there.
- **2026-11-07:** the file list shows hidden files by default, and `.` hides them; while they're hidden, the
  list's title says how many. See [hidden files](2026-11-07-finder-hidden-files.md).
- **2026-11-07:** right-clicking a file in the list, or `m`, opens a menu: rename, copy name, copy path,
  delete (to the trash). See [finder menu](2026-11-07-finder-menu.md).
