# Following filters the docs pane to the file the agent wrote

**Status:** built (2026-11-27) · **Date:** 2026-11-27 · **Related:**
[the docs pane's `/` filter](2026-11-23-docs-pane-filter.md), [the docs pane](2026-11-17-agent-plan-pane.md)

## Decision

The user asked on 2026-11-27: "when claude or any harness write a plan and the doc panel comes out, can the doc
panel comes out in filtered mode, meaning /{doc name} so that the only document visible is it".

**While the docs pane is following, each file the agent writes opens filtered to that file alone.** The title
reads `/` and the file's name, as if you had filtered for it, and no other header shows, so the file gets the
whole pane.

- **Every write that following opens counts,** from any agent with a launcher (`tos claude`, `tos codex`, …) or
  `tos action add-doc`: a plan, a note, an edit to `README.md`. The next write switches the filter to the next file.
- **Exactly that file, not a match on its name.** The filter is the file's path, compared whole. Typing
  `/README.md` would also show `docs/cli/README.md`, because `/` matches letters in order; the filter that
  following sets shows only the one file the agent wrote.
- **`Esc` shows every file again,** and the agent's file stays open. Following goes on, so the agent's next write
  filters to that one.
- **Anything that stops following keeps the filter:** `Tab`, `Enter`, a click on the header, selecting text. The
  agent's next write then doesn't move you.
- **`f` follows again:** the file written last opens, filtered to it (before, `f` cleared the filter).
- **`/` then `Esc`** brings back what showed before `/`, the filter included. With following on, that is the file
  written last, filtered.
- **The filter goes when its file leaves the list,** by `x` or because it was deleted, so the pane never shows
  `(no match)` for it.
- **Opened with `Ctrl+t d`** before the agent has written anything, the pane is the whole list, as before:
  nothing is open, so nothing is filtered.

No measurement: this depends only on what the current code does, read in `crates/tos/src/docsview.rs`.

## Example

You run `tos claude` in `~/termos` and ask for a plan. Claude writes `docs/plans/2026-11-27-x.md`. The
repository has 184 Markdown files.

```
 before (2026-11-23)                          now
┌──────────────────────────────────────────┐  ┌──────────────────────────────────────────┐
│ docs · 184 · following                   │  │ /2026-11-27-x.md · 1 of 184 · following …│
│▾ docs/plans/2026-11-27-x.md         now  │  │▾ docs/plans/2026-11-27-x.md         now  │
│  # Plan: the export button               │  │  # Plan: the export button               │
│  **Status:** proposed                    │  │  **Status:** proposed                    │
│  …   (2/3 of the rows)                   │  │  …                                       │
│▸ docs/plans/2026-11-23-docs-pane-…   4d  │  │  …   (every row: no other header)        │
│▸ README.md                           1h  │  │  …                                       │
└──────────────────────────────────────────┘  └──────────────────────────────────────────┘
```

Walking through it, key by key, in the docs pane:

```
 what happens                       title line                                       what shows
 ─────────────────────────────────  ───────────────────────────────────────────────  ────────────────────────────
 Claude writes the plan             /2026-11-27-x.md · 1 of 184 · following · Esc …  the plan alone, every row
 Claude edits README.md             /README.md · 1 of 184 · following · Esc clears   README.md alone
 Esc (pane focused)                 docs · 184 · following                           all 184; README.md stays open
 Claude writes the plan again       /2026-11-27-x.md · 1 of 184 · following · Esc …  the plan alone again
 Tab                                /2026-11-27-x.md · 1 of 184 · Esc clears         the same; following stopped
 Claude edits README.md             (same)                                           the plan stays; README.md gets ●
 f                                  /README.md · 1 of 184 · following · Esc clears   README.md alone
```

How a write reaches the pane is unchanged; only the last step, in the pane, is new:

```
 ┌──────────────┐ 1. PostToolUse JSON ┌──────────┐ 2. IPC add-doc ┌────────────┐ 3. appends a line  ┌──────────────┐
 │ claude       │ ──────────────────▶ │ tos hook │ ─────────────▶ │ tos server │ ─────────────────▶ │ docs/<id>.   │
 │ (tos claude) │   on stdin          │          │   (socket)     │            │   {"path": …}      │ jsonl        │
 └──────────────┘                     └──────────┘                └────────────┘                    └──────┬───────┘
                                                                                     4. notify: changed    │
                                                     ┌──────────────────────────────────────────┐ ◀────────┘
                                                     │ tos docs --list FILE (the docs pane)     │
                                                     │ Docs::ingest: following, so open it  AND │
                                                     │ 5. (new) filter to it: pin = its path    │
                                                     └──────────────────────────────────────────┘
```

## Options considered

- **Filter by the file's name with the `/` matching** (letters in order). The title would be exactly what you
  could type, but `README.md` would also show `docs/cli/README.md` and others, and the user asked for the file
  alone.
- **Filter only when the write opens the pane,** not on later writes. Following already means "show what the
  agent wrote last"; filtering only the first file and not the next would be two rules for one mode.
- **`Esc` also stops following.** Then the agent's next file wouldn't open at all, which following is for; `Tab`
  or a click already stops it.
- **The full path in the title** (`/docs/plans/2026-11-27-x.md`). The header right below shows the path; the
  title has room for the name and the hints in a pane 40% of the tab wide.

## Steps

1. `docsview.rs`: `Docs::pin`, the path following filters to; `shown` returns only that entry while it's set; set
   in `ingest` while following, by `f`, and by `Esc` after `/` when following was on; cleared by `Esc` and when
   its entry is removed; saved and brought back around `/`.
2. The title: `/` and the file's name, cut from the left to leave room for the count, `following` and `Esc clears`.
3. Unit tests: a write while following filters to it, the next write switches, `Esc` shows all and keeps
   following, `Tab` keeps the filter and stops following, `f` filters to the newest, `x` on it clears the filter,
   `/` then `Esc` brings it back.
4. End-to-end tests (`crates/tos/tests/e2e.rs`): the agent's plan opens alone, `Esc` shows `README.md`.
5. `docs/cli.md`: following filters; `f` no longer clears but filters.
