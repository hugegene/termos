# Finder tabs stay on the left, without numbers

**Status:** built and tested (unit and end-to-end); not yet checked by hand · **Date:** 2026-11-09 · **Related:** [tab names follow the folder](2026-11-09-tab-name-folder.md)

## Decision

**Finder tabs always sit at the left of the tab bar, before every other tab, and show no number.** Numbering
starts at 1 with the first other tab. There can be any number of finders. A new one joins the left group,
right after the finders already there. The user asked for this on 2026-11-09 and took the recommended answer
to each question below.

```
before:   1 shell │ 2 docs │ 3 finder │ 4 finder │ +                     main
after:    finder: crates │ finder: docs │ 1 shell │ 2 docs │ +          main
          └────── pinned, no numbers ───────┘ └── numbered from 1 ──┘
```

- **What counts as a finder tab:** any tab that holds a finder pane (its file list or its preview). No flag is
  stored, so restored sessions and panes moved between tabs (`Ctrl+t [` / `]`) need no special handling. A
  shell tab that a finder is moved into jumps left, and a finder tab whose finder goes elsewhere moves back
  among the numbered tabs.
- **Names:** a finder tab nobody renamed is `finder` when it's the only one. With two or more, each shows the
  folder it lists: `finder: crates`, `finder: docs`. A renamed finder tab keeps its name. Finder tabs now count
  as automatically named (`auto_name: true` in `list-tabs -j`), like shell tabs that follow their folder.
  Finder tabs from older saves (named `finder`, `auto_name: false`) become automatic when restored.
- **Numbers and keys:**
  - `Ctrl+t 1`…`9`, `tb action go-to-tab N` and `GoToTab N` in `config.kdl` go to the tab **numbered** N on
    the bar, skipping finders.
  - `Ctrl+t 0` (new) goes to the first finder.
  - `position` in `list-tabs -j`, the `tab_opened`/`tab_moved` events and `tab` in `list-panes -j` keep
    counting every tab, finders included, so `position` 0 is the leftmost finder.
- **Moving tabs:** `Alt+i`/`Alt+o` and `tb action move-tab` move a tab within its own group and wrap around
  that group's ends. A finder never moves right of a numbered tab, and a numbered tab never moves left of a
  finder.
- **Not changed:** new sessions still start with a shell tab, and `Ctrl+t f` still opens another finder.

### Example, step by step

1. Tabs `[1 shell][2 docs]`, in `docs`. `Ctrl+t f` opens a finder at the far left, which becomes active:
   `[finder][1 shell][2 docs]`.
2. `Ctrl+t f` again, from the finder (listing `crates`). The second finder goes right after the first, and both
   now show their folder: `[finder: terminal-browser][finder: crates][1 shell][2 docs]`.
3. `Ctrl+t 1` goes to `shell`. `Ctrl+t 0` goes back to the first finder.
4. In `1 shell`, `Alt+i` (move left) wraps to the end of the numbered group: `[finder…][finder…][1 docs][2 shell]`.
5. Close the first finder: `[finder][1 docs][2 shell]`. With only one finder left, it's plain `finder` again.

### Where the order is kept

```
 key · click · tb CLI · pane exit
          │ 1 Ev
          ▼
 Server::handle(ev) ──► push_tab · break_pane · break_pane_to · move_tab · close …
          │ 2 after every event
          ▼
 keep_finders_left(): stable sort, finder tabs first; the active tab is followed by its id
          │ 3 next redraw / event tick
          ▼
 draw_tab_bar(): " finder: crates " · " 1 shell "      emit_events(): tab_moved for tabs that shifted
```

One sort after every event covers all the ways tabs change (new tabs, panes moving between tabs, panes
closing, restore), so no code path can leave a finder out of place. A stable sort keeps the order inside each
group. It walks every tab's panes, a few dozen hash lookups per event (**estimate**; not measured, since the
cost is far below anything a redraw does).

## Options considered

- **A second finder at the very left** instead of after the existing finders. Rejected: the first finder would
  shift every time, and it's the one you usually come back to.
- **Two finders both named `finder`.** Rejected: you couldn't tell them apart without numbers.
- **Keep counting finders in `go-to-tab N` and `Ctrl+t 1`…`9`**, just hiding their numbers. Rejected: `Ctrl+t 1`
  would go to a tab with no `1` on it.
- **A flag set on tabs created as finders**, instead of "holds a finder". Rejected: it has to be saved and kept
  in step when panes move. Deriving it from the panes can't go stale.
- **Inserting each tab in the right place** at every call site instead of one sort. Rejected: five call sites
  (and every future one) would each have to get it right.

## Steps

1. `crates/tb/src/server/mod.rs`:
   - `tab_finder` (the tab's finder, focused one first) and `is_finder_tab`.
   - `keep_finders_left()`, called at the end of `handle()` and after `restore`.
   - `make_tab(None, id)` names a finder's tab `finder`, automatic; drop the fixed `Some("finder")` names.
   - `tab_label`: automatic finder tabs show `finder` / `finder: <folder>`.
   - `move_tab` moves within the tab's group.
   - `numbered_tab(n)`; `draw_tab_bar` leaves finders unnumbered.
   - Unit tests for the order and the labels.
2. `keys.rs`: `Ctrl+t 1`…`9` use the numbers on the bar; `Ctrl+t 0` goes to the first finder; update the hints
   and the help screen.
3. `actions.rs`: `go-to-tab N` uses the numbers on the bar; `undo-rename-tab` goes back to the automatic name.
4. `persist.rs`: older finder tabs (`finder`, not automatic) become automatic on restore.
5. `cli.rs`: `go-to-tab` help text, then regenerate `docs/cli/`. `docs/cli.md`: tab numbers, `auto_name`, `GoToTab`.
6. `e2e.rs`: update the restore test (the finder is now first) and add one for two finders, the numbers and
   moving tabs.
7. Build, test, clippy, then check it by hand in a throwaway session.

## Results (2026-11-09)

- Steps 1–6 are done. `cargo test --workspace`, clippy and `cargo deny check licenses` pass.
- New unit tests in `server/mod.rs`: `finder_tabs_go_left_after_other_finders`, `tabs_move_within_their_group`
  and `a_tab_that_gets_a_finder_goes_left`.
- New end-to-end test `finder_tabs_stay_left`. It reads the real tab bar while it runs the steps of the example
  above: ` finder   1 one   2 two `, then ` finder: two   finder: one   1 one   2 two `. It also covers
  `go-to-tab 2`, the error from `go-to-tab 3`, `Alt+i` wrapping inside the numbered tabs, and `Ctrl+t 0`.
- `sessions_come_back_after_kill` now expects the restored finder first: ` finder   1 work `.
- Still to do: step 7's check by hand.
