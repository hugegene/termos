# Finding: the docs pane in the README demo

**Date:** 2026-11-21 · **Plan:** [the demo](../plans/2026-11-13-finder-demo-gif.md), with its Claude part changed
on 2026-11-21

## Question

The demo's Claude part now runs `tos claude`, asks for `plan.md`, then copies lines from the docs pane and
pastes them into Claude's prompt. Three facts decide how `record.py` does this:
- Does `tos claude`'s hook fire in a real Claude Code, so the docs pane opens by itself?
- Does the docs pane's copy (OSC 52) reach Ghostty's clipboard, and does `Ctrl+Shift+v` paste it?
- How does Claude Code show a paste of several lines?

## Method

Three runs of `scripts/demo/record.py`, the first two with `--keep`, then frames pulled from the kept
`raw.mkv` with `ffmpeg -ss T -i raw.mkv -frames:v 1`. Machine: Ubuntu 24.04.5 on x86_64; Claude Code 2.1.330,
Ghostty 1.3.1, ffmpeg 6.1.1, `tos` 0.4.6 (release build).

```bash
python3 scripts/demo/record.py --video ~/ghibli-video/output/full_v5_depth8/babyplayful_ghibli.mp4 \
    --image ~/ghibli-video/output/t2v/flying_baby.mp4 --out $S/full.webp --short-out $S/short.gif --keep
```

The prompt: `Write a short plan in plan.md for taking the bakery's orders online: a title and 5 numbered steps`.
Claude's config is the demo's own: the login, the folder trusted, no hooks of its own.

## Results

**The hook:** in all three runs the docs pane opened on the right of Claude's tab, with `plan.md` open and
`following`, while Claude was still working. Claude took 10.5 s each time (`sped up 2x: 17.6-28.1 s`).

**The copy:** `Ctrl+p` `→` `Esc`, then `V`, `j` ×5 and `y`, selected the title through step 1 and the pane's
status line said `copied 173 characters` (run 1) and `copied 207 characters` (run 3). Ghostty also showed a
"Copied to clipboard" toast over the bottom of the screen, its `app-notifications = clipboard-copy` default.

**The paste:** `Ctrl+Shift+v` in Claude's prompt showed

```
❯ [Pasted text #1 +5 lines]
  paste again to expand
```

A second `Ctrl+Shift+v` replaced it with the copied lines themselves:

```
❯ Plan: Taking the Bakery's Orders
  Online

  1. Define the offering — Choose which
     products can be ordered online, set
     …
```

The copied text keeps the docs pane's line breaks: 6 rows on screen gave 6 lines (`+5 lines`).

**The recordings (run 3, in `docs/images/`):**

```
finder-demo.webp: 1.4 MB, 111 s (116 s recorded), 832x618
finder-demo.gif:  1.3 MB, 111 s (116 s recorded), 832x618
demo-short.gif:   0.9 MB, 44 s (116 s recorded), 832x618
```

## Conclusions

- **`tos claude`'s hook fires** when Claude Code 2.1.330 loads it with `--settings`. This answers the Claude
  question of the agent plan's step 6, for a config without hooks of its own. Whether it also runs alongside a
  user's own hooks is still unchecked.
- **The clipboard works end to end:** docs pane → OSC 52 → tos → Ghostty → `Ctrl+Shift+v` → Claude.
- **The script pastes twice,** because Claude folds a paste of several lines and only shows it after a second
  paste. It also starts Ghostty with `--app-notifications=no-clipboard-copy`, since tos already says what was
  copied.
- **Copying a wrapped paragraph gives a line break at each wrap**, not one line per paragraph. That's how the
  docs pane and the finder's preview copy today; it may be worth changing, as pasted text then has breaks where
  the pane happened to wrap.
