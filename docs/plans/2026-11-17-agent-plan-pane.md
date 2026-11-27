# Plan: `tos claude`, `tos codex`, …: agent launchers with a hook and a starting prompt, and a docs pane for the `.md` files they write

**Status:** built (2026-11-21); the whole repository's files and the rules on explaining are step 7
(2026-11-23); trying it with the real agents (step 6) is yours · **Date:** 2026-11-17, changed 2026-11-18,
2026-11-19, 2026-11-21 and 2026-11-23 ·
**Related:** [the finder's Markdown preview](2026-11-07-markdown-links.md),
[key clashes with agent CLIs](../findings/2026-11-17-agent-cli-key-clashes.md) · **Findings:**
[where agents keep a plan, and their hooks](../findings/2026-11-17-agent-plan-sources.md),
[can tos give an agent a hook without its settings file](../findings/2026-11-19-agent-hooks-without-settings.md),
[which file each agent reads its hooks from](../findings/2026-11-21-agent-hook-files.md),
[what each agent CLI accepts for one session](../findings/2026-11-21-agent-launch-flags.md)

**Changes:**
- **2026-11-18:** said that the hook belongs to the agent, and that its command is the `tos` CLI.
- **2026-11-19:**
  - **The user asked** for every `.md` file the agent writes, as an accordion, with the hooks set from the
    finder.
  - **A per-tab accordion pane drawn by the server** replaced one `tos view` program per agent pane (option D).
  - **Later that day,** the user asked whether the hook could live in `tos`. That added options B (watch the
    files) and G (flags on the agent's command line).
- **2026-11-21:**
  - **First,** the user asked where option A writes each agent's hook, and when (install or `tos update`).
    The [finding](../findings/2026-11-21-agent-hook-files.md) answers it.
  - **Then the user proposed launchers:** `tos claude`, `tos codex`, `tos agent`, `tos agy` and `tos grok`. Each
    starts the agent with a hook and a starting prompt added for that session. This is now the decision
    (option H).
  - **Dropped:** writing hooks into the agents' own files (option A), the finder's `Agent docs…` box,
    `tos hooks on/off`, and the first-run question.
  - **Agents with no per-session hook** fall back to option B, watching the folder.
  - **Last,** the user decided: "leave those out. only show files that are written to the current working
    folder. ok go ahead and implement". The rest of this list follows from that:
    - Only files inside the agent's working folder count (section 3).
    - **The docs pane is a program, `tos docs`, running in an ordinary pane,** not a pane kind drawn by the server.
      The server-drawn pane was chosen on 2026-11-19 so that the server could hold the list for the finder's
      box. The box is gone, so a program is simpler: saving, closing, focus and resizing all work as for any
      pane. The server only opens the pane and writes the list ("How it's built").
    - **The starting instructions go on the command line as text,** so there's no `prompt.md`.
    - **The first version leaves out** plan text and step lists that live only inside a tool call, and the
      folder watch (option B). Both come later.
  - **After it was built,** the user asked for a key to bring a closed docs pane back: `Ctrl+t d`, and
    `ToggleDocs` in `config.kdl` (section 3, "Hiding").
  - Then, with the finder preview's copy merged into `main`, the user asked for the same in the docs pane
    (section 3, "Selecting and copying").
- **2026-11-23:** the user asked: "can the docs pane show all md file in a repo by default. the following claude
  editing a plan still works. can tos claude and tos other stuffs have a base prompt input, "always use ASCII
  diagram draw relationship to explain and explain by walking through example, do not use short form to
  reference and keep reply self-contained"". Two changes follow:
  - **The docs pane lists every Markdown file in the repository,** not only the ones the agent wrote. The files
    the agent writes still go on top and open by themselves (section 3, "Every Markdown file in the
    repository").
  - **The default starting instructions** gain four rules on how to explain (section 1, "The starting
    instructions"). `agent-prompt.md` still replaces the whole text.
- **2026-11-27:** the user asked for the docs pane to come out "in filtered mode, meaning /{doc name} so that the
  only document visible is it". Following now shows the file the agent wrote last alone, as a filter; `Esc`
  shows the rest. See [its plan](2026-11-27-docs-pane-follow-filter.md).

## Decision

The user asked on 2026-11-21: "could we do this? use "tos claude", "tos codex", "tos agent", "tos agy", "tos
grok".. to override with hook and staring prompt? could you find out a list of comprensive harness that needs to
be overidden"

**In short:** you start an agent as `tos claude` instead of `claude`. `tos` then runs the real `claude` with two
additions, for that session only:
- **a hook** that tells `tos` about each file the agent writes;
- **`tos`'s starting instructions,** added to the end of the system prompt.

Nothing is written to Claude's own files, and plain `claude` behaves as before. The `.md` files the agent writes
appear in the tab's docs pane (section 3).

```
 tab "termos"
┌──────────────────────────────────┬─────────────────────────────────┐
│ terminal_7 · claude              │ docs · 3 · following            │
│                                  │ ▾ docs/plans/2026-11-21-x.md now│
│ ● Write(docs/plans/2026-11-21-x… │   # Plan: the export button     │
│   ⎿ Wrote 84 lines               │   **Status:** proposed          │
│ ● Update(README.md)              │   ## Decision                   │
│   ⎿ Updated with 3 additions     │   …                             │
│                                  │ ▸ README.md                ● 2m │
│ > █                              │ ▸ docs/findings/…-sizes.md  14m │
└──────────────────────────────────┴─────────────────────────────────┘
        60%, keeps the focus              40%, one section open
```

### 1. What `tos claude` runs

```
$ tos claude --resume
        │ (1) tos writes its own file, each time:
        │       ~/.local/state/termos/agents/claude/settings.json   (the hook)
        │ (2) and replaces itself (exec) with:
        ▼
claude --settings ~/.local/state/termos/agents/claude/settings.json \
       --append-system-prompt "You are running inside tos, …" \
       --resume
```

- **Your arguments pass through unchanged,** after `tos`'s.
  - `tos claude "fix the failing test"` starts with that prompt, as `claude "…"` does.
  - `tos claude --resume` resumes.
- **It's `exec`, not a child process.** The pane's program becomes `claude` itself. Its keys, exit status and
  `pane_command` are exactly as if you had typed `claude`.
- **`tos` rewrites its files on every launch,** so a newer `tos` takes effect at the next launch. The hook's
  command is the installer's symlink `~/.local/bin/tos`, or else the full path of the running `tos`.
- **Outside a `tos` session,** `tos claude` still starts Claude with the same additions. The hook finds no
  `TOS_PANE_ID` and does nothing.
- **An agent that isn't installed:** `tos grok` with no `grok` on `PATH` stops with `grok isn't on your PATH`.
- **The starting instructions:**
  - **The default text** has two parts. The first says that you're inside `tos`, and that the `.md` files you
    write in this folder appear beside you, so write plans and notes as `.md` files there.
  - **The second part, how to explain** (added 2026-11-23, the user's wording turned into four rules), applies to
    replies and to the files the agent writes:
    1. draw relationships as plain-text (ASCII) diagrams in fenced code blocks: boxes for the parts, numbered
       and labelled arrows for each step of a flow;
    2. explain by walking through one concrete example step by step;
    3. no short forms: no abbreviations or coined names, and no bare references such as "option B" or "the fix
       above"; name things in full;
    4. keep each reply self-contained, so that it reads on its own without the earlier conversation.
  - **Your own text:** if `~/.config/termos/agent-prompt.md` exists (beside `config.kdl`), its text is used
    instead of both parts. An empty file means no starting instructions.
  - **It goes on the command line as text,** for example `--append-system-prompt "…"`, so there's no file for it.
  - **Which agents get it:** `tos claude`, `tos codex` and `tos grok`. `tos agy`, `tos agent` (Cursor) and
    `tos gemini` have no flag that adds to the system prompt
    ([finding](../findings/2026-11-21-agent-launch-flags.md)), so they get neither part, as before.

**The files `tos` writes** (added 2026-11-21, after the user asked). They hold only the hook. They're the same
at every launch, so two launches at once write the same bytes.

```
~/.local/state/termos/            (already there: main.log, prbot.log, sessions/, update-check)
├── docs/                         the docs panes' lists, one file per docs pane ("How it's built")
└── agents/
    ├── claude/settings.json      {"hooks": {"PostToolUse": [...]}} and nothing else
    ├── gemini/settings.json      {"hooks": {"AfterTool": [...]}} and nothing else
    ├── agy/settings.json         the same, if step 0 shows agy --settings takes hooks
    ├── cursor-plugin/            .cursor-plugin/plugin.json + hooks/hooks.json, for `agent --plugin-dir`
    └── grok-plugin/              plugin.json + hooks/hooks.json, for `grok --plugin-dir`
```

`tos codex` writes nothing: its hook and instructions go on the command line as `-c` values.

**The agent's own files stay where they are.** `--settings`, `--plugin-dir` and `-c` add to a session. They don't
move the agent's folder. So with `tos claude`:
- **memory** stays in `~/.claude/projects/<project>/memory/`, for this repo
  `~/.claude/projects/-home-hugegene-termos/memory/` (`MEMORY.md` and one file per memory);
- `CLAUDE.md`, the login, the transcripts (`~/.claude/projects/<project>/*.jsonl`) and `~/.claude/settings.json`
  stay where they are too.

`tos claude --resume` finds sessions started with plain `claude`, and the other way round.

### 2. Which agents, and how each gets its hook and instructions

The [finding](../findings/2026-11-21-agent-launch-flags.md) covers 25 agent CLIs. The first version supports the
ones you named, plus Gemini CLI, which is installed here:

| Launcher | Hook for this session | Hook event and tools | Starting instructions | Not yet known (step 0) |
|---|---|---|---|---|
| `tos claude` | `--settings FILE` with `hooks` | `PostToolUse`, `Write\|Edit\|MultiEdit` | `--append-system-prompt` | do hooks from `--settings` run alongside yours? |
| `tos codex` | `-c 'hooks.PostToolUse=[…]'` (Codex 0.160.1 parses and checks it) | `PostToolUse`, its edit tool (`apply_patch` or `exec`) | `-c developer_instructions="…"` | does the hook run? **Codex asks you to trust a new hook once** |
| `tos agy` | `--settings` ("settings.json keys"), if it takes hooks; else the folder watch (option B) | `PostToolUse`, `write_to_file` | none found. `-i` would send a first message instead | does `--settings` take hooks? |
| `tos agent` (Cursor) | `--plugin-dir DIR`, with a plugin `tos` writes | `postToolUse` `Write`, or `afterFileEdit` | none found | not installed here |
| `tos grok` (xAI Grok Build) | `--plugin-dir DIR`, "always trusted" | `PostToolUse`, `Edit\|Write` | `--rules` | not installed here |
| `tos gemini` | env `GEMINI_CLI_SYSTEM_SETTINGS_PATH=FILE`, holding only `hooks` (they add to yours) | `AfterTool`, `write_file\|replace` | none: `GEMINI_SYSTEM_MD` replaces the whole prompt | does the system file's hook run? |

**For Codex, `tos` never passes `--dangerously-bypass-hook-trust`.** That flag would also run hooks that a
cloned repo carries in its own `.codex/`, without asking. Instead, Codex asks you once to trust `tos`'s hook.
`tos`'s hook is the same at every launch, so whether that one trust then lasts is checked in step 0.

**`tos grok` means xAI's Grok Build.** The community `grok-cli` installs a `grok` command too, and it would
refuse `--plugin-dir`. Telling the two apart before starting is left for later, once Grok Build's
`--version` output is known.

**Later, as the same kind of launcher, in order of effort:**

| Kind | Agents | Notes |
|---|---|---|
| A flag or environment variable, as above | `droid` (`--settings`), `auggie` (`--plugin-dir`), `junie` (`--config-location`), `copilot` (`--plugin-dir`), `qwen` (env), `opencode` (env, with a TS plugin) | most also have an append-to-system-prompt flag |
| Only by moving a config folder | `amp`, `crush`, `cline`; `aider` through `--lint-cmd` | each workaround risks hiding your own config |
| No per-session hook | `kimi`, `goose`, `kiro-cli`, `cn`, `vibe`, `openhands` | the folder watch (option B) |

### 3. The docs pane

**Where:** one per tab, along the tab's right edge, at full height and 40% of the width. 40% is an estimate, to
tune. The server opens it on the tab's first entry, without moving the focus and without switching tabs.

**Later, a narrow tab:** if opening it would leave the rest of the tab fewer than 60 columns, show a status-bar
segment instead.

**What appears:**
- **Every `.md` file the agent creates or edits inside its working folder,** as the hook reports it. The
  working folder is the `cwd` in the hook's JSON, or else the hook's own folder.
- **Agents' own folders are left out,** even inside the working folder: `~/.claude`, `~/.codex`, `~/.gemini`,
  `~/.cursor` and `~/.grok`.
  - This keeps out Claude's memory files (`~/.claude/projects/<project>/memory/*.md`), which the user asked on
    2026-11-21 to leave out.
  - Without it, an agent started in `~` would show them, since its working folder holds `~/.claude`.
- **Only the agent's writes count** for following and for the top of the list. Since 2026-11-23 the
  repository's other Markdown files are listed below them (next part).
- **Each tab has its own list.**

**Every Markdown file in the repository** (added 2026-11-23, after the user asked "can the docs pane show all
md file in a repo by default"):
- **Which folder:** the docs pane's own working folder, widened to its git repository's top folder
  (`git rev-parse --show-toplevel`) when it's inside one.
  - When an agent's write opens the pane, that folder is the agent's working folder, as before.
  - When `Ctrl+t d` opens it, it's the folder of the tab's focused pane. Before, it was no folder in particular.
  - A restored docs pane starts in its saved folder, so it lists the same repository.
- **Which files:**
  - **In a git repository:** what `git ls-files --cached --others --exclude-standard` lists, that is tracked
    files and new files that `.gitignore` doesn't ignore, kept if they end in `.md` or `.markdown` and exist.
  - **Outside one:** the folder and its subfolders down to 3 levels, skipping hidden folders, `node_modules`
    and `target`, at most 1,000 folders and 1,000 files. All of these limits are **estimates**; they keep a pane
    opened in `~` cheap.
  - **Agents' own folders are left out,** as for the agent's writes.
- **The order:**
  1. the files the agent wrote, newest first, as before;
  2. then the rest of the repository's Markdown files, most recently changed first, as found when the pane
     starts;
  3. a file that appears later, such as one you create, goes on top, as any new entry does.
- **When the agent writes a file that's already listed** as one of the repository's, it moves to the top once
  and opens, as a new file would. Its later writes don't move it.
- **Following is unchanged:** only the agent's writes open a file. Opened with `Ctrl+t d` before the agent has
  written anything, the pane shows the list of headers with nothing open.
- **Ages on the headers:** for the agent's files, when the agent wrote them; for the others, when the file last
  changed. `●` marks a file that changed since it was listed or last open.
- **Listing again:** every 20 seconds, and at most every 2 seconds after a change in a folder the pane watches
  (both **estimates**). Listing runs in its own thread, so a slow `git` never holds up the keys. The pane
  watches the repository's top folder, the agent's files' folders and the open file's folder, not every
  folder in the repository: a recursive watch of `target/` alone would need thousands of watches.
- **`x` drops a file** from the list until the pane restarts, so listing again doesn't bring it back. An agent's
  write brings it back.

**Many files** (this repository has 183 `.md` files: 86 in `docs/cli/`, 53 in `docs/findings/`, 37 in
`docs/plans/`). Before, every header stayed on screen and the open section got the rest, which leaves no room at
all once there are more files than rows. Now:
- **The open file gets at least 2/3 of the pane's rows** (an estimate). If every header fits in the rest, all
  show, as before.
- **If they don't fit,** the headers nearest the open one fill the rest, half above it and half below. `Tab` and
  `Shift+Tab` step through the files, opening each, and the headers move with them.
- **With nothing open,** the pane is a list of headers: `j`/`k`, the arrows, `PgUp`/`PgDn`, `g`/`G` and the wheel
  move the highlight, and `Enter` opens the highlighted file. `Enter` on the open file closes it again.

```
 Ctrl+t d in ~/termos, nothing written yet        then tos claude writes docs/plans/2026-11-23-x.md
┌──────────────────────────────────────────┐      ┌──────────────────────────────────────────┐
│ docs · 183 · following                   │      │ docs · 184 · following                   │
│▸ docs/plans/2026-11-17-agent-plan-…  2m  │      │▾ docs/plans/2026-11-23-x.md         now  │
│▸ README.md                           1h  │      │  # Plan: the export button               │
│▸ docs/cli.md                         1h  │      │  **Status:** proposed                    │
│▸ CLAUDE.md                           2d  │      │  …   (2/3 of the rows, at least)         │
│▸ docs/findings/2026-11-21-demo-doc…  2d  │      │                                          │
│▸ …                                       │      │▸ docs/plans/2026-11-17-agent-plan-…  2m  │
│  (j/k move, Enter opens)                 │      │▸ README.md                           1h  │
└──────────────────────────────────────────┘      └──────────────────────────────────────────┘
```

```
┌─ tos docs --list FILE (the docs pane) ───────────────────────────────────────────────────┐
│ main thread: keys, mouse, drawing                                                         │
│   ▲ (1) the list file grew: the agent's writes (ingest, as before)                       │
│   ▲ (2) Files(paths): the repository's .md files, from the lister thread                  │
│   │                                                                                       │
│ lister thread: every 20 s, or 2 s after a change ──▶ git ls-files … (or the folder walk)  │
└───────────────────────────────────────────────────────────────────────────────────────────┘
```

Walking through the first picture:
1. Your shell is in `~/termos/crates/tos`. You press `Ctrl+t d`. The server starts `tos docs --list FILE` in that
   folder.
2. The docs pane asks git for the repository's top folder, `~/termos`, and lists its 183 Markdown files, most
   recently changed first. Nothing is open, so `j`/`k` move the highlight and `Enter` opens a file.
3. You start `tos claude` in the other pane and ask for a plan. Claude writes `docs/plans/2026-11-23-x.md`.
4. The hook sends `add-doc`, the server appends the file to the list, and the pane reads the new line. The file
   is new, so it goes on top, and following opens it with 2/3 of the rows. The headers nearest it stay below.
5. Claude then edits `README.md`, listed third. It moves to the top and opens, and the plan closes to its
   header, now second.
- **Later:** plan text or a step list that only exists inside a tool call, such as Claude's `ExitPlanMode` or
  Codex's `update_plan`, rendered as Markdown and titled "Plan · claude".

For an agent on the folder watch (option B), every `.md` change under its pane's folder counts, as described in
option B.

**The accordion:**
- **One header line per entry.** For example, `▾ docs/plans/2026-11-21-x.md   now`:
  - the path is relative to the repository's top folder (since 2026-11-23; before, the agent pane's folder),
    or else written `~/…`;
  - its age is right-aligned;
  - `●` marks a file that changed since you last had it open.
- **One section is open at a time.** Its rendered Markdown fills the space between its header and the next one.
  The other headers stay visible above and below, as many as fit ("Many files" above).
- **The newest file comes first,** in the order files first appeared. Edits don't move a file, so nothing
  jumps while you read; an edited file just gets its `●`.
- **Following** (`following` in the title): the file written last opens on its own. A file that's already open
  keeps its scroll position. Opening another section yourself stops following, and `f` starts it again.

**Rendering:** the finder's Markdown renderer (`server/finder/view.rs::markdown`), so headings, tables, code and
links look as in the finder's preview. `notify` watches each listed file, so the pane also redraws when you edit
a file yourself. A file that's deleted leaves the list.

**Keys and mouse, when the pane has the focus:**
- `j`/`k`, `↓`/`↑`, `PgDn`/`PgUp` and the wheel scroll the open section.
- `Tab`/`Shift+Tab` move to the next or previous section.
- `Enter`, or a click on a header, opens or closes that section.
- `e` edits the file in the docs pane's place, as the finder's editor does. `o` shows it in a finder tab.
- `x` drops the entry from the list, without touching the file. `f` turns following back on.
- `q` hides the pane.

**Selecting and copying** (added 2026-11-21, after the user asked for the finder preview's copy in the docs pane
too). It works as in the finder's preview ([its plan](2026-11-21-preview-select-copy.md)), with the same code
(`finder/select.rs`):
- **Drag** over the open file selects from one character to another. Letting go copies it, and the selection
  stays highlighted. Dragging above or below the open file scrolls it.
- **`v` or `V`** selects whole lines, starting at the first line on screen. `j`/`k` and the other scroll keys
  extend it. `y`, `Ctrl+c` or `Enter` copies it, and `Esc` cancels.
- **`Ctrl+c`** copies the selection again. It no longer closes the pane; `q` does.
- **What's copied** is the text as shown: rendered Markdown, wrapped, without `#` or `**`.
- **Starting a selection stops following,** so the agent's next file doesn't replace what you're selecting.
  The selection clears when another file opens, or when the open file changes.
- **How it copies:** the docs pane is a program, so it sends OSC 52 itself. `tos` passes a program's OSC 52 to
  your terminal's clipboard, and the status bar says `copied N characters`, as for the preview.

```
 drag in the docs pane ──1. mouse──▶ tos server ──2. forwarded (the pane asked for the mouse)──▶ tos docs
                                                                                                  │ 3. select, highlight
 your terminal ◀──5. OSC 52──── tos server ◀──4. OSC 52 on its output (letting go, y, Ctrl+c)─────┘
 (clipboard)                    "copied 42 characters"
```

**Hiding:**
- `q`, the `×` or `Ctrl+p x` hides the pane, and the tab keeps its list.
- New entries then don't reopen it. They only count up in the segment `▤ docs 4`, and clicking the segment
  shows the pane again.
- `tos action toggle-docs` does the same from scripts.
- **`Ctrl+t` then `d`** (tab mode, `d` for docs) shows or hides the active tab's docs pane. `d` was free in tab
  mode, and the key isn't Alt-based, so it works on macOS without Option-as-Alt. In `config.kdl` the same is
  the action `ToggleDocs`. (Added 2026-11-21, after the user asked for a key to bring the pane back.)

**Kept and visible to extensions:**
- Saving the layout keeps the list, because the list is a file. Attaching again restarts the viewer on it.
- The pane is kind `docs` in `list-panes -j`.
- Any script can add an entry with `tos action add-doc [-p PANE] PATH`. Adding plan text from stdin
  (`--title T -`) comes later.

### 4. The hook must never get in the agent's way

- **It never alters a tool call.** `tos hook` always exits 0 and prints nothing, because a hook's output can
  block or change a tool call. The exception is `agy`, which expects `{}`, so for it `tos hook` prints that
  ([finding](../findings/2026-11-21-agent-hook-files.md)).
- **Most calls cost no server round trip.** A write of anything but `.md` stops after a look at the path. A
  `.md` write sends one `add-doc` (about 3 ms, [finding](../findings/2026-11-07-tb-action-latency.md)).

### 5. What's new on the command line

| Command | Needed? | What it's for |
|---|---|---|
| `tos claude`, `tos codex`, `tos agy`, `tos agent`, `tos grok`, `tos gemini` `[ARGS…]` | **Required** | the launchers |
| `tos hook --agent NAME` | **Required** | what the hook runs. It sends `add-doc` over the session's socket, in the same process |
| `tos action add-doc`, `tos action toggle-docs` | Optional, and cheap | scripts, `Run` bindings, and agents without a launcher yet |

The names `tos agent` and `tos grok` are the agents' own command names: `agent` is Cursor's, and `grok` is xAI's.
Adding them as `tos` commands means a future `tos` command can't use those names.

### Example: `tos claude` writes a plan, then edits the README

You're in tab 2, pane `terminal_7`, in `~/termos`, at a shell prompt.

```
┌─ pane terminal_7 (TOS_SESSION=main, TOS_PANE_ID=7) ─────────────────────────────┐
│ $ tos claude                                                                     │
│   (1) tos writes agents/claude/settings.json, then                               │
│       exec claude --settings … --append-system-prompt "…"                         │
│ claude ──(2) Write docs/plans/2026-11-21-x.md ──▶ the file system                │
│   └──(3) PostToolUse: {"tool_name":"Write","tool_input":{"file_path":…}} on stdin │
│        ▼                                                                         │
│      ~/.local/bin/tos hook --agent claude                                        │
└────────────┬────────────────────────────────────────────────────────────────────┘
             │ (4) a .md path, so IPC: add-doc -p 7 ~/termos/docs/plans/2026-11-21-x.md
             ▼
┌─ tos server (session main) ─────────────────────────────────────────────────────┐
│ (5) pane 7 is in tab 2, which has no docs pane: open one on the tab's right edge │
│     without moving the focus; add the entry, open it, watch the file             │
└─────────────────────────────────────────────────────────────────────────────────┘
```

1. You type `tos claude`. `tos` writes its two files and becomes `claude`. Claude starts with `tos`'s hook and
   with the starting instructions at the end of its system prompt.
2. You ask "plan the export button". Claude writes `docs/plans/2026-11-21-x.md`.
3. Claude runs the hook, and `tos hook` reads the call from stdin. It sees `TOS_PANE_ID=7` and a `.md` path.
4. It sends `add-doc` to the server.
5. The docs pane opens on the right with the plan, while you keep typing on the left.
6. Claude edits `README.md`. That file's section goes on top and opens, and the plan's section closes to its
   header line.
7. You click the plan's header. It opens, and following stops. Later edits to `README.md` only mark it `●`.
8. In another pane you type plain `claude`. That session has no hook, and nothing appears.

### How it's built (added 2026-11-21)

```
┌─ agent pane 7 ─────────────┐ (1) tos hook --agent claude      ┌─ tos server ──────────────────────────┐
│ claude (started by         │ ───────────────────────────────▶ │ (2) add-doc: pane 7 is in tab 2;      │
│ tos claude)                │   IPC add-doc {pane 7, path, cwd} │     tab 2's docs pane? none, so:      │
└────────────────────────────┘                                   │     create docs/<id>.jsonl, and split │
                                                                 │     the tab's root: old | new (40%)   │
                                                                 │ (3) append {"path":…,"cwd":…,"t":…}   │
                                                                 └──────────────┬────────────────────────┘
                                                                                │ the list file
┌─ docs pane (an ordinary terminal pane) ───────────────────────────────────────▼───────────────────────┐
│ tos docs --list ~/.local/state/termos/docs/<id>.jsonl                                                 │
│ (4) notify: the list grew → add the entry, open it if following; a listed file changed → redraw     │
│ (5) draws the accordion with the finder's Markdown renderer; keys and mouse arrive like any program's │
└───────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

- **`tos docs --list FILE`** is a hidden command, the viewer. It runs in its own terminal pane, and uses
  ratatui and crossterm as `tos attach` does.
- **The list file** gets one JSON line per write the hook reports, so the viewer knows which file was written
  last. It lives in `~/.local/state/termos/docs/`; list files untouched for 30 days are removed when a new one is
  made.
- **The server finds a tab's docs pane** by its command (`tos docs --list …`), so a restored session finds it
  again.
  - When you close the docs pane, the tab remembers that. New entries then go into the list without
    reopening the pane, and the status bar shows `▤ docs: N new`.
  - A click on that segment, or `tos action toggle-docs`, reopens the pane on the same list.
- **Restoring:** a saved docs pane is started again directly, rather than typed at a shell's prompt as other
  restored commands are.
- **Not now:** images in the Markdown (the finder shows them; the viewer draws text only), and the narrow-tab
  rule. The pane always opens, at 40%.

## Options considered

| Option | For | Against | Verdict |
|---|---|---|---|
| **H. Launchers (`tos claude`, …) that add the hook and the instructions for one session** | Nothing written to any agent's files. Exact, because the hook names its pane. Only sessions you start through `tos` are affected | You type `tos claude`, not `claude`. Codex still asks you to trust the hook once. Some agents have no per-session way | **Chosen** (2026-11-21, the user's proposal) |
| A. `tos` writes its hook into each agent's own settings file | Works with plain `claude` | Changes files outside `tos`. Every session of the agent runs the hook, even outside `tos` | Not chosen, 2026-11-21 |
| B. No hook: the server watches the agent pane's folder for `.md` changes. It skips `.git`, `node_modules` and `target`, and ignores a burst of 5 or more files within 1 s (estimates) | Works for any agent | Your own edits in that folder show too. Misses files written outside it. No plan text or step lists | **The fallback** for agents with no per-session hook |
| G. Add the flags when you type plain `claude`, through a `PATH` wrapper or by injecting into the shell's startup | No new habit | A `PATH` wrapper loses to `~/.bashrc` ([finding](../findings/2026-11-19-agent-hooks-without-settings.md)). Injecting into bash, zsh and fish is a lot of machinery | Replaced by H: typing `tos claude` makes the wrapper unnecessary |
| C. Read the agents' transcripts | Nothing to set up | Private formats that change between versions | Not chosen |
| D. One `tos view` program per agent pane, following one file (the 2026-11-17 design) | No change to the server's panes | One file only | Replaced on 2026-11-19 |
| F. Read the agent's screen through `tos subscribe -p` | Nothing to set up | Breaks whenever an agent's screen changes | Not chosen |

## Steps

Built on 2026-11-21, in the order below. Step 0 became part of step 6: the hook logs every call it gets when
`TOS_HOOK_LOG=1` is set, so the same run answers the open questions.

1. **Done: the docs pane.**
   - `tos docs --list FILE` (`crates/tos/src/docsview.rs`): the accordion, following, keys and mouse, `notify`,
     and the finder's Markdown renderer.
   - The server's part (`server/docs_pane.rs`):
     - `add-doc` and `toggle-docs`;
     - opening the pane on the tab's right edge without moving the focus;
     - the list files, and staying closed once you close it, with the `▤ docs: N new` segment;
     - `kind: "docs"` in JSON, and restoring.
2. **Done: `tos hook --agent NAME`** (`crates/tos/src/agents.rs`).
   - It finds the file in each agent's documented shape: `tool_input`, `toolCall.args`, `toolArgs`, a top-level
     `file_path`, or an `apply_patch` patch.
   - It keeps only Markdown files inside the `cwd`, and none in agents' own folders.
3. **Done: the launchers** `tos claude`, `codex`, `agy`, `agent`, `grok` and `gemini`, with
   `~/.config/termos/agent-prompt.md`. Each installed agent (Claude Code 2.1.330, Codex 0.160.1, agy 1.5.9,
   Gemini CLI 0.62.0) starts with `tos`'s flags added (`tos <agent> --version`). Cursor and Grok aren't installed
   here, so their plugins follow their docs only. Telling Grok Build from the community `grok` is left for later.
4. **Not done: the folder watch (option B).** It's needed only if step 6 shows `tos agy`'s hook doesn't run.
5. **Done: docs and tests.**
   - `docs/cli.md` "Agents and the docs pane", the generated `docs/cli/` pages, and a README line.
   - Unit tests: payloads, the path filter, each launcher's arguments, the accordion.
   - Two e2e tests:
     - a hook call opens the docs pane on the right without moving the focus; closing it keeps it closed;
       `toggle-docs` reopens it;
     - `tos claude` passes its flags, your own prompt, and `--help`, to a stand-in `claude`.
6. **By hand, yours:** in a `tos` pane, start each agent with the log on, for example `TOS_HOOK_LOG=1 tos claude`.
   Ask it to write a plan as a `.md` file, and check that it shows on the right.
   `~/.local/state/termos/agents/hook.log` then shows each call. That answers the open questions:
   - **Claude:** do hooks from `--settings` run alongside yours? They run (seen in the README demo's recording
     on 2026-11-21, Claude Code 2.1.330, a config without hooks of its own:
     [finding](../findings/2026-11-21-demo-docs-pane.md)); alongside your own hooks is still unchecked.
   - **Codex:** does a `-c` hook run? Codex asks once to trust it; does that trust last?
   - **`agy`:** does `--settings` take hooks?
   - **Gemini:** does a hook from the system settings file run?
   - **Each agent:** do the starting instructions arrive?

7. **2026-11-23: every Markdown file in the repository, and the rules on explaining.**
   - `docsview.rs`: the lister thread (`git ls-files`, or the bounded walk), the repository's files below the
     agent's, moving a file the agent writes to the top, the 2/3 rule with the headers around the open file, and
     moving through the list with nothing open.
   - `docs_pane.rs`: `Ctrl+t d` starts the pane in the folder of the tab's focused pane.
   - `agents.rs`: the default starting instructions get the four rules.
   - Tests: unit tests for the order, the move to the top, `x` staying dropped, the layout with many files and
     the listing (a git repository and a plain folder); an e2e test that `Ctrl+t d` in a repository's subfolder
     lists its files and leaves out ignored ones. Docs: `docs/cli.md`, the README line.

**Later:**
- a `/` filter in the docs pane, for repositories with hundreds of Markdown files;
- more launchers, in the order of the "Later" table in section 2;
- plan text and step lists that live only inside a tool call;
- the narrow-tab rule;
- images in the docs pane.
