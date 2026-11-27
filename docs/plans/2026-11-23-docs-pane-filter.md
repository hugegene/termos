# The docs pane's `/` filter

**Changes:**
- **2026-11-27:** following now filters too, to the file the agent wrote last, by its whole path
  ([that plan](2026-11-27-docs-pane-follow-filter.md)). So `f` no longer clears a filter: it replaces it with
  following's. `Esc` while typing brings back the filter that showed before `/`, as well as the open file.

## Decision

`/` in the docs pane filters its list of files the way `/` filters the finder's list:

- **Type part of a path,** and only the files whose path (as the header shows it) has those letters in order stay,
  best match first. The matching is the finder's own (`finder_core::filter`), shared rather than copied.
- **While typing,** the open file closes so the matches show as a list. `↑`/`↓` move the highlight, and every
  other key, `q` `j` `k` `x` included, types.
- **`Enter`** opens the highlighted file and keeps the filter. **`Esc`** cancels: the filter goes, and the file
  that was open, and following, come back.
- **With a filter kept,** the keys work as usual on the files that show, and `Esc` clears it. `f` (follow) clears
  it too, since the file the agent writes next might not match.

No measurement: this depends only on what the current code does, read in `crates/tos/src/docsview.rs` and
`crates/tos/src/server/finder/mod.rs`.

## Example

The docs pane in this repository lists 60 files, with `README.md` open and the title `docs · 60 · following`.

```
 key        title line                                    list
 ─────────  ────────────────────────────────────────────  ──────────────────────────────────────────
 /          /▏ · 60 of 60 · Enter opens · Esc cancels     README.md closes; all 60 headers show
 a g e n t  /agent▏ · 4 of 60 · Enter opens · Esc cancels  ▸ docs/findings/2026-11-21-agent-hook-files.md
                                                          ▸ docs/findings/2026-11-21-agent-launch-flags.md
                                                          ▸ docs/plans/2026-11-17-agent-plan-pane.md
                                                          ▸ docs/findings/2026-11-19-agent-hooks-without…
 ↓ ↓        (same)                                        the highlight moves to the plan
 Enter      /agent · 4 of 60 · Esc clears                 ▾ docs/plans/2026-11-17-agent-plan-pane.md opens
 Esc        docs · 60 · f follows                         all 60 headers again; the plan stays open
```

Had `Esc` come in place of `Enter`, the filter would go and `README.md` would open again, following as before.

Each key travels as any key to a program in a pane does:

```
 ┌──────────┐ 1. key "/"  ┌────────────┐ 2. key "/"  ┌────────────┐ 3. bytes on its pty  ┌──────────────────┐
 │ terminal │ ──────────▶ │ tos attach │ ──────────▶ │ tos server │ ───────────────────▶ │ tos docs --list  │
 │          │             │  (client)  │   (socket)  │            │                      │ Docs::key: filter│
 │          │ ◀────────── │            │ ◀────────── │            │ ◀─────────────────── │ Docs::draw       │
 └──────────┘ 6. screen   └────────────┘ 5. screen   └────────────┘ 4. redrawn rows      └──────────────────┘
```

## Options considered

- **Substring rather than letters in order.** More predictable, but the finder's `/` matches letters in order;
  one behaviour across `tos` wins.
- **Keep the newest-first order while filtering.** Loose matches in long paths would crowd the top; ranking puts
  `agent-plan` above a path that merely has an `a`, a `g` and so on spread through it.
- **Search the files' text too.** A different feature (a grep); the headers are what you scan. Not now.
- **Leave the open file open while typing.** It takes two thirds of the rows, hiding most of the matches.

## Steps

1. `finder-core`: `filter_names`, the finder's ranking for any list of names; `filter` calls it.
2. `docsview.rs`: the filter and the state `/` closed (for `Esc`); keys while typing; the list, `Tab`, `j`/`k`,
   `Enter`, `e` and `x` working on the files that show; the title line; `(no match)`.
3. Unit tests: typing narrows and ranks, letters like `q` type, `Enter` opens and keeps, `Esc` while typing
   brings back the open file and following, `Esc` after clears.
4. `docs/cli.md`: the key.
