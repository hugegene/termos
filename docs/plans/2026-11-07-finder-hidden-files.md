# Plan: `tb`'s finder shows hidden files by default

**Status:** proposed · **Date:** 2026-11-07 · **Related:** [the finder as two panes](2026-11-05-finder-divider.md),
[saving sessions](2026-11-04-build-steps-4-5.md) · **Findings:** none needed (see below)

## Decision

**New finders show hidden files (names starting with `.`), and `.` hides them.** The user asked for this on
2026-11-07.

- **The key:** `.` already toggles hidden files in the file list. It stays the key; only the starting state
  changes.
- **Telling they're hidden:** while hidden files are off, the file list's title ends with a dim `· N hidden`, so
  a folder that looks empty isn't mistaken for one.
  - Today there's no sign at all, which was fine while hiding was the norm. Once showing is the norm, hiding is
    the surprise.
  - `list_dir` already reads every entry, so the count is free.
- **The status-bar hint** stays `. hidden`. The title already shows which state you're in.
- **Saved sessions show hidden files too after this change,** unless they're saved again with them off.
  - Today `FinderState` saves `show_hidden: false` for every finder, because that was the default. So the saved
    value says nothing about what the user chose.
  - The field becomes `hide_hidden` (default false). Old saves don't have it, so they restore showing hidden
    files. The old `show_hidden` field is ignored (serde ignores unknown fields; `Saved` doesn't deny them).
  - Finders saved after this change keep their choice.
- **Each finder keeps its own setting,** as now. Pressing `.` in one finder doesn't change others.
- **Following a link to a hidden file** still turns hidden files on, as the
  [Markdown links plan](2026-11-07-markdown-links.md) does, for when they're off.
- **Not changed:**
  - `finder_core::list_dir(dir, show_hidden)` keeps its signature. The phone plan's finder uses it.
  - The prototype: yazi keeps its own setting (`yazi/yazi.toml` doesn't set `show_hidden`, so yazi hides them).
- **Size:** about 40–80 lines plus tests (**estimate**).

No measurements were needed: this is a default the user chose, plus a small change to what's saved.

## Options considered

| Topic | Option | Verdict |
|---|---|---|
| Default | **Show hidden files** | ✅ The user's choice (2026-11-07) |
| Key | **`.`, as now** | ✅ Already there; also yazi's and ranger's key |
| | A new key | ❌ Nothing to gain |
| Old saved sessions | Keep their saved `show_hidden: false` | ❌ Every old finder would still hide them, though nobody chose that |
| | **A new `hide_hidden` field; old saves show hidden files** | ✅ |
| A sign that files are hidden | None, as now | ❌ Once showing is the default, a hiding finder looks like a folder with fewer files |
| | **`· N hidden` in the list's title** | ✅ |
| Where the default lives | A config option (`finder { show_hidden false }`) | Later, with the rest of the config at launch (rebuild plan step 9) |
| | A `tb finder --no-hidden` flag | Later, if someone asks |

## Steps

- [ ] **1. The default:**
  - `Finder::new` starts with hidden files shown.
  - `FinderState`: replace `show_hidden` with `#[serde(default)] hide_hidden: bool`.
  - `snapshot` and `restore` map it.
- [ ] **2. The count:**
  - `Finder::reload` counts the entries it leaves out.
  - The list's title (frame label, or the title line without frames) adds `· N hidden` while hidden files are
    off and N > 0.
- [ ] **3. Tests:**
  - **Unit:**
    - a new finder lists `.hidden`
    - `.` hides it, and the title says `1 hidden`
    - `.` again shows it
    - a saved state in the old format (`"show_hidden": false`) restores showing hidden files
    - a state saved with them off restores hiding them
  - **End to end:** a finder on a folder holding `.hidden` and `seen` shows both, and after `.` shows `seen` and
    `1 hidden`.
- [ ] **4. Docs:**
  - README's finder line: `.` hides hidden files (shown by default).
  - The finder's `?` help, if it lists `.`.
  - A change-log line in the [finder plan](2026-11-05-finder-divider.md).
- [ ] **5. Check by hand (you):** your home folder in a finder; `.`; detach and attach again.

Build and test as CLAUDE.md says: `cargo build --offline`, `cargo test --workspace`, clippy,
`cargo fmt -p tb -p finder-core`.

**Order:** after the [Markdown links](2026-11-07-markdown-links.md) work is committed. It edits the same
`finder/mod.rs`, and is uncommitted at the time of writing.

## Change log

- **2026-11-07:** written. The user asked for hidden files shown by default, with a key to hide them.
